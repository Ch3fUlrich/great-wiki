//! The live CRDT state of a page: what is being typed, as against what has been published.
//!
//! Migration 0008 created `crdt_state` for this and said why: it changes on every
//! keystroke, whereas a revision is a deliberate publish, and storing them together would
//! make the history churn and blur "what was saved" with "what is being typed". This module
//! is the accessor that table waited for.
//!
//! # These are content accessors, and they take a `Principal`
//!
//! The state is a `yrs` v1 update — opaque bytes, and opaque is not private. Three lines
//! turn them back into the page:
//!
//! ```text
//! CollabDoc::from_state(&bytes)?.to_block().plain_text()
//! ```
//!
//! So [`Store::crdt_state_for`] is a retrieval path for page content and architecture rule
//! 2 applies to it without qualification: it filters by the caller's permissions at query
//! time, in the retriever, through [`Store::document_for`] — the crate's single
//! permission-checked document accessor — exactly as [`Store::revision_for`] does for a
//! revision body. Handing this to somebody who cannot read the page is the same disclosure
//! as handing them the page, only harder to notice in review because the value is a
//! `Vec<u8>`.
//!
//! [`Store::save_crdt_state`] takes one too, and requires `Action::Write`. Two reasons, and
//! the first is the one that decided it:
//!
//! - **Everything public in this crate is checked; everything unchecked is `pub(crate)` and
//!   says so in its name** — `document_by_path_unchecked`, `revision_unchecked`, `tree`,
//!   and the pool itself. `gw-api` has to call this, so it has to be `pub`, so a `pub`
//!   accessor that wrote page content with no permission check would be the first hole in
//!   that invariant — and it is the second authorisation path, the one nobody remembers
//!   when the rules change, that always gets it wrong.
//! - **There is one answer in this crate to "who may change this page's content"**, and it
//!   is `Action::Write` resolved through `document_for`. A persistence path with its own,
//!   weaker answer would mean the question had two.
//!
//! # What this is NOT
//!
//! It is not a second write path around the revision system (architecture rule 1). Nothing
//! here touches `documents.body` or `revisions`; a page still says what its newest revision
//! says, and [`crate::revisions::append_revision`] is still the only thing that changes
//! that. What is stored here is the editing session, so that closing the last tab and
//! opening it again is not a way to lose an hour's formatting — and so that autosave stops
//! filing versions nobody published.
//!
//! Because of that split, storing state cannot make a page say something new to a *reader*.
//! It changes what an editor is handed when they open the page, and turning that into
//! published content still needs an authorised publish by a person.

use crate::Store;
use anyhow::Result;
use gw_auth::{Action, Principal};

