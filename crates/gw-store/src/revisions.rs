//! Revisions: the append-only history under every page.
//!
//! Two properties hold this file together, and both are load-bearing rather than tidy.
//!
//! **Append-only.** A revision is written once and never updated. Restoring publishes the
//! old content as a NEW revision, so everything it was restored past is still there
//! afterwards — a restore that rewinds is a way to lose the version you were unsure about,
//! which is the opposite of what a history is for. The schema enforces the "never updated"
//! half with a trigger (0008), because a convention only binds the code that exists today.
//!
//! **A revision records WHO, and who is the authenticated principal.** Never a name a
//! caller supplied: the byline is what a reader trusts to answer "who wrote this", and a
//! byline anybody can choose answers nothing. The author's id and a snapshot of their
//! display name are both stored — see the migration for why both.
//!
//! Everything public here takes a `Principal` and goes through [`Store::document_for`],
//! which is the crate's single permission-checked document accessor. A revision body IS
//! page content: handing one to somebody who cannot read the page is the same disclosure
//! as handing them the page, and it is the retriever's job to prevent it rather than the
//! caller's (AGENTS.md, architecture rule 2).

use crate::Store;
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::Block;
use serde::Serialize;
use sqlx::FromRow;

/// The columns of a revision, in the order [`Revision`] declares them.
///
/// One definition so the two readers below cannot drift apart — and so that adding a
/// column is one edit rather than a hunt for the reader that was missed.
const REVISION_COLUMNS: &str = "id, document_id, parent_id, body, summary, author_id, \
                                author_name, byte_size, created_at";

/// One published version of a document.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Revision {
    pub id: String,
    pub document_id: String,
    /// What this was published on top of. `None` for the first revision of a document.
    pub parent_id: Option<String>,
    /// The Block tree as JSON, deserialised by the caller — exactly like
    /// [`crate::StoredDocument::body`], so the store stays agnostic about the content
    /// model's version.
    pub body: String,
    pub summary: Option<String>,
    pub author_id: String,
    /// The display name as it was when this was published. Deliberately denormalised:
    /// history must remain attributable after the account is deleted (D-M3-4), and a join
    /// would render every revision by a departed colleague as "unknown".
    pub author_name: String,
    pub byte_size: i64,
    pub created_at: String,
}

impl Store {
    /// Whether `principal` may `action` the document that `document_id` names.
    ///
    /// Resolves the id to a path and then asks [`Store::document_for`], which is the one
    /// permission-checked document accessor in this crate. Deciding it here instead would
    /// be a second authorisation path, and the second one is always the one that gets it
    /// wrong — it is the one nobody remembers when the rules change.
    ///
    /// `false` covers "no such document" as well as "not permitted", exactly as
    /// `document_for` returns `None` for both: this layer does not decide whether existence
    /// may be revealed, the HTTP layer does. The round trip cannot land on a different
    /// document, because `documents.path` is UNIQUE across every row including soft-deleted
    /// ones — and a soft-deleted document is refused here for the same reason it is refused
    /// there, since `document_for` will not resolve it.
    async fn may(&self, principal: &Principal, document_id: &str, action: Action) -> Result<bool> {
        let path: Option<String> = sqlx::query_scalar("SELECT path FROM documents WHERE id = ?1")
            .bind(document_id)
            .fetch_optional(&self.pool)
            .await?;
        let Some(path) = path else {
            return Ok(false);
        };
        Ok(self.document_for(principal, &path, action).await?.is_some())
    }

