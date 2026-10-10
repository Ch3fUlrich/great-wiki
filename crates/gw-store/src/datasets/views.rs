//! Saved dataset views (ADR 0029 cl. 6, spec section 5): a name, a kind and a JSON config
//! naming field keys. Config, never data - a view cannot show a row its viewer could not read
//! through the row list, because it holds none.
//!
//! Reading goes through `readable_dataset`, every change through `writable_dataset`; an
//! unreadable dataset is `NoDataset`, the same as an absent one. Keys in a config are parsed
//! by `FieldKey` and must be fields of this dataset; they are only ever JSON, never SQL.

use super::query::Filter;
use super::{clean_label, Gate, SchemaOutcome};
use crate::Store;
use anyhow::Result;
use gw_auth::Principal;
use gw_core::dataset::FieldKey;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

/// Most views one dataset may carry.
pub const MAX_VIEWS: usize = 50;
/// Largest view config, in bytes.
const MAX_VIEW_CONFIG_BYTES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DatasetView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config: Value,
    pub position: i64,
}

/// Config of a `table` view. Unknown members are refused, so a typo is an error, not a
/// silently ignored setting.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TableConfig {
    #[serde(default)]
    fields: Vec<FieldKey>,
    sort: Option<ViewSort>,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    widths: BTreeMap<String, u32>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewSort {
    key: FieldKey,
    #[serde(default)]
    #[allow(dead_code)]
    desc: bool,
}

impl TableConfig {
    /// Every field key the config names, validated: widths are keyed by plain strings (a
    /// JSON object), so they pass through `FieldKey::parse` here.
    fn keys(&self) -> Result<Vec<String>, String> {
        let mut out: Vec<String> = self.fields.iter().map(|k| k.as_str().to_string()).collect();
        out.extend(self.sort.iter().map(|s| s.key.as_str().to_string()));
        for f in &self.filters {
            let (Filter::Eq { key, .. }
            | Filter::Contains { key, .. }
            | Filter::IsEmpty { key }
            | Filter::In { key, .. }) = f;
            out.push(key.as_str().to_string());
        }
        for k in self.widths.keys() {
            let parsed = FieldKey::parse(k).map_err(|e| e.to_string())?;
            out.push(parsed.as_str().to_string());
        }
        Ok(out)
    }
}

/// Check `config` for `kind` against the dataset's field keys. M8a has the table view only;
/// the other kinds are in the schema's CHECK for later milestones and refused until then.
fn validate(kind: &str, config: &Value, known: &HashSet<String>) -> Result<(), String> {
    if kind != "table" {
        return Err(format!("the view kind {kind:?} is not available"));
    }
    if config.to_string().len() > MAX_VIEW_CONFIG_BYTES {
        return Err(format!(
            "a view config is at most {MAX_VIEW_CONFIG_BYTES} bytes"
        ));
    }
    let parsed: TableConfig = serde_json::from_value(config.clone())
        .map_err(|e| format!("not a valid table view config: {e}"))?;
    for key in parsed.keys()? {
        if !known.contains(&key) {
            return Err(format!("the dataset has no field {key:?}"));
        }
    }
    Ok(())
}

/// Drop everything in a table config that names a field outside `visible`: the column, a
/// width, and a sort or filter on it (filtering a hidden field is refused, not ignored, so a
/// saved one cannot survive for a caller who may not see the field).
pub(crate) fn strip_hidden(config: &mut Value, visible: &HashSet<String>) {
    let shown = |v: &Value| v.as_str().is_some_and(|k| visible.contains(k));
    let Some(obj) = config.as_object_mut() else {
        return;
    };
    if let Some(Value::Array(fields)) = obj.get_mut("fields") {
        fields.retain(shown);
    }
    if let Some(Value::Object(widths)) = obj.get_mut("widths") {
        widths.retain(|k, _| visible.contains(k));
    }
    if obj
        .get("sort")
        .is_some_and(|s| !s.get("key").is_some_and(shown))
    {
        obj.remove("sort");
    }
    if let Some(Value::Array(filters)) = obj.get_mut("filters") {
        filters.retain(|f| f.get("key").is_some_and(shown));
    }
}

type ViewRow = (String, String, String, String, i64);

fn row_to_view(r: ViewRow) -> DatasetView {
    DatasetView {
        id: r.0,
        name: r.1,
        kind: r.2,
        config: serde_json::from_str(&r.3).unwrap_or(Value::Null),
        position: r.4,
    }
}

