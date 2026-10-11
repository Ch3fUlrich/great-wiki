//! The `relation` kind: a column that points at the rows of another dataset.
//!
//! Core has already checked the SHAPE of the cell (one bounded id, or a deduped list). What is
//! left can only be asked of the database and of the caller, and it is asked in two places:
//!
//! * **at schema time** the caller names the target by its page **path**, and it is resolved to a
//!   doc id under their own read access. The path is never stored — the id is — so renaming or
//!   moving the target does not silently break the column, and a target the caller could not read
//!   cannot be named at all.
//! * **at row time** the cell names target row **ids**, and every one must be a row of that target
//!   *and* the caller must still be able to read the target dataset. Both answers are one uniform
//!   refusal (`relation target is not available`): whether a given row id exists elsewhere, or the
//!   target has been deleted, or it was never the caller's to see, is not something a row write is
//!   allowed to disclose (ADR 0022). A clear — `null` or an empty list — asks nothing and always
//!   writes.
//!
//! The edges are written in `dataset_relation` alongside the ids in the row's JSON. The table is
//! the answer to two questions the JSON cannot answer cheaply: does this target row still exist (a
//! delete cascades the edge away, so the dangling id simply disappears when the source row is next
//! read), and which rows point at a given row (the reverse lookup, served by the target index).

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::DatasetRow;
use crate::Store;
use gw_auth::Principal;
use gw_core::dataset::FieldKind;
use gw_core::formula;

/// The one refusal for a cross-target row, an unreadable target and a deleted one alike. See the
/// module header and ADR 0022: distinguishing them would make a row write an oracle.
pub(super) const RELATION_REFUSAL: &str = "relation target is not available";
/// Most rows a multi-valued relation cell may point at.
pub(super) const MAX_RELATION_TARGETS: usize = 100;

/// The reverse lookup: which source rows point at `target_row_id` through one column. Written as a
/// `const` so the `EXPLAIN QUERY PLAN` test and the query itself can never disagree about which
/// statement is under test.
pub(super) const POINTS_AT_SQL: &str =
    "SELECT row_id FROM dataset_relation WHERE target_row_id = ?1 AND field_id = ?2";

/// One column's relation state, resolved for a caller. Read by `rollup.rs`, which asks the same
/// access question of the column a rollup sits on.
pub(super) struct RelCol {
    pub(super) id: String,
    pub(super) key: String,
    pub(super) multiple: bool,
    pub(super) target_doc: Option<String>,
    /// Whether this caller may read the column's target dataset. `false` hides the whole column.
    pub(super) visible: bool,
}

/// One relation cell the caller is writing, as it must land in `dataset_relation`.
pub(super) struct EdgeWrite {
    field_id: String,
    /// The ids to (re)point at; empty means a clear — drop every edge for the cell.
    ids: Vec<String>,
}

impl Store {
    /// Resolve a caller-supplied `{"target": "<path>", "multiple": bool}` at field-creation time.
    /// The path is read under the caller's own access; unreadable, absent or not-a-dataset are one
    /// answer — `no dataset at <path>` — so creating a column is not a way to ask what is there.
    /// The stored config keeps the target by **id**, not by path.
    pub(super) async fn resolve_target(
        &self,
        principal: &Principal,
        config: &Value,
    ) -> Result<std::result::Result<Value, String>> {
        let target = config.get("target").and_then(Value::as_str).unwrap_or("");
        let multiple = config
            .get("multiple")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        match self.readable_dataset(principal, target).await? {
            Some(doc) => Ok(Ok(json!({ "target_doc": doc.id, "multiple": multiple }))),
            None => Ok(Err(format!("no dataset at {target}"))),
        }
    }

