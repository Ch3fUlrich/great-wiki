//! The `Anhänge` list: which page carries which file, under which name.
//!
//! # This is the authority, and the body is not (D-15)
//!
//! A file appears inline in the prose *and* in a list on the page, and **the list is what
//! says it is attached**. Nothing here is derived from `documents.body`, and nothing may
//! become so: an inline block is a *reference* to a row in this table, so cutting the block
//! out of a paragraph must leave the file exactly where it is. That is the same shape D-2
//! gives a task whose checkbox line was deleted — the card detaches, the due date survives —
//! and it is why publishing a revision runs no reconciliation over this table the way
//! [`crate::tasks`] does over cards.
//!
//! The consequence for whoever builds the inline block: it refers to an attachment by
//! **(page, filename)**, which is exactly the address a download uses. There is no rename,
//! so that reference cannot be broken by one.
//!
//! # A download is authorised against the page (D-16)
//!
//! Blobs are content-addressed, so one PDF on two pages is one file — and each of the two
//! rows here is a separate statement about who may see *a page*. Every function in this
//! module that can reach bytes goes through [`Store::document_access`], the same
//! permission-checked accessor a page read ends in, before it resolves anything.
//!
//! **The digest is never an address.** [`Store::attachment_for`] takes a page and a filename
//! and gives back a digest; there is deliberately no function that takes a digest and gives
//! back anything. If one existed, a reader holding a hash could reach bytes without a page
//! being consulted, and D-16 would be undone — which is why the API never serialises a
//! digest either.
//!
//! # What an upload discloses about what is already stored: nothing
//!
//! Deduplication is invisible. Attaching a file whose bytes are already on the mount produces
//! the same answer, in the same shape, as attaching bytes nobody has ever sent — no flag, no
//! different status, and the rename in [`crate::blobs::PendingBlob::publish`] happens either
//! way so the timing does not differ either.
//!
//! That is not tidiness. The corpus holds a child's medical records and server runbooks; if
//! "this file was already here" were observable, then possessing a file would be a test for
//! whether somebody else had put it on a page you cannot read — a disclosure about a *page*,
//! obtained without ever naming one. The rule is the same one AGENTS.md rule 2 states for
//! every retrieval path, applied to a write.
//!
//! # What a purge leaves behind
//!
//! An `attachments` row cascades away with its page; a `blobs` row does not, and the bytes
//! stay on the mount. `docs/decisions/0013-what-a-purge-leaves-on-the-mount.md` is why, and
//! [`crate::trash::PurgeReport::blobs_orphaned`] is how an administrator is told. What takes
//! them off the mount afterwards is [`crate::Store::reclaim_blobs`], a separate deliberate
//! act — and the reason it is safe is that [`Store::attach`] publishes its bytes *inside* the
//! transaction that will reference them, so the two can never interleave.

use crate::blobs::PendingBlob;
use crate::Store;
use anyhow::Result;
use gw_auth::{Action, Principal};
use serde_json::json;
use sqlx::FromRow;

/// The longest name a file may carry on a page, in characters.
///
/// 255 is what every filesystem anybody will export this to allows, and a name is half of an
/// address rather than a description — a page that wants to say more about a file has a
/// paragraph for it.
pub const MAX_FILENAME_CHARS: usize = 255;

/// One row of a page's `Anhänge` list.
///
/// **There is no digest here and there must not be one.** This is the value the API turns
/// into JSON, so a field added here is a field a reader is handed — and a reader holding a
/// content address can go looking for the same bytes under a page they may not read. The
/// digest lives on [`AttachmentSource`], which never leaves the server.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct Attachment {
    /// What the file is called on this page. One path segment; see [`canonical_filename`].
    pub filename: String,
    /// What the bytes are, from [`crate::blobs::sniff`] — never what an upload claimed.
    pub media_type: String,
    pub byte_size: i64,
    /// As SQLite writes it (`YYYY-MM-DD HH:MM:SS`, UTC).
    pub uploaded_at: String,
    /// Who attached it, as they were called then. A snapshot, exactly as a revision's byline
    /// and a trash entry's are.
    pub uploaded_by_name: String,
}

