//! The `rollup` kind: a column whose value is an aggregate over the rows a relation points at.
//!
//! A rollup stores nothing. Its config names a relation column of this dataset, a field of the
//! dataset that relation points at, and one of six aggregates, so its value is a question about
//! *other* rows — asked again every time a row is read. Keeping the answer anywhere (a cached
//! count, a materialised total) is how a rollup ends up disagreeing with the rows it counts, and a
//! number that is quietly wrong in a knowledge base is worse than one that is asked for.
//!
//! Because the question reaches into another dataset it is asked under the relation's own access
//! rule, and the order matters: **gate first, then aggregate**. A caller who may not read the
//! target does not get `0`, `null` or `[]` either — every one of those is an answer to a question
//! they were never allowed to ask, and each says "there is a column here and it is empty". The key
//! is absent from the schema and from the row, exactly as the relation column is, so a rollup
//! cannot be probed for the size of a dataset somebody else is holding (ADR 0022). Mutation
//! `rollup` removes the gate and removes the omission; both must fail a test.
//!
//! Rollups are filled in after the relation cells have been decorated and before the formulas are
//! evaluated: a formula may read a rollup's value, and a rollup never reads a formula's.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::{json, Number, Value};

use super::computed::error_cell;
use super::{is_computed_kind, DatasetField, DatasetRow};
use crate::Store;
use gw_auth::Principal;
use gw_core::dataset::FieldKind;
use gw_core::formula::FormulaError;

/// The one refusal for a relation whose target the caller may not read and for a column whose
/// stored config has gone bad: whether a dataset exists behind the relation is not something
/// saving a column is allowed to disclose, the same reason a relation cell refuses uniformly.
pub(super) const ROLLUP_REFUSAL: &str = "rollup target is not available";
/// A rollup's config is exactly three strings. Named once so the refusal and the check agree.
const ROLLUP_SHAPE: &str =
    "a rollup's config is exactly {\"relation\": \"...\", \"target_field\": \
                            \"...\", \"fn\": \"count|sum|min|max|avg|list\"}";
/// The aggregates a `fn` member may name, listed in the order a refusal names them.
const ROLLUP_FNS: [&str; 6] = ["count", "sum", "min", "max", "avg", "list"];
/// Most values one `list` rollup returns: a bounded answer, however wide the linked rows are.
const MAX_LIST_VALUES: usize = 100;
/// Most ids one target read asks for at a time. The `IN` list is built from placeholders, and
/// SQLite caps the parameters of a statement; the rest is asked for in a second query.
const MAX_IDS_PER_QUERY: usize = 500;

/// The aggregate a rollup column asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RollupFn {
    Count,
    Sum,
    Min,
    Max,
    Avg,
    List,
}

impl RollupFn {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "count" => Self::Count,
            "sum" => Self::Sum,
            "min" => Self::Min,
            "max" => Self::Max,
            "avg" => Self::Avg,
            "list" => Self::List,
            _ => return None,
        })
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Count => "count",
            Self::Sum => "sum",
            Self::Min => "min",
            Self::Max => "max",
            Self::Avg => "avg",
            Self::List => "list",
        }
    }
}

/// One rollup column, with the relation behind it resolved for a caller.
struct RollupCol {
    key: String,
    /// The relation column this aggregates over: its id (the edges are keyed by it) and its key
    /// (the source cell that lists the target row ids). `None` until the config names one that
    /// exists, which is also when a column stops being answerable at all.
    relation: Option<(String, String)>,
    /// The dataset the aggregated rows live in — the relation's target.
    target_doc: Option<String>,
    target_field: String,
    func: RollupFn,
    /// Whether this caller may read the aggregate. `false` hides the whole column.
    visible: bool,
}