    /// The relation columns of `doc_id` and whether this caller may see each. A dataset with no
    /// relation column costs no queries beyond the one that says so. `rollup.rs` asks the same
    /// question, because a rollup reaches the target through one of these columns.
    pub(super) async fn relation_cols(
        &self,
        principal: &Principal,
        doc_id: &str,
    ) -> Result<Vec<RelCol>> {
        let raw: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT id, key, config FROM dataset_field WHERE doc_id = ?1 AND kind = 'relation' \
             ORDER BY position, created_at, key",
        )
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;
        let mut cached: HashMap<String, bool> = HashMap::new();
        let mut cols = Vec::with_capacity(raw.len());
        for (id, key, config) in raw {
            let value: Value = serde_json::from_str(&config).unwrap_or(Value::Null);
            let multiple = value
                .get("multiple")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let target_doc = value
                .get("target_doc")
                .and_then(Value::as_str)
                .map(str::to_string);
            let visible = match &target_doc {
                None => false,
                Some(td) => match cached.get(td) {
                    Some(&v) => v,
                    None => {
                        let v = self.readable_dataset_by_id(principal, td).await?;
                        cached.insert(td.clone(), v);
                        v
                    }
                },
            };
            cols.push(RelCol {
                id,
                key,
                multiple,
                target_doc,
                visible,
            });
        }
        Ok(cols)
    }

    /// The one definition of "hidden for this caller", computed once per call and read by every
    /// door that decides visibility — the schema, the row and the query. A field is hidden when:
    ///
    /// * (a) it is a `relation` whose target dataset the caller may not read, or
    /// * (b) it is a `rollup` over such a relation, or
    /// * (c) it is a `formula` whose dependency chain — transitive, through other formulas and
    ///   rollups, read off the field keys named by `field_refs` of its stored `expr` — reaches a
    ///   hidden field.
    ///
    /// A hidden column is ABSENT, never shown-as-empty: an empty column or a `0` would announce
    /// that something reaches a dataset the caller cannot see (ADR 0022). Formulas are resolved to a
    /// fixpoint; a cycle is refused when the column is saved so none is expected, and the loop is
    /// bounded by the field count regardless.
    pub(super) async fn hidden_field_keys(
        &self,
        principal: &Principal,
        doc_id: &str,
    ) -> Result<HashSet<String>> {
        let cols = self.relation_cols(principal, doc_id).await?;
        let mut hidden: HashSet<String> = cols
            .iter()
            .filter(|c| !c.visible)
            .map(|c| c.key.clone())
            .collect();
        // (b) A rollup over a relation this caller may not follow. A rollup whose stored config no
        // longer names an existing relation is a hole rather than a secret — it is not caller-
        // dependent, so it stays out of this set and simply reads as no value for everyone.
        let rollups: Vec<(String, String)> = sqlx::query_as(
            "SELECT key, config FROM dataset_field WHERE doc_id = ?1 AND kind = 'rollup'",
        )
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;
        for (key, config) in rollups {
            let relation = serde_json::from_str::<Value>(&config).ok().and_then(|v| {
                v.get("relation")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
            if let Some(rel) = relation {
                if cols.iter().any(|c| c.key == rel && !c.visible) {
                    hidden.insert(key);
                }
            }
        }
        let fields = self.fields_of(doc_id).await?;
        let mut deps: Vec<(String, Vec<String>)> = Vec::new();
        for f in &fields {
            if f.kind != FieldKind::Formula.as_str() {
                continue;
            }
            let reads = f
                .config
                .get("expr")
                .and_then(Value::as_str)
                .and_then(|src| formula::parse(src, &|_| true).ok())
                .map(|expr| formula::field_refs(&expr))
                .unwrap_or_default();
            deps.push((f.key.clone(), reads));
        }
        for _ in 0..fields.len() {
            let mut grew = false;
            for (key, reads) in &deps {
                if !hidden.contains(key.as_str())
                    && reads.iter().any(|d| hidden.contains(d.as_str()))
                {
                    hidden.insert(key.clone());
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        Ok(hidden)
    }

    /// Rewrite each row's relation cells to what a reader sees: a hidden column's key is removed,
    /// and a visible one keeps only the ids whose edge is still alive. Order comes from the stored
    /// cell, the surviving set from `dataset_relation` — so a target row deleted out from under the
    /// source simply disappears rather than reading back as a dangling id.
    pub(super) async fn decorate_rows(
        &self,
        principal: &Principal,
        doc_id: &str,
        rows: &mut [DatasetRow],
    ) -> Result<()> {
        let cols = self.relation_cols(principal, doc_id).await?;
        if cols.is_empty() {
            return Ok(());
        }
        for row in rows {
            let Some(values) = row.values.as_object_mut() else {
                continue;
            };
            // Live edges are fetched once per row, and only if a visible cell needs them.
            let mut live: Option<HashMap<String, HashSet<String>>> = None;
            for col in &cols {
                if !col.visible {
                    values.remove(&col.key);
                    continue;
                }
                if !values.contains_key(&col.key) {
                    continue;
                }
                if live.is_none() {
                    live = Some(self.live_edges(&row.id).await?);
                }
                let edges = live
                    .as_ref()
                    .and_then(|m| m.get(&col.id))
                    .cloned()
                    .unwrap_or_default();
                match values.get(&col.key) {
                    Some(v) if col.multiple => {
                        let kept: Vec<Value> = v
                            .as_array()
                            .map(|ids| {
                                ids.iter()
                                    .filter(|id| id.as_str().is_some_and(|s| edges.contains(s)))
                                    .cloned()
                                    .collect()
                            })
                            .unwrap_or_default();
                        values.insert(col.key.clone(), Value::Array(kept));
                    }
                    Some(v) => {
                        let alive = v.as_str().is_some_and(|id| edges.contains(id));
                        if !alive {
                            values.remove(&col.key);
                        }
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }

    /// The ids a row still points at, by column id — the set that has not been cascaded away. Also
    /// what a rollup counts, so that a target row deleted out from under the source leaves no trace.
    pub(super) async fn live_edges(
        &self,
        row_id: &str,
    ) -> Result<HashMap<String, HashSet<String>>> {
        let edges: Vec<(String, String)> = sqlx::query_as(
            "SELECT field_id, target_row_id FROM dataset_relation WHERE row_id = ?1",
        )
        .bind(row_id)
        .fetch_all(&self.pool)
        .await?;
        let mut map: HashMap<String, HashSet<String>> = HashMap::new();
        for (field_id, target) in edges {
            map.entry(field_id).or_default().insert(target);
        }
        Ok(map)
    }

    /// Validate the relation cells a patch writes and produce the edge rewrites to apply inside the
    /// row's own transaction. Runs before any transaction is open — the pool is one connection and
    /// an access or membership query made while it is held would wait on it (see `links.rs`).
    /// `Err(reason)` refuses the whole write; `Ok(writes)` lists every touched cell, clears included.
    pub(super) async fn plan_write(
        &self,
        principal: &Principal,
        doc_id: &str,
        patch: &Map<String, Value>,
    ) -> Result<std::result::Result<Vec<EdgeWrite>, String>> {
        let cols = self.relation_cols(principal, doc_id).await?;
        let mut writes = Vec::new();
        for col in cols {
            let Some(value) = patch.get(&col.key) else {
                continue;
            };
            // A clear: nothing to ask of a target the cell is leaving, so it always writes.
            if value.is_null() || (col.multiple && value.as_array().is_some_and(Vec::is_empty)) {
                writes.push(EdgeWrite {
                    field_id: col.id.clone(),
                    ids: Vec::new(),
                });
                continue;
            }
            let ids: Vec<String> = if col.multiple {
                match value.as_array() {
                    Some(arr) => arr
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    None => return Ok(Err(RELATION_REFUSAL.into())),
                }
            } else {
                match value.as_str() {
                    Some(s) => vec![s.to_string()],
                    None => return Ok(Err(RELATION_REFUSAL.into())),
                }
            };
            if col.multiple && ids.len() > MAX_RELATION_TARGETS {
                return Ok(Err(format!(
                    "a relation cell points at at most {MAX_RELATION_TARGETS} rows"
                )));
            }
            let Some(target_doc) = col.target_doc.as_deref() else {
                return Ok(Err(RELATION_REFUSAL.into()));
            };
            if !col.visible {
                return Ok(Err(RELATION_REFUSAL.into()));
            }
            for id in &ids {
                if !self.row_is_in(target_doc, id).await? {
                    return Ok(Err(RELATION_REFUSAL.into()));
                }
            }
            writes.push(EdgeWrite {
                field_id: col.id.clone(),
                ids,
            });
        }
        Ok(Ok(writes))
    }

    /// Rewrite the edges for the cells in `writes` inside the row's transaction: every touched cell
    /// is deleted and re-inserted, so an update leaves no stale edge behind. On a create the delete
    /// is a no-op against a row that had none.
    pub(super) async fn rewrite_edges(
        &self,
        tx: &mut sqlx::SqliteConnection,
        row_id: &str,
        writes: &[EdgeWrite],
    ) -> Result<()> {
        for w in writes {
            sqlx::query("DELETE FROM dataset_relation WHERE row_id = ?1 AND field_id = ?2")
                .bind(row_id)
                .bind(&w.field_id)
                .execute(&mut *tx)
                .await?;
            for id in &w.ids {
                sqlx::query(
                    "INSERT INTO dataset_relation (row_id, field_id, target_row_id) \
                     VALUES (?1, ?2, ?3)",
                )
                .bind(row_id)
                .bind(&w.field_id)
                .bind(id)
                .execute(&mut *tx)
                .await?;
            }
        }
        Ok(())
    }

    /// Which source rows point at `target_row_id` through one column, straight off the index.
    ///
    /// Kept although nothing calls it yet: this is the store-side entry the board milestone
    /// (reverse-lookup a card's referrers) will use, and here a test proves it hits the index.
    #[allow(dead_code)]
    pub(crate) async fn rows_pointing_at(
        &self,
        field_id: &str,
        target_row_id: &str,
    ) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(POINTS_AT_SQL)
            .bind(target_row_id)
            .bind(field_id)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Whether `id` is a row of the dataset `target_doc` — not merely a row somewhere. An id from
    /// another dataset is refused in the same words as a target the caller cannot read, so a cell
    /// never confirms which dataset a row id belongs to.
    async fn row_is_in(&self, target_doc: &str, id: &str) -> Result<bool> {
        let found: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM dataset_row WHERE doc_id = ?1 AND id = ?2")
                .bind(target_doc)
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(found.is_some())
    }
}