/// A page's `Anhänge` list, and what this caller may do to it.
///
/// One value, because it is one answer — the shape [`crate::DocumentAccess`] takes and for
/// the same reason (ADR 0010). The read that authorised the list is what produced the write
/// verdict beside it, so a control offered on that bit and the refusal that would follow
/// pressing it are the same verdict rather than two that agree today.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachmentList {
    pub attachments: Vec<Attachment>,
    /// Whether this caller may attach and detach here.
    pub may_write: bool,
}

/// An attachment the caller has been authorised for, **and the bytes it names**.
///
/// The only value in this workspace that carries a digest out of the store, and it exists
/// solely so the HTTP layer can ask [`crate::BlobStore`] for a file it has already earned.
/// It is not serialised anywhere: see [`Attachment`].
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct AttachmentSource {
    pub filename: String,
    pub media_type: String,
    pub byte_size: i64,
    /// Lowercase hex SHA-256. **Never put in a response.**
    pub sha256: String,
}

/// What an upload came to.
///
/// The three-outcome shape [`crate::TrashOutcome`] uses, for the same reason: the refusals
/// are different mistakes with different fixes, and one of them has an answer the caller can
/// act on.
#[derive(Debug)]
pub enum AttachOutcome {
    Done(Attachment),
    /// No such page, or not one this caller may write. Conflated deliberately, as everywhere
    /// else in this crate: the HTTP layer decides whether existence may be revealed.
    Refused,
    /// Refused for a reason the caller can act on, in the caller's words: a name that could
    /// not be an address, or one already taken on this page.
    Blocked(String),
}

/// What a detach came to.
#[derive(Debug)]
pub enum DetachOutcome {
    /// What was detached, so an interface can say which file went.
    Done(Attachment),
    /// No such page, or not one this caller may write.
    Refused,
    /// The page is this caller's to write, and carries no file by that name. Told apart from
    /// [`DetachOutcome::Refused`] because it is a different mistake with a different fix, and
    /// only ever told to somebody who may already list the page's files.
    NoSuchFile,
}

/// The name a file may be given on a page, or `None` for one that could not be an address.
///
/// Trimmed and otherwise left exactly as it was typed: `Befund 2024.PDF` and
/// `Röntgen – links.png` are both fine, because a filename is shown to a person and
/// slugifying it would be inventing a different name for their file.
///
/// What is refused, and why each one:
///
/// * **empty, or over [`MAX_FILENAME_CHARS`]** — an address needs a segment, and a segment
///   has a length.
/// * **`/` or `\`** — `/` is the separator in `/api/attachment/{filename}/{page}`, so a name
///   holding one is a different page; `\` is the separator on the other operating system and
///   the escape character inside a quoted `Content-Disposition` filename.
/// * **`.` and `..`** — these name directories, and a name that reads as one is a name
///   somebody will eventually join onto a path.
/// * **`"` and any control character** — both end the quoted string in a
///   `Content-Disposition` header, and a newline in one is header injection.
///
/// **None of this is what stops a directory traversal.** The file on disk is named by its
/// digest and this string never becomes part of a filesystem path — see
/// [`crate::blobs::BlobStore::path_for`]. What it stops is an UNREACHABLE row: a file
/// attached to a page and downloadable from nowhere, because its name cannot be spelled in
/// the only address that reaches it.
pub fn canonical_filename(raw: &str) -> Option<String> {
    let name = raw.trim();
    if name.is_empty() || name.chars().count() > MAX_FILENAME_CHARS {
        return None;
    }
    if name == "." || name == ".." {
        return None;
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || c == '"' || c.is_control())
    {
        return None;
    }
    Some(name.to_string())
}