fn view_name(name: &str) -> Result<String, String> {
    clean_label(name).map_err(|m| m.replace("field label", "view name"))
}

impl Store {
    /// The field keys `principal` may see in the dataset. The seam for per-field access:
    /// in M8a no field is hidden from a reader, so this is every key.
    async fn visible_field_keys(
        &self,
        _principal: &Principal,
        doc_id: &str,
    ) -> Result<HashSet<String>> {
        self.all_field_keys(doc_id).await
    }

    async fn all_field_keys(&self, doc_id: &str) -> Result<HashSet<String>> {
        Ok(self
            .fields_of(doc_id)
            .await?
            .into_iter()
            .map(|f| f.key)
            .collect())
    }

    async fn views_of(&self, doc_id: &str) -> Result<Vec<DatasetView>> {
        let rows: Vec<ViewRow> = sqlx::query_as(
            "SELECT id, name, kind, config, position FROM dataset_view \
             WHERE doc_id = ? ORDER BY position, created_at, id",
        )
        .bind(doc_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_view).collect())
    }

    /// The saved views of the dataset at `path`, for anybody who may read it, each with
    /// the fields hidden from this caller removed.
    pub async fn dataset_views(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<SchemaOutcome<Vec<DatasetView>>> {
        let Some(doc) = self.readable_dataset(principal, path).await? else {
            return Ok(SchemaOutcome::NoDataset);
        };
        let visible = self.visible_field_keys(principal, &doc.id).await?;
        let mut views = self.views_of(&doc.id).await?;
        for v in &mut views {
            strip_hidden(&mut v.config, &visible);
        }
        Ok(SchemaOutcome::Done(views))
    }

    /// Save a view. Needs write.
    pub async fn create_dataset_view(
        &self,
        principal: &Principal,
        path: &str,
        name: &str,
        kind: &str,
        config: &Value,
    ) -> Result<SchemaOutcome<DatasetView>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let name = match view_name(name) {
            Ok(n) => n,
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let known = self.all_field_keys(&doc_id).await?;
        if let Err(m) = validate(kind, config, &known) {
            return Ok(SchemaOutcome::Invalid(m));
        }
        let id = uuid::Uuid::now_v7().to_string();
        // Count and insert in one statement so two racing writers cannot pass the cap.
        let done = sqlx::query(
            "INSERT INTO dataset_view (id, doc_id, kind, name, config, position) \
             SELECT ?1, ?2, ?3, ?4, ?5, COALESCE(MAX(position) + 1, 0) \
             FROM dataset_view WHERE doc_id = ?2 HAVING COUNT(*) < ?6",
        )
        .bind(&id)
        .bind(&doc_id)
        .bind(kind)
        .bind(&name)
        .bind(config.to_string())
        .bind(MAX_VIEWS as i64)
        .execute(&self.pool)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(SchemaOutcome::Invalid(format!(
                "a dataset has at most {MAX_VIEWS} views"
            )));
        }
        let view = self
            .views_of(&doc_id)
            .await?
            .into_iter()
            .find(|v| v.id == id);
        Ok(view.map_or(SchemaOutcome::NoField, SchemaOutcome::Done))
    }

    /// Rename a view and/or replace its config. Needs write; the kind never changes.
    pub async fn update_dataset_view(
        &self,
        principal: &Principal,
        path: &str,
        id: &str,
        name: Option<&str>,
        config: Option<&Value>,
    ) -> Result<SchemaOutcome<DatasetView>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let Some(current) = self
            .views_of(&doc_id)
            .await?
            .into_iter()
            .find(|v| v.id == id)
        else {
            return Ok(SchemaOutcome::NoField);
        };
        let name = match name.map(view_name).transpose() {
            Ok(n) => n.unwrap_or(current.name),
            Err(m) => return Ok(SchemaOutcome::Invalid(m)),
        };
        let config = config.cloned().unwrap_or(current.config);
        let known = self.all_field_keys(&doc_id).await?;
        if let Err(m) = validate(&current.kind, &config, &known) {
            return Ok(SchemaOutcome::Invalid(m));
        }
        sqlx::query("UPDATE dataset_view SET name = ?, config = ? WHERE id = ? AND doc_id = ?")
            .bind(&name)
            .bind(config.to_string())
            .bind(id)
            .bind(&doc_id)
            .execute(&self.pool)
            .await?;
        let view = self
            .views_of(&doc_id)
            .await?
            .into_iter()
            .find(|v| v.id == id);
        Ok(view.map_or(SchemaOutcome::NoField, SchemaOutcome::Done))
    }

    /// Delete a view. Needs write.
    pub async fn delete_dataset_view(
        &self,
        principal: &Principal,
        path: &str,
        id: &str,
    ) -> Result<SchemaOutcome<()>> {
        let doc_id = match self.writable_dataset(principal, path).await? {
            Gate::Open(id) => id,
            Gate::NoDataset => return Ok(SchemaOutcome::NoDataset),
            Gate::ReadOnly => return Ok(SchemaOutcome::ReadOnly),
        };
        let gone = sqlx::query("DELETE FROM dataset_view WHERE id = ? AND doc_id = ?")
            .bind(id)
            .bind(&doc_id)
            .execute(&self.pool)
            .await?;
        Ok(if gone.rows_affected() == 0 {
            SchemaOutcome::NoField
        } else {
            SchemaOutcome::Done(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datasets::{NewField, SchemaOutcome};
    use crate::documents::NewDocument;
    use crate::revisions::Author;
    use crate::{CreateOutcome, CreateRequest, Store};
    use gw_auth::{Permission, Principal, Subject};
    use gw_core::{Block, DocumentType, FieldKind, Visibility};
    use serde_json::json;

    async fn fixture() -> (Store, Principal, Principal, Principal, String) {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let body: Block = serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap();
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "raum".into(),
                    slug: Some("raum".into()),
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
        let anna = Principal::test("anna", &[], &[]);
        let bea = Principal::test("bea", &[], &[]);
        let cleo = Principal::test("cleo", &[], &[]);
        for (who, perm) in [(&anna, Permission::Write), (&bea, Permission::Read)] {
            store
                .add_grant("/raum", Subject::Principal(who.id.clone()), perm)
                .await
                .unwrap();
        }
        let req = CreateRequest {
            parent: Some("/raum".into()),
            title: "Tabelle".into(),
            slug: None,
            template: None,
        };
        let CreateOutcome::Created { path, .. } = store
            .create_dataset_for(&anna, &req, false, "d")
            .await
            .unwrap()
        else {
            panic!("no dataset")
        };
        for (k, kind) in [("name", FieldKind::Text), ("alter", FieldKind::Number)] {
            let f = NewField {
                key: k.into(),
                label: k.into(),
                kind,
                config: json!({}),
            };
            store.add_dataset_field(&anna, &path, &f).await.unwrap();
        }
        (store, anna, bea, cleo, path)
    }

    fn table() -> serde_json::Value {
        json!({"fields": ["name", "alter"], "sort": {"key": "alter", "desc": true},
               "filters": [{"op": "contains", "key": "name", "value": "a"}],
               "widths": {"name": 200}})
    }

    async fn made(store: &Store, who: &Principal, path: &str) -> DatasetView {
        match store
            .create_dataset_view(who, path, "Alle", "table", &table())
            .await
            .unwrap()
        {
            SchemaOutcome::Done(v) => v,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_writer_creates_lists_updates_and_deletes_a_view() {
        let (store, anna, bea, _, path) = fixture().await;
        let v = made(&store, &anna, &path).await;
        assert_eq!((v.name.as_str(), v.kind.as_str()), ("Alle", "table"));
        let SchemaOutcome::Done(list) = store.dataset_views(&bea, &path).await.unwrap() else {
            panic!("reader lists")
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["widths"]["name"], 200);
        let out = store
            .update_dataset_view(
                &anna,
                &path,
                &v.id,
                Some("Neu"),
                Some(&json!({"fields": ["name"]})),
            )
            .await
            .unwrap();
        let SchemaOutcome::Done(u) = out else {
            panic!("{out:?}")
        };
        assert_eq!(u.name, "Neu");
        assert_eq!(u.config["fields"], json!(["name"]));
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::Done(())
        );
        assert_eq!(
            store
                .delete_dataset_view(&anna, &path, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
    }

    #[tokio::test]
    async fn view_writes_need_write_and_leave_the_views_unchanged() {
        let (store, anna, bea, cleo, path) = fixture().await;
        let v = made(&store, &anna, &path).await;
        let cfg = table();
        assert_eq!(
            store
                .create_dataset_view(&bea, &path, "X", "table", &cfg)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store
                .update_dataset_view(&bea, &path, &v.id, Some("X"), None)
                .await
                .unwrap(),
            SchemaOutcome::ReadOnly
        );
        assert_eq!(
            store.delete_dataset_view(&bea, &path, &v.id).await.unwrap(),
            SchemaOutcome::ReadOnly
        );
        // A stranger learns nothing: the same verdict as an absent dataset.
        assert_eq!(
            store
                .create_dataset_view(&cleo, &path, "X", "table", &cfg)
                .await
                .unwrap(),
            store
                .create_dataset_view(&cleo, "/raum/nirgends", "X", "table", &cfg)
                .await
                .unwrap()
        );
        assert_eq!(
            store.dataset_views(&cleo, &path).await.unwrap(),
            SchemaOutcome::NoDataset
        );
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Alle");
    }

    #[tokio::test]
    async fn a_config_naming_a_missing_field_or_a_hostile_key_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        for cfg in [
            json!({"fields": ["gibtsnicht"]}),
            json!({"sort": {"key": "gibtsnicht"}}),
            json!({"filters": [{"op": "is_empty", "key": "gibtsnicht"}]}),
            json!({"widths": {"gibtsnicht": 10}}),
            json!({"fields": ["a'; DROP TABLE dataset_row; --"]}),
            json!({"widths": {"name\") OR 1=1": 10}}),
            json!({"unknown": 1}),
            json!([1]),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, "X", "table", &cfg)
                .await
                .unwrap();
            assert!(matches!(out, SchemaOutcome::Invalid(_)), "{cfg}: {out:?}");
        }
        for (name, kind) in [
            ("", "table"),
            ("  ", "table"),
            ("x", "nonsense"),
            ("x", "board"),
        ] {
            let out = store
                .create_dataset_view(&anna, &path, name, kind, &table())
                .await
                .unwrap();
            assert!(
                matches!(out, SchemaOutcome::Invalid(_)),
                "{name}/{kind}: {out:?}"
            );
        }
        // An update is validated the same way, and a refused update changes nothing.
        let v = made(&store, &anna, &path).await;
        let out = store
            .update_dataset_view(&anna, &path, &v.id, None, Some(&json!({"fields": ["weg"]})))
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].config["widths"]["name"], 200);
    }

    #[tokio::test]
    async fn the_hidden_field_filter_drops_hidden_keys_from_a_listed_view() {
        let (_, _, _, _, _) = fixture().await;
        let mut cfg = table();
        let visible: std::collections::HashSet<String> = ["name".to_string()].into();
        strip_hidden(&mut cfg, &visible);
        assert_eq!(cfg["fields"], json!(["name"]));
        assert!(cfg.get("sort").is_none());
        assert_eq!(cfg["filters"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["widths"], json!({"name": 200}));
    }

    #[tokio::test]
    async fn views_go_with_their_dataset_and_are_bounded() {
        let (store, anna, _, _, path) = fixture().await;
        made(&store, &anna, &path).await;
        let n = || async {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM dataset_view")
                .fetch_one(&store.pool)
                .await
                .unwrap()
        };
        assert_eq!(n().await, 1);
        sqlx::query("DELETE FROM documents")
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(n().await, 0);
    }

    #[tokio::test]
    async fn the_fifty_first_view_is_refused() {
        let (store, anna, _, _, path) = fixture().await;
        for _ in 0..MAX_VIEWS {
            made(&store, &anna, &path).await;
        }
        let out = store
            .create_dataset_view(&anna, &path, "x", "table", &table())
            .await
            .unwrap();
        assert!(matches!(out, SchemaOutcome::Invalid(_)), "{out:?}");
    }

    #[tokio::test]
    async fn a_view_id_from_another_dataset_is_not_reachable() {
        let (store, anna, _, _, path) = fixture().await;
        let v = made(&store, &anna, &path).await;
        let req = CreateRequest {
            parent: Some("/raum".into()),
            title: "Zweite".into(),
            slug: None,
            template: None,
        };
        let CreateOutcome::Created { path: other, .. } = store
            .create_dataset_for(&anna, &req, false, "d")
            .await
            .unwrap()
        else {
            panic!("no second dataset")
        };
        assert_eq!(
            store
                .delete_dataset_view(&anna, &other, &v.id)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
        assert_eq!(
            store
                .update_dataset_view(&anna, &other, &v.id, Some("X"), None)
                .await
                .unwrap(),
            SchemaOutcome::NoField
        );
        let SchemaOutcome::Done(list) = store.dataset_views(&anna, &path).await.unwrap() else {
            panic!()
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Alle");
    }
}