impl Store {
    /// The stored CRDT state of a document — for a caller who may read the document.
    ///
    /// `None` means three things at once, and deliberately: there is no such document, the
    /// caller may not read it, or the page has never been edited. The first two are the
    /// same closed conflation [`Store::document_for`] makes everywhere else in this crate.
    ///
    /// The third is the one a caller has to act on, and it is the difference between
    /// building an editing room from this state and *seeding* one from the page body.
    /// Seeding twice from one body produces two documents that never converge — `gw-collab`
    /// has a test named for it — so seeding is only ever correct for a page that has none
    /// of this, which is exactly what `None` reports.
    pub async fn crdt_state_for(
        &self,
        principal: &Principal,
        document_id: &str,
    ) -> Result<Option<Vec<u8>>> {
        if !self.may(principal, document_id, Action::Read).await? {
            return Ok(None);
        }
        Ok(
            sqlx::query_scalar("SELECT state FROM crdt_state WHERE document_id = ?1")
                .bind(document_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    /// Store the CRDT state of a document, replacing whatever was there.
    ///
    /// `false` means it was not written: no such document, or `principal` may not write it.
    /// Loud rather than silent at the call site — a refusal here is work that will be lost
    /// when the room is dropped, so it is the caller's job to say so.
    ///
    /// **Replacing and not appending.** A CRDT state is not a version; it is the whole
    /// document as one value, and every keystroke produces a new one. Keeping the old ones
    /// is what `revisions` is for, and the reason the two tables are separate at all.
    ///
    /// It is written as one statement rather than a delete and an insert, so a concurrent
    /// reader never sees the row missing. `updated_at` is set explicitly because
    /// `DEFAULT (datetime('now'))` only applies to an INSERT, and an upsert that kept the
    /// original timestamp would make the row look stale for as long as the page is edited.
    pub async fn save_crdt_state(
        &self,
        principal: &Principal,
        document_id: &str,
        state: &[u8],
    ) -> Result<bool> {
        if !self.may(principal, document_id, Action::Write).await? {
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO crdt_state (document_id, state) VALUES (?1, ?2) \
             ON CONFLICT (document_id) DO UPDATE \
             SET state = excluded.state, updated_at = datetime('now')",
        )
        .bind(document_id)
        .bind(state)
        .execute(&self.pool)
        .await?;
        Ok(true)
    }

    /// Forget a document's live CRDT state, with NO permission check.
    ///
    /// `pub(crate)` and named so the danger is unmissable, exactly as
    /// [`Store::document_by_path_unchecked`] is. It has one caller — [`Store::restore_revision`],
    /// which has already taken the `Action::Write` decision through
    /// [`Store::publish_revision`] and would only be asking the same question twice.
    ///
    /// **Why restoring must do this.** A restore means "make the page say what it said
    /// before". The stored CRDT state says something else — it is the content the restore
    /// was reaching *past* — so leaving it would mean the next editing session opened on
    /// exactly what was just undone, with the restore visible to readers and invisible to
    /// editors. Discarding it puts the next session back on the restored body, which is
    /// what [`crate::crdt`]'s `None` case is for.
    ///
    /// Two limits, stated rather than left to be discovered:
    ///
    /// - **Not atomic with the revision.** It is a second statement after
    ///   [`Store::publish_revision`]'s transaction has committed, so a crash in between
    ///   leaves the restore done and the stale state in place. Narrow, self-correcting on
    ///   the next restore, and not worth threading a transaction through two modules for.
    /// - **A live room outranks it.** If somebody has the page open, their room still holds
    ///   the pre-restore document and the next sweep writes it back. That is `gw-collab`'s
    ///   stated rule — the live room outranks the stored body — and undoing it needs the
    ///   HTTP layer to close the room, which is a decision for a request handler and not
    ///   for the store.
    pub(crate) async fn clear_crdt_state_unchecked(&self, document_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM crdt_state WHERE document_id = ?1")
            .bind(document_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Author, NewDocument, Store};
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

    async fn page(store: &Store, visibility: Visibility) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Notiz".into(),
                    slug: None,
                    language: "de".into(),
                    visibility,
                    body: body("hallo"),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap()
    }

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

    /// Bytes that are not valid UTF-8, so a column typed as TEXT rather than BLOB, or a
    /// round trip through a `String`, cannot pass these tests by accident.
    const STATE: &[u8] = &[0x00, 0x01, 0xff, 0xfe, 0x80, 0x7f, 0x00];

    // --- the round trip ----------------------------------------------------------------

    #[tokio::test]
    async fn state_saved_comes_back_byte_for_byte() {
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        assert!(store.crdt_state_for(&autorin, &id).await.unwrap().is_none());
        assert!(store.save_crdt_state(&autorin, &id, STATE).await.unwrap());

        assert_eq!(
            store
                .crdt_state_for(&autorin, &id)
                .await
                .unwrap()
                .as_deref(),
            Some(STATE),
            "a CRDT state that changes in storage is a document that will not load"
        );
    }

    #[tokio::test]
    async fn saving_again_replaces_rather_than_accumulating() {
        // A CRDT state is the whole document as one value, not a version of it. Two rows
        // for one document would make "load the state" a question with two answers.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        store.save_crdt_state(&autorin, &id, b"alt").await.unwrap();
        store.save_crdt_state(&autorin, &id, b"neu").await.unwrap();

        assert_eq!(
            store
                .crdt_state_for(&autorin, &id)
                .await
                .unwrap()
                .as_deref(),
            Some(&b"neu"[..])
        );
        let rows: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM crdt_state WHERE document_id = ?1")
                .bind(&id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(rows, 1);
    }

    #[tokio::test]
    async fn an_empty_state_is_stored_rather_than_treated_as_absent() {
        // The empty Yjs update is two zero bytes, and a document that has had everything
        // deleted from it is a real state. Collapsing it to "never edited" would re-seed
        // the room from the page body and undo the deletion.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;

        store.save_crdt_state(&autorin, &id, &[]).await.unwrap();
        assert_eq!(
            store
                .crdt_state_for(&autorin, &id)
                .await
                .unwrap()
                .as_deref(),
            Some(&[][..]),
            "an empty state came back as `never edited`"
        );
    }

    #[tokio::test]
    async fn saving_for_a_document_that_does_not_exist_writes_nothing() {
        // `crdt_state.document_id` is a foreign key, so an unchecked insert here would be
        // a database error rather than a quiet no-op. The permission check gets there
        // first, and both answers are the same `false`.
        let store = store().await;
        let autorin = writer(&store).await;
        assert!(!store
            .save_crdt_state(&autorin, "kein-dokument", STATE)
            .await
            .unwrap());
        assert!(store
            .crdt_state_for(&autorin, "kein-dokument")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn state_is_purged_with_the_document_it_belongs_to() {
        // D-M3-6's purge, and the reason the cascade is worth a test: a document deleted
        // for good must not leave its live text behind in another table.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        sqlx::query("DELETE FROM documents WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();

        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM crdt_state")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "the live text outlived the page it belonged to");
    }

    // --- who may read it (architecture rule 2) -----------------------------------------

    #[tokio::test]
    async fn reading_the_state_follows_being_able_to_read_the_page() {
        // The state IS the page, encoded. `CollabDoc::from_state` turns these bytes back
        // into text in one call, so handing them to somebody who cannot read the page is
        // the same disclosure as handing them the page.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        let autorin = writer(&store).await;
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        let leserin = granted(&store, "leserin", Permission::Read).await;
        assert_eq!(
            store
                .crdt_state_for(&leserin, &id)
                .await
                .unwrap()
                .as_deref(),
            Some(STATE),
            "somebody who may read the page may read what is being typed into it"
        );

        for outsider in [Principal::test("fremde", &[], &[]), Principal::anonymous()] {
            assert!(
                store
                    .crdt_state_for(&outsider, &id)
                    .await
                    .unwrap()
                    .is_none(),
                "the live text of a page `{}` cannot read was handed over",
                outsider.username
            );
        }
    }

    #[tokio::test]
    async fn a_deactivated_account_can_no_longer_read_the_state() {
        // D-M2-7. The grant is untouched; the account is not.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        let autorin = writer(&store).await;
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        let mut suspended = autorin.clone();
        suspended.active = false;
        assert!(store
            .crdt_state_for(&suspended, &id)
            .await
            .unwrap()
            .is_none());
    }

    // --- who may write it (D-M2-8) -----------------------------------------------------

    #[tokio::test]
    async fn a_reader_cannot_save_the_state() {
        // Writing is only ever an explicit grant, and this is a write of page content in
        // the form the editor will be handed it in. A reader who could save it could
        // replace what every editor of the page sees next time they open it.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        let leserin = granted(&store, "leserin", Permission::Read).await;
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        assert!(
            !store
                .save_crdt_state(&leserin, &id, b"meins")
                .await
                .unwrap(),
            "a reader wrote the live state of a page"
        );
        assert_eq!(
            store
                .crdt_state_for(&autorin, &id)
                .await
                .unwrap()
                .as_deref(),
            Some(STATE),
            "the refused save changed the stored state anyway"
        );
    }

    #[tokio::test]
    async fn an_instance_admin_with_no_grant_cannot_save_the_state() {
        // The admin baseline is reach, not write (D-M2-8). It is what makes the reader
        // test above mean something: without this, "checked" could mean "checked for
        // read".
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        let autorin = writer(&store).await;
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();
        let chef = store
            .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
            .await
            .unwrap();

        // He reaches the page, so he reaches what is being typed into it. Asserted so the
        // refusal below is known to be about the ACTION and not about the page being out
        // of his reach — the two accessors ask different questions, and this is the pair
        // that proves it.
        assert_eq!(
            store.crdt_state_for(&chef, &id).await.unwrap().as_deref(),
            Some(STATE),
            "the admin baseline reaches restricted content (D-M2-1)"
        );
        assert!(
            !store
                .save_crdt_state(&chef, &id, b"vom Chef")
                .await
                .unwrap(),
            "the admin baseline conferred writing"
        );
        assert_eq!(
            store.crdt_state_for(&chef, &id).await.unwrap().as_deref(),
            Some(STATE),
            "the refused save changed the stored state anyway"
        );
    }

    #[tokio::test]
    async fn an_anonymous_caller_may_save_where_anyone_may_write() {
        // Stated rather than assumed, because it differs from `publish_revision` on
        // purpose. A revision refuses an anonymous author because a revision RECORDS an
        // author and a byline nobody chose is worth nothing; `crdt_state` records nobody,
        // so the only question left is the permission one — and on a path carrying
        // `anyone: write` (a public share link) the answer is yes. Refusing here would
        // mean a share-link editor's work is the one kind that is never saved.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        store
            .add_grant("/notiz", Subject::Anyone, Permission::Write)
            .await
            .unwrap();

        let anon = Principal::anonymous();
        assert!(store.save_crdt_state(&anon, &id, STATE).await.unwrap());
        assert_eq!(
            store.crdt_state_for(&anon, &id).await.unwrap().as_deref(),
            Some(STATE)
        );
    }

    // --- restoring ---------------------------------------------------------------------

    #[tokio::test]
    async fn restoring_a_revision_discards_the_live_state_it_was_reaching_past() {
        // Otherwise a restore is visible to readers — `documents.body` moved — and
        // invisible to editors, because the next session would open on the stored CRDT
        // state, which holds exactly the content the restore was undoing.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        let first = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("zwei"), None)
            .await
            .unwrap();
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        store.restore_revision(&autorin, &first).await.unwrap();

        assert!(
            store.crdt_state_for(&autorin, &id).await.unwrap().is_none(),
            "the next editing session would have opened on the content that was restored past"
        );
    }

    #[tokio::test]
    async fn a_refused_restore_leaves_the_live_state_alone() {
        // A reader cannot restore, and a refusal must change nothing at all — least of all
        // throw away what somebody is in the middle of typing.
        let store = store().await;
        let id = page(&store, Visibility::Restricted).await;
        let autorin = writer(&store).await;
        let first = store
            .publish_revision(&autorin, &id, &body("eins"), None)
            .await
            .unwrap()
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("zwei"), None)
            .await
            .unwrap();
        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        let leserin = granted(&store, "leserin", Permission::Read).await;
        assert!(store
            .restore_revision(&leserin, &first)
            .await
            .unwrap()
            .is_none());

        assert_eq!(
            store
                .crdt_state_for(&autorin, &id)
                .await
                .unwrap()
                .as_deref(),
            Some(STATE),
            "a refused restore threw away the live editing state"
        );
    }

    // --- what it is NOT ----------------------------------------------------------------

    #[tokio::test]
    async fn saving_state_changes_neither_the_page_nor_its_history() {
        // Architecture rule 1: a body changes in one place, by publishing a revision. This
        // is the reason autosave may stop writing revisions without becoming a second write
        // path — it writes nothing a reader can see.
        let store = store().await;
        let id = page(&store, Visibility::Public).await;
        let autorin = writer(&store).await;
        let before = store.revisions_for(&autorin, &id).await.unwrap();

        store.save_crdt_state(&autorin, &id, STATE).await.unwrap();

        let after = store.revisions_for(&autorin, &id).await.unwrap();
        assert_eq!(after.len(), before.len(), "saving state wrote a revision");
        assert_eq!(
            after[0].id, before[0].id,
            "saving state moved the page to a different revision"
        );
        let doc = store
            .document_by_path_unchecked("/notiz")
            .await
            .unwrap()
            .unwrap();
        assert!(
            doc.body.contains("hallo"),
            "saving state changed the page body: {}",
            doc.body
        );
    }
}
