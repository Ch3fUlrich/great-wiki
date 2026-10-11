//! Datasets (ADR 0029): a dataset is a page of type `dataset`; its typed schema lives in
//! `dataset_field` and its rows in `dataset_row` (one JSON object of values per row).
//! Rows inherit the page's access; there is no per-row ACL. Migration 0020 owns the tables.

use crate::events::{EventKind, NewEvent};
use crate::{CreateOutcome, CreateRequest, Store};
use anyhow::Result;
use gw_auth::Action;
use gw_auth::Principal;
use gw_core::dataset::{validate_row, FieldKey, FieldKind, FieldSpec, MAX_ROW_BYTES};
use gw_core::DocumentType;
use serde::Serialize;
use serde_json::Value;

mod computed;
mod links;
mod query;
mod relation;
mod rollup;
mod views;
pub use query::{Cursor, Filter, RowQuery, Sort, SortKind};
pub use views::{DatasetView, MAX_VIEWS};

/// What the caller asks of a row list beyond paging. `sort` is the field and whether it
/// runs descending; `after` is a cursor from a previous page's `next`.
#[derive(Debug, Clone, Default)]
pub struct RowSelect {
    pub filters: Vec<Filter>,
    pub sort: Option<(FieldKey, bool)>,
    pub after: Option<String>,
    pub limit: i64,
}

/// Most fields one dataset may carry (spec: bounded schema).
pub const MAX_FIELDS: usize = 100;
/// Longest field label, in characters.
pub const MAX_LABEL_CHARS: usize = 120;
/// Largest `config` JSON a field may carry, in bytes.
pub const MAX_CONFIG_BYTES: usize = 4096;
/// Longest formula source a field may carry, in characters.
pub const MAX_FORMULA_EXPR_CHARS: usize = 1000;

/// One column of a dataset. `key` and `kind` never change after creation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatasetField {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub config: Value,
    pub position: i64,
}

#[derive(Debug, Clone)]
pub struct NewField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    pub config: Value,
}

/// How a schema call ended. `NoDataset` is the uniform refusal: no such page, not a dataset,
/// or not readable by the caller - all the same words (ADR 0022).
#[derive(Debug, Clone, PartialEq)]
pub enum SchemaOutcome<T> {
    Done(T),
    NoDataset,
    /// The caller may read the dataset but not change it.
    ReadOnly,
    NoField,
    Invalid(String),
    Conflict(String),
}

impl Store {
    /// Create an empty dataset page (no fields, no rows) as `principal`. It is the page
    /// door of ADR 0028 with the type fixed, not a second access path: write on the parent,
    /// an unreadable parent answered as "there is no page at …", admin at the top level.
    pub async fn create_dataset_for(
        &self,
        principal: &Principal,
        request: &CreateRequest,
        administers_destination: bool,
        today: &str,
    ) -> Result<CreateOutcome> {
        self.create_typed(
            principal,
            request,
            administers_destination,
            today,
            Some(DocumentType::Dataset),
        )
        .await
    }

    /// The dataset page at `path` if `principal` may read it, as a plain document. A missing
    /// page, a page of another type and an unreadable page are the same `None`.
    async fn readable_dataset(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<Option<crate::documents::StoredDocument>> {
        let found = self.document_for(principal, path, Action::Read).await?;
        Ok(found.filter(|d| d.doc_type == DocumentType::Dataset.as_str()))
    }

    /// Whether `principal` may read the dataset page with id `doc_id` — [`Store::readable_dataset`]
    /// reached by id, for a relation's `target_doc` which is stored as an id so it survives a move.
    /// Absent, not-a-dataset and unreadable are one `false`.
    async fn readable_dataset_by_id(&self, principal: &Principal, doc_id: &str) -> Result<bool> {
        let found = self
            .document_for_id(principal, doc_id, Action::Read)
            .await?;
        Ok(found.is_some_and(|d| d.doc_type == DocumentType::Dataset.as_str()))
    }

    /// The dataset page at `path` if `principal` may read it **and** write it. Every schema
    /// change goes through here; the write check is the line a mutation test removes.
    async fn writable_dataset(&self, principal: &Principal, path: &str) -> Result<Gate> {
        let Some(readable) = self.readable_dataset(principal, path).await? else {
            return Ok(Gate::NoDataset);
        };
        let writable = self.document_for(principal, path, Action::Write).await?;
        Ok(match writable {
            Some(_) => Gate::Open(readable.id),
            None => Gate::ReadOnly,
        })
    }

    async fn fields_of(&self, doc_id: &str) -> Result<Vec<DatasetField>> {
        let rows: Vec<(String, String, String, String, i64)> = sqlx::query_as(
            "SELECT key, label, kind, config, position FROM dataset_field \
             WHERE doc_id = ? ORDER BY position, created_at, key",
        )
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(key, label, kind, config, position)| DatasetField {
                key,
                label,
                kind,
                config: serde_json::from_str(&config).unwrap_or(Value::Null),
                position,
            })
            .collect())
    }

    /// The schema of the dataset at `path`, for anybody who may read it. A `relation` column whose
    /// target the caller may not read is absent from the list, not shown empty — and so is a
    /// `rollup` over it, which would otherwise be a number about that target.
    pub async fn dataset_fields(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<SchemaOutcome<Vec<DatasetField>>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(SchemaOutcome::NoDataset);
        };
        let mut fields = self.fields_of(&doc.id).await?;
        self.filter_fields(principal, &doc.id, &mut fields).await?;
        self.filter_rollups(principal, &doc.id, &mut fields).await?;
        Ok(SchemaOutcome::Done(fields))
    }

    /// Add a column. Needs write. `key` is validated by [`FieldKey::parse`] before it
    /// reaches SQL; `kind` is fixed for the life of the field. Every stored kind, `formula` and
    /// `rollup` are offered. A `person` column may say `{"multiple": true}`, a `file` column takes
    /// no config, and a `relation` column says `{"target": "<dataset path>", "multiple": bool}` —
    /// its target resolved to a doc id under the caller's own read access here, and stored by id so
    /// a rename cannot break the column. A `rollup` says `{"relation": "<key>", "target_field":
    /// "<key>", "fn": "count|sum|min|max|avg|list"}` and is checked against both ends here: the
    /// relation must be one of this dataset's columns, its target readable by this caller, and the
    /// field it aggregates a stored one the aggregate can answer.
    pub async fn add_dataset_field(
        &self,
        principal: &Principal,
        path: &str,
        new: &NewField,
    ) -> Result<SchemaOutcome<DatasetField>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let key = match FieldKey::parse(&new.key) {
            Ok(k) => k,
            Err(e) => return Ok(SchemaOutcome::Invalid(e.to_string())),
        };
        let label = match clean_label(&new.label) {
            Ok(l) => l,
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let caller_config = new.config.to_string();
        if !new.config.is_object() || caller_config.len() > MAX_CONFIG_BYTES {
            return Ok(SchemaOutcome::Invalid(format!(
                "field config must be an object of at most {MAX_CONFIG_BYTES} bytes"
            )));
        }
        if let Some(msg) = link_config_check(new.kind, &new.config) {
            return Ok(SchemaOutcome::Invalid(msg));
        }
        // A relation names its target by path; only the caller who can read that page may bind a
        // column to it, so the path is resolved to a doc id here and the path itself is discarded.
        // A rollup names its target by the relation that reaches it, and is refused here if this
        // caller cannot read what that relation points at.
        let stored_config = if new.kind == FieldKind::Relation {
            match self.resolve_target(principal, &new.config).await? {
                Ok(cfg) => cfg,
                Err(m) => return Ok(SchemaOutcome::Invalid(m)),
            }
        } else if new.kind == FieldKind::Rollup {
            match self.rollup_check(principal, &doc_id, &new.config).await? {
                Ok(cfg) => cfg,
                Err(m) => return Ok(SchemaOutcome::Invalid(m)),
            }
        } else {
            new.config.clone()
        };
        let config = stored_config.to_string();
        if new.kind == FieldKind::Formula {
            if let Some(msg) = self
                .formula_check(&doc_id, key.as_str(), &new.config)
                .await?
            {
                return Ok(SchemaOutcome::Invalid(msg));
            }
        }
        let mut tx = self.pool.begin().await?;
        let taken: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dataset_field WHERE doc_id = ? AND key = ?")
                .bind(&doc_id)
                .bind(key.as_str())
                .fetch_one(&mut *tx)
                .await?;
        if taken > 0 {
            return Ok(SchemaOutcome::Conflict(format!(
                "there is already a field `{}`",
                key.as_str()
            )));
        }
        // Count and insert in one statement so two racing writers cannot pass the cap.
        let id = uuid::Uuid::now_v7().to_string();
        let done = sqlx::query(
            "INSERT INTO dataset_field (id, doc_id, key, label, kind, config, position) \
             SELECT ?1, ?2, ?3, ?4, ?5, ?6, COALESCE(MAX(position) + 1, 0) \
             FROM dataset_field WHERE doc_id = ?2 HAVING COUNT(*) < ?7",
        )
        .bind(&id)
        .bind(&doc_id)
        .bind(key.as_str())
        .bind(&label)
        .bind(new.kind.as_str())
        .bind(&config)
        .bind(MAX_FIELDS as i64)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(SchemaOutcome::Invalid(format!(
                "a dataset has at most {MAX_FIELDS} fields"
            )));
        }
        let position: i64 = sqlx::query_scalar("SELECT position FROM dataset_field WHERE id = ?")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(SchemaOutcome::Done(DatasetField {
            key: key.as_str().to_string(),
            label,
            kind: new.kind.as_str().to_string(),
            config: stored_config,
            position,
        }))
    }

    /// The formula-specific checks taken before a `formula` column is written: the config must
    /// be one `expr` string, its `{…}` references must name fields of this dataset (or the new
    /// column itself, which then fails as a self-cycle), and it must close no cycle with the
    /// formulas already in the dataset. `Ok(None)` means the column is sound; `Ok(Some(reason))`
    /// is the message to refuse it with. A formula may read any other kind, rollup included —
    /// only another formula can be part of a cycle.
    async fn formula_check(
        &self,
        doc_id: &str,
        new_key: &str,
        config: &Value,
    ) -> Result<Option<String>> {
        let src = match formula_src(config) {
            Ok(s) => s,
            Err(m) => return Ok(Some(m)),
        };
        let existing = self.fields_of(doc_id).await?;
        let known = |k: &str| k == new_key || existing.iter().any(|f| f.key == k);
        let parsed = match gw_core::formula::parse(src, &known) {
            Ok(p) => p,
            Err(e) => return Ok(Some(e.to_string())),
        };
        // The graph is every formula column plus this one. Stored and rollup columns never
        // appear as nodes, so an edge to one is ignored: a cycle runs formula to formula only.
        let mut graph: Vec<(String, Vec<String>)> = existing
            .iter()
            .filter(|f| f.kind == FieldKind::Formula.as_str())
            .map(|f| {
                let deps = f
                    .config
                    .get("expr")
                    .and_then(Value::as_str)
                    .and_then(|s| gw_core::formula::parse(s, &|_| true).ok())
                    .map(|p| gw_core::formula::field_refs(&p))
                    .unwrap_or_default();
                (f.key.clone(), deps)
            })
            .collect();
        graph.push((new_key.to_string(), gw_core::formula::field_refs(&parsed)));
        Ok(gw_core::formula::deps::find_cycle(&graph)
            .map(|cycle| format!("formula cycle: {}", cycle.join(" -> "))))
    }

    /// Change a column's label. Needs write. The key and the kind are not parameters: they
    /// cannot be changed, because every stored row is keyed by the one and typed by the other.
    pub async fn rename_dataset_field(
        &self,
        principal: &Principal,
        path: &str,
        key: &str,
        label: &str,
    ) -> Result<SchemaOutcome<DatasetField>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let label = match clean_label(label) {
            Ok(l) => l,
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let done = sqlx::query("UPDATE dataset_field SET label = ? WHERE doc_id = ? AND key = ?")
            .bind(&label)
            .bind(&doc_id)
            .bind(key)
            .execute(&self.pool)
            .await?;
        if done.rows_affected() == 0 {
            return Ok(SchemaOutcome::NoField);
        }
        let field = self
            .fields_of(&doc_id)
            .await?
            .into_iter()
            .find(|f| f.key == key);
        Ok(field.map_or(SchemaOutcome::NoField, SchemaOutcome::Done))
    }

    /// Put the columns in the order given. Needs write. `keys` must be exactly the current
    /// keys, each once: a reorder never adds or drops a column.
    pub async fn reorder_dataset_fields(
        &self,
        principal: &Principal,
        path: &str,
        keys: &[String],
    ) -> Result<SchemaOutcome<Vec<DatasetField>>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let mut tx = self.pool.begin().await?;
        let mut current: Vec<String> =
            sqlx::query_scalar("SELECT key FROM dataset_field WHERE doc_id = ?")
                .bind(&doc_id)
                .fetch_all(&mut *tx)
                .await?;
        let mut wanted = keys.to_vec();
        current.sort();
        wanted.sort();
        if current != wanted {
            return Ok(SchemaOutcome::Invalid(
                "the order must name every field exactly once".into(),
            ));
        }
        for (position, key) in keys.iter().enumerate() {
            sqlx::query("UPDATE dataset_field SET position = ? WHERE doc_id = ? AND key = ?")
                .bind(position as i64)
                .bind(&doc_id)
                .bind(key)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(SchemaOutcome::Done(self.fields_of(&doc_id).await?))
    }

    /// Remove a column and its values from every row, in one transaction. Needs write.
    /// Touched rows move to the next `version`, so an editor holding the old row is told.
    pub async fn delete_dataset_field(
        &self,
        principal: &Principal,
        path: &str,
        key: &str,
    ) -> Result<SchemaOutcome<()>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let Ok(parsed) = FieldKey::parse(key) else {
            return Ok(SchemaOutcome::NoField);
        };
        let mut tx = self.pool.begin().await?;
        let gone = sqlx::query("DELETE FROM dataset_field WHERE doc_id = ? AND key = ?")
            .bind(&doc_id)
            .bind(parsed.as_str())
            .execute(&mut *tx)
            .await?;
        if gone.rows_affected() == 0 {
            return Ok(SchemaOutcome::NoField);
        }
        let json_path = format!("$.{}", parsed.as_str());
        sqlx::query(
            "UPDATE dataset_row SET \"values\" = json_remove(\"values\", ?1), \
             version = version + 1, updated_at = datetime('now') \
             WHERE doc_id = ?2 AND json_type(\"values\", ?1) IS NOT NULL",
        )
        .bind(&json_path)
        .bind(&doc_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(SchemaOutcome::Done(()))
    }
}

/// Outcome of the access gate shared by every schema write.
enum Gate {
    Open(String),
    NoDataset,
    ReadOnly,
}

/// Most rows one dataset may hold (ADR 0029: filters scan, so the table is bounded).
pub const MAX_ROWS: usize = 50_000;

/// One row. `version` starts at 1 and moves on with every change, so an editor holding an
/// old copy is told rather than overwriting.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatasetRow {
    pub id: String,
    pub values: Value,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Most rows one list call returns.