/// The three members of a rollup config, as strings, or `None` if it is not exactly that. An extra
/// or mistyped member is refused rather than ignored: `{"relation": "mit", "fn": "cnt"}` typed one
/// letter short would otherwise read as a column that counts nothing at all.
fn rollup_members(config: &Value) -> Option<(String, String, String)> {
    let map = config.as_object()?;
    if map.len() != 3 || !map.values().all(Value::is_string) {
        return None;
    }
    let member = |key: &str| {
        map.get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some((member("relation")?, member("target_field")?, member("fn")?))
}

/// Whether the aggregate can be answered from a target column of this kind, or the reason it
/// cannot. `count` never looks at the column, so this is never asked for it.
fn target_kind_refusal(func: RollupFn, kind: &str) -> Option<String> {
    if is_computed_kind(kind) {
        return Some(format!(
            "{} over `{kind}` needs a stored field to aggregate",
            func.as_str()
        ));
    }
    let number = kind == FieldKind::Number.as_str();
    let date = kind == FieldKind::Date.as_str();
    match func {
        RollupFn::Sum | RollupFn::Avg if !number => Some(format!(
            "{} over `{kind}` needs a number field",
            func.as_str()
        )),
        RollupFn::Min | RollupFn::Max if !(number || date) => Some(format!(
            "{} over `{kind}` needs a number or date field",
            func.as_str()
        )),
        _ => None,
    }
}

/// The target rows one rollup counts: the ids its relation cell names, in that order, keeping only
/// those whose edge is still alive. A target row deleted out from under the source leaves no id in
/// the set, so it counts for nothing and lists as nothing.
fn target_ids(
    row: &DatasetRow,
    col: &RollupCol,
    edges: &HashMap<String, HashSet<String>>,
) -> Vec<String> {
    let Some((field_id, relation_key)) = col.relation.as_ref() else {
        return Vec::new();
    };
    let Some(live) = edges.get(field_id) else {
        return Vec::new();
    };
    let cell: Vec<&str> = match row.values.get(relation_key) {
        Some(Value::Array(ids)) => ids.iter().filter_map(Value::as_str).collect(),
        Some(Value::String(id)) => vec![id.as_str()],
        _ => Vec::new(),
    };
    cell.into_iter()
        .filter(|id| live.contains(*id))
        .map(str::to_string)
        .collect()
}

/// The cell a rollup answers with. `value` is the target column's cell for one row id — a row that
/// is gone, or a cell nobody filled, contributes nothing rather than a zero.
fn aggregate(
    func: RollupFn,
    ids: &[String],
    mut value: impl FnMut(&str) -> Option<Value>,
) -> Value {
    match func {
        // Counting the links is all count asks, so it is the one aggregate that never reads a
        // target cell: a linked row with an empty cell is still a linked row.
        RollupFn::Count => json!(ids.len()),
        RollupFn::Sum => {
            let mut total = 0.0;
            for id in ids {
                if let Some(n) = numeric(&mut value, id) {
                    total += n;
                }
            }
            number_cell(total)
        }
        RollupFn::Avg => {
            let (mut total, mut seen) = (0.0, 0usize);
            for id in ids {
                if let Some(n) = numeric(&mut value, id) {
                    total += n;
                    seen += 1;
                }
            }
            // The mean of nothing is not nought: nought would be a claim about the value.
            if seen == 0 {
                return Value::Null;
            }
            number_cell(total / seen as f64)
        }
        RollupFn::Min | RollupFn::Max => {
            let mut best: Option<Value> = None;
            for id in ids {
                let Some(cell) = value(id).filter(|v| !v.is_null()) else {
                    continue;
                };
                let take = match &best {
                    None => true,
                    Some(old) => {
                        if func == RollupFn::Min {
                            before(&cell, old)
                        } else {
                            !before(&cell, old)
                        }
                    }
                };
                if take {
                    best = Some(cell);
                }
            }
            best.unwrap_or(Value::Null)
        }
        RollupFn::List => {
            let mut out: Vec<Value> = Vec::new();
            for id in ids {
                let Some(cell) = value(id) else { continue };
                // A cell that is itself a list (`tags`, a multi-select) is flattened one level,
                // and the whole answer stays bounded however many values the rows hold.
                let items: Vec<Value> = match cell {
                    Value::Array(inner) => inner,
                    other => vec![other],
                };
                for item in items {
                    if item.is_null() {
                        continue;
                    }
                    if out.len() == MAX_LIST_VALUES {
                        return Value::Array(out);
                    }
                    out.push(item);
                }
            }
            Value::Array(out)
        }
    }
}

/// The number a cell contributes to a `sum` or an `avg`: a number counts, anything else — text, a
/// bool, an absent cell — is skipped rather than turning the whole column into an error.
fn numeric(value: &mut impl FnMut(&str) -> Option<Value>, id: &str) -> Option<f64> {
    value(id)?.as_f64()
}

/// A rollup's number result, or the same error cell a formula leaves behind: `serde_json` holds no
/// non-finite number, so a sum that overflowed answers `#ERR` instead of a value that cannot be
/// written down.
fn number_cell(value: f64) -> Value {
    Number::from_f64(value)
        .map(Value::Number)
        .unwrap_or_else(|| error_cell(&FormulaError::NotFinite))
}

/// Whether one aggregated value sorts before another. Two numbers compare as numbers; two strings
/// as text, which is why `min` over a date column answers in calendar order — `YYYY-MM-DD` sorts as
/// written. Nothing else displaces what it is given.
fn before(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            x.as_f64().zip(y.as_f64()).is_some_and(|(x, y)| x < y)
        }
        (Value::String(x), Value::String(y)) => x < y,
        _ => false,
    }
}

