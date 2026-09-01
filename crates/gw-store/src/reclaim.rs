//! The reclamation sweep: the second, deliberate act that takes the bytes off the mount.
//!
//! # What this closes
//!
//! `docs/decisions/0013-what-a-purge-leaves-on-the-mount.md` decides that a purge takes a
//! page's attachment rows and leaves the bytes, and that a `blobs` row **outlives its last
//! reference on purpose**. Until this module existed the consequence was that the wiki could
//! not fully forget a file: purging the last page carrying a mistakenly-imported scan removed
//! every trace of it from the database and left the scan on `/mnt/cloud`. This is the second
//! act. [`Store::purge_document`] reports `blobs_orphaned`, and [`Store::reclaim_blobs`] is
//! what an administrator runs to make that number go away.
//!
//! # Why the worklist is a table and never a directory walk
//!
//! ADR 0013's least obvious decision, and everything here rests on it. A `blobs` row that
//! nothing references is an **indexed worklist**: "what is on this mount that nothing points
//! at" is one query, answerable inside a transaction, on a table no concurrent upload can
//! change without taking the store's single connection. A walk of `blobs/` is not the same
//! question asked differently — it races every upload in flight, because a file written a
//! millisecond before its row is inserted looks exactly like an orphan. A sweep built on a
//! walk would eventually delete a file somebody had just uploaded.
//!
//! So this module never reads a directory. It reads the table, and a file the table does not
//! know about is left exactly where it is — see
//! `a_file_no_row_ever_knew_about_is_left_exactly_where_it_is`, which pins that as a decision
//! rather than an oversight.
//!
//! # How it cannot delete a blob an upload is about to reference
//!
//! The dangerous window is inside [`Store::attach`]: the bytes have been renamed into
//! `blobs/<digest>` and the row that references them has not been written yet. A sweep
//! landing there would unlink a *live* page's file — the failure ADR 0013 rejects every
//! ordering of "destroy rows" and "unlink files" for.
//!
//! Three things close it, and all three have to stay true:
//!
//! 1. **[`Store::open`] fixes the pool at one connection.** Whoever holds it holds it for the
//!    whole of their transaction, so an upload and a sweep cannot interleave at all. This is
//!    checked at the top of [`Store::reclaim_blobs`] rather than assumed, because ADR 0013
//!    names a second connection as the thing that would invalidate every argument here, and a
//!    guard that fires is better than a comment nobody reads. The sweep **refuses to run** on
//!    a store that could be written to behind it.
//! 2. **[`Store::attach`] takes that connection before it publishes**, not after. `tx.begin()`
//!    comes first and `pending.publish()` happens inside it, so the window above is entirely
//!    inside the attach's hold. Moving the publish out of the transaction would reopen the
//!    race with nothing else changing.
//! 3. **The worklist and the deletion are one statement.** `DELETE … RETURNING` rather than a
//!    `SELECT` followed by a `DELETE` written to resemble it — ADR 0012's shape, for ADR
//!    0012's reason: two statements describing one truth can be edited apart.
//!
//! `the_sweep_cannot_take_a_file_an_upload_is_about_to_reference` stands exactly where
//! `attach` stands and proves the sweep makes no progress at all.
//!
//! # The unlink comes before the commit, and that is the safer half of a bad choice
//!
//! A filesystem is not in the transaction: `unlink` does not roll back. So one of the two
//! crash windows has to be accepted, and they are not equally bad.
//!
//! * **Unlink, then commit** — a crash in between leaves a `blobs` row for a file that is
//!   gone. That row is unreferenced, so nothing can even try to download it, and the next
//!   sweep finishes the job. It is also a state the system already handles: a download whose
//!   bytes are missing answers 503 and re-uploading repairs it.
//! * **Commit, then unlink** — a crash in between leaves a file with **no row**, which is
//!   precisely the thing only a directory walk could ever find again. The sweep would have
//!   created, by crashing, the state it exists to avoid needing.
//!
//! So: unlink first. A genuine failure to unlink — a permission, a stale NFS handle — aborts
//! the whole sweep and rolls the rows back, so the index and the mount stay in step and the
//! operator gets one error naming the digest rather than a partial reclamation nobody can
//! reconstruct. The cost is stated where it is paid: one permanently undeletable file blocks
//! every later sweep until somebody looks at it.
//!
//! # Why this is a command and not an endpoint
//!
//! It is a **deliberate act**, like the purge it follows, and D-14's argument against an
//! automatic purge applies unchanged. Beyond that, two reasons it is not an HTTP route:
//!
//! * `blobs` is instance-wide and has no path to hang a permission on. Every other
//!   destructive operation in this system is authorised against a page; this one would have
//!   to be authorised against the instance, and an instance-wide destruction with no undo
//!   delivered by a button is exactly the shape D-14 argues against.
//! * It holds the store's only connection for the whole of its transaction, so every other
//!   request in the process waits behind it. A sweep over a large mount is a stop-the-world
//!   event and must not be reachable from the internet-facing surface.
//!
//! `great-wiki reclaim` is therefore the whole interface, previewing by default and
//! destroying only on `--commit`. If it should be periodic, something else calls it on a
//! schedule — Semaphore, per AGENTS.md, and never host cron.