    /// A revision with NO permission check whatsoever.
    ///
    /// Crate-private and named so the danger is unmissable, exactly as
    /// [`Store::document_by_path_unchecked`] is. It exists because authorising a revision
    /// means knowing which document it belongs to, and that is a chicken-and-egg the public
    /// accessors resolve by looking the row up first and refusing afterwards.
    async fn revision_unchecked(&self, revision_id: &str) -> Result<Option<Revision>> {
        Ok(sqlx::query_as::<_, Revision>(&format!(
            "SELECT {REVISION_COLUMNS} FROM revisions WHERE id = ?1"
        ))
        .bind(revision_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Append a revision and point the document at it.
    ///
    /// `Ok(None)` means the document is not there or `author` may not write it — the same
    /// conflation [`Store::document_for`] makes, and for the same reason.
    ///
    /// The revision and the document body are written in ONE transaction, so the document
    /// can never point at a revision that does not exist, nor hold content with no revision
    /// behind it. `parent_id` is read inside that transaction rather than before it, so it
    /// is what the document actually pointed at when this revision landed.
    pub async fn publish_revision(
        &self,
        author: &Principal,
        document_id: &str,
        body: &Block,
        summary: Option<&str>,
    ) -> Result<Option<String>> {
        // WHO is established before WHETHER. A revision names an author, and an author is
        // a signed-in, active account. `can()` deliberately answers an `Anyone` grant
        // before it looks at authentication at all — that is what a public share link is —
        // so on a path carrying `anyone: write` the permission check alone would accept an
        // edit from a request that never said who it was and file it in the history under
        // nobody. `an_anonymous_caller_cannot_publish_even_where_anyone_may_write` is what
        // proves this line does something.
        if !author.is_authenticated() || !author.active {
            return Ok(None);
        }

        // Writing is only ever an explicit grant (D-M2-8): no baseline confers it, not
        // even the admin one, and reading a page never implies editing it.
        if !self.may(author, document_id, Action::Write).await? {
            return Ok(None);
        }

        let json = serde_json::to_string(body)?;
        let id = uuid::Uuid::now_v7().to_string();
        let size = json.len() as i64;

        let mut tx = self.pool.begin().await?;

        let parent: Option<String> =
            sqlx::query_scalar("SELECT current_revision_id FROM documents WHERE id = ?1")
                .bind(document_id)
                .fetch_optional(&mut *tx)
                .await?
                .flatten();

        sqlx::query(
            "INSERT INTO revisions \
             (id, document_id, parent_id, body, summary, author_id, author_name, byte_size) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(&id)
        .bind(document_id)
        .bind(parent)
        .bind(&json)
        .bind(summary)
        .bind(&author.id)
        .bind(byline(author))
        .bind(size)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE documents SET body = ?2, current_revision_id = ?3, \
             updated_at = datetime('now') WHERE id = ?1",
        )
        .bind(document_id)
        .bind(&json)
        .bind(&id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(Some(id))
    }

    /// The history of a document, newest first — for a caller who may read the document.
    ///
    /// Seeing history follows read (D-M3-5): anyone who can read a page can read how it
    /// got that way. An empty list is the answer both for a page with no revisions and for
    /// one this caller may not read, which is the same closed conflation everywhere else
    /// in this crate.
    ///
    /// Ordered by `created_at` and then by `id`, because `datetime('now')` has one-second
    /// resolution: without the tie-break, two edits made in the same second come back in
    /// an arbitrary order and the list contradicts the `parent_id` chain it is drawn from.
    pub async fn revisions_for(
        &self,
        principal: &Principal,
        document_id: &str,
    ) -> Result<Vec<Revision>> {
        if !self.may(principal, document_id, Action::Read).await? {
            return Ok(Vec::new());
        }
        Ok(sqlx::query_as::<_, Revision>(&format!(
            "SELECT {REVISION_COLUMNS} FROM revisions \
             WHERE document_id = ?1 ORDER BY created_at DESC, id DESC"
        ))
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// One revision, body and all — for a caller who may read the document it belongs to.
    ///
    /// The row is looked up before the permission check because the check needs to know
    /// which document it concerns. Nothing is returned until the check has passed.
    pub async fn revision_for(
        &self,
        principal: &Principal,
        revision_id: &str,
    ) -> Result<Option<Revision>> {
        let Some(revision) = self.revision_unchecked(revision_id).await? else {
            return Ok(None);
        };
        let readable = self
            .may(principal, &revision.document_id, Action::Read)
            .await?;
        if !readable {
            return Ok(None);
        }
        Ok(Some(revision))
    }

    /// Restore by publishing the old content as a NEW revision.
    ///
    /// Never by rewinding. A restore that destroys the revisions after it turns "let me
    /// look at the old version" into data loss, and it does so at the exact moment
    /// somebody is least sure of themselves. Appending costs one row and keeps both
    /// versions, so the restore is itself undoable — by restoring the other one.
    ///
    /// Restoring follows write (D-M3-5): it changes what the page says, so it is an edit
    /// like any other. That is enforced by the [`Store::publish_revision`] this delegates
    /// to and NOT checked again here — one restore, one authorisation decision, in the
    /// place that performs the write. A second check would be a second thing to forget to
    /// update, and it would silently make the first one untestable.
    pub async fn restore_revision(
        &self,
        author: &Principal,
        revision_id: &str,
    ) -> Result<Option<String>> {
        let Some(rev) = self.revision_unchecked(revision_id).await? else {
            return Ok(None);
        };
        let body: Block = serde_json::from_str(&rev.body)?;
        let summary = format!("Restored revision {}", short(&rev.id));
        self.publish_revision(author, &rev.document_id, &body, Some(&summary))
            .await
    }
}

/// The name a revision is filed under: the display name, falling back to the username.
///
/// The fallback is not decoration. `author_name` is `NOT NULL` and is what the timeline
/// renders, so a principal that somehow reached here with an empty display name would
/// produce a byline of nothing at all — a revision that looks unattributed while being
/// perfectly attributable. The username always exists, because
/// [`Principal::is_authenticated`] requires it.
fn byline(author: &Principal) -> &str {
    let name = author.display_name.trim();
    if name.is_empty() {
        &author.username
    } else {
        name
    }
}

/// The first characters of a uuid — enough to recognise, short enough to read.
fn short(id: &str) -> &str {
    &id[..8.min(id.len())]
}

#[cfg(test)]
mod tests {
    use crate::{NewDocument, Store};
    use gw_auth::{Permission, Principal, Subject};
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

    /// A page at `/notiz`, at the visibility asked for.
    async fn page(store: &Store, visibility: Visibility) -> String {
        store
            .insert_document(&NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: "Notiz".into(),
                slug: None,
                language: "de".into(),
                visibility,
                body: body("hallo"),
                sort_key: 0,
            })
            .await
            .unwrap()
    }

    /// Somebody holding exactly `permission` on `/notiz`.
    ///
    /// Every fixture here carries an explicit grant because writing is only ever an
    /// explicit grant (D-M2-8): no baseline confers it, not even the admin one.
    async fn granted(store: &Store, username: &str, permission: Permission) -> Principal {
        let principal = Principal::test(username, &[], &[]);
        store
            .add_grant(
                "/notiz",
                Subject::Principal(principal.id.clone()),
                permission,
            )
            .await
            .unwrap();
        principal
    }

    async fn writer(store: &Store) -> Principal {
        granted(store, "autorin", Permission::Write).await
    }

    // --- what a publish does -----------------------------------------------------------

    #[tokio::test]
    async fn publishing_creates_a_revision_and_advances_the_document() {
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        let rev = store
            .publish_revision(&autorin, &id, &body("erste Fassung"), Some("initial"))
            .await
            .unwrap()
            .expect("the writer may publish");

        let doc = store
            .document_by_path_unchecked("/notiz")
            .await
            .unwrap()
            .unwrap();
        assert!(doc.body.contains("erste Fassung"), "got {}", doc.body);
        assert_eq!(store.revisions_for(&autorin, &id).await.unwrap().len(), 1);
        assert_eq!(
            store
                .revision_for(&autorin, &rev)
                .await
                .unwrap()
                .unwrap()
                .summary
                .as_deref(),
            Some("initial")
        );
    }

    #[tokio::test]
    async fn each_revision_links_to_its_parent() {
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        let first = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();
        let second = store
            .publish_revision(&autorin, &id, &body("zwei"), None)
            .await
            .unwrap()
            .unwrap();

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs.len(), 2);
        // Both are published within the same second, so `created_at` cannot order them
        // and the tie-break on the uuid v7 id is what makes this deterministic.
        assert_eq!(revs[0].id, second, "newest first");
        assert_eq!(revs[0].parent_id.as_deref(), Some(first.as_str()));
        assert!(revs[1].parent_id.is_none(), "the first has no parent");
    }

    #[test]
    fn the_id_the_timeline_breaks_ties_on_is_monotonic() {
        // `revisions_for` orders by `created_at DESC, id DESC`, and the tie-break carries
        // the whole ordering whenever two edits land in the same second — which
        // `datetime('now')` cannot tell apart. That only works because `Uuid::now_v7`
        // keeps a counter for ids generated inside the same millisecond; without it the
        // low bits are random and the timeline would shuffle a rapid pair at random.
        //
        // This asserts a property of a DEPENDENCY on purpose. It is not something this
        // crate can enforce, it is invisible when it breaks — the list is merely wrong,
        // never loud — and a `uuid` upgrade is exactly how it would go.
        let mut previous = uuid::Uuid::now_v7().to_string();
        let mut compared = 0;
        for _ in 0..20_000 {
            let next = uuid::Uuid::now_v7().to_string();
            // The first 13 characters are the 48-bit millisecond timestamp plus a dash.
            if next[..13] == previous[..13] {
                assert!(
                    next > previous,
                    "{next} <= {previous} in the same millisecond"
                );
                compared += 1;
            }
            previous = next;
        }
        assert!(compared > 100, "only {compared} pairs shared a millisecond");
    }

    #[tokio::test]
    async fn byte_size_is_recorded_so_the_timeline_can_show_a_delta() {
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        store
            .publish_revision(&autorin, &id, &body("kurz"), None)
            .await
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("deutlich laenger als vorher"), None)
            .await
            .unwrap();

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert!(revs[0].byte_size > revs[1].byte_size);
    }

    #[tokio::test]
    async fn publishing_to_a_document_that_does_not_exist_writes_nothing() {
        let store = store().await;
        let autorin = writer(&store).await;
        assert!(store
            .publish_revision(&autorin, "kein-dokument", &body("eins"), None)
            .await
            .unwrap()
            .is_none());
    }

    // --- the property the whole design exists for --------------------------------------

    #[tokio::test]
    async fn restoring_creates_a_new_revision_rather_than_rewinding() {
        // History must never be destroyed by a restore, or "restore" becomes a way to
        // lose the thing you were unsure about.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        let first = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();
        let second = store
            .publish_revision(&autorin, &id, &body("zwei"), None)
            .await
            .unwrap()
            .unwrap();

        let restored = store
            .restore_revision(&autorin, &first)
            .await
            .unwrap()
            .expect("the writer may restore");

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs.len(), 3, "restore appends; it does not remove");
        assert_eq!(revs[0].id, restored, "the restore is the newest revision");
        assert_ne!(
            restored, first,
            "a restore is a new revision, not the old one"
        );

        // The revisions it restored PAST are still readable afterwards. Length alone
        // would be satisfied by an implementation that deleted one and wrote two.
        let ids: Vec<&str> = revs.iter().map(|r| r.id.as_str()).collect();
        assert!(
            ids.contains(&first.as_str()),
            "the restored-from revision is gone"
        );
        assert!(
            ids.contains(&second.as_str()),
            "the revision restored PAST is gone"
        );

        let doc = store
            .document_by_path_unchecked("/notiz")
            .await
            .unwrap()
            .unwrap();
        assert!(
            doc.body.contains("eins"),
            "the page did not go back: {}",
            doc.body
        );
    }

    #[tokio::test]
    async fn a_restore_is_attributed_to_whoever_restored_it() {
        // Restoring is an edit, so it is filed under the person who made it — not under
        // the author of the version that came back. "Who changed the page back to that?"
        // is a question the history has to be able to answer, and it is a different
        // question from "who wrote that in the first place".
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        let kollege = granted(&store, "kollege", Permission::Write).await;

        let first = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("zwei"), None)
            .await
            .unwrap();
        store.restore_revision(&kollege, &first).await.unwrap();

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs[0].author_id, kollege.id, "the restorer is the author");
        assert_eq!(
            revs[2].author_id, autorin.id,
            "the original author is untouched by somebody else's restore"
        );
        assert!(
            revs[0]
                .summary
                .as_deref()
                .unwrap_or_default()
                .contains(&first[..8]),
            "the summary says which revision came back: {:?}",
            revs[0].summary
        );
    }

