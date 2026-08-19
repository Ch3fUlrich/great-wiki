use crate::revisions::{append_revision, Author};
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
    /// Create a document **and publish its first revision**, atomically.
    ///
    /// Named `create_document` rather than `insert_document` because it is no longer one
    /// INSERT: a page and the revision its body came from arrive together or not at all.
    /// The old name was accurate about the SQL and wrong about the model — it wrote
    /// `documents.body` directly, which made it a second write path beside the revision
    /// system (AGENTS.md rule 1) and left every imported page with an empty history, so its
    /// first edit became revision 1 with nothing to diff against.
    ///
    /// **Atomicity.** One transaction covers the document INSERT, the revision INSERT and
    /// the `current_revision_id` update. A failure anywhere inside it — a path collision, a
    /// refused author, a trigger — rolls back the whole thing, so there is no ordering in
    /// which a page ends up with a body and no revision, or a revision and no page. The
    /// revision half is [`append_revision`], the same function [`Store::publish_revision`]
    /// uses, rather than a second copy of the same SQL.
    ///
    /// **Authorisation is NOT decided here**, deliberately. There is no permission-checked
    /// "create a document" in this system yet: nothing HTTP creates one, and the only caller
    /// is the importer, which asks its own question (write on the parent) and documents why.
    /// Answering it here as well would put a second, weaker rule next to the real one. What
    /// *is* enforced here is what an author is — an `Author::Account` must be signed in and
    /// active — because that is a property of a revision, not a policy about documents.
    ///
    /// `summary` is revision 1's summary: where this page came from, in the history, from
    /// the beginning. `None` is fine; a made-up one would be worse than none.
    pub async fn create_document(
        &self,
        author: Author<'_>,
        doc: &NewDocument,
        summary: Option<&str>,
    ) -> Result<String> {
        let slug = doc.resolved_slug()?;
        let path = doc.resolved_path()?;
        let id = uuid::Uuid::now_v7().to_string();
        let body = serde_json::to_string(&doc.body)?;

        let mut tx = self.pool.begin().await?;

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
        .execute(&mut *tx)
        .await?;

        // `body` is bound above because the column is NOT NULL, and written again here as
        // the revision's body. The second write is the one that matters: it is what makes
        // `current_revision_id` point at a row that exists, and it keeps the "a body changes
        // only through a revision" rule true of creation as well as of editing.
        append_revision(
            &mut tx,
            &id,
            author,
            &body,
            summary,
            self.public_origin.as_ref(),
        )
        .await?;

        tx.commit().await?;
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

#[cfg(test)]
mod tests {
    //! Creating a document, and the two things it must never do: leave a page with no
    //! history, or leave half of one behind.

    use crate::{Author, NewDocument, Store, IMPORT_AUTHOR_ID, IMPORT_AUTHOR_NAME};
    use gw_core::{Block, DocumentType, Visibility};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    fn body(text: &str) -> Block {
        serde_json::from_str(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}"#
        ))
        .unwrap()
    }

    fn new_doc(text: &str) -> NewDocument {
        NewDocument {
            parent_path: None,
            doc_type: DocumentType::Page,
            title: "Notiz".into(),
            slug: None,
            language: "de".into(),
            visibility: Visibility::Public,
            body: body(text),
            sort_key: 0,
        }
    }

    /// How many rows are in a table, straight from SQL.
    ///
    /// The permission-checked accessors are the right way to read content, and the wrong
    /// way to ask "did anything at all survive that failure": they answer `None` for
    /// "absent" and for "not permitted" alike, which is exactly the distinction these tests
    /// are about.
    async fn count(store: &Store, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    // --- what creating a page leaves behind ---------------------------------------------

    #[tokio::test]
    async fn creating_a_page_publishes_revision_one_saying_the_same_thing() {
        let store = store().await;
        let id = store
            .create_document(Author::Import, &new_doc("hallo"), Some("aus notiz.md"))
            .await
            .unwrap();

        let doc = store
            .document_by_path_unchecked("/notiz")
            .await
            .unwrap()
            .unwrap();
        let revisions: Vec<(String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, body, parent_id, summary FROM revisions WHERE document_id = ?1",
        )
        .bind(&id)
        .fetch_all(&store.pool)
        .await
        .unwrap();

        assert_eq!(revisions.len(), 1, "a new page starts with exactly one");
        let (rev_id, rev_body, parent, summary) = &revisions[0];
        assert_eq!(
            rev_body, &doc.body,
            "the revision must be what the page says"
        );
        assert!(parent.is_none(), "revision 1 has nothing behind it");
        assert_eq!(summary.as_deref(), Some("aus notiz.md"));

        let current: Option<String> =
            sqlx::query_scalar("SELECT current_revision_id FROM documents WHERE id = ?1")
                .bind(&id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(
            current.as_deref(),
            Some(rev_id.as_str()),
            "the page must point at the revision its body came from, or the next edit's \
             parent is a guess at `the newest one`"
        );
    }

    #[tokio::test]
    async fn a_named_account_authors_the_page_it_creates() {
        // `seed --as <account>`, and the future editor: the person is the author of
        // revision 1, exactly as they are the author of revision 2.
        let store = store().await;
        let autorin = store
            .create_local_principal("smaulser", "Sergej Maulser", None, "x")
            .await
            .unwrap();
        let id = store
            .create_document(Author::Account(&autorin), &new_doc("hallo"), None)
            .await
            .unwrap();

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs[0].author_id, autorin.id);
        assert_eq!(revs[0].author_name, "Sergej Maulser");
        assert!(revs[0].author_is_an_account());
    }

    #[tokio::test]
    async fn an_operator_import_is_attributed_to_nobody_and_says_so() {
        // The bootstrap case: `seed` with no `--as` has no identity to record, so it records
        // that instead of borrowing one. Both halves matter — the id, because that is what
        // code checks, and the name, because that is what a reader sees.
        let store = store().await;
        let id = store
            .create_document(Author::Import, &new_doc("hallo"), None)
            .await
            .unwrap();
        let admin = gw_auth::Principal::test("chefin", &["admins"], &[]);

        let revs = store.revisions_for(&admin, &id).await.unwrap();
        assert_eq!(revs[0].author_id, IMPORT_AUTHOR_ID);
        assert_eq!(revs[0].author_name, IMPORT_AUTHOR_NAME);
        assert!(
            !revs[0].author_is_an_account(),
            "a byline renderer must be able to tell this apart from a person"
        );
    }

    #[tokio::test]
    async fn the_import_author_is_an_id_no_account_can_hold() {
        // What stops the import byline from ever being mistaken for somebody: it is not
        // merely an unusual name, it is an id outside the space accounts are minted in.
        // Every `principals.id` is a uuid v7 chosen inside this crate — no caller supplies
        // one — so this cannot collide today and cannot be claimed later.
        let store = store().await;
        for username in ["import", "system", "system:import"] {
            let created = store
                .create_local_principal(username, "Wer auch immer", None, "x")
                .await
                .unwrap();
            assert_ne!(
                created.id, IMPORT_AUTHOR_ID,
                "an account was minted holding the import author's id"
            );
        }
        assert!(
            store
                .principal_by_id(IMPORT_AUTHOR_ID)
                .await
                .unwrap()
                .is_none(),
            "the import author must never resolve to an account"
        );
        assert!(
            IMPORT_AUTHOR_ID.contains(':'),
            "a uuid cannot contain a colon, and that is the whole reason this is safe"
        );
    }

    // --- and what a failure half-way through must NOT leave behind -----------------------

    #[tokio::test]
    async fn a_failure_writing_the_revision_takes_the_document_with_it() {
        // The atomicity claim, forced rather than argued. A trigger makes the revision
        // INSERT fail after the document INSERT has already succeeded — the exact ordering
        // in which a page could end up with a body and no history — and the page must not
        // be there afterwards.
        let store = store().await;
        sqlx::query(
            "CREATE TRIGGER no_revisions BEFORE INSERT ON revisions \
             BEGIN SELECT RAISE(ABORT, 'kaputt'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        let outcome = store
            .create_document(Author::Import, &new_doc("hallo"), None)
            .await;
        assert!(outcome.is_err(), "the failed revision was not reported");

        assert_eq!(
            count(&store, "documents").await,
            0,
            "a page with no history"
        );
        assert_eq!(count(&store, "revisions").await, 0);
        assert!(!store.document_exists("/notiz").await.unwrap());
    }

    #[tokio::test]
    async fn creating_as_nobody_writes_neither_a_document_nor_a_revision() {
        // The other way the same half state could arise: an author the revision cannot be
        // filed under. The refusal happens INSIDE the transaction, after the document row
        // exists, so this is an atomicity test as much as an authorship one — and it is the
        // check that stops a future HTTP route from creating pages for a caller who never
        // said who they were.
        let store = store().await;
        let deactivated = {
            let mut principal = gw_auth::Principal::test("weg", &[], &[]);
            principal.active = false;
            principal
        };

        for author in [
            Author::Account(&gw_auth::Principal::anonymous()),
            Author::Account(&deactivated),
        ] {
            let outcome = store.create_document(author, &new_doc("hallo"), None).await;
            let error = outcome.expect_err("a page was created with nobody to attribute it to");
            assert!(
                error.to_string().contains("signed-in, active account"),
                "{error}"
            );
        }

        assert_eq!(count(&store, "documents").await, 0);
        assert_eq!(count(&store, "revisions").await, 0);
    }

    #[tokio::test]
    async fn a_colliding_path_leaves_no_revision_behind_either() {
        // The failure that happens BEFORE the revision, rather than after it. Nothing may
        // accumulate: a revision row pointing at a document that was rolled back would be
        // history nothing can reach and nothing can permission-check.
        let store = store().await;
        store
            .create_document(Author::Import, &new_doc("eins"), None)
            .await
            .unwrap();
        assert!(store
            .create_document(Author::Import, &new_doc("zwei"), None)
            .await
            .is_err());

        assert_eq!(count(&store, "documents").await, 1);
        assert_eq!(count(&store, "revisions").await, 1);
    }
}
