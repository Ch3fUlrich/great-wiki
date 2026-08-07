use crate::Store;
use anyhow::Result;
use gw_core::{slugify, Block, DocumentType, Visibility};
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone)]
pub struct NewDocument {
    pub parent_path: Option<String>,
    pub doc_type: DocumentType,
    pub title: String,
    /// When `None`, derived from `title`. An explicit slug wins so a long title can have
    /// a short URL.
    pub slug: Option<String>,
    pub language: String,
    pub visibility: Visibility,
    pub body: Block,
    pub sort_key: i64,
}

impl NewDocument {
    /// The slug this document will occupy: an explicit one wins, otherwise the title.
    ///
    /// Public because a caller that inserts many documents — the seeder, the M13 importer
    /// — has to know the resulting path *before* the insert in order to report a
    /// collision usefully. Re-deriving it at the call site would put two copies of this
    /// rule in the tree, and the day they disagree the error message points at the wrong
    /// path.
    pub fn resolved_slug(&self) -> Result<String> {
        let slug = self
            .slug
            .as_deref()
            .map(slugify)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| slugify(&self.title));
        anyhow::ensure!(
            !slug.is_empty(),
            "title `{}` produced an empty slug",
            self.title
        );
        Ok(slug)
    }

    /// The materialised path this document will occupy.
    pub fn resolved_path(&self) -> Result<String> {
        let parent = self.parent_path.as_deref().unwrap_or("");
        Ok(format!("{parent}/{}", self.resolved_slug()?))
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct StoredDocument {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub slug: String,
    pub doc_type: String,
    pub title: String,
    pub language: String,
    pub visibility: String,
    /// The Block tree as JSON. Deserialised by the caller so the store stays agnostic
    /// about the content model's version.
    pub body: String,
    pub sort_key: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub path: String,
    pub slug: String,
    pub title: String,
    pub doc_type: String,
    pub visibility: String,
    pub children: Vec<TreeNode>,
}

impl Store {
    pub async fn insert_document(&self, doc: &NewDocument) -> Result<String> {
        let slug = doc.resolved_slug()?;
        let path = doc.resolved_path()?;
        let id = uuid::Uuid::now_v7().to_string();
        let body = serde_json::to_string(&doc.body)?;

        // The UNIQUE constraint on `path` is what turns a slug collision into an error
        // instead of a silent overwrite. Do NOT add ON CONFLICT here.
        sqlx::query(
            r#"
            INSERT INTO documents
              (id, parent_path, path, slug, doc_type, title, language, visibility, body, sort_key)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&id)
        .bind(doc.parent_path.as_deref())
        .bind(&path)
        .bind(&slug)
        .bind(doc.doc_type.as_str())
        .bind(&doc.title)
        .bind(&doc.language)
        .bind(doc.visibility.as_str())
        .bind(&body)
        .bind(doc.sort_key)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    pub async fn document_by_path(&self, path: &str) -> Result<Option<StoredDocument>> {
        let row = sqlx::query_as::<_, StoredDocument>(
            r#"
            SELECT id, path, parent_path, slug, doc_type, title, language, visibility, body, sort_key
            FROM documents
            WHERE path = ?1 AND deleted_at IS NULL
            "#,
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// The whole tree, nested.
    ///
    /// Fetched flat in one query and assembled in memory rather than issuing a query per
    /// level: a wiki's tree is small enough that one round trip beats N, and the ordering
    /// is then unambiguous.
    ///
    /// UNFILTERED. Use [`Store::tree_for`], which takes a principal and is the only tree
    /// accessor any caller should ever reach for.
    ///
    /// STILL BLOCKED, and this is the one thing M2 is not yet finished with: this method
    /// must become `pub(crate)`, so that no caller outside this crate can obtain an
    /// unfiltered tree and no later handler can leak one by forgetting to filter. The
    /// change is one word, but it does not compile until the three remaining callers are
    /// gone, and all three live in `gw-api`:
    ///
    /// - `gw-api/src/routes/tree.rs` — the M1 handler, which post-filters with `may_read`
    ///   (M2 Task 4 deletes both, routing the handler through `tree_for` instead);
    /// - `gw-api/tests/seed.rs`, two assertions that only need "is the tree empty".
    ///
    /// `gw-api` does not depend on `gw-auth`, so it cannot construct a `Principal` to call
    /// `tree_for` with until Task 4 adds that dependency. Flipping this word is therefore
    /// the FIRST step of Task 4, not the last step of Task 3.
    #[doc(hidden)]
    pub async fn tree(&self) -> Result<Vec<TreeNode>> {
        #[derive(FromRow)]
        struct Row {
            path: String,
            parent_path: Option<String>,
            slug: String,
            title: String,
            doc_type: String,
            visibility: String,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT path, parent_path, slug, title, doc_type, visibility
            FROM documents
            WHERE deleted_at IS NULL
            ORDER BY parent_path NULLS FIRST, sort_key, slug
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        fn build(rows: &[Row], parent: Option<&str>) -> Vec<TreeNode> {
            rows.iter()
                .filter(|r| r.parent_path.as_deref() == parent)
                .map(|r| TreeNode {
                    path: r.path.clone(),
                    slug: r.slug.clone(),
                    title: r.title.clone(),
                    doc_type: r.doc_type.clone(),
                    visibility: r.visibility.clone(),
                    children: build(rows, Some(&r.path)),
                })
                .collect()
        }

        Ok(build(&rows, None))
    }
}
