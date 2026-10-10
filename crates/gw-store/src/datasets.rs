//! Datasets (ADR 0029): a dataset is a page of type `dataset`; its typed schema lives in
//! `dataset_field` and its rows in `dataset_row` (one JSON object of values per row).
//! Rows inherit the page's access; there is no per-row ACL. Migration 0020 owns the tables.

use crate::{CreateOutcome, CreateRequest, Store};
use anyhow::Result;
use gw_auth::Principal;
use gw_core::DocumentType;

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
}