impl Store {
    /// Attach bytes that have already arrived to a page. Needs **write** on the page, and a
    /// signed-in, active account.
    ///
    /// The account is required for the reason [`Store::trash_document`] requires one: the row
    /// records who put the file there, and "nobody" is not an answer. A path carrying
    /// `anyone: write` is a public share link (see [`crate::DocumentAccess::may_write`]), and
    /// putting a quarter of a gigabyte on the mount through one is not the same act as
    /// editing a paragraph.
    ///
    /// # The order, which is the whole design
    ///
    /// An upload cannot be authorised before it is read — its digest is a function of all of
    /// it — so [`PendingBlob`] holds the bytes in `tmp/` while this function decides. Then,
    /// in this order:
    ///
    /// 1. the caller, the name and the permission, none of which touch the mount;
    /// 2. the name is not already taken on this page;
    /// 3. **the bytes are published**, under their digest;
    /// 4. the rows.
    ///
    /// A refusal at 1 or 2 drops the [`PendingBlob`], which removes the temporary file, so
    /// nothing a caller was not entitled to attach reaches the mount. A failure at 4 leaves
    /// bytes on the mount with no row — an orphan whose contents are exactly what its name
    /// says, which the next upload of the same file adopts. The other order fails the other
    /// way: a row naming bytes that are not there, which is a broken download on a page
    /// nobody touched.
    ///
    /// **The permission pass happens before the transaction, not inside it.**
    /// [`Store::open`] fixes the pool at one connection, so a check made while a transaction
    /// is open would wait for a connection the transaction is holding — the same constraint
    /// [`Store::trash_document`] records.
    pub async fn attach(
        &self,
        principal: &Principal,
        path: &str,
        filename: &str,
        pending: PendingBlob,
    ) -> Result<AttachOutcome> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(AttachOutcome::Refused);
        }
        let Some(filename) = canonical_filename(filename) else {
            return Ok(AttachOutcome::Blocked(format!(
                "`{filename}` cannot be a file name here: one segment, at most \
                 {MAX_FILENAME_CHARS} characters, and no `/`, `\\`, `\"` or control character"
            )));
        };
        let Some(access) = self.document_access(principal, path, Action::Write).await? else {
            return Ok(AttachOutcome::Refused);
        };
        let doc_id = access.document.id;

        let mut tx = self.pool.begin().await?;
        let taken: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM attachments WHERE doc_id = ?1 AND filename = ?2")
                .bind(&doc_id)
                .bind(&filename)
                .fetch_optional(&mut *tx)
                .await?;
        if taken.is_some() {
            tx.rollback().await?;
            // Never a silent replacement: an inline block already points at this name, and
            // changing what it shows without touching the page is a change nobody made.
            return Ok(AttachOutcome::Blocked(format!(
                "`{filename}` is already attached to this page — detach it first, or use \
                 another name"
            )));
        }

        // Bytes before rows. See the doc comment above for why this way round — and note
        // that this is INSIDE the transaction, which [`Store::reclaim_blobs`] depends on:
        // between this line and the INSERT below the bytes are on the mount referenced by
        // nothing, and a reclamation sweep that could run here would delete a live page's
        // file. It cannot, because this transaction holds the store's only connection. Moving
        // the publish above `begin()` would reopen that race with nothing else changing.
        let blob = pending.publish().await?;

        // `OR IGNORE`: the same bytes may already be indexed from another page, and that is
        // D-16 working rather than a collision. Nothing about the existing row is updated —
        // its `created_at` is when these bytes first arrived in the wiki, which is a fact
        // about the mount and not about this page.
        sqlx::query(
            "INSERT OR IGNORE INTO blobs (sha256, byte_size, media_type) VALUES (?1, ?2, ?3)",
        )
        .bind(&blob.sha256)
        .bind(blob.byte_size)
        .bind(blob.media_type)
        .execute(&mut *tx)
        .await?;

        let row: Attachment = sqlx::query_as(
            "INSERT INTO attachments \
               (id, doc_id, sha256, filename, uploaded_by, uploaded_by_name) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
             RETURNING filename, \
                       (SELECT media_type FROM blobs WHERE sha256 = ?3) AS media_type, \
                       (SELECT byte_size FROM blobs WHERE sha256 = ?3) AS byte_size, \
                       uploaded_at, uploaded_by_name",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&doc_id)
        .bind(&blob.sha256)
        .bind(&filename)
        .bind(&principal.id)
        .bind(&principal.display_name)
        .fetch_one(&mut *tx)
        .await?;

        // Scoped to the page, so the space's own administrator can read it. **The digest is
        // deliberately not in the detail**: the audit log is the one place a page
        // administrator could otherwise compare content addresses and learn that the same
        // bytes sit on a page they may not read, which is the disclosure this module's header
        // is about.
        Self::record_audit(
            &mut *tx,
            Some(&principal.id),
            "attachment.attach",
            Some(&filename),
            Some(&access.document.path),
            &json!({
                "filename": &row.filename,
                "media_type": &row.media_type,
                "byte_size": row.byte_size,
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(AttachOutcome::Done(row))
    }

    /// The `Anhänge` list of a page. Needs **read** on it.
    ///
    /// `None` for "no such page" and for "not this caller's page" alike — the conflation
    /// every accessor in this crate makes, because a list of filenames says a page exists and
    /// says what is on it. A page **in the trash** is `None` as well, and for free:
    /// [`Store::document_access`] resolves through
    /// [`Store::document_by_path_unchecked`], which refuses a soft-deleted row.
    pub async fn attachments_for(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<Option<AttachmentList>> {
        // `document_access` rather than `document_for`: the read that authorises this list is
        // what produces the write verdict beside it, so there is one authorisation here and
        // not a read followed by a separate "could I also write this".
        let Some(access) = self.document_access(principal, path, Action::Read).await? else {
            return Ok(None);
        };
        let attachments: Vec<Attachment> = sqlx::query_as(
            "SELECT a.filename, b.media_type, b.byte_size, a.uploaded_at, a.uploaded_by_name \
               FROM attachments a JOIN blobs b ON b.sha256 = a.sha256 \
              WHERE a.doc_id = ?1 ORDER BY a.filename",
        )
        .bind(&access.document.id)
        .fetch_all(&self.pool)
        .await?;
        Ok(Some(AttachmentList {
            attachments,
            may_write: access.may_write,
        }))
    }

    /// One attachment of a page, **with the digest of its bytes**. Needs **read** on the page.
    ///
    /// This is the whole of D-16's access half: the question asked is about a page, the
    /// permission consulted is the page's, and the digest is what comes back rather than what
    /// goes in. There is no counterpart that starts from a digest, and adding one would make
    /// every page check optional.
    pub async fn attachment_for(
        &self,
        principal: &Principal,
        path: &str,
        filename: &str,
    ) -> Result<Option<AttachmentSource>> {
        let Some(document) = self.document_for(principal, path, Action::Read).await? else {
            return Ok(None);
        };
        Ok(sqlx::query_as(
            "SELECT a.filename, b.media_type, b.byte_size, a.sha256 \
               FROM attachments a JOIN blobs b ON b.sha256 = a.sha256 \
              WHERE a.doc_id = ?1 AND a.filename = ?2",
        )
        .bind(&document.id)
        .bind(filename)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Take a file off a page. Needs **write** on it, and a signed-in, active account.
    ///
    /// It removes the row and nothing else: the bytes stay on the mount and the `blobs` row
    /// stays in the index, because this is not a destruction and must not become one — the
    /// same file may be attached to another page, and finding out is not this operation's
    /// business. ADR 0013 is where the bytes go, and when: [`Store::reclaim_blobs`], run
    /// deliberately, is what takes them.
    ///
    /// The account is required for the same reason attaching needs one, pointed the other
    /// way: a file that quietly stopped being on a page is exactly the change somebody asks
    /// about months later, and the audit row is the only thing that will answer.
    pub async fn detach(
        &self,
        principal: &Principal,
        path: &str,
        filename: &str,
    ) -> Result<DetachOutcome> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(DetachOutcome::Refused);
        }
        let Some(access) = self.document_access(principal, path, Action::Write).await? else {
            return Ok(DetachOutcome::Refused);
        };

        let mut tx = self.pool.begin().await?;
        // The names come out of the destroying statement itself, exactly as a purge's do
        // (ADR 0012): a `SELECT` beside the `DELETE` is a second statement that can be edited
        // apart from it.
        let removed: Option<Attachment> = sqlx::query_as(
            "DELETE FROM attachments WHERE doc_id = ?1 AND filename = ?2 \
             RETURNING filename, \
                       (SELECT media_type FROM blobs WHERE sha256 = attachments.sha256) \
                           AS media_type, \
                       (SELECT byte_size FROM blobs WHERE sha256 = attachments.sha256) \
                           AS byte_size, \
                       uploaded_at, uploaded_by_name",
        )
        .bind(&access.document.id)
        .bind(filename)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(removed) = removed else {
            tx.rollback().await?;
            return Ok(DetachOutcome::NoSuchFile);
        };

        Self::record_audit(
            &mut *tx,
            Some(&principal.id),
            "attachment.detach",
            Some(&removed.filename),
            Some(&access.document.path),
            &json!({
                "filename": &removed.filename,
                "media_type": &removed.media_type,
                "byte_size": removed.byte_size,
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(DetachOutcome::Done(removed))
    }
}

#[cfg(test)]
mod tests {
    //! What a page carries, who may reach it, and what an upload discloses.

    use super::{canonical_filename, AttachOutcome, DetachOutcome};
    use crate::{Author, BlobOutcome, BlobStore, NewDocument, PendingBlob, Store};
    use gw_auth::{Permission, Principal, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Visibility};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    /// A media directory that lives exactly as long as the test holds it. The `TempDir` has
    /// to stay bound: dropping it removes the mount out from under the store.
    fn media() -> (tempfile::TempDir, BlobStore) {
        let dir = tempfile::tempdir().unwrap();
        let blobs = BlobStore::open(dir.path()).unwrap();
        (dir, blobs)
    }

    fn body() -> Block {
        Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        }
    }

    async fn page(store: &Store, title: &str, v: Visibility) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: v,
                    body: body(),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    /// Somebody holding `permission` on each of `paths`. There is no baseline that confers
    /// write (D-M2-8), so a grant per path is the only way to build one.
    async fn who(store: &Store, name: &str, permission: Permission, paths: &[&str]) -> Principal {
        let principal = Principal::test(name, &[], &[]);
        for path in paths {
            store
                .add_grant(path, Subject::Principal(principal.id.clone()), permission)
                .await
                .unwrap();
        }
        principal
    }

    fn png(tail: &str) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(tail.as_bytes());
        bytes
    }

    /// Bytes that have arrived and been typed, waiting for somebody to authorise them.
    async fn pending(blobs: &BlobStore, bytes: &[u8]) -> PendingBlob {
        let mut writer = blobs.writer().unwrap();
        writer.push(bytes).await.unwrap();
        match writer.finish().await.unwrap() {
            BlobOutcome::Accepted(pending) => pending,
            other => panic!("these bytes must be acceptable, got {other:?}"),
        }
    }

    async fn count(store: &Store, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    /// How many finished files are on the mount.
    fn on_mount(blobs: &BlobStore) -> usize {
        fn walk(dir: &std::path::Path, out: &mut usize) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path(), out);
                } else {
                    *out += 1;
                }
            }
        }
        let mut out = 0;
        walk(&blobs.root().join("blobs"), &mut out);
        out
    }

    fn done(outcome: AttachOutcome) -> super::Attachment {
        match outcome {
            AttachOutcome::Done(attachment) => attachment,
            other => panic!("expected the file to be attached, got {other:?}"),
        }
    }

    // --- attaching ------------------------------------------------------------------

    #[tokio::test]
    async fn attaching_puts_the_file_in_the_page_s_list() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;

        let attached = done(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );
        assert_eq!(attached.filename, "befund.png");
        assert_eq!(attached.media_type, "image/png");
        assert_eq!(attached.byte_size, png("a").len() as i64);
        assert_eq!(
            attached.uploaded_by_name, "schreiber",
            "the list says who put it there"
        );

        let listed = store
            .attachments_for(&schreiber, "/notiz")
            .await
            .unwrap()
            .expect("the page is readable");
        assert_eq!(
            listed
                .attachments
                .iter()
                .map(|a| a.filename.as_str())
                .collect::<Vec<_>>(),
            vec!["befund.png"]
        );
        assert!(listed.may_write, "and they may add another");
        assert_eq!(on_mount(&blobs), 1, "and the bytes are on the mount");
    }

    #[tokio::test]
    async fn attaching_needs_write_on_the_page() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let leser = who(&store, "leser", Permission::Read, &["/notiz"]).await;

        assert!(matches!(
            store
                .attach(&leser, "/notiz", "x.png", pending(&blobs, &png("a")).await)
                .await
                .unwrap(),
            AttachOutcome::Refused
        ));
        assert_eq!(count(&store, "attachments").await, 0);
    }

    #[tokio::test]
    async fn an_upload_nobody_may_attach_never_reaches_the_mount() {
        // The reason `PendingBlob` exists. A refused upload has already been read and hashed
        // — there is no other way to know what it is — so what must not happen is that it
        // ends up stored anyway under a name nothing will ever reference.
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let leser = who(&store, "leser", Permission::Read, &["/notiz"]).await;

        let outcome = store
            .attach(&leser, "/notiz", "x.png", pending(&blobs, &png("a")).await)
            .await
            .unwrap();
        assert!(matches!(outcome, AttachOutcome::Refused));
        assert_eq!(on_mount(&blobs), 0, "the mount must be untouched");
        assert_eq!(count(&store, "blobs").await, 0);
    }

    #[tokio::test]
    async fn attaching_needs_a_signed_in_account_even_where_anyone_may_write() {
        // The same argument `trash_document` makes: the list records who put the file there,
        // and "nobody" is not an answer. A path carrying `anyone: write` is a public share
        // link, and editing a paragraph through one is not the same act as putting a
        // quarter of a gigabyte on the mount.
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Offen", Visibility::Public).await;
        store
            .add_grant("/offen", Subject::Anyone, Permission::Write)
            .await
            .unwrap();

        let anonymous = Principal::anonymous();
        assert!(matches!(
            store
                .attach(
                    &anonymous,
                    "/offen",
                    "x.png",
                    pending(&blobs, &png("a")).await
                )
                .await
                .unwrap(),
            AttachOutcome::Refused
        ));
        // And the write bit really is there, or the refusal above would prove nothing.
        let schreiber = who(&store, "schreiber", Permission::Write, &[]).await;
        done(
            store
                .attach(
                    &schreiber,
                    "/offen",
                    "x.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );

        // The other direction, for the same reason: a file that quietly stopped being on a
        // page must be attributable to somebody.
        assert!(matches!(
            store.detach(&anonymous, "/offen", "x.png").await.unwrap(),
            DetachOutcome::Refused
        ));
        assert!(matches!(
            store.detach(&schreiber, "/offen", "x.png").await.unwrap(),
            DetachOutcome::Done(_)
        ));
    }

    #[tokio::test]
    async fn a_page_in_the_trash_carries_nothing_that_can_be_reached() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;
        done(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );
        store.trash_document(&schreiber, "/notiz").await.unwrap();

        assert!(store
            .attachments_for(&schreiber, "/notiz")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .attachment_for(&schreiber, "/notiz", "befund.png")
            .await
            .unwrap()
            .is_none());
        assert!(matches!(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "zweit.png",
                    pending(&blobs, &png("b")).await
                )
                .await
                .unwrap(),
            AttachOutcome::Refused
        ));
    }

    #[tokio::test]
    async fn a_name_already_taken_on_the_page_is_refused_and_the_message_says_so() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;
        done(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );

        let outcome = store
            .attach(
                &schreiber,
                "/notiz",
                "befund.png",
                pending(&blobs, &png("b")).await,
            )
            .await
            .unwrap();
        let AttachOutcome::Blocked(reason) = outcome else {
            panic!("a taken name is a conflict, got {outcome:?}");
        };
        assert!(reason.contains("befund.png"), "{reason}");
        // And nothing was replaced: an inline block pointing at this name still shows the
        // same picture.
        let listed = store
            .attachments_for(&schreiber, "/notiz")
            .await
            .unwrap()
            .unwrap()
            .attachments;
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].byte_size, png("a").len() as i64);
    }

    #[tokio::test]
    async fn one_file_may_sit_on_one_page_under_two_names() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;
        for name in ["vorne.png", "hinten.png"] {
            done(
                store
                    .attach(&schreiber, "/notiz", name, pending(&blobs, &png("a")).await)
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(count(&store, "attachments").await, 2);
        assert_eq!(count(&store, "blobs").await, 1);
        assert_eq!(on_mount(&blobs), 1);
    }

    #[tokio::test]
    async fn a_name_that_could_not_be_an_address_is_refused() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;

        for bad in [
            "",
            "   ",
            ".",
            "..",
            "../../etc/passwd",
            "unter/ordner.png",
            "rueck\\schlag.png",
            "zeilen\numbruch.png",
            "an\"fuehrung.png",
        ] {
            let outcome = store
                .attach(&schreiber, "/notiz", bad, pending(&blobs, &png("a")).await)
                .await
                .unwrap();
            assert!(
                matches!(outcome, AttachOutcome::Blocked(_)),
                "`{bad}` must be refused, got {outcome:?}"
            );
        }
        assert_eq!(count(&store, "attachments").await, 0);
        assert_eq!(on_mount(&blobs), 0, "and none of them reached the mount");
    }

    #[test]
    fn a_name_is_trimmed_and_otherwise_left_exactly_as_it_was_typed() {
        assert_eq!(
            canonical_filename("  Befund 2024.PDF "),
            Some("Befund 2024.PDF".into())
        );
        assert_eq!(
            canonical_filename("Röntgen – links.png"),
            Some("Röntgen – links.png".into())
        );
        assert_eq!(canonical_filename("a/b"), None);
        assert_eq!(canonical_filename(".."), None);
        assert_eq!(canonical_filename(&"x".repeat(256)), None);
        assert_eq!(canonical_filename(&"x".repeat(255)).unwrap().len(), 255);
    }

    // --- the whole of D-16 -------------------------------------------------------------

    #[tokio::test]
    async fn the_same_bytes_on_two_pages_are_one_blob_and_two_statements_about_access() {
        // D-16, in one test. `chefin` writes both pages and attaches the identical file to
        // each; `leser` may read only one of them. There is one file on the mount and one
        // row in `blobs`, and the answer to "may I have these bytes" is different depending
        // on which page they were reached through — which is only possible because nothing
        // authorises anything against the blob.
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Offen", Visibility::Public).await;
        page(&store, "Geheim", Visibility::Restricted).await;
        let chefin = who(&store, "chefin", Permission::Write, &["/offen", "/geheim"]).await;
        let leser = who(&store, "leser", Permission::Read, &["/offen"]).await;

        for path in ["/offen", "/geheim"] {
            done(
                store
                    .attach(
                        &chefin,
                        path,
                        "gleich.png",
                        pending(&blobs, &png("a")).await,
                    )
                    .await
                    .unwrap(),
            );
        }
        assert_eq!(count(&store, "blobs").await, 1, "one copy of the bytes");
        assert_eq!(count(&store, "attachments").await, 2, "two statements");
        assert_eq!(on_mount(&blobs), 1);

        let through_open = store
            .attachment_for(&leser, "/offen", "gleich.png")
            .await
            .unwrap()
            .expect("reachable through the page they may read");
        assert!(
            store
                .attachment_for(&leser, "/geheim", "gleich.png")
                .await
                .unwrap()
                .is_none(),
            "and not through the one they may not — the identical bytes are not the question"
        );

        // Anti-vacuity: somebody who may read the restricted page reaches exactly the same
        // digest through it, so the refusal above is about the page and not about the file.
        let through_secret = store
            .attachment_for(&chefin, "/geheim", "gleich.png")
            .await
            .unwrap()
            .expect("the page's own reader reaches it");
        assert_eq!(through_open.sha256, through_secret.sha256);
    }

    #[tokio::test]
    async fn listing_a_page_s_files_needs_read_on_that_page() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Geheim", Visibility::Restricted).await;
        let chefin = who(&store, "chefin", Permission::Write, &["/geheim"]).await;
        done(
            store
                .attach(
                    &chefin,
                    "/geheim",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );

        let fremde = Principal::test("fremde", &[], &[]);
        assert!(
            store
                .attachments_for(&fremde, "/geheim")
                .await
                .unwrap()
                .is_none(),
            "a list of filenames is a disclosure like any other"
        );
        assert!(store
            .attachments_for(&chefin, "/geheim")
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn a_page_that_does_not_exist_and_one_that_is_not_yours_are_one_answer() {
        let store = store().await;
        let fremde = Principal::test("fremde", &[], &[]);
        assert!(store
            .attachments_for(&fremde, "/gibt-es-nicht")
            .await
            .unwrap()
            .is_none());
    }

    // --- detaching ---------------------------------------------------------------------

    #[tokio::test]
    async fn detaching_removes_the_row_and_leaves_the_bytes() {
        // ADR 0013: the list is what says a file is attached, and letting go of it is not a
        // destruction. The blob row survives its last reference on purpose, so an orphan is
        // something a query can find rather than something only a directory walk could.
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;
        done(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );

        let outcome = store
            .detach(&schreiber, "/notiz", "befund.png")
            .await
            .unwrap();
        let DetachOutcome::Done(gone) = outcome else {
            panic!("expected the file to be detached, got {outcome:?}");
        };
        assert_eq!(gone.filename, "befund.png");
        assert_eq!(count(&store, "attachments").await, 0);
        assert_eq!(
            count(&store, "blobs").await,
            1,
            "the index keeps the orphan"
        );
        assert_eq!(on_mount(&blobs), 1, "and the bytes stay put");
    }

    #[tokio::test]
    async fn detaching_needs_write_and_a_name_that_is_there() {
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;
        let leser = who(&store, "leser", Permission::Read, &["/notiz"]).await;
        done(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );

        assert!(matches!(
            store.detach(&leser, "/notiz", "befund.png").await.unwrap(),
            DetachOutcome::Refused
        ));
        assert_eq!(
            count(&store, "attachments").await,
            1,
            "and nothing happened"
        );

        // Write, but no such file: a different mistake with a different fix, so a different
        // answer. Told only to somebody who may write the page.
        assert!(matches!(
            store
                .detach(&schreiber, "/notiz", "andere.png")
                .await
                .unwrap(),
            DetachOutcome::NoSuchFile
        ));
        assert!(matches!(
            store.detach(&leser, "/notiz", "andere.png").await.unwrap(),
            DetachOutcome::Refused
        ));
    }

    #[tokio::test]
    async fn attaching_and_detaching_are_both_recorded() {
        // The row itself says who attached a file. Nothing would say who took one away, and
        // a file that quietly stopped being on a page is exactly the kind of change somebody
        // asks about months later.
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Notiz", Visibility::Public).await;
        let schreiber = who(&store, "schreiber", Permission::Write, &["/notiz"]).await;
        done(
            store
                .attach(
                    &schreiber,
                    "/notiz",
                    "befund.png",
                    pending(&blobs, &png("a")).await,
                )
                .await
                .unwrap(),
        );
        store
            .detach(&schreiber, "/notiz", "befund.png")
            .await
            .unwrap();

        let actions: Vec<String> = sqlx::query_scalar("SELECT action FROM audit_log ORDER BY id")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        assert!(
            actions.contains(&"attachment.attach".to_string()),
            "{actions:?}"
        );
        assert!(
            actions.contains(&"attachment.detach".to_string()),
            "{actions:?}"
        );

        // Scoped to the page, so the space's own administrator can read it.
        let paths: Vec<Option<String>> =
            sqlx::query_scalar("SELECT path FROM audit_log WHERE action LIKE 'attachment.%'")
                .fetch_all(&store.pool)
                .await
                .unwrap();
        assert!(
            paths.iter().all(|p| p.as_deref() == Some("/notiz")),
            "{paths:?}"
        );
    }

    // --- what an upload discloses ------------------------------------------------------

    #[tokio::test]
    async fn attaching_bytes_somebody_else_already_stored_says_nothing_about_them() {
        // The dedup oracle. `chefin` attaches a file to a page `neuling` cannot read. When
        // `neuling` later attaches the very same bytes to their own page, the answer must be
        // indistinguishable from attaching bytes nobody had — otherwise "it was already
        // here" is a test for whether a file exists somewhere in the corpus, which is a
        // disclosure about a page rather than about a file.
        let store = store().await;
        let (_dir, blobs) = media();
        page(&store, "Geheim", Visibility::Restricted).await;
        page(&store, "Eigen", Visibility::Public).await;
        let chefin = who(&store, "chefin", Permission::Write, &["/geheim"]).await;
        let neuling = who(&store, "neuling", Permission::Write, &["/eigen"]).await;
        done(
            store
                .attach(
                    &chefin,
                    "/geheim",
                    "geteilt.png",
                    pending(&blobs, &png("shared")).await,
                )
                .await
                .unwrap(),
        );

        let already_stored = done(
            store
                .attach(
                    &neuling,
                    "/eigen",
                    "meins.png",
                    pending(&blobs, &png("shared")).await,
                )
                .await
                .unwrap(),
        );
        let never_seen = done(
            store
                .attach(
                    &neuling,
                    "/eigen",
                    "auch-meins.png",
                    pending(&blobs, &png("unique")).await,
                )
                .await
                .unwrap(),
        );

        // The two answers, with the only two fields that are ABOUT this attachment rather
        // than about the bytes blanked out. Everything else has to match, and there is
        // nowhere for a "this was already here" field to be added without this failing —
        // which is the point: the type is the disclosure boundary, not a rule in a handler.
        let blank = |a: &super::Attachment| super::Attachment {
            filename: String::new(),
            uploaded_at: String::new(),
            ..a.clone()
        };
        assert_eq!(blank(&already_stored), blank(&never_seen));

        // Anti-vacuity: the first upload really was a duplicate and the second really was
        // not, so there was something to disclose.
        assert_eq!(count(&store, "attachments").await, 3);
        assert_eq!(count(&store, "blobs").await, 2);
        assert_eq!(on_mount(&blobs), 2);
    }
}