impl Store {
    /// The checks a `rollup` column passes before it is saved: its config is exactly the three
    /// strings, `relation` names a relation column of **this** dataset, that column's target is
    /// readable by the caller, and `target_field` names a stored column of the target whose type
    /// the aggregate can answer. `count` ignores the field it names but still needs the string, so
    /// every rollup carries the same three members. `Ok(stored)` is the config as it lands.
    pub(super) async fn rollup_check(
        &self,
        principal: &Principal,
        doc_id: &str,
        config: &Value,
    ) -> Result<std::result::Result<Value, String>> {
        let Some((relation, target_field, fn_name)) = rollup_members(config) else {
            return Ok(Err(ROLLUP_SHAPE.into()));
        };
        let Some(func) = RollupFn::parse(&fn_name) else {
            return Ok(Err(format!(
                "a rollup's fn is one of {}",
                ROLLUP_FNS.join(", ")
            )));
        };
        let fields = self.fields_of(doc_id).await?;
        let Some(rel) = fields
            .iter()
            .find(|f| f.key == relation && f.kind == FieldKind::Relation.as_str())
        else {
            return Ok(Err(format!(
                "no relation field `{relation}` in this dataset"
            )));
        };
        // The relation stores its target by id, resolved when the column was made. Whether that
        // dataset is there and readable is not the caller's to find out, so both answers are one.
        let target_doc = rel
            .config
            .get("target_doc")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let Some(target_doc) = target_doc else {
            return Ok(Err(ROLLUP_REFUSAL.into()));
        };
        if !self.readable_dataset_by_id(principal, target_doc).await? {
            return Ok(Err(ROLLUP_REFUSAL.into()));
        }
        if func != RollupFn::Count {
            let wanted = self.fields_of(target_doc).await?;
            let Some(target) = wanted.iter().find(|f| f.key == target_field) else {
                return Ok(Err(format!(
                    "no field `{target_field}` in the rollup's target dataset"
                )));
            };
            if let Some(reason) = target_kind_refusal(func, &target.kind) {
                return Ok(Err(reason));
            }
        }
        Ok(Ok(json!({
            "relation": relation,
            "target_field": target_field,
            "fn": fn_name,
        })))
    }