pub const MAX_PAGE: i64 = 200;

/// One page of rows and how many the dataset holds in all.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowPage {
    pub rows: Vec<DatasetRow>,
    /// Rows matching the filters (all rows when there are none), counted after the access gate.
    pub total: i64,
    /// Cursor for the next page; absent on the last one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
}

/// How a row call ended. `NoDataset` is the same uniform refusal as [`SchemaOutcome`].
#[derive(Debug, Clone, PartialEq)]
pub enum RowOutcome<T> {
    Done(T),
    NoDataset,
    ReadOnly,
    NoRow,
    Invalid(String),
    /// The dataset already holds [`MAX_ROWS`] rows.
    Full(String),
    /// The caller's `version` is out of date; carries the row as it is now.
    Stale(DatasetRow),
}

type RowTuple = (String, String, i64, String, String);

fn row_from(t: RowTuple) -> DatasetRow {
    DatasetRow {
        id: t.0,
        values: serde_json::from_str(&t.1).unwrap_or(Value::Null),
        version: t.2,
        created_at: t.3,
        updated_at: t.4,
    }
}

const ROW_COLS: &str = "id, \"values\", version, created_at, updated_at";

impl Store {
    /// Validate `patch` against the dataset's current schema. Unknown keys and wrong types are
    /// refused by core; the two kinds whose value names something outside the row — a `person`
    /// who must be able to read this page, a `file` that must be on it — are refused here, where
    /// the store can ask. `null` stays as `null` so an update can clear a cell.
    async fn checked_values(
        &self,
        path: &str,
        doc_id: &str,
        patch: &serde_json::Map<String, Value>,
    ) -> Result<std::result::Result<serde_json::Map<String, Value>, String>> {
        let fields = self.fields_of(doc_id).await?;
        let parsed: Vec<(FieldKey, FieldKind, Value)> = fields
            .into_iter()
            .filter_map(|f| {
                let key = FieldKey::parse(&f.key).ok()?;
                let kind = serde_json::from_value(Value::String(f.kind)).ok()?;
                Some((key, kind, f.config))
            })
            .collect();
        let specs: Vec<FieldSpec<'_>> = parsed
            .iter()
            .map(|(key, kind, config)| FieldSpec {
                key,
                kind: *kind,
                config,
            })
            .collect();
        let clean = match validate_row(&specs, patch) {
            Ok(clean) => clean,
            Err(e) => return Ok(Err(e.to_string())),
        };
        let kinds: Vec<(&str, FieldKind)> = parsed
            .iter()
            .map(|(key, kind, _)| (key.as_str(), *kind))
            .collect();
        if let Some(reason) = self.link_refusal(path, doc_id, &kinds, &clean).await? {
            return Ok(Err(reason));
        }
        Ok(Ok(clean))
    }

    /// SQLite's current date, for `today()` in a formula: fetched once per read call, never per
    /// row, so a whole page sees one clock.
    async fn today(&self) -> Result<String> {
        Ok(sqlx::query_scalar("SELECT date('now')")
            .fetch_one(&self.pool)
            .await?)
    }

    async fn fetch_row(&self, doc_id: &str, row_id: &str) -> Result<Option<DatasetRow>> {
        let t: Option<RowTuple> = sqlx::query_as(&format!(
            "SELECT {ROW_COLS} FROM dataset_row WHERE doc_id = ? AND id = ?"
        ))
        .bind(doc_id)
        .bind(row_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(t.map(row_from))
    }

    /// One row of the dataset at `path`, for anybody who may read it, with its file cells resolved,
    /// its relation cells narrowed to the links that are still alive and visible to this caller, its
    /// rollups aggregated over those rows and its formulas then evaluated over both.
    pub async fn dataset_row(
        &self,
        principal: &Principal,
        path: &str,
        row_id: &str,
    ) -> Result<RowOutcome<DatasetRow>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(RowOutcome::NoDataset);
        };
        let Some(row) = self.fetch_row(&doc.id, row_id).await? else {
            return Ok(RowOutcome::NoRow);
        };
        let fields = self.fields_of(&doc.id).await?;
        let names = self.attachment_names(&doc.id, &fields).await?;
        let today = self.today().await?;
        let mut row = links::resolve_files(row, &fields, &names);
        self.decorate_rows(principal, &doc.id, std::slice::from_mut(&mut row))
            .await?;
        self.with_rollups(principal, &doc.id, std::slice::from_mut(&mut row))
            .await?;
        let row = computed::with_computed(&fields, row, &today);
        Ok(RowOutcome::Done(row))
    }

    /// A page of rows of the dataset at `path`, with the dataset's row count, for anybody who
    /// may read it. The count is taken only after the access seam has passed, and only over
    /// that dataset: an unreadable dataset yields `NoDataset`, never a number.
    pub async fn dataset_rows(
        &self,
        principal: &Principal,
        path: &str,
        limit: i64,
        offset: i64,
    ) -> Result<RowOutcome<RowPage>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(RowOutcome::NoDataset);
        };
        let tuples: Vec<RowTuple> = sqlx::query_as(&format!(
            "SELECT {ROW_COLS} FROM dataset_row WHERE doc_id = ? \
             ORDER BY created_at, id LIMIT ? OFFSET ?"
        ))
        .bind(&doc.id)
        .bind(limit.clamp(1, MAX_PAGE))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dataset_row WHERE doc_id = ?")
            .bind(&doc.id)
            .fetch_one(&self.pool)
            .await?;
        let fields = self.fields_of(&doc.id).await?;
        let names = self.attachment_names(&doc.id, &fields).await?;
        let today = self.today().await?;
        let mut rows: Vec<DatasetRow> = tuples
            .into_iter()
            .map(row_from)
            .map(|r| links::resolve_files(r, &fields, &names))
            .collect();
        self.decorate_rows(principal, &doc.id, &mut rows).await?;
        self.with_rollups(principal, &doc.id, &mut rows).await?;
        let rows: Vec<DatasetRow> = rows
            .into_iter()
            .map(|r| computed::with_computed(&fields, r, &today))
            .collect();
        Ok(RowOutcome::Done(RowPage {
            rows,
            total,
            next: None,
        }))
    }

    /// Filtered, sorted, cursor-paged rows. Same access seam and count-after-gate as
    /// [`Store::dataset_rows`]; see `query.rs` for how the SQL is kept free of caller text.
    pub async fn dataset_rows_query(
        &self,
        principal: &Principal,
        path: &str,
        select: &RowSelect,
    ) -> Result<RowOutcome<RowPage>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(RowOutcome::NoDataset);
        };
        // Every named field must exist in this dataset's schema; the sort kind comes from it.
        let fields = self.fields_of(&doc.id).await?;
        // A relation column the caller may not resolve (its target is unreadable) is a key they
        // never see, and so is a rollup over it: naming either names no field at all.
        let mut hidden = self.hidden_relation_keys(principal, &doc.id).await?;
        hidden.extend(self.hidden_rollup_keys(principal, &doc.id).await?);
        let kind_of = |key: &FieldKey| fields.iter().find(|f| f.key == key.as_str());
        for f in &select.filters {
            let (Filter::Eq { key, .. }
            | Filter::Contains { key, .. }
            | Filter::IsEmpty { key }
            | Filter::In { key, .. }) = f;
            match kind_of(key) {
                None => return Ok(RowOutcome::Invalid(format!("no field `{}`", key.as_str()))),
                Some(_) if hidden.contains(key.as_str()) => {
                    return Ok(RowOutcome::Invalid(format!("no field `{}`", key.as_str())))
                }
                Some(field) if is_computed_kind(&field.kind) => {
                    return Ok(RowOutcome::Invalid(computed_refusal(key.as_str())))
                }
                Some(field) if field.kind == FieldKind::Relation.as_str() => {
                    return Ok(RowOutcome::Invalid(relation_refusal(key.as_str(), &hidden)))
                }
                Some(_) => {}
            }
        }
        let sort = match &select.sort {
            None => None,
            Some((key, desc)) => {
                let Some(field) = kind_of(key) else {
                    return Ok(RowOutcome::Invalid(format!("no field `{}`", key.as_str())));
                };
                if hidden.contains(key.as_str()) {
                    return Ok(RowOutcome::Invalid(format!("no field `{}`", key.as_str())));
                }
                if is_computed_kind(&field.kind) {
                    return Ok(RowOutcome::Invalid(computed_refusal(key.as_str())));
                }
                if field.kind == FieldKind::Relation.as_str() {
                    return Ok(RowOutcome::Invalid(relation_refusal(key.as_str(), &hidden)));
                }
                Some(Sort {
                    key: key.clone(),
                    kind: if field.kind == FieldKind::Number.as_str() {
                        SortKind::Number
                    } else {
                        SortKind::Text
                    },
                    desc: *desc,
                })
            }
        };
        let after = match select.after.as_deref().map(Cursor::decode) {
            None => None,
            Some(Ok(c)) => Some(c),
            Some(Err(m)) => return Ok(RowOutcome::Invalid(m)),
        };
        let limit = select.limit.clamp(1, MAX_PAGE);
        let query = RowQuery {
            filters: select.filters.clone(),
            sort,
            after,
            limit,
        };
        let built = match query.build(&doc.id) {
            Ok(b) => b,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        let sql = format!(
            "SELECT {ROW_COLS}{} FROM dataset_row WHERE {}{}",
            built.sort_cols_sql, built.where_sql, built.page_sql
        );
        let mut q = sqlx::query_as::<
            _,
            (
                String,
                String,
                i64,
                String,
                String,
                Option<f64>,
                Option<String>,
            ),
        >(&sql);
        for b in built
            .sort_cols_binds
            .iter()
            .chain(&built.where_binds)
            .chain(&built.page_binds)
        {
            q = match b {
                query::Bind::Text(s) => q.bind(s),
                query::Bind::Real(r) => q.bind(*r),
                query::Bind::Int(i) => q.bind(*i),
            };
        }
        let mut fetched = q.fetch_all(&self.pool).await?;
        let more = fetched.len() as i64 > limit;
        fetched.truncate(limit as usize);
        let next = match (more, fetched.last()) {
            (true, Some(last)) => Some(Cursor::of(last.5, last.6.clone(), last.0.clone()).encode()),
            _ => None,
        };
        let count_sql = format!("SELECT COUNT(*) FROM dataset_row WHERE {}", built.where_sql);
        let mut c = sqlx::query_scalar::<_, i64>(&count_sql);
        for b in &built.where_binds {
            c = match b {
                query::Bind::Text(s) => c.bind(s),
                query::Bind::Real(r) => c.bind(*r),
                query::Bind::Int(i) => c.bind(*i),
            };
        }
        let total = c.fetch_one(&self.pool).await?;
        let names = self.attachment_names(&doc.id, &fields).await?;
        let today = self.today().await?;
        let mut rows: Vec<DatasetRow> = fetched
            .into_iter()
            .map(|t| row_from((t.0, t.1, t.2, t.3, t.4)))
            .map(|r| links::resolve_files(r, &fields, &names))
            .collect();
        self.decorate_rows(principal, &doc.id, &mut rows).await?;
        self.with_rollups(principal, &doc.id, &mut rows).await?;
        let rows: Vec<DatasetRow> = rows
            .into_iter()
            .map(|r| computed::with_computed(&fields, r, &today))
            .collect();
        Ok(RowOutcome::Done(RowPage { rows, total, next }))
    }

    /// Add a row. Needs write. Values are validated against the schema; a dataset holds at
    /// most [`MAX_ROWS`].
    pub async fn create_dataset_row(
        &self,
        principal: &Principal,
        path: &str,
        values: &serde_json::Map<String, Value>,
    ) -> Result<RowOutcome<DatasetRow>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(RowOutcome::NoDataset),
            Gate::ReadOnly => return Ok(RowOutcome::ReadOnly),
        };
        let mut clean = match self.checked_values(path, &doc_id, values).await? {
            Ok(v) => v,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        clean.retain(|_, v| !v.is_null());
        // Every relation cell is checked against the target and the caller's read access BEFORE the
        // transaction opens (the pool is one connection); only the edge writes happen inside it.
        let edges = match self.plan_write(principal, &doc_id, &clean).await? {
            Ok(w) => w,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        let id = uuid::Uuid::now_v7().to_string();
        let mut tx = self.pool.begin().await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dataset_row WHERE doc_id = ?")
            .bind(&doc_id)
            .fetch_one(&mut *tx)
            .await?;
        if count as usize >= MAX_ROWS {
            return Ok(RowOutcome::Full(format!(
                "a dataset has at most {MAX_ROWS} rows"
            )));
        }
        sqlx::query("INSERT INTO dataset_row (id, doc_id, \"values\") VALUES (?, ?, ?)")
            .bind(&id)
            .bind(&doc_id)
            .bind(Value::Object(clean).to_string())
            .execute(&mut *tx)
            .await?;
        self.rewrite_edges(&mut tx, &id, &edges).await?;
        tx.commit().await?;
        let row = self
            .fetch_row(&doc_id, &id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("row vanished after insert"))?;
        self.emit_row_event(principal, &doc_id, EventKind::DatasetRowCreated, &id)
            .await;
        Ok(RowOutcome::Done(row))
    }

    /// Change cells of a row. Needs write. `patch` is merged over the stored values (`null`
    /// clears a cell). `version` must be the row's current one, else the call answers
    /// [`RowOutcome::Stale`] with the row as it is, and nothing is written.
    pub async fn update_dataset_row(
        &self,
        principal: &Principal,
        path: &str,
        row_id: &str,
        version: i64,
        patch: &serde_json::Map<String, Value>,
    ) -> Result<RowOutcome<DatasetRow>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(RowOutcome::NoDataset),
            Gate::ReadOnly => return Ok(RowOutcome::ReadOnly),
        };
        let patch = match self.checked_values(path, &doc_id, patch).await? {
            Ok(v) => v,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        let Some(current) = self.fetch_row(&doc_id, row_id).await? else {
            return Ok(RowOutcome::NoRow);
        };
        if current.version != version {
            return Ok(RowOutcome::Stale(current));
        }
        // Checked against target and access before the transaction, from the patch as it stands
        // (a `null` clears the cell and its edges, `[]` clears a multi cell).
        let edges = match self.plan_write(principal, &doc_id, &patch).await? {
            Ok(w) => w,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        let mut merged = current.values.as_object().cloned().unwrap_or_default();
        for (k, v) in patch {
            if v.is_null() {
                merged.remove(&k);
            } else {
                merged.insert(k, v);
            }
        }
        let text = Value::Object(merged).to_string();
        if text.len() > MAX_ROW_BYTES {
            return Ok(RowOutcome::Invalid(format!(
                "row is larger than {MAX_ROW_BYTES} bytes"
            )));
        }
        // The row write and its edge rewrite share one transaction, so a cell and the edges that
        // back it can never disagree. The version in the WHERE is the lock: two editors holding
        // version N, one wins and the other is rolled back and told.
        let mut tx = self.pool.begin().await?;
        let done = sqlx::query(
            "UPDATE dataset_row SET \"values\" = ?, version = version + 1, \
             updated_at = datetime('now') WHERE doc_id = ? AND id = ? AND version = ?",
        )
        .bind(&text)
        .bind(&doc_id)
        .bind(row_id)
        .bind(version)
        .execute(&mut *tx)
        .await?;
        if done.rows_affected() == 0 {
            tx.rollback().await?;
            let Some(now) = self.fetch_row(&doc_id, row_id).await? else {
                return Ok(RowOutcome::NoRow);
            };
            return Ok(RowOutcome::Stale(now));
        }
        self.rewrite_edges(&mut tx, row_id, &edges).await?;
        tx.commit().await?;
        let Some(now) = self.fetch_row(&doc_id, row_id).await? else {
            return Ok(RowOutcome::NoRow);
        };
        self.emit_row_event(principal, &doc_id, EventKind::DatasetRowUpdated, row_id)
            .await;
        Ok(RowOutcome::Done(now))
    }

    /// Remove a row. Needs write.
    pub async fn delete_dataset_row(
        &self,
        principal: &Principal,
        path: &str,
        row_id: &str,
    ) -> Result<RowOutcome<()>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(RowOutcome::NoDataset),
            Gate::ReadOnly => return Ok(RowOutcome::ReadOnly),
        };
        let gone = sqlx::query("DELETE FROM dataset_row WHERE doc_id = ? AND id = ?")
            .bind(&doc_id)
            .bind(row_id)
            .execute(&self.pool)
            .await?;
        if gone.rows_affected() == 0 {
            return Ok(RowOutcome::NoRow);
        }
        self.emit_row_event(principal, &doc_id, EventKind::DatasetRowDeleted, row_id)
            .await;
        Ok(RowOutcome::Done(()))
    }

    /// Record a row event (ADR 0025): candidates are everyone who ever revised the dataset
    /// page, minus the actor. Ids only, no cell text; delivery re-asks read access, so a
    /// candidate who lost the page hears nothing. Per-dataset dedupe keeps a burst of edits
    /// to one unread row. A failure is logged, never the writer's problem.
    async fn emit_row_event(&self, actor: &Principal, doc_id: &str, kind: EventKind, row_id: &str) {
        let authors: Result<Vec<String>> = async {
            Ok(sqlx::query_scalar(
                "SELECT DISTINCT author_id FROM revisions WHERE document_id = ?1 \
                 AND author_id <> ?2",
            )
            .bind(doc_id)
            .bind(crate::revisions::IMPORT_AUTHOR_ID)
            .fetch_all(&self.pool)
            .await?)
        }
        .await;
        match authors {
            Ok(recipients) => {
                for recipient in recipients {
                    self.emit_logged(&NewEvent {
                        kind,
                        recipient,
                        actor: Some(actor.id.clone()),
                        doc_id: Some(doc_id.to_string()),
                        path: None,
                        subject: Some(row_id.to_string()),
                        dedupe_key: Some(format!("{}:{doc_id}", kind.as_str())),
                    })
                    .await;
                }
            }
            Err(err) => tracing::warn!(error = %err, "row event not recorded"),
        }
    }
}