use crate::blobs::BlobStore;
use crate::Store;
use anyhow::{Context, Result};
use serde_json::json;

/// Whether a sweep is being asked for or merely described.
///
/// The shape [`crate::Purge`] takes, with the one difference ADR 0013 forces. A purge's
/// preview *is* the purge, rolled back, because everything it touches is in the transaction.
/// A sweep's is not quite: the database half runs and is rolled back exactly as a purge's is,
/// but the `unlink` is skipped, because performing it would be describing a destruction by
/// doing half of it. What a preview reports about the mount it learns by looking, not by
/// acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reclaim {
    /// Say what would go. Nothing is unlinked, nothing is committed, nothing is recorded.
    Preview,
    /// Take it.
    Commit,
}

/// What a sweep reclaimed, or would.
///
/// The numbers are what an administrator checks against
/// [`crate::PurgeReport::blobs_orphaned`]: a purge says how many stored files it has just
/// left referenced by nothing, and [`ReclaimReport::blobs`] is how many of those a sweep
/// took. They accumulate — two purges and one sweep afterwards is one number equal to the sum
/// of the other two — because "unreferenced" is a property of the whole corpus rather than of
/// one purge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReclaimReport {
    /// Whether this actually happened. `false` for a preview.
    pub committed: bool,
    /// Rows that had lost their last reference. Not a count taken beside the `DELETE` — it is
    /// the `DELETE`'s own `RETURNING`, so it cannot describe something other than what went.
    pub blobs: i64,
    /// What those rows said those files weighed. The database's number rather than the
    /// mount's: it is what the `blobs` row recorded when the bytes were first stored, and it
    /// is available for a row whose file has already gone.
    pub bytes: i64,
    /// Files that were on the mount and are not any more.
    pub files_removed: i64,
    /// Rows whose file was already absent. Not an error — it is the repair for a crash
    /// between an unlink and a commit, and for a mount that lost a file some other way. It is
    /// reported apart from [`ReclaimReport::files_removed`] because a number greater than
    /// zero means the index and the mount had drifted, which is worth knowing even though
    /// this run has just fixed it.
    pub files_already_absent: i64,
}

impl std::fmt::Display for ReclaimReport {
    /// One line, for a person reading a terminal or a Semaphore log.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let verb = if self.committed {
            "reclaimed"
        } else {
            "would reclaim"
        };
        write!(
            f,
            "{verb} {} file(s), {}",
            self.blobs,
            human_bytes(self.bytes)
        )?;
        if self.files_already_absent > 0 {
            write!(
                f,
                " — {} taken off the mount, {} already gone from it",
                self.files_removed, self.files_already_absent
            )?;
        }
        Ok(())
    }
}

