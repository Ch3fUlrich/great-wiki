//! The kinds whose value names something outside the row: `person` and `file`.
//!
//! Core checks their SHAPE — a non-empty, bounded id, an array deduped — and stops there,
//! because only the store can answer whether the id names something the writer is entitled to
//! put here. That question is asked on the write path, of the cells a patch actually names, and
//! never of the rest of the row.
//!
//! # A person is stored only if THEY could read this page (ADR 0009 clause 2)
//!
//! The same question [`Store::assignee_named`] asks before a task may rest on somebody, asked
//! with the person's own principal rather than the writer's: "may I name this account" is not
//! the question being answered, "could this account read the page it is being named on" is.
//! Reusing the writer's access would let a page's editor record which of their colleagues can
//! read a page those colleagues have never been given.
//!
//! Both refusals — nobody by that id, and somebody who may not read — are one message. Two
//! messages would answer "does this account exist" to anyone allowed to write a row, which is a
//! disclosure about the account list rather than about the dataset.
//!
//! # Clearing is always allowed
//!
//! `null` (and an empty list) writes without asking anything of the person it drops: the cell
//! goes away, so nothing is disclosed by it, and a refusal there would mean a page could never
//! be cleaned up after somebody lost access. A patch that does not mention the field never
//! re-asks either — this is a rule about what is being written now, not a background audit of
//! what was written before.
//!
//! # A file is one of this page's attachments, and reads back as a name
//!
//! The id must be a row of `attachments` whose `doc_id` is this dataset's page: a cell naming
//! some other page's file would be a way of putting a filename that belongs elsewhere onto this
//! page's rows. At read the id is resolved to `{"id":…, "filename":…}`, or to
//! `{"id":…, "missing":true}` once the file is off the page — a detach is an ordinary act on
//! another part of the wiki and must not turn a stored row into an error.

use std::collections::HashMap;

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::{DatasetField, DatasetRow};
use crate::Store;
use gw_core::dataset::FieldKind;

/// The one refusal both dead ids and unreadable people get. See the module header.
pub(super) const PERSON_REFUSAL: &str = "person is not available for this dataset";
/// The refusal for an id that is not an attachment of this dataset's page.
pub(super) const FILE_REFUSAL: &str = "file is not an attachment of this dataset";

impl Store {
    /// The reason a validated patch may not be written, or `None` when every `person` cell
    /// names somebody who could read this page and every `file` cell is a file of it.
    ///
    /// The caller's own access is not re-asked here: write on the page has already been gated,
    /// and the person question is asked of the named account, not of the writer. This runs
    /// before any transaction is open — the pool is one connection, so an access check made
    /// inside a transaction would wait on the connection the transaction is holding, the
    /// constraint `crate::attachments` records.
    pub(super) async fn link_refusal(
        &self,
        path: &str,
        doc_id: &str,
        kinds: &[(&str, FieldKind)],
        values: &Map<String, Value>,
    ) -> Result<Option<String>> {
        for (key, kind) in kinds {
            // Not named by this patch: the row keeps what it holds, unchecked.
            let Some(value) = values.get(*key) else {
                continue;
            };
            // A clear: no id to ask about, and asking would be refusing the removal.
            if value.is_null() || value.as_array().is_some_and(Vec::is_empty) {
                continue;
            }
            match kind {
                FieldKind::Person => {
                    for id in ids_of(value) {
                        if !self.may_be_named_here(id, path).await? {
                            return Ok(Some(PERSON_REFUSAL.into()));
                        }
                    }
                }
                FieldKind::File => {
                    let Some(id) = value.as_str() else { continue };
                    let on_this_page: Option<(i64,)> =
                        sqlx::query_as("SELECT 1 FROM attachments WHERE id = ?1 AND doc_id = ?2")
                            .bind(id)
                            .bind(doc_id)
                            .fetch_optional(&self.pool)
                            .await?;
                    if on_this_page.is_none() {
                        return Ok(Some(FILE_REFUSAL.into()));
                    }
                }
                _ => {}
            }
        }
        Ok(None)
    }

    /// ADR 0009 clause 2 for one id: an account that exists **and** could read the page at
    /// `path`, asked as them and not as the writer.
    async fn may_be_named_here(&self, id: &str, path: &str) -> Result<bool> {
        let Some((account, _)) = self.principal_by_id(id).await? else {
            return Ok(false);
        };
        Ok(self.assignee_named(&account, path).await?.is_some())
    }

    /// The names of this page's attachments, by id — read out once per list call, and only for
    /// a dataset that has a `file` column at all.
    pub(super) async fn attachment_names(
        &self,
        doc_id: &str,
        fields: &[DatasetField],
    ) -> Result<HashMap<String, String>> {
        if !fields.iter().any(|f| f.kind == FieldKind::File.as_str()) {
            return Ok(HashMap::new());
        }
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, filename FROM attachments WHERE doc_id = ?1")
                .bind(doc_id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().collect())
    }
}

/// The id strings a `person` cell names: one for a single cell, every entry for a list.
fn ids_of(value: &Value) -> Vec<&str> {
    match value {
        Value::String(id) => vec![id.as_str()],
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// One row's `file` cells as a reader sees them: the id plus the name it carries on this page,
/// or `missing` when the file has gone. Person cells stay as stored — resolving a name is a
/// disclosure decision that has not been taken yet.
pub(super) fn resolve_files(
    mut row: DatasetRow,
    fields: &[DatasetField],
    names: &HashMap<String, String>,
) -> DatasetRow {
    if !fields.iter().any(|f| f.kind == FieldKind::File.as_str()) {
        return row;
    }
    let Some(values) = row.values.as_object_mut() else {
        return row;
    };
    for field in fields.iter().filter(|f| f.kind == FieldKind::File.as_str()) {
        let Some(id) = values
            .get(&field.key)
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        values.insert(
            field.key.clone(),
            match names.get(&id) {
                Some(filename) => json!({ "id": id, "filename": filename }),
                None => json!({ "id": id, "missing": true }),
            },
        );
    }
    row
}