fn clean_label(label: &str) -> Result<String, String> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(format!(
            "a field label is 1 to {MAX_LABEL_CHARS} characters"
        ));
    }
    Ok(label.to_string())
}

/// A `formula` column's config is exactly `{"expr": "..."}`: one string member, nothing else,
/// the source bounded. Returns the source text, or the reason the shape is wrong.
fn formula_src(config: &Value) -> Result<&str, String> {
    let shape = "a formula's config is exactly {\"expr\": \"...\"}";
    let Some(map) = config.as_object() else {
        return Err(shape.into());
    };
    if map.len() != 1 {
        return Err(shape.into());
    }
    let Some(Value::String(src)) = map.get("expr") else {
        return Err(shape.into());
    };
    if src.chars().count() > MAX_FORMULA_EXPR_CHARS {
        return Err(format!(
            "a formula is at most {MAX_FORMULA_EXPR_CHARS} characters"
        ));
    }
    Ok(src)
}

/// What the two store-dependent kinds accept as config, or the reason to refuse it. A `person`
/// column is single unless it says `{"multiple": true}`; a `file` column names one attachment and
/// so takes no config at all. An unknown member is refused rather than ignored: `{"mutiple":
/// true}` typed one letter short would otherwise silently leave a column that stores one id, and
/// the person who typed it would have no way to find out.
fn link_config_check(kind: FieldKind, config: &Value) -> Option<String> {
    let Some(map) = config.as_object() else {
        return None; // the general config check above has already refused this
    };
    match kind {
        FieldKind::Person
            if map.len() <= 1 && map.iter().all(|(k, v)| k == "multiple" && v.is_boolean()) => {}
        FieldKind::Person => {
            return Some("a person's config is {} or {\"multiple\": true|false}".into())
        }
        FieldKind::File if map.is_empty() => {}
        FieldKind::File => return Some("a file's config is {}".into()),
        FieldKind::Relation
            if map
                .get("target")
                .and_then(Value::as_str)
                .is_some_and(|t| !t.is_empty())
                && map.len() <= 2
                && map.iter().all(|(k, v)| match k.as_str() {
                    "target" => true,
                    "multiple" => v.is_boolean(),
                    _ => false,
                }) => {}
        FieldKind::Relation => {
            return Some(
                "a relation's config is {\"target\": \"<dataset path>\", \"multiple\": true|false}"
                    .into(),
            )
        }
        _ => {}
    }
    None
}

/// Whether a stored field's `kind` is a computed one (formula or rollup): evaluated at read,
/// never written, filtered or sorted.
fn is_computed_kind(kind: &str) -> bool {
    serde_json::from_value::<FieldKind>(Value::String(kind.to_string()))
        .is_ok_and(|k| k.is_computed())
}

/// The one refusal a computed field gets when it is named in a filter or a sort.
fn computed_refusal(key: &str) -> String {
    format!("computed field `{key}` cannot be filtered or sorted yet")
}

