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

/// Most fields one dataset may carry (spec: bounded schema).
pub const MAX_FIELDS: usize = 100;
/// Longest field label, in characters.
pub const MAX_LABEL_CHARS: usize = 120;
/// Largest `config` JSON a field may carry, in bytes.
pub const MAX_CONFIG_BYTES: usize = 4096;

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

    /// The schema of the dataset at `path`, for anybody who may read it.
    pub async fn dataset_fields(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<SchemaOutcome<Vec<DatasetField>>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(SchemaOutcome::NoDataset);
        };
        Ok(SchemaOutcome::Done(self.fields_of(&doc.id).await?))
    }

    /// Add a column. Needs write. `key` is validated by [`FieldKey::parse`] before it
    /// reaches SQL; `kind` is fixed for the life of the field. M8a offers the stored,
    /// store-independent kinds only.
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
        if matches!(
            new.kind,
            FieldKind::Person
                | FieldKind::File
                | FieldKind::Relation
                | FieldKind::Rollup
                | FieldKind::Formula
        ) {
            return Ok(SchemaOutcome::Invalid(format!(
                "field kind `{}` is not available yet",
                new.kind.as_str()
            )));
        }
        let label = match clean_label(&new.label) {
            Ok(l) => l,
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let config = new.config.to_string();
        if !new.config.is_object() || config.len() > MAX_CONFIG_BYTES {
            return Ok(SchemaOutcome::Invalid(format!(
                "field config must be an object of at most {MAX_CONFIG_BYTES} bytes"
            )));
        }
        let mut tx = self.pool.begin().await?;
        let (count, last): (i64, Option<i64>) =
            sqlx::query_as("SELECT COUNT(*), MAX(position) FROM dataset_field WHERE doc_id = ?")
                .bind(&doc_id)
                .fetch_one(&mut *tx)
                .await?;
        if count as usize >= MAX_FIELDS {
            return Ok(SchemaOutcome::Invalid(format!(
                "a dataset has at most {MAX_FIELDS} fields"
            )));
        }
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
        let position = last.map_or(0, |p| p + 1);
        sqlx::query(
            "INSERT INTO dataset_field (id, doc_id, key, label, kind, config, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&doc_id)
        .bind(key.as_str())
        .bind(&label)
        .bind(new.kind.as_str())
        .bind(&config)
        .bind(position)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(SchemaOutcome::Done(DatasetField {
            key: key.as_str().to_string(),
            label,
            kind: new.kind.as_str().to_string(),
            config: new.config.clone(),
            position,
        }))
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
    pub total: i64,
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
    /// Validate `patch` against the dataset's current schema. Unknown keys, wrong types and
    /// the kinds that need the store (person, file, relation: M8b) are refused; `null` stays
    /// as `null` so an update can clear a cell.
    async fn checked_values(
        &self,
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
        Ok(validate_row(&specs, patch).map_err(|e| e.to_string()))
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

    /// One row of the dataset at `path`, for anybody who may read it.
    pub async fn dataset_row(
        &self,
        principal: &Principal,
        path: &str,
        row_id: &str,
    ) -> Result<RowOutcome<DatasetRow>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(RowOutcome::NoDataset);
        };
        Ok(match self.fetch_row(&doc.id, row_id).await? {
            Some(r) => RowOutcome::Done(r),
            None => RowOutcome::NoRow,
        })
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
        Ok(RowOutcome::Done(RowPage {
            rows: tuples.into_iter().map(row_from).collect(),
            total,
        }))
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
        let mut clean = match self.checked_values(&doc_id, values).await? {
            Ok(v) => v,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        clean.retain(|_, v| !v.is_null());
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
        let patch = match self.checked_values(&doc_id, patch).await? {
            Ok(v) => v,
            Err(m) => return Ok(RowOutcome::Invalid(m)),
        };
        let Some(current) = self.fetch_row(&doc_id, row_id).await? else {
            return Ok(RowOutcome::NoRow);
        };
        if current.version != version {
            return Ok(RowOutcome::Stale(current));
        }
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
        // The version in the WHERE is the lock: two editors holding version N, one wins.
        let done = sqlx::query(
            "UPDATE dataset_row SET \"values\" = ?, version = version + 1, \
             updated_at = datetime('now') WHERE doc_id = ? AND id = ? AND version = ?",
        )
        .bind(&text)
        .bind(&doc_id)
        .bind(row_id)
        .bind(version)
        .execute(&self.pool)
        .await?;
        let Some(now) = self.fetch_row(&doc_id, row_id).await? else {
            return Ok(RowOutcome::NoRow);
        };
        if done.rows_affected() == 0 {
            return Ok(RowOutcome::Stale(now));
        }
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
        // Computed and store-dependent kinds are not offered in M8a.
        for kind in [
            FieldKind::Formula,
            FieldKind::Rollup,
            FieldKind::Relation,
            FieldKind::Person,
            FieldKind::File,
        ] {
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
    async fn needs_store_kinds_are_refused_in_m8a() {
        let (store, anna, _, _, path) = row_fixture().await;
        let doc = doc_id_of(&store, &path).await;
        // A person column cannot be added through the API, so plant one underneath.
        add_field(&store, &doc, "wer", "person").await.unwrap();
        assert!(matches!(
            store
                .create_dataset_row(&anna, &path, &vals(json!({"wer": "p1"})))
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
}
