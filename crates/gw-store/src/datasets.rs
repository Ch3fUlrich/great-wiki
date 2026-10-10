//! Datasets (ADR 0029): a dataset is a page of type `dataset`; its typed schema lives in
//! `dataset_field` and its rows in `dataset_row` (one JSON object of values per row).
//! Rows inherit the page's access; there is no per-row ACL. Migration 0020 owns the tables.

#[cfg(test)]
mod tests {
    use crate::documents::NewDocument;
    use crate::revisions::Author;
    use crate::Store;
    use gw_core::{Block, DocumentType, FieldKind, Visibility};

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