/// A size a person can read, to one decimal. Powers of 1024, because that is what `du` and
/// every filesystem tool an administrator will compare this against use.
fn human_bytes(bytes: i64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64 / 1024.0;
    let mut unit = UNITS[0];
    for next in &UNITS[1..] {
        if size < 1024.0 {
            break;
        }
        size /= 1024.0;
        unit = next;
    }
    format!("{size:.1} {unit}")
}

impl Store {
    /// Take every stored file nothing references any more off the mount and out of the index.
    ///
    /// The whole of ADR 0013's unfinished half. `actor` is recorded and never consulted — the
    /// same shape [`Store::purge_document`] takes, and for the same reason: the caller is a
    /// command line rather than a signed-in person, and "nobody" is not an answer for a
    /// destruction.
    ///
    /// See this module's header for why the worklist is a table, why the unlink precedes the
    /// commit, and why an upload cannot be racing it. The short version is that all of it
    /// rests on the store having exactly one connection, which is why that is the first thing
    /// checked.
    ///
    /// A sweep that found nothing writes no audit row. This is a command something else may
    /// call on a schedule, and a log with a "reclaimed 0" entry per run is a log nobody reads
    /// — while the entries that matter are exactly the ones that say bytes were destroyed.
    pub async fn reclaim_blobs(
        &self,
        blobs: &BlobStore,
        actor: &str,
        mode: Reclaim,
    ) -> Result<ReclaimReport> {
        // ADR 0013's switch-back criterion, as a guard rather than a sentence. Every safety
        // argument in this module's header is an argument about one connection; with two,
        // an upload can publish its bytes while this transaction is open and the unlink below
        // would take a live page's file. Fail closed (AGENTS.md rule 3).
        anyhow::ensure!(
            self.pool.options().get_max_connections() == 1,
            "refusing to reclaim: this store has more than one connection, so an upload can \
             publish bytes while the sweep is running and the sweep would delete them. ADR \
             0013 names this as the change that requires a real lock before a sweep is safe"
        );

        let mut tx = self.pool.begin().await?;

        // THE destroying statement, and the worklist comes out of it rather than out of a
        // `SELECT` written to resemble it (ADR 0012). `attachments.sha256` carries no
        // `ON DELETE` clause, so if this predicate were ever wrong the database would refuse
        // the delete rather than silently detaching a page's file.
        let taken: Vec<(String, i64)> = sqlx::query_as(
            "DELETE FROM blobs WHERE NOT EXISTS ( \
               SELECT 1 FROM attachments a WHERE a.sha256 = blobs.sha256) \
             RETURNING sha256, byte_size",
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut report = ReclaimReport {
            committed: mode == Reclaim::Commit,
            blobs: taken.len() as i64,
            bytes: 0,
            files_removed: 0,
            files_already_absent: 0,
        };

        for (sha256, byte_size) in &taken {
            report.bytes += byte_size;
            // `path_for` refuses a digest that is not one. Nothing can produce a bad one —
            // the column is CHECKed to 64 lowercase hex characters — and the defence that
            // survives a second writer is the one at the point of use.
            let path = blobs.path_for(sha256)?;
            let was_there = match mode {
                Reclaim::Commit => match tokio::fs::remove_file(&path).await {
                    Ok(()) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    // Anything else — a permission, a stale NFS handle — aborts the sweep.
                    // Dropping `tx` rolls the rows back, so the index still describes the
                    // mount and running this again is the whole retry.
                    Err(error) => {
                        return Err(error).with_context(|| {
                            format!(
                                "could not remove {} while reclaiming: the sweep has been \
                                 rolled back and nothing was reclaimed",
                                path.display()
                            )
                        })
                    }
                },
                // A preview looks instead of acting, and classifies what it sees the
                // same way the commit above does — including the part that is not
                // symmetrical. A stale NFS handle says nothing about what is on the mount,
                // so anything that is not a plain "no such file" is counted as present:
                // a preview must never under-report what a commit will do, and the commit
                // will turn that same error into a refusal rather than a smaller number.
                Reclaim::Preview => match tokio::fs::metadata(&path).await {
                    Ok(_) => true,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(_) => true,
                },
            };
            if was_there {
                report.files_removed += 1;
            } else {
                report.files_already_absent += 1;
            }
        }

        // The integrity check that makes the report a measurement rather than a claim, in the
        // shape `purge_document` uses: every row taken was accounted for exactly once.
        anyhow::ensure!(
            report.files_removed + report.files_already_absent == report.blobs,
            "the sweep took {} row(s) and accounted for {}",
            report.blobs,
            report.files_removed + report.files_already_absent
        );
        // And it is exhaustive: the statement above is the only thing standing between this
        // wiki and a file it can never forget, so "it missed some" must be an error rather
        // than a smaller number in a report.
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM blobs b WHERE NOT EXISTS ( \
               SELECT 1 FROM attachments a WHERE a.sha256 = b.sha256)",
        )
        .fetch_one(&mut *tx)
        .await?;
        anyhow::ensure!(
            remaining == 0,
            "the sweep left {remaining} unreferenced blob row(s) behind"
        );

        if mode == Reclaim::Preview {
            tx.rollback().await?;
            return Ok(report);
        }

        if report.blobs > 0 {
            // Instance-wide: no path, because these bytes belong to no page any more — that
            // is what made them reclaimable. Readable only at the admin baseline, which is
            // what `crate::audit` does with an entry carrying no path.
            //
            // **No digest in the detail**, for the reason `crate::attachments` gives about
            // its own audit rows: a content address in the log is a way to learn that
            // particular bytes were once in this wiki.
            Self::record_audit(
                &mut *tx,
                Some(actor),
                "blobs.reclaim",
                None,
                None,
                &json!({
                    "blobs": report.blobs,
                    "bytes": report.bytes,
                    "files_removed": report.files_removed,
                    "files_already_absent": report.files_already_absent,
                }),
            )
            .await?;
        }
        tx.commit().await?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    //! What the sweep takes, what it refuses to take, and what it cannot be made to race.

    use super::{Reclaim, ReclaimReport};
    use crate::{Author, BlobOutcome, BlobStore, NewDocument, Purge, Store, StoredBlob};
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

    /// A top-level page, by its path — which is what a grant and an attach both take.
    /// `create_document` hands back an id, and using that as a path is a silent no-match.
    async fn page(store: &Store, slug: &str) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: slug.into(),
                    slug: Some(slug.into()),
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body(),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
        format!("/{slug}")
    }