    #[tokio::test]
    async fn an_empty_display_name_falls_back_to_the_username() {
        // `author_name` is NOT NULL and is what the timeline renders. A principal with a
        // blank display name would otherwise produce a revision that looks unattributed
        // while being perfectly attributable.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let mut autorin = writer(&store).await;
        autorin.display_name = "   ".into();

        store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap();

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs[0].author_name, "autorin");
    }

    #[tokio::test]
    async fn a_revision_cannot_be_updated_in_place() {
        // Append-only is enforced by the schema, not by everybody remembering. A repair
        // script, an importer or a future "fix a typo in the summary" is exactly the kind
        // of caller this refuses.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        let rev = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();

        let outcome = sqlx::query("UPDATE revisions SET body = ?2 WHERE id = ?1")
            .bind(&rev)
            .bind("umgeschrieben")
            .execute(&store.pool)
            .await;
        assert!(outcome.is_err(), "a revision was rewritten in place");

        let stored = store.revision_for(&autorin, &rev).await.unwrap().unwrap();
        assert!(stored.body.contains("eins"), "got {}", stored.body);
    }

    #[tokio::test]
    async fn purging_a_document_takes_its_whole_history_with_it() {
        // The cascade D-M3-6's purge rests on, and it is worth a test of its own because
        // `parent_id` references this same table: a chain of revisions deleted in one
        // statement must not trip its own foreign key.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        for text in ["eins", "zwei", "drei"] {
            store
                .publish_revision(&autorin, &id, &body(text), None)
                .await
                .unwrap();
        }

        sqlx::query("DELETE FROM documents WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .expect("purging a document with a chain of revisions");

        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM revisions WHERE document_id = ?1")
            .bind(&id)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "revisions outlived the document they belong to");
    }

    // --- who wrote it ------------------------------------------------------------------

    #[tokio::test]
    async fn the_author_is_the_signed_in_principal_and_not_a_name_the_caller_chose() {
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = store
            .create_local_principal("smaulser", "Sergej Maulser", None, "x")
            .await
            .unwrap();
        store
            .add_grant(
                "/notiz",
                Subject::Principal(autorin.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap();

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(
            revs[0].author_id, autorin.id,
            "the id must be the principal's"
        );
        assert_ne!(
            revs[0].author_id, autorin.username,
            "the username is not the identity"
        );
        assert_eq!(
            revs[0].author_name, "Sergej Maulser",
            "the byline is the display name at the time of writing"
        );
    }

    #[tokio::test]
    async fn the_author_name_survives_the_account_being_removed() {
        // D-M3-4: offboarding removes access and nothing else. A history that forgets who
        // wrote the current text the moment somebody leaves has destroyed most of what it
        // is for — and a foreign key on `author_id` would make the deletion fail instead,
        // which is the same bug wearing a different hat.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = store
            .create_local_principal("smaulser", "Sergej Maulser", None, "x")
            .await
            .unwrap();
        store
            .add_grant(
                "/notiz",
                Subject::Principal(autorin.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap();

        sqlx::query("DELETE FROM principals WHERE id = ?1")
            .bind(&autorin.id)
            .execute(&store.pool)
            .await
            .expect("removing an account must not be blocked by its revisions");

        // Read back as somebody else who may read the page: the author is gone.
        let leserin = granted(&store, "leserin", Permission::Read).await;
        let revs = store.revisions_for(&leserin, &id).await.unwrap();
        assert_eq!(revs.len(), 1);
        assert_eq!(
            revs[0].author_name, "Sergej Maulser",
            "history lost its attribution"
        );
        assert_eq!(revs[0].author_id, autorin.id);
    }

    // --- who may do what (D-M3-5, D-M2-8) ----------------------------------------------

    #[tokio::test]
    async fn a_reader_cannot_publish_a_revision() {
        // Writing is always an explicit grant (D-M2-8); read never implies it.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        let leserin = granted(&store, "leserin", Permission::Read).await;

        store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap();

        assert!(
            store
                .publish_revision(&leserin, &id, &body("meins"), None)
                .await
                .unwrap()
                .is_none(),
            "a reader published a revision"
        );

        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs.len(), 1, "the refusal still wrote a revision");
        assert!(!revs[0].body.contains("meins"), "got {}", revs[0].body);
        let doc = store
            .document_by_path_unchecked("/notiz")
            .await
            .unwrap()
            .unwrap();
        assert!(!doc.body.contains("meins"), "the page was changed anyway");
    }

    #[tokio::test]
    async fn an_anonymous_caller_cannot_publish_even_where_anyone_may_write() {
        // The `Anyone` grant is what makes this test mean something. `can()` answers an
        // `Anyone` grant BEFORE it checks whether the caller is signed in — that is what a
        // public share link is — so with this grant in place the only thing standing
        // between an anonymous request and the history is the authentication check in
        // `publish_revision`. Without the grant the test would pass with that check
        // deleted, and would be asserting the right thing for the wrong reason.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        store
            .add_grant("/notiz", Subject::Anyone, Permission::Write)
            .await
            .unwrap();

        assert!(
            store
                .publish_revision(&Principal::anonymous(), &id, &body("niemand"), None)
                .await
                .unwrap()
                .is_none(),
            "an anonymous caller wrote a revision"
        );

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM revisions")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 0, "a revision was written by nobody");
    }

    #[tokio::test]
    async fn a_deactivated_author_cannot_publish() {
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let mut autorin = writer(&store).await;
        autorin.active = false;

        assert!(store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn seeing_the_history_follows_being_able_to_read_the_page() {
        // D-M3-5, and the half that is a disclosure: a revision body is page content, so
        // handing one to somebody who cannot read the page is the same leak as handing
        // them the page.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        let autorin = writer(&store).await;
        let rev = store
            .publish_revision(&autorin, &id, &body("vertraulich"), None)
            .await
            .unwrap()
            .unwrap();

        let leserin = granted(&store, "leserin", Permission::Read).await;
        let fremde = Principal::test("fremde", &[], &[]);

        assert_eq!(
            store.revisions_for(&leserin, &id).await.unwrap().len(),
            1,
            "somebody who may read the page may read its history"
        );
        assert!(store.revision_for(&leserin, &rev).await.unwrap().is_some());

        assert!(
            store.revisions_for(&fremde, &id).await.unwrap().is_empty(),
            "the history of a page this caller cannot read was handed over"
        );
        assert!(
            store.revision_for(&fremde, &rev).await.unwrap().is_none(),
            "a revision body of an unreadable page was handed over"
        );
        assert!(store
            .revisions_for(&Principal::anonymous(), &id)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn restoring_needs_write_even_though_reading_the_history_does_not() {
        // D-M3-5: seeing history follows read, restoring follows write. A restore changes
        // what the page says, so it is an edit like any other.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        let autorin = writer(&store).await;
        let leserin = granted(&store, "leserin", Permission::Read).await;

        let first = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("zwei"), None)
            .await
            .unwrap();

        // She can see it — that is the point of an open history.
        assert_eq!(store.revisions_for(&leserin, &id).await.unwrap().len(), 2);

        assert!(
            store
                .restore_revision(&leserin, &first)
                .await
                .unwrap()
                .is_none(),
            "a reader restored an old revision"
        );
        let revs = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(revs.len(), 2, "the refused restore changed the history");
        let doc = store
            .document_by_path_unchecked("/notiz")
            .await
            .unwrap()
            .unwrap();
        assert!(
            doc.body.contains("zwei"),
            "the refused restore changed the page"
        );
    }

    #[tokio::test]
    async fn restoring_a_revision_that_does_not_exist_is_none_not_an_error() {
        let store = store().await;
        let autorin = writer(&store).await;
        assert!(store
            .restore_revision(&autorin, "keine-revision")
            .await
            .unwrap()
            .is_none());
    }
}
