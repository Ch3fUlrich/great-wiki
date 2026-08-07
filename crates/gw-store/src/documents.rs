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

    /// A document with NO permission check whatsoever.
    ///
    /// Crate-private, and named so the danger is unmissable at every call site. The one
    /// public way to obtain a document is [`Store::document_for`], which takes a principal;
    /// that is the invariant M2 exists to establish — no code outside `gw-store` can hold
    /// an unfiltered document — and a `pub` spelling of this method is precisely how a
    /// later handler would leak one by forgetting to filter.
    pub(crate) async fn document_by_path_unchecked(
        &self,
        path: &str,
    ) -> Result<Option<StoredDocument>> {
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

    /// Whether anything lives at `path`. Public because it is the *only* thing a caller
    /// outside this crate is allowed to learn without a principal.
    ///
    /// The HTTP layer needs it to tell 404 from 403: [`Store::document_for`] returns `None`
    /// both for a path that is absent and for one the caller may not have, and collapsing
    /// the two either hides configuration mistakes or confirms the existence of every path
    /// somebody guesses. The seeder needs it too, to refuse to invent a parent and to name
    /// a collision. A boolean is the whole answer, so this discloses strictly less than the
    /// response it is used to choose.
    pub async fn document_exists(&self, path: &str) -> Result<bool> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM documents WHERE path = ?1 AND deleted_at IS NULL")
                .bind(path)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.is_some())
    }

    /// The whole tree, nested.
    ///
    /// Fetched flat in one query and assembled in memory rather than issuing a query per
    /// level: a wiki's tree is small enough that one round trip beats N, and the ordering
    /// is then unambiguous.
    ///
    /// UNFILTERED, and crate-private for exactly that reason. [`Store::tree_for`] takes a
    /// principal and is the only tree accessor reachable from outside this crate: a
    /// restricted title in the navigation is a disclosure even when the body is protected.
    pub(crate) async fn tree(&self) -> Result<Vec<TreeNode>> {
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