    async fn writer(store: &Store, paths: &[&str]) -> Principal {
        let principal = Principal::test("schreiber", &[], &[]);
        for path in paths {
            store
                .add_grant(
                    path,
                    Subject::Principal(principal.id.clone()),
                    Permission::Write,
                )
                .await
                .unwrap();
        }
        principal
    }

    /// **Unique bytes per test.** The store is content-addressed, so two tests attaching the
    /// same file would share one row and one file on the mount, and a sweep in one would
    /// reclaim the other's.
    fn png(marker: &str) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        bytes.extend_from_slice(marker.as_bytes());
        bytes
    }

    /// Put bytes on a page, the way a request does.
    async fn attach(
        store: &Store,
        blobs: &BlobStore,
        principal: &Principal,
        path: &str,
        filename: &str,
        bytes: &[u8],
    ) {
        let mut writer = blobs.writer().unwrap();
        writer.push(bytes).await.unwrap();
        let BlobOutcome::Accepted(pending) = writer.finish().await.unwrap() else {
            panic!("these bytes must be acceptable");
        };
        match store
            .attach(principal, path, filename, pending)
            .await
            .unwrap()
        {
            crate::AttachOutcome::Done(_) => {}
            other => panic!("the attach must succeed, got {other:?}"),
        }
    }

    /// Bytes on the mount and in the index, referenced by nothing at all.
    async fn publish_unreferenced(blobs: &BlobStore, bytes: &[u8]) -> StoredBlob {
        let mut writer = blobs.writer().unwrap();
        writer.push(bytes).await.unwrap();
        let BlobOutcome::Accepted(pending) = writer.finish().await.unwrap() else {
            panic!("these bytes must be acceptable");
        };
        pending.publish().await.unwrap()
    }

    /// What these bytes will be filed under. The store is content-addressed, so this is
    /// also where on the mount they live.
    fn digest(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(bytes))
    }

    fn on_mount(blobs: &BlobStore, sha256: &str) -> bool {
        blobs.path_for(sha256).unwrap().exists()
    }

    async fn indexed(store: &Store, sha256: &str) -> bool {
        let found: Option<(String,)> = sqlx::query_as("SELECT sha256 FROM blobs WHERE sha256 = ?1")
            .bind(sha256)
            .fetch_optional(&store.pool)
            .await
            .unwrap();
        found.is_some()
    }

    async fn audit_rows(store: &Store) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE action = 'blobs.reclaim'")
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    fn nothing_taken(report: &ReclaimReport) {
        assert_eq!(
            (report.blobs, report.bytes, report.files_removed),
            (0, 0, 0),
            "{report:?}"
        );
    }

    // -----------------------------------------------------------------------------------
    // What it may not take.
    // -----------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_file_a_page_still_carries_is_never_reclaimed() {
        let store = store().await;
        let (_dir, blobs) = media();
        let path = page(&store, "befunde").await;
        let principal = writer(&store, &[&path]).await;
        let bytes = png("still-attached");
        attach(&store, &blobs, &principal, &path, "befund.png", &bytes).await;
        let sha = digest(&bytes);

        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();

        nothing_taken(&report);
        assert!(indexed(&store, &sha).await, "the row must stay");
        assert!(on_mount(&blobs, &sha), "and so must the file");
    }

    #[tokio::test]
    async fn a_file_no_row_ever_knew_about_is_left_exactly_where_it_is() {
        // The reverse of the case above, and the one the sweep deliberately cannot see. A
        // file with no row comes from an `attach` that failed after publishing its bytes;
        // finding it would need a walk of `blobs/`, and that walk races every upload in
        // flight — which is the whole reason the worklist is a table. ADR 0013.
        let store = store().await;
        let (_dir, blobs) = media();
        let stray = publish_unreferenced(&blobs, &png("never-indexed")).await;
        // Deliberately NOT inserted into `blobs`.
        assert!(!indexed(&store, &stray.sha256).await);

        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();

        nothing_taken(&report);
        assert!(
            on_mount(&blobs, &stray.sha256),
            "the sweep works from the index and must not go looking on the mount"
        );
    }

    // -----------------------------------------------------------------------------------
    // What it takes.
    // -----------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_file_nothing_references_leaves_the_index_and_the_mount() {
        let store = store().await;
        let (_dir, blobs) = media();
        let path = page(&store, "befunde").await;
        let principal = writer(&store, &[&path]).await;
        let bytes = png("detached-then-swept");
        attach(&store, &blobs, &principal, &path, "befund.png", &bytes).await;
        let sha = digest(&bytes);
        store.detach(&principal, &path, "befund.png").await.unwrap();
        assert!(
            indexed(&store, &sha).await,
            "detaching leaves the row (D-15)"
        );

        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();

        assert!(report.committed);
        assert_eq!(report.blobs, 1);
        assert_eq!(report.bytes, bytes.len() as i64);
        assert_eq!(report.files_removed, 1);
        assert_eq!(report.files_already_absent, 0);
        assert!(!indexed(&store, &sha).await, "the row is gone");
        assert!(!on_mount(&blobs, &sha), "and so are the bytes");
    }

    #[tokio::test]
    async fn a_preview_says_what_would_go_and_takes_nothing() {
        // The purge's shape, with the one difference ADR 0013 forces: an `unlink` does not
        // roll back, so a preview must not perform one. The database half still runs and is
        // rolled back, which is what makes the counts a measurement.
        let store = store().await;
        let (_dir, blobs) = media();
        let bytes = png("previewed");
        let blob = publish_unreferenced(&blobs, &bytes).await;
        sqlx::query("INSERT INTO blobs (sha256, byte_size, media_type) VALUES (?1, ?2, ?3)")
            .bind(&blob.sha256)
            .bind(blob.byte_size)
            .bind(blob.media_type)
            .execute(&store.pool)
            .await
            .unwrap();

        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Preview)
            .await
            .unwrap();

        assert!(!report.committed);
        assert_eq!(report.blobs, 1);
        assert_eq!(report.bytes, bytes.len() as i64);
        assert_eq!(report.files_removed, 1, "what a commit would remove");
        assert!(
            indexed(&store, &blob.sha256).await,
            "the row is still there"
        );
        assert!(on_mount(&blobs, &blob.sha256), "and so is the file");
        assert_eq!(audit_rows(&store).await, 0, "a preview records nothing");

        // And the commit that follows says the same thing and does it.
        let done = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();
        assert_eq!(
            (done.blobs, done.bytes, done.files_removed),
            (report.blobs, report.bytes, report.files_removed)
        );
        assert!(!on_mount(&blobs, &blob.sha256));
        assert_eq!(audit_rows(&store).await, 1, "and a commit records one row");
    }

    #[tokio::test]
    async fn a_row_whose_file_has_already_gone_is_reclaimed_and_counted_apart() {
        // The state ADR 0013 names as the residual risk of a crash between the unlink and
        // the commit, and the state a stale mount produces on its own. It is a repair, not
        // an error: the row goes, and the number is reported separately so an administrator
        // can see that the mount and the index had drifted.
        let store = store().await;
        let (_dir, blobs) = media();
        let bytes = png("row-without-file");
        let blob = publish_unreferenced(&blobs, &bytes).await;
        sqlx::query("INSERT INTO blobs (sha256, byte_size, media_type) VALUES (?1, ?2, ?3)")
            .bind(&blob.sha256)
            .bind(blob.byte_size)
            .bind(blob.media_type)
            .execute(&store.pool)
            .await
            .unwrap();
        std::fs::remove_file(blobs.path_for(&blob.sha256).unwrap()).unwrap();

        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();

        assert_eq!(report.blobs, 1);
        assert_eq!(report.files_removed, 0);
        assert_eq!(report.files_already_absent, 1);
        assert!(!indexed(&store, &blob.sha256).await);
    }

    // -----------------------------------------------------------------------------------
    // The safety property: it cannot race an upload.
    // -----------------------------------------------------------------------------------

    #[tokio::test]
    async fn the_sweep_cannot_take_a_file_an_upload_is_about_to_reference() {
        // THE test. The dangerous window is inside `Store::attach`: the bytes have been
        // published under their digest and the row that references them has not been written
        // yet. A sweep landing there would delete a live page's file — the failure ADR 0013
        // rejects every ordering of "destroy rows" and "unlink files" for.
        //
        // It cannot happen because `Store::open` fixes the pool at ONE connection and
        // `attach` takes it BEFORE it publishes. This test stands exactly where `attach`
        // stands and proves the sweep makes no progress at all.
        let store = store().await;
        let (_dir, blobs) = media();
        let bytes = png("in-flight");

        // An orphan row for these bytes, left by an earlier purge: the sweep's worklist
        // would take it, which is what makes the race reachable rather than theoretical.
        let blob = publish_unreferenced(&blobs, &bytes).await;
        sqlx::query("INSERT INTO blobs (sha256, byte_size, media_type) VALUES (?1, ?2, ?3)")
            .bind(&blob.sha256)
            .bind(blob.byte_size)
            .bind(blob.media_type)
            .execute(&store.pool)
            .await
            .unwrap();

        // Stand where `attach` stands: holding the store's only connection, bytes already
        // renamed into place, row not yet written.
        let tx = store.pool.begin().await.unwrap();
        let republished = publish_unreferenced(&blobs, &bytes).await;
        assert_eq!(republished.sha256, blob.sha256);

        let raced = tokio::time::timeout(
            std::time::Duration::from_millis(250),
            store.reclaim_blobs(&blobs, "test", Reclaim::Commit),
        )
        .await;
        assert!(
            raced.is_err(),
            "the sweep ran while an upload held the store's connection: {raced:?}"
        );
        assert!(
            on_mount(&blobs, &blob.sha256),
            "and the file the upload is about to reference is still there"
        );

        // Not vacuous: once the upload lets go, the same sweep does take it.
        tx.rollback().await.unwrap();
        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();
        assert_eq!(report.files_removed, 1);
        assert!(!on_mount(&blobs, &blob.sha256));
    }

    #[tokio::test]
    async fn the_sweep_refuses_a_store_something_else_could_write_to_behind_it() {
        // ADR 0013's switch-back criterion, asserted rather than trusted: every safety
        // argument above rests on the pool being one connection. A second one makes the
        // interlock imaginary, so the sweep refuses to run at all rather than running
        // unsafely — AGENTS.md rule 3.
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("wiki.db").display());
        // Migrate through the ordinary door first, so this store differs from a real one in
        // exactly one respect.
        Store::open(&url).await.unwrap();
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .unwrap();
        let loose = Store {
            pool,
            public_origin: None,
        };
        let (_media, blobs) = media();

        let refused = loose.reclaim_blobs(&blobs, "test", Reclaim::Commit).await;
        assert!(
            refused.is_err(),
            "a sweep on a multi-connection store must refuse: {refused:?}"
        );
    }

    // -----------------------------------------------------------------------------------
    // What an administrator can check it against.
    // -----------------------------------------------------------------------------------

    #[tokio::test]
    async fn what_a_purge_reported_as_orphaned_is_what_the_sweep_reclaims() {
        // The two numbers are the two halves of one act, and this is what makes them
        // comparable rather than merely both present. A purge says how many files it left
        // behind; the sweep is what an administrator runs next, and it has to take exactly
        // those.
        let store = store().await;
        let (_dir, blobs) = media();
        let path = page(&store, "befunde").await;
        let principal = writer(&store, &[&path]).await;
        let bytes = png("purged-then-swept");
        attach(&store, &blobs, &principal, &path, "befund.png", &bytes).await;
        let sha = digest(&bytes);

        store.trash_document(&principal, &path).await.unwrap();
        let crate::PurgeOutcome::Done(purge) = store
            .purge_document("test", &path, Purge::Commit)
            .await
            .unwrap()
        else {
            panic!("the purge must run");
        };
        assert_eq!(purge.blobs_orphaned, 1);
        assert!(
            on_mount(&blobs, &sha),
            "a purge leaves the bytes (ADR 0013)"
        );

        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();
        assert_eq!(
            report.blobs, purge.blobs_orphaned,
            "the sweep takes exactly what the purge said it had left"
        );
        assert!(!on_mount(&blobs, &sha), "and the wiki has now forgotten it");
    }

    #[test]
    fn a_report_reads_as_a_sentence() {
        // It goes into a terminal and into a Semaphore log, and the number an administrator
        // compares against `blobs_orphaned` has to be legible in both.
        let report = ReclaimReport {
            committed: true,
            blobs: 3,
            bytes: 4 * 1024 * 1024,
            files_removed: 3,
            files_already_absent: 0,
        };
        assert_eq!(report.to_string(), "reclaimed 3 file(s), 4.0 MiB");

        // A preview says so, and drift between the index and the mount is spelled out
        // rather than folded into one number.
        let preview = ReclaimReport {
            committed: false,
            blobs: 2,
            bytes: 900,
            files_removed: 1,
            files_already_absent: 1,
        };
        assert_eq!(
            preview.to_string(),
            "would reclaim 2 file(s), 900 B — 1 taken off the mount, 1 already gone from it"
        );
    }

    #[tokio::test]
    async fn a_sweep_that_finds_nothing_records_nothing() {
        // This is a command something else may call on a schedule. A log line per run would
        // be a log nobody reads, and the audit trail is where a destruction is looked for.
        let store = store().await;
        let (_dir, blobs) = media();
        let report = store
            .reclaim_blobs(&blobs, "test", Reclaim::Commit)
            .await
            .unwrap();
        nothing_taken(&report);
        assert_eq!(audit_rows(&store).await, 0);
    }
}