/// The refusal a relation field gets when it is named in a filter or a sort: `no field` when it is
/// hidden from this caller (their read of the target went, so the column is not one they have), and
/// otherwise the honest "not yet" — a relation cannot be filtered or sorted on in this milestone.
fn relation_refusal(key: &str, hidden: &std::collections::HashSet<String>) -> String {
    if hidden.contains(key) {
        format!("no field `{key}`")
    } else {
        "relation fields cannot be filtered or sorted yet".to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::documents::NewDocument;
    use crate::revisions::Author;
    use crate::{CreateOutcome, CreateRequest, Store};
    use gw_auth::{Action, Permission, Principal, Subject};
    use gw_core::{Block, DocumentType, FieldKind, Visibility};

    fn dreq(parent: Option<&str>, template: Option<&str>) -> CreateRequest {
        CreateRequest {
            parent: parent.map(Into::into),
            title: "Tabelle".into(),
            slug: None,
            template: template.map(Into::into),
        }
    }

    async fn room(store: &Store, slug: &str) {
        let body: Block = serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap();
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: slug.into(),
                    slug: Some(slug.into()),
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn creating_a_dataset_needs_write_and_hides_unreadable_parents() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        room(&store, "raum").await;
        room(&store, "lesen").await;
        room(&store, "privat").await;
        let anna = Principal::test("anna", &[], &[]);
        for (path, perm) in [("/raum", Permission::Write), ("/lesen", Permission::Read)] {
            store
                .add_grant(path, Subject::Principal(anna.id.clone()), perm)
                .await
                .unwrap();
        }
        // Happy path: a page of type dataset with an empty schema.
        let out = store
            .create_dataset_for(&anna, &dreq(Some("/raum"), None), false, "d")
            .await
            .unwrap();
        let CreateOutcome::Created { path, id } = out else {
            panic!("{out:?}")
        };
        let made = store
            .document_for(&anna, &path, Action::Read)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(made.doc_type, "dataset");
        assert_eq!(made.visibility, "restricted");
        let fields: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dataset_field WHERE doc_id = ?")
            .bind(&id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(fields, 0);
        // Read-only parent: told why. Unreadable parent: same words as an absent one.
        assert_eq!(
            store
                .create_dataset_for(&anna, &dreq(Some("/lesen"), None), false, "d")
                .await
                .unwrap(),
            CreateOutcome::Blocked("you may not add pages under /lesen".into())
        );
        let hidden = store
            .create_dataset_for(&anna, &dreq(Some("/privat"), None), false, "d")
            .await
            .unwrap();
        let absent = store
            .create_dataset_for(&anna, &dreq(Some("/nichts"), None), false, "d")
            .await
            .unwrap();
        assert_eq!(
            hidden,
            CreateOutcome::Blocked("there is no page at /privat".into())
        );
        assert_eq!(
            absent,
            CreateOutcome::Blocked("there is no page at /nichts".into())
        );
        // Anonymous refused; a template makes no sense for a dataset.
        assert_eq!(
            store
                .create_dataset_for(
                    &Principal::anonymous(),
                    &dreq(Some("/raum"), None),
                    false,
                    "d"
                )
                .await
                .unwrap(),
            CreateOutcome::Refused
        );
        assert!(matches!(
            store
                .create_dataset_for(&anna, &dreq(Some("/raum"), Some("/vorlagen/x")), false, "d")
                .await
                .unwrap(),
            CreateOutcome::Blocked(_)
        ));
    }

    async fn dataset_doc(store: &Store) -> String {
        let body: Block = serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap();
        let doc = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Dataset,
                    title: "Tabelle".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
        doc
    }

    async fn add_field(store: &Store, doc: &str, key: &str, kind: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO dataset_field (id, doc_id, key, label, kind, position) \
             VALUES (?, ?, ?, ?, ?, 0)",
        )
        .bind(format!("{doc}-{key}"))
        .bind(doc)
        .bind(key)
        .bind(key)
        .bind(kind)
        .execute(&store.pool)
        .await
        .map(|_| ())
    }

    #[tokio::test]
    async fn field_key_is_unique_per_dataset() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let doc = dataset_doc(&store).await;
        add_field(&store, &doc, "name", "text").await.unwrap();
        assert!(add_field(&store, &doc, "name", "number").await.is_err());
    }

    #[tokio::test]
    async fn every_field_kind_is_accepted_and_unknown_refused() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let doc = dataset_doc(&store).await;
        for k in FieldKind::ALL {
            add_field(&store, &doc, k.as_str(), k.as_str())
                .await
                .unwrap_or_else(|e| panic!("{} refused: {e}", k.as_str()));
        }
        assert!(add_field(&store, &doc, "bogus", "nonsense").await.is_err());
    }

    #[tokio::test]
    async fn purging_the_page_cascades_to_fields_and_rows() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let doc = dataset_doc(&store).await;
        add_field(&store, &doc, "name", "text").await.unwrap();
        sqlx::query("INSERT INTO dataset_row (id, doc_id, \"values\") VALUES ('r1', ?, '{}')")
            .bind(&doc)
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM documents WHERE id = ?")
            .bind(&doc)
            .execute(&store.pool)
            .await
            .unwrap();
        for t in ["dataset_field", "dataset_row"] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {t}"))
                .fetch_one(&store.pool)
                .await
                .unwrap();
            assert_eq!(n, 0, "{t} survived its page");
        }
    }

    use super::{NewField, SchemaOutcome};
    use serde_json::json;

    /// `/raum/tabelle`: anna writes, bea only reads, cleo holds nothing.
    async fn schema_fixture() -> (Store, Principal, Principal, Principal, String) {
        let store = Store::open("sqlite::memory:").await.unwrap();
        room(&store, "raum").await;
        let anna = Principal::test("anna", &[], &[]);
        let bea = Principal::test("bea", &[], &[]);
        let cleo = Principal::test("cleo", &[], &[]);
        for (who, perm) in [(&anna, Permission::Write), (&bea, Permission::Read)] {
            store
                .add_grant("/raum", Subject::Principal(who.id.clone()), perm)
                .await
                .unwrap();
        }
        let out = store
            .create_dataset_for(&anna, &dreq(Some("/raum"), None), false, "d")
            .await
            .unwrap();
        let CreateOutcome::Created { path, .. } = out else {
            panic!("{out:?}")
        };
        (store, anna, bea, cleo, path)
    }

    fn nf(key: &str, kind: FieldKind) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind,
            config: json!({}),
        }
    }

    async fn keys(store: &Store, who: &Principal, path: &str) -> Vec<String> {
        let SchemaOutcome::Done(f) = store.dataset_fields(who, path).await.unwrap() else {
            panic!("no schema")
        };
        f.into_iter().map(|f| f.key).collect()
    }

    fn order(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[tokio::test]
    async fn fields_are_added_in_order_with_their_kind() {
        let (store, anna, _, _, path) = schema_fixture().await;
        for (k, kind) in [("name", FieldKind::Text), ("alter", FieldKind::Number)] {
            let out = store
                .add_dataset_field(&anna, &path, &nf(k, kind))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Done(_)), "{out:?}");
        }
        let SchemaOutcome::Done(f) = store.dataset_fields(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].key.as_str(), f[0].kind.as_str()), ("name", "text"));
        assert_eq!((f[1].key.as_str(), f[1].kind.as_str()), ("alter", "number"));
        assert_eq!(f[0].label, "NAME");
        assert!(f[0].position < f[1].position);
    }

    #[tokio::test]
    async fn bad_keys_kinds_labels_and_duplicates_are_refused() {
        let (store, anna, _, _, path) = schema_fixture().await;
        store
            .add_dataset_field(&anna, &path, &nf("name", FieldKind::Text))
            .await
            .unwrap();
        for bad in ["a') OR 1=1 --", "Name", "id", "", "1x"] {
            let out = store
                .add_dataset_field(&anna, &path, &nf(bad, FieldKind::Text))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{bad}: {out:?}");
        }
        let dup = store
            .add_dataset_field(&anna, &path, &nf("name", FieldKind::Number))
            .await
            .unwrap();
        assert!(matches!(dup, SchemaOutcome::Conflict(_)), "{dup:?}");
        // A rollup with no config, a relation without a target and `formula` with an empty config
        // are each refused by their own shape check; `person`/`file` are accepted since M8b.
        for kind in [FieldKind::Rollup, FieldKind::Relation] {
            let out = store
                .add_dataset_field(&anna, &path, &nf("x", kind))
                .await
                .unwrap();
            assert!(
                matches!(out, SchemaOutcome::Invalid(_)),
                "{kind:?}: {out:?}"
            );
        }
        let mut blank = nf("y", FieldKind::Text);
        blank.label = "   ".into();
        assert!(matches!(
            store.add_dataset_field(&anna, &path, &blank).await.unwrap(),
            SchemaOutcome::Invalid(_)
        ));
        blank.label = "x".repeat(121);
        assert!(matches!(
            store.add_dataset_field(&anna, &path, &blank).await.unwrap(),
            SchemaOutcome::Invalid(_)
        ));
        let mut cfg = nf("z", FieldKind::Text);
        cfg.config = json!([1]);
        assert!(matches!(
            store.add_dataset_field(&anna, &path, &cfg).await.unwrap(),
            SchemaOutcome::Invalid(_)
        ));
        assert_eq!(keys(&store, &anna, &path).await, ["name"]);
    }

    #[tokio::test]
    async fn the_hundred_and_first_field_is_refused() {
        let (store, anna, _, _, path) = schema_fixture().await;
        for i in 0..100 {
            let out = store
                .add_dataset_field(&anna, &path, &nf(&format!("f{i}"), FieldKind::Text))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Done(_)), "{i}: {out:?}");
        }
        let out = store
            .add_dataset_field(&anna, &path, &nf("extra", FieldKind::Text))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
        assert_eq!(keys(&store, &anna, &path).await.len(), 100);
    }

    #[tokio::test]
    async fn renaming_changes_the_label_only() {
        let (store, anna, _, _, path) = schema_fixture().await;
        store
            .add_dataset_field(&anna, &path, &nf("name", FieldKind::Text))
            .await
            .unwrap();
        let out = store
            .rename_dataset_field(&anna, &path, "name", "  Vorname ")
            .await
            .unwrap();
        let SchemaOutcome::Done(f) = out else {
            panic!("{out:?}")
        };
        assert_eq!(
            (f.key.as_str(), f.label.as_str(), f.kind.as_str()),
            ("name", "Vorname", "text")
        );
        assert_eq!(
            store
                .rename_dataset_field(&anna, &path, "nope", "x")
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
        assert!(matches!(
            store
                .rename_dataset_field(&anna, &path, "name", " ")
                .await
                .unwrap(),
            SchemaOutcome::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn reordering_takes_exactly_the_existing_keys() {
        let (store, anna, _, _, path) = schema_fixture().await;
        for k in ["a", "b", "c"] {
            store
                .add_dataset_field(&anna, &path, &nf(k, FieldKind::Text))
                .await
                .unwrap();
        }
        let out = store
            .reorder_dataset_fields(&anna, &path, &order(&["c", "a", "b"]))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Done(_)), "{out:?}");
        assert_eq!(keys(&store, &anna, &path).await, ["c", "a", "b"]);
        for bad in [
            &["a", "b"][..],
            &["a", "b", "c", "d"],
            &["a", "a", "b"],
            &["a", "b", "x"],
        ] {
            let out = store
                .reorder_dataset_fields(&anna, &path, &order(bad))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{bad:?}: {out:?}");
        }
        assert_eq!(keys(&store, &anna, &path).await, ["c", "a", "b"]);
    }

    #[tokio::test]
    async fn deleting_a_field_strips_its_key_from_every_row_only() {
        let (store, anna, _, _, path) = schema_fixture().await;
        for k in ["name", "alter"] {
            store
                .add_dataset_field(&anna, &path, &nf(k, FieldKind::Text))
                .await
                .unwrap();
        }
        let doc = store
            .document_for(&anna, &path, Action::Read)
            .await
            .unwrap()
            .unwrap()
            .id;
        for (id, v) in [
            ("r1", r#"{"name":"Ada","alter":"36"}"#),
            ("r2", r#"{"alter":"41"}"#),
            ("r3", r#"{}"#),
        ] {
            sqlx::query("INSERT INTO dataset_row (id, doc_id, \"values\") VALUES (?, ?, ?)")
                .bind(id)
                .bind(&doc)
                .bind(v)
                .execute(&store.pool)
                .await
                .unwrap();
        }
        let out = store
            .delete_dataset_field(&anna, &path, "alter")
            .await
            .unwrap();
        assert_eq!(out, SchemaOutcome::Done(()));
        assert_eq!(keys(&store, &anna, &path).await, ["name"]);
        let rows: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT id, \"values\", version FROM dataset_row ORDER BY id")
                .fetch_all(&store.pool)
                .await
                .unwrap();
        assert_eq!(rows[0].1, r#"{"name":"Ada"}"#);
        assert_eq!(rows[1].1, "{}");
        assert_eq!(rows[2].1, "{}");
        // Touched rows move on; the untouched one does not.
        assert_eq!((rows[0].2, rows[1].2, rows[2].2), (2, 2, 1));
        assert_eq!(
            store
                .delete_dataset_field(&anna, &path, "alter")
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
        // The key is free again, and a re-added field starts empty everywhere.
        store
            .add_dataset_field(&anna, &path, &nf("alter", FieldKind::Number))
            .await
            .unwrap();
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM dataset_row WHERE json_type(\"values\", '$.alter') IS NOT NULL",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn schema_writes_need_write_and_unreadable_looks_absent() {
        let (store, anna, bea, cleo, path) = schema_fixture().await;
        store
            .add_dataset_field(&anna, &path, &nf("name", FieldKind::Text))
            .await
            .unwrap();
        // A reader sees the schema but changes nothing.
        assert_eq!(keys(&store, &bea, &path).await, ["name"]);
        assert_eq!(
            store
                .add_dataset_field(&bea, &path, &nf("x", FieldKind::Text))
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .rename_dataset_field(&bea, &path, "name", "Z")
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .reorder_dataset_fields(&bea, &path, &order(&["name"]))
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .delete_dataset_field(&bea, &path, "name")
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(keys(&store, &anna, &path).await, ["name"]);
        // A stranger and an anonymous caller: the uniform absence.
        for who in [&cleo, &Principal::anonymous()] {
            assert_eq!(
                store.dataset_fields(who, &path).await.unwrap(),
                SchemaOutcome::NoDataset
            );
            assert_eq!(
                store
                    .add_dataset_field(who, &path, &nf("x", FieldKind::Text))
                    .await
                    .unwrap(),
                SchemaOutcome::NoDataset
            );
            assert_eq!(
                store
                    .delete_dataset_field(who, &path, "name")
                    .await
                    .unwrap(),
                SchemaOutcome::NoDataset
            );
        }
        // Identical to a path that does not exist; a plain page is not a dataset.
        assert_eq!(
            store.dataset_fields(&anna, "/raum/nichts").await.unwrap(),
            SchemaOutcome::NoDataset
        );
        assert_eq!(
            store.dataset_fields(&anna, "/raum").await.unwrap(),
            SchemaOutcome::NoDataset
        );
    }

    // --- row CRUD (A6) ---------------------------------------------------------------

    use super::{DatasetRow, RowOutcome, MAX_ROWS};
    use serde_json::Map;

    fn vals(v: serde_json::Value) -> Map<String, serde_json::Value> {
        v.as_object().unwrap().clone()
    }

    async fn row_fixture() -> (Store, Principal, Principal, Principal, String) {
        let (store, anna, bea, cleo, path) = schema_fixture().await;
        for (k, kind) in [("name", FieldKind::Text), ("alter", FieldKind::Number)] {
            store
                .add_dataset_field(&anna, &path, &nf(k, kind))
                .await
                .unwrap();
        }
        (store, anna, bea, cleo, path)
    }

    async fn made(store: &Store, who: &Principal, path: &str, v: serde_json::Value) -> DatasetRow {
        match store.create_dataset_row(who, path, &vals(v)).await.unwrap() {
            RowOutcome::Done(r) => r,
            other => panic!("{other:?}"),
        }
    }

    async fn doc_id_of(store: &Store, path: &str) -> String {
        sqlx::query_scalar("SELECT id FROM documents WHERE path = ?")
            .bind(path)
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_validated_row_is_created_read_updated_and_deleted() {
        let (store, anna, _, _, path) = row_fixture().await;
        let r = made(&store, &anna, &path, json!({"name": "Ada", "alter": 36})).await;
        assert_eq!(r.version, 1);
        assert_eq!(r.values, json!({"name": "Ada", "alter": 36}));
        // Wrong type and unknown key: refused, nothing stored.
        for bad in [json!({"alter": "viel"}), json!({"nope": 1})] {
            assert!(matches!(
                store
                    .create_dataset_row(&anna, &path, &vals(bad))
                    .await
                    .unwrap(),
                RowOutcome::Invalid(_)
            ));
        }
        // Update merges, null clears, version moves on.
        let RowOutcome::Done(u) = store
            .update_dataset_row(
                &anna,
                &path,
                &r.id,
                1,
                &vals(json!({"alter": null, "name": "Eva"})),
            )
            .await
            .unwrap()
        else {
            panic!("update")
        };
        assert_eq!(u.version, 2);
        assert_eq!(u.values, json!({"name": "Eva"}));
        let RowOutcome::Done(got) = store.dataset_row(&anna, &path, &r.id).await.unwrap() else {
            panic!("get")
        };
        assert_eq!(got, u);
        // An unknown key on update is refused and leaves the row alone.
        assert!(matches!(
            store
                .update_dataset_row(&anna, &path, &r.id, 2, &vals(json!({"nope": 1})))
                .await
                .unwrap(),
            RowOutcome::Invalid(_)
        ));
        assert_eq!(
            store.delete_dataset_row(&anna, &path, &r.id).await.unwrap(),
            RowOutcome::Done(())
        );
        assert_eq!(
            store.delete_dataset_row(&anna, &path, &r.id).await.unwrap(),
            RowOutcome::NoRow
        );
        assert_eq!(
            store.dataset_row(&anna, &path, &r.id).await.unwrap(),
            RowOutcome::NoRow
        );
    }

    #[tokio::test]
    async fn a_stale_version_conflicts_and_returns_the_current_row() {
        let (store, anna, _, _, path) = row_fixture().await;
        let r = made(&store, &anna, &path, json!({"name": "Ada"})).await;
        let RowOutcome::Done(v2) = store
            .update_dataset_row(&anna, &path, &r.id, 1, &vals(json!({"name": "Bo"})))
            .await
            .unwrap()
        else {
            panic!("first update")
        };
        let out = store
            .update_dataset_row(&anna, &path, &r.id, 1, &vals(json!({"name": "Cy"})))
            .await
            .unwrap();
        assert_eq!(out, RowOutcome::Stale(v2));
        let RowOutcome::Done(now) = store.dataset_row(&anna, &path, &r.id).await.unwrap() else {
            panic!("get")
        };
        assert_eq!(now.values, json!({"name": "Bo"}));
    }

    #[tokio::test]
    async fn the_row_cap_refuses_the_next_row() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2) \
             INSERT INTO dataset_row (id, doc_id) SELECT 'r' || i, ?1 FROM n",
        )
        .bind(&doc)
        .bind(MAX_ROWS as i64)
        .execute(&store.pool)
        .await
        .unwrap();
        assert!(matches!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"name": "zu viel"})))
                .await
                .unwrap(),
            RowOutcome::Full(_)
        ));
        // Deleting one makes room for exactly one.
        store.delete_dataset_row(&anna, &path, "r1").await.unwrap();
        made(&store, &anna, &path, json!({"name": "passt"})).await;
    }

    #[tokio::test]
    async fn a_relation_column_planted_without_a_target_refuses_every_write() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        // A relation column that names no target can only be planted underneath the API, which
        // always resolves one. Writing through it is refused rather than left to point nowhere.
        add_field(&store, &doc, "mit", "relation").await.unwrap();
        assert!(matches!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"mit": "r1"})))
                .await
                .unwrap(),
            RowOutcome::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn the_row_list_counts_what_the_caller_may_see_and_hides_what_they_may_not() {
        let (store, anna, bea, cleo, path) = row_fixture().await;
        for n in 0..3 {
            made(&store, &anna, &path, json!({ "name": format!("r{n}") })).await;
        }
        // A reader gets a page and the whole count; the limit does not shrink the count.
        let RowOutcome::Done(page) = store.dataset_rows(&bea, &path, 2, 0).await.unwrap() else {
            panic!("a reader lists rows");
        };
        assert_eq!((page.rows.len(), page.total), (2, 3));
        let RowOutcome::Done(rest) = store.dataset_rows(&bea, &path, 2, 2).await.unwrap() else {
            panic!("second page");
        };
        assert_eq!((rest.rows.len(), rest.total), (1, 3));
        // A stranger learns neither rows nor count: the same answer as for an absent page.
        assert_eq!(
            store.dataset_rows(&cleo, &path, 2, 0).await.unwrap(),
            RowOutcome::NoDataset
        );
        assert_eq!(
            store
                .dataset_rows(&cleo, "/nirgends/nichts", 2, 0)
                .await
                .unwrap(),
            RowOutcome::NoDataset
        );
    }

    #[tokio::test]
    async fn row_writes_need_write_and_unreadable_looks_absent() {
        let (store, anna, bea, cleo, path) = row_fixture().await;
        let r = made(&store, &anna, &path, json!({"name": "Ada"})).await;
        // Reader: reads, cannot write.
        assert!(matches!(
            store.dataset_row(&bea, &path, &r.id).await.unwrap(),
            RowOutcome::Done(_)
        ));
        assert_eq!(
            store
                .create_dataset_row(&bea, &path, &vals(json!({"name": "x"})))
                .await
                .unwrap(),
            RowOutcome::ReadOnly
        );
        assert_eq!(
            store
                .update_dataset_row(&bea, &path, &r.id, 1, &vals(json!({"name": "x"})))
                .await
                .unwrap(),
            RowOutcome::ReadOnly
        );
        assert_eq!(
            store.delete_dataset_row(&bea, &path, &r.id).await.unwrap(),
            RowOutcome::ReadOnly
        );
        // Stranger, anonymous: the uniform absence.
        for who in [&cleo, &Principal::anonymous()] {
            assert_eq!(
                store.dataset_row(who, &path, &r.id).await.unwrap(),
                RowOutcome::NoDataset
            );
            assert_eq!(
                store
                    .create_dataset_row(who, &path, &vals(json!({})))
                    .await
                    .unwrap(),
                RowOutcome::NoDataset
            );
            assert_eq!(
                store.delete_dataset_row(who, &path, &r.id).await.unwrap(),
                RowOutcome::NoDataset
            );
        }
        // A plain page and a missing path look the same.
        for p in ["/raum", "/raum/nichts"] {
            assert_eq!(
                store.dataset_row(&anna, p, &r.id).await.unwrap(),
                RowOutcome::NoDataset
            );
        }
        assert_eq!(
            store.dataset_row(&anna, &path, "fremd").await.unwrap(),
            RowOutcome::NoRow
        );
    }

    #[tokio::test]
    async fn row_events_are_recorded_and_withheld_once_access_goes() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        room(&store, "raum").await;
        let mut who = Vec::new();
        for u in ["anna", "bea"] {
            let p = store
                .create_local_principal(u, u, None, "$argon2id$fake")
                .await
                .unwrap();
            store
                .add_grant("/raum", Subject::Principal(p.id.clone()), Permission::Write)
                .await
                .unwrap();
            who.push(p);
        }
        let (anna, bea) = (&who[0], &who[1]);
        let CreateOutcome::Created { path, .. } = store
            .create_dataset_for(anna, &dreq(Some("/raum"), None), false, "d")
            .await
            .unwrap()
        else {
            panic!("create")
        };
        store
            .add_dataset_field(anna, &path, &nf("name", FieldKind::Text))
            .await
            .unwrap();
        let r = made(&store, bea, &path, json!({"name": "Ada"})).await;
        store
            .update_dataset_row(bea, &path, &r.id, 1, &vals(json!({"name": "Bo"})))
            .await
            .unwrap();
        store.delete_dataset_row(bea, &path, &r.id).await.unwrap();
        // The dataset's author hears of it; the actor does not hear of themselves.
        let inbox = store.notifications_for(anna, 10).await.unwrap();
        assert!(!inbox.is_empty());
        assert!(inbox
            .iter()
            .all(|n| n.kind.as_str().starts_with("dataset.row.") && n.page.path == path));
        assert!(store.notifications_for(bea, 10).await.unwrap().is_empty());
        // No cell text rides on the event row.
        let stored: Vec<String> =
            sqlx::query_scalar("SELECT COALESCE(subject, '') || COALESCE(path, '') FROM events")
                .fetch_all(&store.pool)
                .await
                .unwrap();
        assert!(stored
            .iter()
            .all(|s| !s.contains("Ada") && !s.contains("Bo")));
        // Delivery re-asks: take anna's access away and her inbox empties.
        store
            .remove_grant(
                "/raum",
                &Subject::Principal(anna.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        assert!(store.notifications_for(anna, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn racing_creates_at_the_cap_admit_exactly_one() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2) \
             INSERT INTO dataset_row (id, doc_id) SELECT 'r' || i, ?1 FROM n",
        )
        .bind(&doc)
        .bind(MAX_ROWS as i64 - 1)
        .execute(&store.pool)
        .await
        .unwrap();
        let (va, vb) = (vals(json!({"name": "a"})), vals(json!({"name": "b"})));
        let (a, b) = tokio::join!(
            store.create_dataset_row(&anna, &path, &va),
            store.create_dataset_row(&anna, &path, &vb)
        );
        let done = [a, b]
            .into_iter()
            .filter(|r| matches!(r, Ok(RowOutcome::Done(_))))
            .count();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dataset_row WHERE doc_id = ?")
            .bind(&doc)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!((done, n as usize), (1, MAX_ROWS));
    }

    #[tokio::test]
    async fn racing_field_adds_at_the_cap_admit_exactly_one() {
        let (store, anna, _, _, path) = schema_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?2) \
             INSERT INTO dataset_field (id, doc_id, key, label, kind, config, position) \
             SELECT 'f' || i, ?1, 'k' || i, 'L', 'text', '{}', i FROM n",
        )
        .bind(&doc)
        .bind(super::MAX_FIELDS as i64 - 1)
        .execute(&store.pool)
        .await
        .unwrap();
        let (fa, fb) = (nf("ra", FieldKind::Text), nf("rb", FieldKind::Text));
        let (a, b) = tokio::join!(
            store.add_dataset_field(&anna, &path, &fa),
            store.add_dataset_field(&anna, &path, &fb)
        );
        let done = [a, b]
            .into_iter()
            .filter(|r| matches!(r, Ok(SchemaOutcome::Done(_))))
            .count();
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dataset_field WHERE doc_id = ?")
            .bind(&doc)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!((done, n as usize), (1, super::MAX_FIELDS));
    }

    #[tokio::test]
    async fn an_oversized_row_is_refused_on_create() {
        let (store, anna, _, _, path) = row_fixture().await;
        let big = "x".repeat(gw_core::dataset::MAX_ROW_BYTES);
        assert!(matches!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"name": big})))
                .await
                .unwrap(),
            RowOutcome::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn the_actor_who_revised_the_page_does_not_hear_of_their_own_row() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        let authors: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM revisions WHERE document_id = ? AND author_id = ?",
        )
        .bind(&doc)
        .bind(&anna.id)
        .fetch_one(&store.pool)
        .await
        .unwrap();
        assert!(authors > 0, "fixture: anna must have revised the page");
        made(&store, &anna, &path, json!({"name": "Ada"})).await;
        let to_actor: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE recipient = ?")
            .bind(&anna.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(to_actor, 0);
    }

    use super::{Filter, RowPage, RowSelect};

    async fn select_fixture() -> (Store, Principal, Principal, String) {
        let (store, anna, _bea, cleo, path) = schema_fixture().await;
        for (k, kind) in [
            ("title", FieldKind::Text),
            ("code", FieldKind::Text),
            ("amount", FieldKind::Number),
        ] {
            store
                .add_dataset_field(&anna, &path, &nf(k, kind))
                .await
                .unwrap();
        }
        for (t, c, a) in [
            ("Apple pie", "9", 9),
            ("banana", "10", 10),
            ("Cherry", "100", 100),
            ("", "7", 50),
            ("apple tart", "8", 2),
        ] {
            let mut v = json!({"code": c, "amount": a});
            if !t.is_empty() {
                v["title"] = json!(t);
            }
            store
                .create_dataset_row(&anna, &path, v.as_object().unwrap())
                .await
                .unwrap();
        }
        (store, anna, cleo, path)
    }

    fn k(s: &str) -> gw_core::dataset::FieldKey {
        gw_core::dataset::FieldKey::parse(s).unwrap()
    }

    async fn select(
        store: &Store,
        who: &Principal,
        path: &str,
        sel: RowSelect,
    ) -> RowOutcome<RowPage> {
        store.dataset_rows_query(who, path, &sel).await.unwrap()
    }

    fn done(o: RowOutcome<RowPage>) -> RowPage {
        let RowOutcome::Done(p) = o else {
            panic!("{o:?}")
        };
        p
    }

    fn col(p: &RowPage, key: &str) -> Vec<String> {
        p.rows
            .iter()
            .map(|r| r.values.get(key).map(|v| v.to_string()).unwrap_or_default())
            .collect()
    }

    #[tokio::test]
    async fn filters_match_eq_contains_is_empty_and_in() {
        let (store, anna, _cleo, path) = select_fixture().await;
        let run = |f: Filter| {
            let (store, anna, path) = (&store, &anna, &path);
            async move {
                done(
                    select(
                        store,
                        anna,
                        path,
                        RowSelect {
                            filters: vec![f],
                            limit: 50,
                            sort: Some((k("amount"), false)),
                            ..Default::default()
                        },
                    )
                    .await,
                )
            }
        };
        let p = run(Filter::Eq {
            key: k("amount"),
            value: json!(10),
        })
        .await;
        assert_eq!((col(&p, "amount"), p.total), (vec!["10".into()], 1));
        let p = run(Filter::Eq {
            key: k("code"),
            value: json!("9"),
        })
        .await;
        assert_eq!(col(&p, "amount"), vec!["9"]);
        // Case-insensitive substring; `%` is not a wildcard.
        let p = run(Filter::Contains {
            key: k("title"),
            value: "APPLE".into(),
        })
        .await;
        assert_eq!(col(&p, "amount"), vec!["2", "9"]);
        let p = run(Filter::Contains {
            key: k("title"),
            value: "%".into(),
        })
        .await;
        assert_eq!(p.rows.len(), 0);
        let p = run(Filter::IsEmpty { key: k("title") }).await;
        assert_eq!((col(&p, "amount"), p.total), (vec!["50".into()], 1));
        let p = run(Filter::In {
            key: k("amount"),
            values: vec![json!(100), json!(2), json!(3)],
        })
        .await;
        assert_eq!(
            (col(&p, "amount"), p.total),
            (vec!["2".into(), "100".into()], 2)
        );
    }

    #[tokio::test]
    async fn sort_is_typed_ten_follows_nine_for_numbers_and_precedes_it_as_text() {
        let (store, anna, _cleo, path) = select_fixture().await;
        let sorted = |key: &str, desc: bool| {
            let (store, anna, path, key) = (&store, &anna, &path, k(key));
            async move {
                done(
                    select(
                        store,
                        anna,
                        path,
                        RowSelect {
                            sort: Some((key, desc)),
                            limit: 50,
                            ..Default::default()
                        },
                    )
                    .await,
                )
            }
        };
        assert_eq!(
            col(&sorted("amount", false).await, "amount"),
            ["2", "9", "10", "50", "100"]
        );
        assert_eq!(
            col(&sorted("amount", true).await, "amount"),
            ["100", "50", "10", "9", "2"]
        );
        // The same digits in a text field compare as text.
        assert_eq!(
            col(&sorted("code", false).await, "code"),
            ["\"10\"", "\"100\"", "\"7\"", "\"8\"", "\"9\""]
        );
        // A row without a value sorts last either way.
        assert_eq!(
            sorted("title", false).await.rows[4].values.get("title"),
            None
        );
        assert_eq!(
            sorted("title", true).await.rows[4].values.get("title"),
            None
        );
    }

    #[tokio::test]
    async fn a_cursor_pages_without_repeats_or_gaps_while_rows_are_inserted() {
        let (store, anna, _cleo, path) = select_fixture().await;
        for (key, desc) in [("amount", false), ("amount", true), ("title", false)] {
            let mut seen: Vec<String> = Vec::new();
            let mut after = None;
            let mut inserted = false;
            loop {
                let p = done(
                    select(
                        &store,
                        &anna,
                        &path,
                        RowSelect {
                            sort: Some((k(key), desc)),
                            limit: 2,
                            after,
                            ..Default::default()
                        },
                    )
                    .await,
                );
                seen.extend(p.rows.iter().map(|r| r.id.clone()));
                if !inserted {
                    // Between pages a row arrives that sorts before the cursor.
                    let v = json!({"amount": if desc { 1000 } else { -5 }, "title": "!!"});
                    store
                        .create_dataset_row(&anna, &path, v.as_object().unwrap())
                        .await
                        .unwrap();
                    inserted = true;
                }
                match p.next {
                    Some(n) => after = Some(n),
                    None => break,
                }
            }
            let unique: std::collections::HashSet<_> = seen.iter().collect();
            assert_eq!(unique.len(), seen.len(), "{key} {desc}: a row repeated");
            // Five original rows seen; the newcomer, behind the cursor, was not (title
            // sort: "!!" sorts before every letter, so also behind it).
            assert_eq!(seen.len(), 5, "{key} {desc}: {seen:?}");
            let all = done(
                select(
                    &store,
                    &anna,
                    &path,
                    RowSelect {
                        limit: 200,
                        ..Default::default()
                    },
                )
                .await,
            );
            for r in all
                .rows
                .iter()
                .filter(|r| r.values.get("title") == Some(&json!("!!")))
            {
                store.delete_dataset_row(&anna, &path, &r.id).await.unwrap();
            }
        }
    }

    #[tokio::test]
    async fn unknown_fields_bad_cursors_and_unreadable_datasets_are_refused() {
        let (store, anna, cleo, path) = select_fixture().await;
        let sel = |s: RowSelect| select(&store, &anna, &path, s);
        let unknown = sel(RowSelect {
            filters: vec![Filter::IsEmpty { key: k("nope") }],
            limit: 5,
            ..Default::default()
        })
        .await;
        assert!(matches!(unknown, RowOutcome::Invalid(_)), "{unknown:?}");
        let unknown = sel(RowSelect {
            sort: Some((k("nope"), false)),
            limit: 5,
            ..Default::default()
        })
        .await;
        assert!(matches!(unknown, RowOutcome::Invalid(_)));
        let forged = sel(RowSelect {
            after: Some("zz".into()),
            limit: 5,
            ..Default::default()
        })
        .await;
        assert!(matches!(forged, RowOutcome::Invalid(_)));
        // A stranger gets the uniform nothing, the same as for an absent path.
        let want = RowSelect {
            limit: 5,
            ..Default::default()
        };
        let gone = select(&store, &cleo, &path, want.clone()).await;
        assert_eq!(gone, RowOutcome::NoDataset);
        let gone = select(&store, &cleo, "/raum/absent", want).await;
        assert_eq!(gone, RowOutcome::NoDataset);
    }

    // --- formula fields (M8b) ------------------------------------------------------

    use super::MAX_FORMULA_EXPR_CHARS;
    use serde_json::Value;

    /// A column planted straight into the table, bypassing `add_dataset_field`: the only way to
    /// reach a kind the API refuses (a rollup) or to seed half a cycle underneath a write.
    async fn add_field_with(store: &Store, doc: &str, key: &str, kind: &str, config: Value) {
        sqlx::query(
            "INSERT INTO dataset_field (id, doc_id, key, label, kind, config, position) \
             VALUES (?, ?, ?, ?, ?, ?, 0)",
        )
        .bind(format!("{doc}-{key}"))
        .bind(doc)
        .bind(key)
        .bind(key)
        .bind(kind)
        .bind(config.to_string())
        .execute(&store.pool)
        .await
        .unwrap();
    }

    fn formula_field(key: &str, expr: &str) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind: FieldKind::Formula,
            config: json!({ "expr": expr }),
        }
    }

    fn plain_field(key: &str, kind: FieldKind) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind,
            config: json!({}),
        }
    }

    async fn read(store: &Store, who: &Principal, path: &str, id: &str) -> Value {
        let RowOutcome::Done(r) = store.dataset_row(who, path, id).await.unwrap() else {
            panic!("read")
        };
        r.values
    }

    fn has(keys: &[String], key: &str) -> bool {
        keys.iter().any(|k| k == key)
    }

    #[tokio::test]
    async fn a_formula_over_two_numbers_is_evaluated_when_the_row_is_read() {
        let (store, anna, _, _, path) = row_fixture().await;
        store
            .add_dataset_field(&anna, &path, &formula_field("twice", "{alter} * 2"))
            .await
            .unwrap();
        let r = made(&store, &anna, &path, json!({"name": "Ada", "alter": 21})).await;
        let v = read(&store, &anna, &path, &r.id).await;
        assert_eq!(v["twice"].as_f64(), Some(42.0));
        assert_eq!(v["name"], json!("Ada"));
        assert_eq!(v["alter"].as_f64(), Some(21.0));
    }

    #[tokio::test]
    async fn a_formula_that_divides_by_zero_leaves_an_error_cell_not_a_panic() {
        let (store, anna, _, _, path) = row_fixture().await;
        store
            .add_dataset_field(&anna, &path, &plain_field("zero", FieldKind::Number))
            .await
            .unwrap();
        store
            .add_dataset_field(&anna, &path, &formula_field("ratio", "1 / {zero}"))
            .await
            .unwrap();
        let r = made(&store, &anna, &path, json!({"zero": 0})).await;
        let v = read(&store, &anna, &path, &r.id).await;
        assert_eq!(v["ratio"], json!({ "error": "#ERR(divide by zero)" }));
    }

    #[tokio::test]
    async fn a_formula_cannot_read_itself() {
        let (store, anna, _, _, path) = row_fixture().await;
        let out = store
            .add_dataset_field(&anna, &path, &formula_field("loop", "{loop} + 1"))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
        assert!(!has(&keys(&store, &anna, &path).await, "loop"));
    }

    #[tokio::test]
    async fn two_formulas_reading_each_other_are_refused_as_a_cycle() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        // Seed a formula that reads `b`, which does not exist yet, underneath the API.
        add_field_with(&store, &doc, "a", "formula", json!({ "expr": "{b}" })).await;
        let out = store
            .add_dataset_field(&anna, &path, &formula_field("b", "{a}"))
            .await
            .unwrap();
        let SchemaOutcome::Invalid(msg) = out else {
            panic!("{out:?}")
        };
        assert_eq!(msg, "formula cycle: a -> b -> a");
        let ks = keys(&store, &anna, &path).await;
        assert!(has(&ks, "a") && !has(&ks, "b"), "{ks:?}");
    }

    #[tokio::test]
    async fn a_formula_may_read_a_rollup_column() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        add_field_with(&store, &doc, "total", "rollup", json!({})).await;
        let out = store
            .add_dataset_field(&anna, &path, &formula_field("shown", "{total} + 1"))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Done(_)), "{out:?}");
        // Seeded straight into storage, so the config names no relation: a rollup that cannot be
        // answered has no value to read, the formula reads null and the sum is an error cell.
        let r = made(&store, &anna, &path, json!({"name": "Ada"})).await;
        let v = read(&store, &anna, &path, &r.id).await;
        assert!(v["shown"].get("error").is_some(), "{:?}", v["shown"]);
    }

    #[tokio::test]
    async fn a_formula_naming_an_unknown_field_is_refused() {
        let (store, anna, _, _, path) = row_fixture().await;
        let out = store
            .add_dataset_field(&anna, &path, &formula_field("calc", "{nope} * 2"))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
        assert!(!has(&keys(&store, &anna, &path).await, "calc"));
    }

    #[tokio::test]
    async fn a_formula_reading_another_formula_uses_its_value_in_order() {
        let (store, anna, _, _, path) = row_fixture().await;
        store
            .add_dataset_field(&anna, &path, &formula_field("a", "{alter} + 1"))
            .await
            .unwrap();
        store
            .add_dataset_field(&anna, &path, &formula_field("b", "{a} * 10"))
            .await
            .unwrap();
        let r = made(&store, &anna, &path, json!({"name": "Ada", "alter": 4})).await;
        let v = read(&store, &anna, &path, &r.id).await;
        assert_eq!(v["a"].as_f64(), Some(5.0));
        assert_eq!(v["b"].as_f64(), Some(50.0));
    }

    #[tokio::test]
    async fn a_formula_result_is_never_written_into_the_stored_row() {
        let (store, anna, _, _, path) = row_fixture().await;
        store
            .add_dataset_field(&anna, &path, &formula_field("a", "{alter} + 1"))
            .await
            .unwrap();
        let r = made(&store, &anna, &path, json!({"name": "Ada", "alter": 4})).await;
        assert_eq!(
            read(&store, &anna, &path, &r.id).await["a"].as_f64(),
            Some(5.0)
        );
        let raw: String = sqlx::query_scalar("SELECT \"values\" FROM dataset_row WHERE id = ?")
            .bind(&r.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert!(raw.contains("alter"), "{raw}");
        assert!(
            !raw.contains("\"a\""),
            "computed key reached storage: {raw}"
        );
    }

    #[tokio::test]
    async fn a_value_cannot_be_written_to_a_formula_field() {
        let (store, anna, _, _, path) = row_fixture().await;
        store
            .add_dataset_field(&anna, &path, &formula_field("a", "{alter} + 1"))
            .await
            .unwrap();
        assert!(matches!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"a": 5})))
                .await
                .unwrap(),
            RowOutcome::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn filtering_or_sorting_a_formula_field_is_refused() {
        let (store, anna, _, _, path) = row_fixture().await;
        store
            .add_dataset_field(&anna, &path, &formula_field("a", "{alter} + 1"))
            .await
            .unwrap();
        let want = "computed field `a` cannot be filtered or sorted yet";
        let filt = store
            .dataset_rows_query(
                &anna,
                &path,
                &RowSelect {
                    filters: vec![Filter::Eq {
                        key: k("a"),
                        value: json!(5),
                    }],
                    limit: 5,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(filt, RowOutcome::Invalid(want.into()));
        let srt = store
            .dataset_rows_query(
                &anna,
                &path,
                &RowSelect {
                    sort: Some((k("a"), false)),
                    limit: 5,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(srt, RowOutcome::Invalid(want.into()));
    }

    #[tokio::test]
    async fn a_formula_config_must_be_exactly_one_expr_string() {
        let (store, anna, _, _, path) = row_fixture().await;
        let bads = [
            json!({}),                                               // no expr
            json!({"expr": 1}),                                      // not a string
            json!({"expr": "{alter}", "extra": 1}),                  // a second member
            json!({"other": "{alter}"}),                             // the wrong member
            json!({"expr": "x".repeat(MAX_FORMULA_EXPR_CHARS + 1)}), // too long
        ];
        for cfg in bads {
            let out = store
                .add_dataset_field(
                    &anna,
                    &path,
                    &NewField {
                        key: "bad".into(),
                        label: "B".into(),
                        kind: FieldKind::Formula,
                        config: cfg.clone(),
                    },
                )
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        assert!(matches!(
            store
                .add_dataset_field(&anna, &path, &formula_field("ok", "{alter}"))
                .await
                .unwrap(),
            SchemaOutcome::Done(_)
        ));
    }

    #[tokio::test]
    async fn adding_a_formula_field_still_needs_write() {
        let (store, anna, bea, _, path) = row_fixture().await;
        assert_eq!(
            store
                .add_dataset_field(&bea, &path, &formula_field("a", "{alter} + 1"))
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert!(!has(&keys(&store, &anna, &path).await, "a"));
    }

    // --- person and file fields (M8b B3) ---------------------------------------------

    use crate::{AttachOutcome, BlobOutcome, BlobStore, DetachOutcome};

    /// The two refusals, spelled out rather than imported from `links`: an error message is
    /// part of the behaviour, and renaming it quietly is exactly what a test should notice.
    const NO_PERSON: &str = "person is not available for this dataset";
    const NO_FILE: &str = "file is not an attachment of this dataset";

    fn person_field(key: &str, multiple: bool) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind: FieldKind::Person,
            config: json!({ "multiple": multiple }),
        }
    }

    fn file_field(key: &str) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind: FieldKind::File,
            config: json!({}),
        }
    }

    /// A text column to patch beside a person, a single-person column, a multi-person one and
    /// a file column.
    async fn link_fixture() -> (Store, Principal, Principal, String) {
        let (store, anna, bea, _cleo, path) = schema_fixture().await;
        for field in [
            plain_field("name", FieldKind::Text),
            person_field("wer", false),
            person_field("team", true),
            file_field("dok"),
        ] {
            let out = store.add_dataset_field(&anna, &path, &field).await.unwrap();
            assert!(matches!(out, SchemaOutcome::Done(_)), "{out:?}");
        }
        (store, anna, bea, path)
    }

    /// A real account, because a `person` cell is checked against `principals` and
    /// [`Principal::test`] names nobody there.
    async fn account(
        store: &Store,
        name: &str,
        permission: Permission,
        paths: &[&str],
    ) -> Principal {
        let who = store
            .create_local_principal(name, name, None, "$argon2id$fake")
            .await
            .unwrap();
        for path in paths {
            store
                .add_grant(path, Subject::Principal(who.id.clone()), permission)
                .await
                .unwrap();
        }
        who
    }

    /// A media mount that lives exactly as long as the test holds it.
    fn media() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(dir.path()).unwrap();
        (dir, blobs)
    }

    /// Bytes that have arrived and been typed, and the id a `file` cell would name.
    async fn attach_to(
        store: &Store,
        who: &Principal,
        path: &str,
        name: &str,
        blobs: &BlobStore,
    ) -> String {
        let mut writer = blobs.writer().unwrap();
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(name.as_bytes());
        writer.push(&bytes).await.unwrap();
        let pending = match writer.finish().await.unwrap() {
            BlobOutcome::Accepted(pending) => pending,
            other => panic!("these bytes must be acceptable, got {other:?}"),
        };
        match store.attach(who, path, name, pending).await.unwrap() {
            AttachOutcome::Done(_) => {}
            other => panic!("`{name}` must attach to {path}, got {other:?}"),
        }
        sqlx::query_scalar("SELECT id FROM attachments WHERE doc_id = ?1 AND filename = ?2")
            .bind(doc_id_of(store, path).await)
            .bind(name)
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_person_cell_is_written_only_for_somebody_who_could_read_this_page() {
        let (store, anna, bea, path) = link_fixture().await;
        let leser = account(&store, "leser", Permission::Read, &["/raum"]).await;
        let fremder = account(&store, "fremder", Permission::Read, &[]).await;

        let r = made(&store, &anna, &path, json!({"wer": leser.id.clone()})).await;
        assert_eq!(r.values["wer"], json!(leser.id));

        // Somebody who may not read this page is refused — and so is an id that names nobody,
        // in the same words, so a row write is not a way to ask whether an account exists.
        let unreadable = store
            .create_dataset_row(&anna, &path, &vals(json!({"wer": fremder.id.clone()})))
            .await
            .unwrap();
        let unknown = store
            .create_dataset_row(&anna, &path, &vals(json!({"wer": "es-gibt-sie-nicht"})))
            .await
            .unwrap();
        assert_eq!(unreadable, RowOutcome::Invalid(NO_PERSON.into()));
        assert_eq!(unknown, unreadable, "one answer for two different mistakes");
        // The same on the update path, with the row left exactly as it was.
        assert_eq!(
            store
                .update_dataset_row(
                    &anna,
                    &path,
                    &r.id,
                    1,
                    &vals(json!({"wer": fremder.id.clone()}))
                )
                .await
                .unwrap(),
            RowOutcome::Invalid(NO_PERSON.into())
        );
        assert_eq!(
            read(&store, &anna, &path, &r.id).await["wer"],
            json!(leser.id)
        );
        // A person who can read the page is accepted on both paths.
        let chef = account(&store, "chef", Permission::Read, &["/raum"]).await;
        let RowOutcome::Done(second) = store
            .create_dataset_row(&anna, &path, &vals(json!({"wer": chef.id.clone()})))
            .await
            .unwrap()
        else {
            panic!("a reader may be named")
        };
        assert_eq!(second.values["wer"], json!(chef.id));
        // The gate that was there before is still there: naming somebody who may read is not a
        // way to write a row you may not write.
        assert_eq!(
            store
                .create_dataset_row(&bea, &path, &vals(json!({"wer": leser.id.clone()})))
                .await
                .unwrap(),
            RowOutcome::ReadOnly
        );
    }

    #[tokio::test]
    async fn clearing_a_person_asks_nothing_of_the_person_it_drops() {
        let (store, anna, _, path) = link_fixture().await;
        let leser = account(&store, "leser", Permission::Read, &["/raum"]).await;
        let r = made(
            &store,
            &anna,
            &path,
            json!({"wer": leser.id.clone(), "team": [leser.id.clone()]}),
        )
        .await;
        // Access withdrawn after the write — the stored person can no longer read the page.
        store
            .remove_grant(
                "/raum",
                &Subject::Principal(leser.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();

        // A patch that does not mention the field never re-asks.
        assert!(matches!(
            store
                .update_dataset_row(&anna, &path, &r.id, 1, &vals(json!({"name": "Ada"})))
                .await
                .unwrap(),
            RowOutcome::Done(_)
        ));
        // Naming them again is now refused.
        assert_eq!(
            store
                .update_dataset_row(
                    &anna,
                    &path,
                    &r.id,
                    2,
                    &vals(json!({"wer": leser.id.clone()}))
                )
                .await
                .unwrap(),
            RowOutcome::Invalid(NO_PERSON.into())
        );
        // Clearing — by `null` and by an empty list — is not: it discloses nothing, and a page
        // that could not do this could never be tidied up after somebody left.
        let RowOutcome::Done(cleared) = store
            .update_dataset_row(
                &anna,
                &path,
                &r.id,
                2,
                &vals(json!({"wer": Value::Null, "team": []})),
            )
            .await
            .unwrap()
        else {
            panic!("a clear must always be allowed")
        };
        assert_eq!(cleared.values["wer"], Value::Null);
        assert_eq!(
            cleared.values["team"],
            json!([]),
            "an empty list is a clear, and is stored as empty"
        );
    }

    #[tokio::test]
    async fn a_multi_person_cell_dedupes_and_needs_read_of_every_name() {
        let (store, anna, _, path) = link_fixture().await;
        let leser = account(&store, "leser", Permission::Read, &["/raum"]).await;
        let chef = account(&store, "chef", Permission::Read, &["/raum"]).await;
        let fremder = account(&store, "fremder", Permission::Read, &[]).await;

        let r = made(
            &store,
            &anna,
            &path,
            json!({"team": [chef.id.clone(), leser.id.clone(), chef.id.clone()]}),
        )
        .await;
        assert_eq!(r.values["team"], json!([chef.id, leser.id]));
        // One unreadable name poisons the whole cell, in the same uniform words.
        assert_eq!(
            store
                .create_dataset_row(
                    &anna,
                    &path,
                    &vals(json!({"team": [chef.id.clone(), fremder.id.clone()]}))
                )
                .await
                .unwrap(),
            RowOutcome::Invalid(NO_PERSON.into())
        );
        // A single-person column takes one id, never a list.
        assert!(matches!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"wer": [chef.id.clone()]})))
                .await
                .unwrap(),
            RowOutcome::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn person_and_file_columns_take_only_their_own_config() {
        let (store, anna, _, _, path) = schema_fixture().await;
        for field in [
            person_field("wer", false),
            person_field("team", true),
            file_field("dok"),
        ] {
            let out = store.add_dataset_field(&anna, &path, &field).await.unwrap();
            assert!(matches!(out, SchemaOutcome::Done(_)), "{out:?}");
        }
        // An unknown or mistyped member is refused rather than ignored.
        for (field, bad) in [
            (person_field("x", false), json!({"multiple": "yes"})),
            (person_field("y", false), json!({"many": true})),
            (file_field("z"), json!({"multiple": true})),
        ] {
            let mut mistyped = field;
            mistyped.config = bad.clone();
            let out = store
                .add_dataset_field(&anna, &path, &mistyped)
                .await
                .unwrap();
            assert!(
                matches!(out, SchemaOutcome::Invalid(_)),
                "{}: {out:?}",
                mistyped.key
            );
            assert!(!has(&keys(&store, &anna, &path).await, &mistyped.key));
        }
        // A relation with no target and a rollup with no members are both refused: each by its own
        // shape check, which is exactly the check a typo would otherwise slip past.
        for kind in [FieldKind::Relation, FieldKind::Rollup] {
            let out = store
                .add_dataset_field(&anna, &path, &nf("later", kind))
                .await
                .unwrap();
            assert!(
                matches!(out, SchemaOutcome::Invalid(_)),
                "{kind:?}: {out:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_file_cell_is_one_of_this_page_s_attachments_and_reads_back_as_a_name() {
        let (store, anna, _, path) = link_fixture().await;
        let (_dir, blobs) = media();
        let CreateOutcome::Created { path: andere, .. } = store
            .create_dataset_for(
                &anna,
                &CreateRequest {
                    parent: Some("/raum".into()),
                    title: "Ablage".into(),
                    slug: None,
                    template: None,
                },
                false,
                "d",
            )
            .await
            .unwrap()
        else {
            panic!("a second dataset")
        };
        // A file that sits on another page is not this dataset's to name.
        let fremder = attach_to(&store, &anna, &andere, "fremd.png", &blobs).await;
        assert_eq!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"dok": fremder.clone()})))
                .await
                .unwrap(),
            RowOutcome::Invalid(NO_FILE.into())
        );
        assert_eq!(
            store
                .update_dataset_row(&anna, &path, "nichts", 1, &vals(json!({"dok": fremder})))
                .await
                .unwrap(),
            RowOutcome::Invalid(NO_FILE.into()),
            "the patch is refused before the row is even looked for"
        );

        let id = attach_to(&store, &anna, &path, "befund.png", &blobs).await;
        let on_page = json!({"id": id.clone(), "filename": "befund.png"});
        let r = made(&store, &anna, &path, json!({"dok": id.clone()})).await;
        assert_eq!(read(&store, &anna, &path, &r.id).await["dok"], on_page);
        // The list and the filtered query decorate the same way as the single read.
        for page in [
            match store.dataset_rows(&anna, &path, 10, 0).await.unwrap() {
                RowOutcome::Done(p) => p,
                other => panic!("{other:?}"),
            },
            match store
                .dataset_rows_query(
                    &anna,
                    &path,
                    &RowSelect {
                        limit: 10,
                        ..Default::default()
                    },
                )
                .await
                .unwrap()
            {
                RowOutcome::Done(p) => p,
                other => panic!("{other:?}"),
            },
        ] {
            assert_eq!(page.rows[0].values["dok"], on_page);
        }
        // The cell in storage stays the id: a filename is a read-time answer.
        let raw: String = sqlx::query_scalar("SELECT \"values\" FROM dataset_row WHERE id = ?")
            .bind(&r.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert!(raw.contains(&id) && !raw.contains("befund.png"), "{raw}");
        // A detach is ordinary and must not turn this row into an error.
        assert!(matches!(
            store.detach(&anna, &path, "befund.png").await.unwrap(),
            DetachOutcome::Done(_)
        ));
        assert_eq!(
            read(&store, &anna, &path, &r.id).await["dok"],
            json!({"id": id, "missing": true})
        );
    }

    // --- relation fields (M8b B4) ----------------------------------------------------

    /// The write-path refusal, spelled out here rather than imported: an unreadable target, a
    /// deleted one and an id that belongs to another dataset are one sentence (ADR 0022), and a
    /// test that renames that sentence quietly is exactly what should break.
    const NO_RELATION: &str = "relation target is not available";

    fn relation_field(key: &str, target: &str, multiple: bool) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind: FieldKind::Relation,
            config: json!({ "target": target, "multiple": multiple }),
        }
    }

    /// Two datasets one writer (`anna`) may change: `source` under `/raum`, which the reader
    /// `bea` may see, and a `target` under `/geheim`, which she may not. A relation on the source
    /// points at the target, so the very same column is visible to `anna` and invisible to `bea`
    /// for exactly one reason — the target.
    async fn relation_fixture() -> (Store, Principal, Principal, String, String) {
        let store = Store::open("sqlite::memory:").await.unwrap();
        room(&store, "raum").await;
        room(&store, "geheim").await;
        let anna = Principal::test("anna", &[], &[]);
        let bea = Principal::test("bea", &[], &[]);
        for (slug, who, perm) in [
            ("/raum", &anna, Permission::Write),
            ("/geheim", &anna, Permission::Write),
            ("/raum", &bea, Permission::Read),
        ] {
            store
                .add_grant(slug, Subject::Principal(who.id.clone()), perm)
                .await
                .unwrap();
        }
        let source = created_dataset(&store, &anna, "/raum", "d").await;
        let target = created_dataset(&store, &anna, "/geheim", "z").await;
        (store, anna, bea, source, target)
    }

    async fn created_dataset(store: &Store, who: &Principal, parent: &str, slug: &str) -> String {
        let out = store
            .create_dataset_for(who, &dreq(Some(parent), None), false, slug)
            .await
            .unwrap();
        let CreateOutcome::Created { path, .. } = out else {
            panic!("{out:?}")
        };
        path
    }

    /// A row of the target dataset, laid straight into storage: the id a source cell points at.
    async fn target_row(store: &Store, target: &str) -> String {
        let doc = doc_id_of(store, target).await;
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO dataset_row (id, doc_id, \"values\") VALUES (?, ?, '{}')")
            .bind(&id)
            .bind(&doc)
            .execute(&store.pool)
            .await
            .unwrap();
        id
    }

    /// The stored id of a column, for addressing the edge table directly.
    async fn field_id(store: &Store, path: &str, key: &str) -> String {
        sqlx::query_scalar("SELECT id FROM dataset_field WHERE doc_id = ?1 AND key = ?2")
            .bind(doc_id_of(store, path).await)
            .bind(key)
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_relation_column_needs_write_on_the_source_and_read_on_its_target() {
        let (store, anna, bea, source, target) = relation_fixture().await;
        // A writer who can read the target: the column lands, and keeps the target by id, not path.
        let SchemaOutcome::Done(f) = store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, false))
            .await
            .unwrap()
        else {
            panic!("anna may point at a target she can read")
        };
        assert!(f.config.get("target_doc").is_some());
        assert!(
            f.config.get("target").is_none(),
            "the caller's path is resolved away, never stored"
        );
        // A reader of the source may not change its schema, however valid the target.
        assert_eq!(
            store
                .add_dataset_field(&bea, &source, &relation_field("x", &target, false))
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        // A writer of the source who cannot read the target: an unreadable dataset, an absent path
        // and a plain page are one answer, so binding a column is not a way to survey what is there.
        let blind = Principal::test("blind", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(blind.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        for (path, want) in [
            (target.as_str(), format!("no dataset at {target}")),
            ("/geheim/nichts", "no dataset at /geheim/nichts".to_string()),
            ("/geheim", "no dataset at /geheim".to_string()),
        ] {
            let out = store
                .add_dataset_field(&blind, &source, &relation_field("c", path, false))
                .await
                .unwrap();
            assert_eq!(out, SchemaOutcome::Invalid(want), "target {path}");
        }
    }

    #[tokio::test]
    async fn a_relation_cell_must_name_a_row_of_its_own_target() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, false))
            .await
            .unwrap();
        let live = target_row(&store, &target).await;
        let ok = made(&store, &anna, &source, json!({"mit": live.clone()})).await;
        assert_eq!(
            read(&store, &anna, &source, &ok.id).await["mit"],
            json!(live)
        );
        // A row of the source dataset is not a row of the target; that and a made-up id are refused
        // in the same words, so a cell cannot confirm which dataset some row id belongs to.
        let foreign = made(&store, &anna, &source, json!({})).await;
        for id in [foreign.id.clone(), "es-gibt-sie-nicht".into()] {
            assert_eq!(
                store
                    .create_dataset_row(&anna, &source, &vals(json!({"mit": id})))
                    .await
                    .unwrap(),
                RowOutcome::Invalid(NO_RELATION.into()),
                "one answer for a foreign row and a stranger"
            );
        }
    }

    #[tokio::test]
    async fn naming_a_relation_you_cannot_see_is_the_same_refusal_as_a_bad_target() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, false))
            .await
            .unwrap();
        // A writer of the source whose read of the target has gone (never granted here).
        let blind = Principal::test("blind", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(blind.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        let live = target_row(&store, &target).await;
        assert_eq!(
            store
                .create_dataset_row(&blind, &source, &vals(json!({"mit": live.clone()})))
                .await
                .unwrap(),
            RowOutcome::Invalid(NO_RELATION.into())
        );
        // The column reads back as absent for this writer, exactly as for a plain reader.
        assert!(!has(&keys(&store, &blind, &source).await, "mit"));
        // anna, who can see the target, writes the cell; blind may still clear it — removing a
        // name asks nothing of the target it drops.
        let r = made(&store, &anna, &source, json!({"mit": live.clone()})).await;
        assert!(
            matches!(
                store
                    .update_dataset_row(
                        &blind,
                        &source,
                        &r.id,
                        1,
                        &vals(json!({"mit": Value::Null}))
                    )
                    .await
                    .unwrap(),
                RowOutcome::Done(_)
            ),
            "a clear is allowed even when the target is now blind to the writer"
        );
    }

    #[tokio::test]
    async fn a_relation_and_its_cells_vanish_for_a_reader_who_cannot_see_the_target() {
        let (store, anna, bea, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        let one = target_row(&store, &target).await;
        let two = target_row(&store, &target).await;
        let r = made(
            &store,
            &anna,
            &source,
            json!({"mit": [one.clone(), two.clone()]}),
        )
        .await;
        assert!(has(&keys(&store, &anna, &source).await, "mit"));
        assert_eq!(
            read(&store, &anna, &source, &r.id).await["mit"],
            json!([one, two])
        );
        // The reader of the source who cannot read the target: the column is not in the schema and
        // its key is not in the row — not null, not empty, simply gone. An empty column would
        // announce that a relation reaches somewhere she may not see.
        assert!(!has(&keys(&store, &bea, &source).await, "mit"));
        let blind_read = read(&store, &bea, &source, &r.id).await;
        assert!(blind_read.get("mit").is_none(), "{blind_read}");
        let RowOutcome::Done(page) = store.dataset_rows(&bea, &source, 10, 0).await.unwrap() else {
            panic!("bea may read the source")
        };
        assert!(page.rows[0].values.get("mit").is_none());
    }

    #[tokio::test]
    async fn a_deleted_target_row_leaves_no_dangling_id_in_the_source() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        let keep = target_row(&store, &target).await;
        let gone = target_row(&store, &target).await;
        let r = made(
            &store,
            &anna,
            &source,
            json!({"mit": [keep.clone(), gone.clone()]}),
        )
        .await;
        // The target row is removed; its edge must go with it, and the id with the edge.
        sqlx::query("DELETE FROM dataset_row WHERE id = ?")
            .bind(&gone)
            .execute(&store.pool)
            .await
            .unwrap();
        let edges: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM dataset_relation WHERE target_row_id = ?")
                .bind(&gone)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(edges, 0, "the edge went with its target");
        assert_eq!(
            read(&store, &anna, &source, &r.id).await["mit"],
            json!([keep])
        );
    }

    #[tokio::test]
    async fn rewriting_a_cell_moves_the_edge_and_leaves_no_stale_one() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        let fid = field_id(&store, &source, "mit").await;
        let a = target_row(&store, &target).await;
        let b = target_row(&store, &target).await;
        let r = made(&store, &anna, &source, json!({"mit": [a.clone()]})).await;
        assert_eq!(
            store.rows_pointing_at(&fid, &a).await.unwrap(),
            vec![r.id.clone()]
        );
        // Re-point at b: the a-edge is gone, a b-edge is there, and the cell reads back as b alone.
        let RowOutcome::Done(u) = store
            .update_dataset_row(&anna, &source, &r.id, 1, &vals(json!({"mit": [b.clone()]})))
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(u.version, 2);
        assert!(
            store.rows_pointing_at(&fid, &a).await.unwrap().is_empty(),
            "no stale edge behind"
        );
        assert_eq!(
            store.rows_pointing_at(&fid, &b).await.unwrap(),
            vec![r.id.clone()]
        );
        assert_eq!(read(&store, &anna, &source, &r.id).await["mit"], json!([b]));
    }

    #[tokio::test]
    async fn clearing_a_relation_drops_its_edges_whether_by_null_or_empty_list() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        let fid = field_id(&store, &source, "mit").await;
        let a = target_row(&store, &target).await;
        let r = made(&store, &anna, &source, json!({"mit": [a.clone()]})).await;
        assert_eq!(
            store.rows_pointing_at(&fid, &a).await.unwrap(),
            vec![r.id.clone()]
        );
        // `[]` is a clear: stored empty, and every edge for the cell goes.
        let RowOutcome::Done(u) = store
            .update_dataset_row(&anna, &source, &r.id, 1, &vals(json!({"mit": []})))
            .await
            .unwrap()
        else {
            panic!("an empty list is a clear, and a clear always writes")
        };
        assert_eq!(
            u.values["mit"],
            json!([]),
            "an empty list is stored as empty"
        );
        assert!(store.rows_pointing_at(&fid, &a).await.unwrap().is_empty());
        assert_eq!(read(&store, &anna, &source, &r.id).await["mit"], json!([]));
    }

    #[tokio::test]
    async fn a_multi_relation_cell_dedupes_and_a_single_one_refuses_a_list() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        let a = target_row(&store, &target).await;
        let b = target_row(&store, &target).await;
        let r = made(
            &store,
            &anna,
            &source,
            json!({"mit": [a.clone(), b.clone(), a.clone()]}),
        )
        .await;
        assert_eq!(
            read(&store, &anna, &source, &r.id).await["mit"],
            json!([a.clone(), b]),
            "duplicated, order kept"
        );
        // A single column never takes a list: core refuses the shape before the store is asked.
        store
            .add_dataset_field(&anna, &source, &relation_field("eins", &target, false))
            .await
            .unwrap();
        assert!(matches!(
            store
                .create_dataset_row(&anna, &source, &vals(json!({"eins": [a]})))
                .await
                .unwrap(),
            RowOutcome::Invalid(_)
        ));
    }

    #[tokio::test]
    async fn the_reverse_lookup_reads_off_the_target_index_not_a_scan() {
        let (store, anna, _, source, target) = relation_fixture().await;
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        let fid = field_id(&store, &source, "mit").await;
        let a = target_row(&store, &target).await;
        let r = made(&store, &anna, &source, json!({"mit": [a.clone()]})).await;
        assert_eq!(
            store.rows_pointing_at(&fid, &a).await.unwrap(),
            vec![r.id.clone()]
        );
        // The very statement `rows_pointing_at` runs (one shared `const`, so the plan under test
        // cannot drift from the query in use) must be a SEARCH on the target index.
        let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(&format!(
            "EXPLAIN QUERY PLAN {}",
            super::relation::POINTS_AT_SQL
        ))
        .bind(&a)
        .bind(&fid)
        .fetch_all(&store.pool)
        .await
        .unwrap();
        let text = plan
            .iter()
            .map(|(_, _, _, step)| step.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("dataset_relation_by_target"), "{text}");
        assert!(text.contains("SEARCH"), "{text}");
        assert!(
            !text.contains("SCAN dataset_relation"),
            "a scan of the edge table: {text}"
        );
    }

    // --- rollup fields (ADR 0029 cl. 3, M8b B5) -------------------------------------

    /// The refusal for a rollup whose target the caller may not read, spelled out here: an
    /// unreadable dataset and a relation column whose stored config has gone are one sentence
    /// (ADR 0022), so saving a rollup is no way to ask what sits behind the relation.
    const NO_ROLLUP: &str = "rollup target is not available";
    /// The six rollup keys `rollup_fixture` puts on the source dataset.
    const ROLLUP_KEYS: [&str; 6] = ["n", "sum", "avg", "min", "max", "list"];

    fn rollup_field(key: &str, relation: &str, target_field: &str, func: &str) -> NewField {
        NewField {
            key: key.into(),
            label: key.to_uppercase(),
            kind: FieldKind::Rollup,
            config: json!({ "relation": relation, "target_field": target_field, "fn": func }),
        }
    }

    /// A row of the target dataset with cells in it, so a rollup has something to aggregate.
    async fn valued_row(store: &Store, target: &str, values: serde_json::Value) -> String {
        let doc = doc_id_of(store, target).await;
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO dataset_row (id, doc_id, \"values\") VALUES (?, ?, ?)")
            .bind(&id)
            .bind(&doc)
            .bind(values.to_string())
            .execute(&store.pool)
            .await
            .unwrap();
        id
    }

    /// The target's `amount` (number), `when` (date) and `note` (text) columns, a multi relation
    /// from the source to it, and one rollup of each kind over it — all as `anna`, who may read
    /// both ends. Every call is unwrapped, so a refusal fails the test that made it.
    async fn rollup_fixture(store: &Store, anna: &Principal, source: &str, target: &str) {
        for (key, kind) in [
            ("amount", FieldKind::Number),
            ("when", FieldKind::Date),
            ("note", FieldKind::Text),
        ] {
            store
                .add_dataset_field(anna, target, &plain_field(key, kind))
                .await
                .unwrap();
        }
        store
            .add_dataset_field(anna, source, &relation_field("mit", target, true))
            .await
            .unwrap();
        for (key, func, field) in [
            ("n", "count", "amount"),
            ("sum", "sum", "amount"),
            ("avg", "avg", "amount"),
            ("min", "min", "amount"),
            ("max", "max", "amount"),
            ("list", "list", "amount"),
            ("first", "min", "when"),
        ] {
            let out = store
                .add_dataset_field(anna, source, &rollup_field(key, "mit", field, func))
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Done(_)), "{key}: {out:?}");
        }
    }

    /// Everything one caller sees of a dataset — its schema and its first page — as one JSON value.
    /// Two snapshots are equal only when the reader's answer is equal down to the last byte, which
    /// is the only way to say "nothing about the hidden dataset reached them".
    async fn snapshot(store: &Store, who: &Principal, path: &str) -> Value {
        let SchemaOutcome::Done(fields) = store.dataset_fields(who, path).await.unwrap() else {
            panic!("no schema")
        };
        let RowOutcome::Done(page) = store.dataset_rows(who, path, 10, 0).await.unwrap() else {
            panic!("no rows")
        };
        json!({ "fields": fields, "rows": page })
    }

    #[tokio::test]
    async fn a_rollup_aggregates_the_rows_its_relation_points_at() {
        let (store, anna, _, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        let one = valued_row(&store, &target, json!({"amount": 10, "when": "2026-01-05"})).await;
        let two = valued_row(
            &store,
            &target,
            json!({"amount": 2.5, "when": "2026-01-02"}),
        )
        .await;
        let blank = valued_row(&store, &target, json!({"when": "2026-01-01"})).await;
        let r = made(
            &store,
            &anna,
            &source,
            json!({"mit": [one.clone(), two.clone(), blank.clone()]}),
        )
        .await;
        let v = read(&store, &anna, &source, &r.id).await;
        assert_eq!(
            v["n"],
            json!(3),
            "count counts the links, not the filled cells"
        );
        assert_eq!(v["sum"].as_f64(), Some(12.5));
        assert_eq!(
            v["avg"].as_f64(),
            Some(6.25),
            "the mean of the two cells that hold a number"
        );
        assert_eq!(v["min"], json!(2.5));
        assert_eq!(
            v["max"],
            json!(10),
            "a value passes through as it was stored"
        );
        assert_eq!(
            v["list"],
            json!([10, 2.5]),
            "in edge order, an empty cell skipped"
        );
        assert_eq!(
            v["first"],
            json!("2026-01-01"),
            "min over dates compares the ISO text, which is calendar order"
        );
        assert_eq!(v["mit"], json!([one, two, blank]), "the relation itself");
        // Nothing of a rollup is in storage: it is an answer, not a value.
        let raw: String = sqlx::query_scalar("SELECT \"values\" FROM dataset_row WHERE id = ?")
            .bind(&r.id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert!(!raw.contains("sum"), "a rollup reached storage: {raw}");
    }

    #[tokio::test]
    async fn a_rollup_over_no_rows_counts_zero_and_answers_nothing_else() {
        let (store, anna, _, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        // Both shapes of empty: a cleared cell, and one nobody ever wrote.
        let cleared = made(&store, &anna, &source, json!({"mit": []})).await;
        let never = made(&store, &anna, &source, json!({})).await;
        for id in [cleared.id, never.id] {
            let v = read(&store, &anna, &source, &id).await;
            assert_eq!(v["n"], json!(0), "count is a number, never null");
            assert_eq!(v["sum"].as_f64(), Some(0.0), "a sum of nothing is nought");
            assert!(
                v["avg"].is_null(),
                "the mean of nothing is nothing: {:?}",
                v["avg"]
            );
            assert!(v["min"].is_null(), "{:?}", v["min"]);
            assert!(v["max"].is_null(), "{:?}", v["max"]);
            assert_eq!(v["list"], json!([]));
            assert!(v["first"].is_null());
        }
    }

    #[tokio::test]
    async fn a_rollup_follows_relinking_and_a_deleted_target_row() {
        let (store, anna, _, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        let a = valued_row(&store, &target, json!({"amount": 4})).await;
        let b = valued_row(&store, &target, json!({"amount": 6})).await;
        let r = made(&store, &anna, &source, json!({"mit": [a.clone()]})).await;
        let v = read(&store, &anna, &source, &r.id).await;
        assert_eq!((v["n"].as_i64(), v["avg"].as_f64()), (Some(1), Some(4.0)));
        // Re-point the cell: the aggregate moves with the edges, and no rollup was ever written.
        assert!(matches!(
            store
                .update_dataset_row(
                    &anna,
                    &source,
                    &r.id,
                    1,
                    &vals(json!({"mit": [a.clone(), b.clone()]}))
                )
                .await
                .unwrap(),
            RowOutcome::Done(_)
        ));
        let v = read(&store, &anna, &source, &r.id).await;
        assert_eq!((v["n"].as_i64(), v["sum"].as_f64()), (Some(2), Some(10.0)));
        assert_eq!(v["list"], json!([4, 6]));
        // A target row deleted out from under the source: its edge cascades, and it leaves the
        // count with the id it took.
        sqlx::query("DELETE FROM dataset_row WHERE id = ?")
            .bind(&b)
            .execute(&store.pool)
            .await
            .unwrap();
        let v = read(&store, &anna, &source, &r.id).await;
        assert_eq!(v["n"], json!(1));
        assert_eq!(v["list"], json!([4]));
        assert_eq!(v["sum"].as_f64(), Some(4.0));
    }

    #[tokio::test]
    async fn a_rollup_over_a_target_the_caller_cannot_read_is_absent_not_zero() {
        let (store, anna, bea, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        let one = valued_row(&store, &target, json!({"amount": 7})).await;
        let r = made(&store, &anna, &source, json!({"mit": [one.clone()]})).await;
        // anna, who may read the target, gets real numbers — so the comparison below is not
        // vacuous: there is something to hide, and it is hidden for exactly one reason.
        let seen = read(&store, &anna, &source, &r.id).await;
        assert_eq!(
            (seen["n"].as_i64(), seen["list"].as_array().map(Vec::len)),
            (Some(1), Some(1))
        );
        let ks = keys(&store, &bea, &source).await;
        assert!(!has(&ks, "mit"), "the relation is hidden, so: {ks:?}");
        let blind = read(&store, &bea, &source, &r.id).await;
        for key in ROLLUP_KEYS.iter().chain(["first"].iter()) {
            assert!(
                !has(&ks, key),
                "hidden rollup `{key}` in the schema: {ks:?}"
            );
            assert!(
                blind.get(*key).is_none(),
                "hidden rollup `{key}` in the row: {blind}"
            );
        }
        // And her answer cannot move while the hidden dataset grows. Both the target row and the
        // edge are laid straight into storage, with the source cell rewritten in place: a write
        // through the API would bump the row's `version`, which she may legitimately see.
        // A rollup that answers where it should be silent also upsets
        // `a_formula_may_read_a_rollup_column`, whose unanswerable column must stay a hole rather
        // than become a number — but one test noticing a hole is no licence to leave the checking
        // to it.
        let before = snapshot(&store, &bea, &source).await;
        let two = valued_row(&store, &target, json!({"amount": 9999})).await;
        let mit = field_id(&store, &source, "mit").await;
        sqlx::query(
            "INSERT INTO dataset_relation (row_id, field_id, target_row_id) VALUES (?1, ?2, ?3)",
        )
        .bind(&r.id)
        .bind(&mit)
        .bind(&two)
        .execute(&store.pool)
        .await
        .unwrap();
        sqlx::query("UPDATE dataset_row SET \"values\" = ?1 WHERE id = ?2")
            .bind(json!({"mit": [one, two]}).to_string())
            .bind(&r.id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            read(&store, &anna, &source, &r.id).await["n"],
            json!(2),
            "anna sees the link bea must not feel"
        );
        assert_eq!(
            snapshot(&store, &bea, &source).await,
            before,
            "a hidden target's growth moved bea's answer"
        );
    }

    #[tokio::test]
    async fn a_formula_reads_a_rollup_because_the_rollup_is_answered_first() {
        let (store, anna, _, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        store
            .add_dataset_field(&anna, &source, &formula_field("plus", "{n} + 1"))
            .await
            .unwrap();
        let one = valued_row(&store, &target, json!({"amount": 7})).await;
        let r = made(&store, &anna, &source, json!({"mit": [one]})).await;
        let v = read(&store, &anna, &source, &r.id).await;
        assert_eq!(v["plus"].as_f64(), Some(2.0), "{:?}", v["plus"]);
    }

    #[tokio::test]
    async fn a_rollup_column_is_validated_when_it_is_saved() {
        let (store, anna, _, source, target) = relation_fixture().await;
        for (key, kind) in [
            ("amount", FieldKind::Number),
            ("when", FieldKind::Date),
            ("note", FieldKind::Text),
        ] {
            store
                .add_dataset_field(&anna, &target, &plain_field(key, kind))
                .await
                .unwrap();
        }
        store
            .add_dataset_field(&anna, &target, &formula_field("doubled", "{amount} * 2"))
            .await
            .unwrap();
        store
            .add_dataset_field(&anna, &source, &relation_field("mit", &target, true))
            .await
            .unwrap();
        store
            .add_dataset_field(&anna, &source, &plain_field("name", FieldKind::Text))
            .await
            .unwrap();
        let refusals = [
            // Not exactly the three string members.
            (json!({}), "all three members are needed"),
            (
                json!({"relation": "mit", "fn": "count"}),
                "a target_field is needed even to count",
            ),
            (
                json!({"relation": "mit", "target_field": "amount", "fn": "count", "multiple": true}),
                "an extra member is refused, not ignored",
            ),
            (
                json!({"relation": "mit", "target_field": 3, "fn": "count"}),
                "a member that is not a string",
            ),
            // The relation must be a relation column of THIS dataset.
            (
                json!({"relation": "nichts", "target_field": "amount", "fn": "count"}),
                "no relation of this dataset by that name",
            ),
            (
                json!({"relation": "name", "target_field": "amount", "fn": "count"}),
                "a column of the wrong kind named as the relation",
            ),
            // The aggregate must be one of six.
            (
                json!({"relation": "mit", "target_field": "amount", "fn": "median"}),
                "an unknown fn",
            ),
            // The field aggregated must exist, be stored, and be of a type the fn can answer.
            (
                json!({"relation": "mit", "target_field": "nichts", "fn": "sum"}),
                "a target field that is not there",
            ),
            (
                json!({"relation": "mit", "target_field": "doubled", "fn": "list"}),
                "a target field that is itself computed",
            ),
            (
                json!({"relation": "mit", "target_field": "note", "fn": "sum"}),
                "sum over text",
            ),
            (
                json!({"relation": "mit", "target_field": "note", "fn": "avg"}),
                "avg over text",
            ),
            (
                json!({"relation": "mit", "target_field": "note", "fn": "min"}),
                "min over text",
            ),
            (
                json!({"relation": "mit", "target_field": "note", "fn": "max"}),
                "max over text",
            ),
            (
                json!({"relation": "mit", "target_field": "when", "fn": "sum"}),
                "sum over a date",
            ),
        ];
        for (i, (config, why)) in refusals.into_iter().enumerate() {
            let mut field = rollup_field(&format!("r{i}"), "mit", "amount", "count");
            field.config = config;
            let out = store
                .add_dataset_field(&anna, &source, &field)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{why}: {out:?}");
            assert!(!has(&keys(&store, &anna, &source).await, &field.key));
        }
        // `min`/`max` over a date and over a number are both allowed, and `count` ignores the
        // field it names but still needs the string.
        for field in [
            rollup_field("latest", "mit", "when", "max"),
            rollup_field("later", "mit", "when", "min"),
            rollup_field("how", "mit", "whatever", "count"),
            rollup_field("amounts", "mit", "amount", "max"),
        ] {
            let out = store
                .add_dataset_field(&anna, &source, &field)
                .await
                .unwrap();
            assert!(
                matches!(out, SchemaOutcome::Done(_)),
                "{}: {out:?}",
                field.key
            );
        }
        // A writer of the source who cannot read the target: the uniform refusal, the same words
        // whatever the reason the target is not available.
        let blind = Principal::test("blind", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(blind.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        assert_eq!(
            store
                .add_dataset_field(&blind, &source, &rollup_field("x", "mit", "amount", "sum"))
                .await
                .unwrap(),
            SchemaOutcome::Invalid(NO_ROLLUP.into())
        );
        // The config that lands is the three members, nothing resolved away and nothing added.
        let SchemaOutcome::Done(f) = store
            .add_dataset_field(
                &anna,
                &source,
                &rollup_field("total", "mit", "amount", "sum"),
            )
            .await
            .unwrap()
        else {
            panic!("a rollup over a readable target lands")
        };
        assert_eq!(
            f.config,
            json!({"relation": "mit", "target_field": "amount", "fn": "sum"})
        );
    }

    #[tokio::test]
    async fn writing_a_rollup_cell_is_refused_as_computed() {
        let (store, anna, _, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        for patch in [json!({"n": 3}), json!({"list": []}), json!({"sum": null})] {
            let out = store
                .create_dataset_row(&anna, &source, &vals(patch))
                .await
                .unwrap();
            let RowOutcome::Invalid(msg) = out else {
                panic!("a rollup cell is not a value anybody may write: {out:?}")
            };
            assert!(msg.contains("computed and never stored"), "{msg}");
        }
    }

    #[tokio::test]
    async fn filtering_or_sorting_a_rollup_is_refused_as_a_computed_field() {
        let (store, anna, bea, source, target) = relation_fixture().await;
        rollup_fixture(&store, &anna, &source, &target).await;
        let want = "computed field `n` cannot be filtered or sorted yet";
        for which in [
            RowSelect {
                filters: vec![Filter::Eq {
                    key: k("n"),
                    value: json!(3),
                }],
                limit: 5,
                ..Default::default()
            },
            RowSelect {
                sort: Some((k("n"), false)),
                limit: 5,
                ..Default::default()
            },
        ] {
            assert_eq!(
                select(&store, &anna, &source, which.clone()).await,
                RowOutcome::Invalid(want.into())
            );
            // Hidden, it is refused as the field it is not: the same words as a made-up key.
            assert_eq!(
                select(&store, &bea, &source, which).await,
                RowOutcome::Invalid("no field `n`".into())
            );
        }
    }
}