    /// The rollup columns of `doc_id`, each with its relation resolved, for a caller. A dataset
    /// with no rollup column costs no queries beyond the one that says so.
    async fn rollup_cols(&self, principal: &Principal, doc_id: &str) -> Result<Vec<RollupCol>> {
        let raw: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, config FROM dataset_field WHERE doc_id = ?1 AND kind = 'rollup' \
             ORDER BY position, created_at, key",
        )
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;
        if raw.is_empty() {
            return Ok(Vec::new());
        }
        // The gate is the relation's own: aggregating rows of a dataset the caller may not read is
        // not made lawful by returning only a number over them.
        let relations = self.relation_cols(principal, doc_id).await?;
        let mut cols = Vec::with_capacity(raw.len());
        for (key, config) in raw {
            let value: Value = serde_json::from_str(&config).unwrap_or(Value::Null);
            let mut col = RollupCol {
                key,
                relation: None,
                target_doc: None,
                target_field: String::new(),
                func: RollupFn::Count,
                visible: false,
            };
            if let Some((relation, target_field, fn_name)) = rollup_members(&value) {
                if let Some(func) = RollupFn::parse(&fn_name) {
                    if let Some(rel) = relations.iter().find(|c| c.key == relation) {
                        col.relation = Some((rel.id.clone(), rel.key.clone()));
                        col.target_doc = rel.target_doc.clone();
                        col.target_field = target_field;
                        col.func = func;
                        col.visible = rel.visible && col.target_doc.is_some();
                    }
                }
            }
            cols.push(col);
        }
        Ok(cols)
    }

    /// Drop from the schema every rollup whose relation the caller may not follow. Absent, not
    /// shown-as-empty: a column that reads zero would announce that something is being counted
    /// somewhere the caller cannot see.
    pub(super) async fn filter_rollups(
        &self,
        principal: &Principal,
        doc_id: &str,
        fields: &mut Vec<DatasetField>,
    ) -> Result<()> {
        if !fields.iter().any(|f| f.kind == FieldKind::Rollup.as_str()) {
            return Ok(());
        }
        let cols = self.rollup_cols(principal, doc_id).await?;
        fields.retain(|f| {
            f.kind != FieldKind::Rollup.as_str() || cols.iter().any(|c| c.key == f.key && c.visible)
        });
        Ok(())
    }

    /// The rollup keys hidden from this caller — the ones a filter or sort may not name, answered
    /// exactly as if the column did not exist.
    pub(super) async fn hidden_rollup_keys(
        &self,
        principal: &Principal,
        doc_id: &str,
    ) -> Result<HashSet<String>> {
        let cols = self.rollup_cols(principal, doc_id).await?;
        Ok(cols
            .into_iter()
            .filter(|c| !c.visible)
            .map(|c| c.key)
            .collect())
    }

    /// Fill in each row's rollup cells: every column this caller may read gets its aggregate, every
    /// column they may not gets its key removed. Runs after the relation cells have been decorated
    /// and before the formulas are evaluated, so a formula can read a rollup and never the other
    /// way round. Nothing here is stored: the row in the database holds only what somebody typed.
    pub(super) async fn with_rollups(
        &self,
        principal: &Principal,
        doc_id: &str,
        rows: &mut [DatasetRow],
    ) -> Result<()> {
        let cols = self.rollup_cols(principal, doc_id).await?;
        if cols.is_empty() || rows.is_empty() {
            return Ok(());
        }
        // Gate first, before a single target row is read: which columns this caller may aggregate
        // at all is settled here, not by blanking an answer afterwards.
        let visible: Vec<&RollupCol> = cols.iter().filter(|c| c.visible).collect();
        // The ids each row's rollups count, and every id that will be asked for.
        let mut linked: Vec<Vec<Vec<String>>> = Vec::with_capacity(rows.len());
        let mut wanted: HashMap<String, HashSet<String>> = HashMap::new();
        for row in rows.iter() {
            // Live edges are fetched once per row, and a column with none simply counts nothing.
            let edges = self.live_edges(&row.id).await?;
            let mut per_col = Vec::with_capacity(visible.len());
            for col in &visible {
                let ids = target_ids(row, col, &edges);
                if let Some(doc) = &col.target_doc {
                    wanted
                        .entry(doc.clone())
                        .or_default()
                        .extend(ids.iter().cloned());
                }
                per_col.push(ids);
            }
            linked.push(per_col);
        }
        // Each target row is read once per page, for exactly the ids named above.
        let mut targets: HashMap<String, HashMap<String, Value>> = HashMap::new();
        for (doc, ids) in &wanted {
            targets.insert(doc.clone(), self.target_values(doc, ids).await?);
        }
        for (row, per_col) in rows.iter_mut().zip(&linked) {
            let Some(values) = row.values.as_object_mut() else {
                continue;
            };
            // A column the caller may not read leaves no trace behind: not a zero, not a null.
            for col in cols.iter().filter(|c| !c.visible) {
                values.remove(&col.key);
            }
            for (col, ids) in visible.iter().zip(per_col) {
                let doc = col.target_doc.as_deref().unwrap_or_default();
                let field = col.target_field.as_str();
                let cell = aggregate(col.func, ids, |id| {
                    targets
                        .get(doc)
                        .and_then(|rows| rows.get(id))
                        .and_then(Value::as_object)
                        .and_then(|stored| stored.get(field))
                        .filter(|v| !v.is_null())
                        .cloned()
                });
                values.insert(col.key.clone(), cell);
            }
        }
        Ok(())
    }

    /// The stored `values` of the target rows a page of rollups needs, by id. Every id is bound and
    /// the `IN` list is built from placeholders, so caller data never reaches the SQL text; the
    /// ids are asked for in bounded groups.
    async fn target_values(
        &self,
        doc_id: &str,
        ids: &HashSet<String>,
    ) -> Result<HashMap<String, Value>> {
        let mut found: HashMap<String, Value> = HashMap::with_capacity(ids.len());
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        for chunk in ids.chunks(MAX_IDS_PER_QUERY) {
            let marks = (0..chunk.len())
                .map(|i| format!("?{}", i + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT id, \"values\" FROM dataset_row WHERE doc_id = ?1 AND id IN ({marks})"
            );
            let mut q = sqlx::query_as::<_, (String, String)>(&sql).bind(doc_id);
            for id in chunk {
                q = q.bind(id);
            }
            for (id, values) in q.fetch_all(&self.pool).await? {
                found.insert(id, serde_json::from_str(&values).unwrap_or(Value::Null));
            }
        }
        Ok(found)
    }
}
