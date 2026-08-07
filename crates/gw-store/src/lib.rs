pub mod documents;

pub use documents::{NewDocument, StoredDocument, TreeNode};

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub struct Store {
    pub pool: SqlitePool,
}

impl Store {
    /// Open (creating if needed) and migrate.
    ///
    /// `max_connections(1)` is deliberate and load-bearing for tests: an in-memory SQLite
    /// database is private per connection, so a larger pool silently gives some queries an
    /// empty database. Production is a single-writer workload anyway.
    pub async fn open(url: &str) -> Result<Self> {
        ensure_parent_dir(url)?;

        let opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}

/// Create the directory the database file lives in.
///
/// `create_if_missing` creates the *file* and never its directory, and the default
/// configuration points at `./data/great-wiki.db` — a path a fresh clone does not have.
/// Without this, the first command anyone runs after cloning fails on a directory the
/// application itself chose.
fn ensure_parent_dir(url: &str) -> Result<()> {
    // In-memory databases have no path. `mode=memory` is the URI spelling of the same.
    if url.contains(":memory:") || url.contains("mode=memory") {
        return Ok(());
    }
    let path = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .unwrap_or(url);
    // Query parameters are part of the URL, not of the filename.
    let path = path.split('?').next().unwrap_or(path);

    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_core::{Block, DocumentType, Visibility};

    fn body(text: &str) -> Block {
        serde_json::from_str(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}"#
        ))
        .unwrap()
    }

    fn new_doc(parent: Option<&str>, title: &str, vis: Visibility) -> NewDocument {
        NewDocument {
            parent_path: parent.map(str::to_string),
            doc_type: DocumentType::Page,
            title: title.to_string(),
            slug: None,
            language: "de".into(),
            visibility: vis,
            body: body("hallo"),
            sort_key: 0,
        }
    }

    #[tokio::test]
    async fn insert_then_fetch_by_path() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .insert_document(&new_doc(None, "Größe und Maß", Visibility::Public))
            .await
            .unwrap();

        let got = store
            .document_by_path("/groesse-und-mass")
            .await
            .unwrap()
            .expect("document should exist");
        assert_eq!(got.title, "Größe und Maß");
        assert_eq!(got.visibility, "public");
        assert!(got.body.contains("hallo"));
    }

    #[tokio::test]
    async fn path_is_derived_from_parent_and_slug() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .insert_document(&new_doc(None, "Handbuch", Visibility::Public))
            .await
            .unwrap();
        store
            .insert_document(&new_doc(
                Some("/handbuch"),
                "Erste Schritte",
                Visibility::Public,
            ))
            .await
            .unwrap();

        let child = store
            .document_by_path("/handbuch/erste-schritte")
            .await
            .unwrap();
        assert!(child.is_some(), "child path must nest under its parent");
    }

    #[tokio::test]
    async fn duplicate_path_is_rejected_rather_than_overwriting() {
        // Silent overwrite on slug collision is data loss. It must be an error.
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .insert_document(&new_doc(None, "Notes", Visibility::Public))
            .await
            .unwrap();
        let second = store
            .insert_document(&new_doc(None, "Notes", Visibility::Public))
            .await;
        assert!(second.is_err(), "a colliding path must fail loudly");
    }

    #[tokio::test]
    async fn opening_a_database_creates_the_directory_it_lives_in() {
        // The default configuration points at ./data/great-wiki.db, which a fresh clone
        // does not have. `create_if_missing` creates the file, never the directory.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("nested/great-wiki.db");
        let store = Store::open(&format!("sqlite://{}", db.display()))
            .await
            .unwrap();
        assert!(db.exists());
        drop(store);
    }

    #[tokio::test]
    async fn resolved_path_matches_the_path_the_insert_actually_uses() {
        // The seeder pre-checks collisions with `resolved_path`. If it could disagree
        // with the insert, the error message would name a path that is not the one that
        // collided.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let doc = new_doc(Some("/handbuch"), "Größe und Maß", Visibility::Public);
        store
            .insert_document(&new_doc(None, "Handbuch", Visibility::Public))
            .await
            .unwrap();
        store.insert_document(&doc).await.unwrap();
        assert_eq!(doc.resolved_path().unwrap(), "/handbuch/groesse-und-mass");
        assert!(store
            .document_by_path(&doc.resolved_path().unwrap())
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn missing_document_is_none_not_an_error() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        assert!(store.document_by_path("/nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn tree_nests_children_under_parents_in_sort_order() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .insert_document(&new_doc(None, "Handbuch", Visibility::Public))
            .await
            .unwrap();

        let mut b = new_doc(Some("/handbuch"), "Beta", Visibility::Public);
        b.sort_key = 2;
        let mut a = new_doc(Some("/handbuch"), "Alpha", Visibility::Public);
        a.sort_key = 1;
        store.insert_document(&b).await.unwrap();
        store.insert_document(&a).await.unwrap();

        let tree = store.tree().await.unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].path, "/handbuch");
        let kids: Vec<&str> = tree[0].children.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(
            kids,
            vec!["alpha", "beta"],
            "children must respect sort_key"
        );
    }

    #[tokio::test]
    async fn soft_deleted_documents_are_excluded() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store
            .insert_document(&new_doc(None, "Temporär", Visibility::Public))
            .await
            .unwrap();
        sqlx::query("UPDATE documents SET deleted_at = datetime('now') WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();

        assert!(store
            .document_by_path("/temporaer")
            .await
            .unwrap()
            .is_none());
        assert!(store.tree().await.unwrap().is_empty());
    }
}
