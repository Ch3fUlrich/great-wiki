//! The Papierkorb (D-14): deleting a page, putting it back, and the second, deliberate act
//! that makes a deletion permanent.
//!
//! # Three operations and two different permissions
//!
//! **Deleting is an edit.** A page leaves the tree, keeps its ACL and can be put back, so it
//! follows `Action::Write` on the page — asked through [`Store::document_access`], the one
//! permission-checked accessor, per document. There is no new rule here and there must not
//! be one.
//!
//! **Purging is not.** It destroys the page, its history, its cards and its edges, and
//! nothing anywhere else in this system can lose data. It is gated by
//! `gw_api::routes::admin::path_admin` — admin on the page's own path — which is the gate
//! `set_visibility` already uses and is decided in the API for the reason
//! [`crate::admin`]'s header gives: a store method that also had an opinion would be a
//! second rule to disagree with the first. ADR 0012 is why that gate and not the instance
//! one.
//!
//! # A page in the trash takes its subtree with it
//!
//! Not a convenience. [`Store::tree`] assembles the navigation by matching each row's
//! `parent_path` against a parent it has already emitted, so a page whose parent is in the
//! trash is **not filtered out — it is unreachable**: absent from the navigation, absent from
//! the markdown export (which walks `tree_for`), and still readable at its own URL, still on
//! its board, still in the graph. That is a hole in the tree that nothing reports, so the
//! subtree moves as one and the whole of it is what a restore puts back.
//!
//! The price is stated where it is paid: trashing a page requires **write on every page that
//! moves**, so a subtree somebody has deliberately fenced off with its own grants cannot be
//! swept away by whoever writes the page above it. That refusal tells the caller that
//! something below them is not theirs — one bit they did not have — and it is the minimum a
//! correct refusal can carry. It is told to somebody holding write on the parent, not to an
//! anonymous prober, which is the distinction `docs/decisions/0011-what-a-topic-discloses.md`
//! draws around aggregate listings.
//!
//! # The listing is an aggregate view, so it filters per document
//!
//! [`Store::trash_for`] authorises every entry through the same body a page read goes
//! through — [`crate::acl`]'s `access_to`, reached with the trashed row rather than the live
//! one. A page you could not see before deleting it is not one you can see in the trash, and
//! the count beside an entry is the pages **you** may read in it, taken from the same
//! filtered pass, for the reason `TopicSummary::documents` is.

use crate::topics::prune_empty_topics;
use crate::{Store, StoredDocument};
use anyhow::Result;
use gw_auth::{Action, Principal};
use serde_json::json;

/// What moved into the trash, or came back out of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashSummary {
    /// The page that was named. The entry is known by this path.
    pub path: String,
    pub title: String,
    /// How many pages moved, including the named one. A page with no subpages moves one.
    pub pages: usize,
}

/// What a delete or a restore actually did.
///
/// Three outcomes rather than a boolean, for the reason [`crate::admin::MembershipOutcome`]
/// has four: the refusals are different mistakes with different fixes, and one of them has an
/// answer the caller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrashOutcome {
    Done(TrashSummary),
    /// Nothing at that path, or nothing this caller may have. Conflated deliberately, as
    /// everywhere else in this crate: the HTTP layer decides whether existence may be
    /// revealed.
    Refused,
    /// Refused for a reason the caller can act on, in the caller's words.
    ///
    /// The shape `LAST_ADMIN` established: a refusal that names the way out rather than one
    /// that leaves somebody pressing the same button again. The HTTP layer answers 409.
    Blocked(String),
}

/// One act in the Papierkorb: a page somebody deleted, and what went down with it.
#[derive(Debug, Clone)]
pub struct TrashEntry {
    pub path: String,
    pub title: String,
    /// When it was deleted, as SQLite writes it (`YYYY-MM-DD HH:MM:SS`, UTC).
    pub deleted_at: String,
    /// Who deleted it, as they were called then. A snapshot, exactly as a revision's byline
    /// is: the Papierkorb still says who emptied a shelf after they have left.
    pub deleted_by_name: String,
    /// Pages in this entry **the caller may read**, including the named one.
    ///
    /// Taken from the same filtered pass the entry itself survived, never from a total. A
    /// count of what the filter removed would say that something is there, which is the leak
    /// ADR 0011 is about.
    pub pages: usize,
    /// Whether this caller may put it back.
    ///
    /// Write on every page the restore would move, composed with "is a signed-in, active
    /// account" — which is exactly what [`Store::restore_document`] requires, asked once and
    /// answered here so a control can be offered honestly (ADR 0010). Fail-closed for a page
    /// in the entry the caller cannot even read.
    pub may_restore: bool,
}

/// Whether a purge is being asked for or merely described.
///
/// The preview **is** the purge, rolled back — see ADR 0012. There is no second query that
/// counts what a purge would destroy, because a second query is a second answer and this is
/// the one operation in the system where the two disagreeing loses data nobody can get back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purge {
    /// Run it, report it, and roll it back. Nothing is destroyed and nothing is recorded.
    Preview,
    /// Run it, report it, and keep it.
    Commit,
}

/// A page a purge destroyed, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgedPage {
    pub path: String,
    pub title: String,
}

/// What a purge destroyed — or would destroy, which is the same report from the same run.
///
/// Every count here is a **difference measured across the DELETE itself**: the table's total
/// before it and after it, inside the one transaction. Not a `SELECT` written to resemble the
/// `DELETE`'s `WHERE` clause — that is two statements which can be edited apart, and the day
/// they drift the number an administrator confirmed is not the number of things that went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurgeReport {
    /// Whether this actually happened. `false` for a preview.
    pub committed: bool,
    /// Every page destroyed, in path order — including ones that were already in the trash
    /// under their own entry, which a purge of an ancestor necessarily takes as well.
    pub pages: Vec<PurgedPage>,
    /// Versions in the history of those pages. The whole of it: a purge is what D-14 makes
    /// "the second, deliberate act", and cascading the history away is what it is for.
    pub revisions: i64,
    /// Cards — the tasks anchored in those pages, and the standalone cards of any project
    /// homed on one of them.
    pub tasks: i64,
    /// Projects homed on one of those pages.
    pub projects: i64,
    /// Edges of the graph with either end on one of those pages.
    pub links: i64,
    /// Filings — "this page is about that topic" — not topics.
    pub topic_filings: i64,
    /// Topics that no page carries any more once those filings are gone.
    pub topics: i64,
    /// Rows of the `Anhänge` list — "this page carries that file, under this name" — that
    /// went with those pages. Not files: the same file on a surviving page is still there.
    pub attachments: i64,
    /// Stored files that **no page references any more** once those rows are gone.
    ///
    /// Not "files deleted". A purge takes the list and leaves the bytes, so this is the count
    /// of things that are now taking up space on the mount and are reachable from nowhere —
    /// `docs/decisions/0013-what-a-purge-leaves-on-the-mount.md` is why that is the design
    /// rather than an omission, and this number is how an administrator is told about it
    /// rather than discovering it.
    ///
    /// **It is a number with something to do about it.** [`crate::Store::reclaim_blobs`] —
    /// `great-wiki reclaim` — is the second act that takes those files off the mount, and
    /// [`crate::ReclaimReport::blobs`] is what this should be checked against afterwards.
    ///
    /// Measured the same way as every other number here: the count of unreferenced blobs
    /// before the DELETE subtracted from the count after it. It is the one that goes UP.
    pub blobs_orphaned: i64,
}

/// The subtree predicate, written once. `substr(...) = ?1 || '/'` rather than `LIKE ?1 ||
/// '/%'`: `LIKE` reads `%` and `_` in a path as wildcards, and `/projektierung` is not inside
/// `/projekt` — the same argument `crate::tasks` makes for the board's own subtree query.
const SUBTREE: &str = "(path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/')";

impl Store {
    /// Move a page and everything under it to the trash. Needs **write on every page that
    /// moves**, and a signed-in, active account.
    ///
    /// See this module's header for why the subtree goes too. The account is required for the
    /// reason [`crate::revisions::Author::refuse_if_nobody`] gives about a revision: a trash
    /// entry records who made it, and "nobody" is not an answer. A path carrying
    /// `anyone: write` — a public share link — makes a page editable by a caller who has not
    /// said who they are (see [`crate::DocumentAccess::may_write`]); emptying the wiki into a
    /// Papierkorb that cannot say who did it is not the same act as editing a paragraph.
    ///
    /// **The permission pass happens before the transaction, not inside it**, and that is not
    /// laziness. [`Store::open`] fixes the pool at one connection, so a permission check made
    /// while a transaction is open would be waiting for a connection the transaction is
    /// holding. Nothing is lost by the order: the same single connection is what makes the
    /// window between the two empty.
    pub async fn trash_document(&self, principal: &Principal, path: &str) -> Result<TrashOutcome> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(TrashOutcome::Refused);
        }
        let baseline = self.baseline_for(principal).await?;

        // The page itself, as a WRITE, through the accessor every read goes through.
        let Some(access) = self
            .document_access_with_baseline(principal, path, Action::Write, baseline)
            .await?
        else {
            return Ok(TrashOutcome::Refused);
        };

        // And every live page below it, one at a time. Per document, never by prefix: grants
        // do not union up the tree — the nearest ancestor carrying any wins outright — so
        // holding write at `/a` says nothing about `/a/b` once `/a/b` carries its own.
        for member in self.live_subtree(path).await? {
            if member == access.document.path {
                continue;
            }
            if self
                .document_access_with_baseline(principal, &member, Action::Write, baseline)
                .await?
                .is_none()
            {
                return Ok(TrashOutcome::Blocked(format!(
                    "{path} has a subpage you may not write, and a page goes to the trash \
                     with everything under it — whoever administers that subpage has to \
                     delete it first"
                )));
            }
        }

        let mut tx = self.pool.begin().await?;
        // One statement, so every row in the entry carries the same `deleted_at` and the
        // same root. `AND deleted_at IS NULL` is what leaves a page somebody threw away
        // earlier in its OWN entry rather than quietly adopting it into this one — which is
        // what makes `restore` able to put back exactly what went down here.
        let moved = sqlx::query(&format!(
            "UPDATE documents SET deleted_at = datetime('now'), deleted_root = ?2, \
             deleted_by = ?3, deleted_by_name = ?4 \
             WHERE {SUBTREE} AND deleted_at IS NULL"
        ))
        .bind(path)
        .bind(&access.document.id)
        .bind(&principal.id)
        .bind(&principal.display_name)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;

        refuse_a_hole_in_the_tree(&mut tx).await?;
        Self::record_audit(
            &mut *tx,
            Some(&principal.id),
            "document.trash",
            Some(path),
            // Scoped to the page: whoever administers this subtree is entitled to read that
            // it happened, and 0004 is what makes that possible.
            Some(path),
            &json!({ "pages": moved }),
        )
        .await?;
        tx.commit().await?;

        Ok(TrashOutcome::Done(TrashSummary {
            path: access.document.path,
            title: access.document.title,
            pages: moved,
        }))
    }

    /// Put a trash entry back, exactly as it went in. Needs **write on every page it
    /// restores**, and a signed-in, active account.
    ///
    /// `path` names the entry — the page somebody deleted — and not one of the pages that
    /// went down with it. Restoring a member on its own would put a live page under a parent
    /// still in the trash, which is the hole this module exists to make unrepresentable.
    ///
    /// **A page whose parent is still in the trash is refused, and the refusal names the
    /// parent.** The shape `LAST_ADMIN` established: an administrator told "no" and nothing
    /// else presses the button again.
    ///
    /// **Nothing here has to worry about the path having been taken in the meantime.**
    /// `documents.path` is UNIQUE across every row including soft-deleted ones, so a page in
    /// the trash keeps its address and no second page can be created there —
    /// [`Store::create_document`] says so in as many words. That is the whole reason a
    /// restore cannot collide, and it is the reason a delete does not free a path.
    pub async fn restore_document(
        &self,
        principal: &Principal,
        path: &str,
    ) -> Result<TrashOutcome> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(TrashOutcome::Refused);
        }
        let baseline = self.baseline_for(principal).await?;

        // A trash ENTRY: a row that is in the trash and is its own root. A member of somebody
        // else's entry answers the same `None` an absent page does.
        let entry: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT id, parent_path FROM documents \
             WHERE path = ?1 AND deleted_at IS NOT NULL AND deleted_root = id",
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        let Some((root_id, parent_path)) = entry else {
            return Ok(TrashOutcome::Refused);
        };

        let Some(access) = self
            .trashed_document_access(principal, path, Action::Write, baseline)
            .await?
        else {
            return Ok(TrashOutcome::Refused);
        };

        for member in self.entry_members(&root_id).await? {
            if member == access.document.path {
                continue;
            }
            if self
                .trashed_document_access(principal, &member, Action::Write, baseline)
                .await?
                .is_none()
            {
                return Ok(TrashOutcome::Blocked(format!(
                    "{path} went to the trash with a subpage you may not write, and it comes \
                     back with everything that went down with it — whoever administers that \
                     subpage has to restore it"
                )));
            }
        }

        // Somewhere to put it back.
        if let Some(parent) = &parent_path {
            if !self.document_exists(parent).await? {
                let in_trash = self.document_by_path_in_trash(parent).await?.is_some();
                return Ok(TrashOutcome::Blocked(if in_trash {
                    format!("{parent} is still in the trash: restore it first")
                } else {
                    format!("{parent} no longer exists, so there is nowhere to restore {path} to")
                }));
            }
        }

        let mut tx = self.pool.begin().await?;
        // Keyed on the ROOT, not on the path prefix: the entry is what went down together,
        // and a page thrown away separately before this one must not be swept back up with
        // it. Undoing somebody's deliberate delete is "it came back" — the mirror of the
        // outcome D-8 exists to prevent.
        let restored = sqlx::query(
            "UPDATE documents SET deleted_at = NULL, deleted_root = NULL, \
             deleted_by = NULL, deleted_by_name = NULL WHERE deleted_root = ?1",
        )
        .bind(&root_id)
        .execute(&mut *tx)
        .await?
        .rows_affected() as usize;

        refuse_a_hole_in_the_tree(&mut tx).await?;
        // Recorded because the row stops saying it: a restore clears the four columns that
        // were the only trace of the delete, so without this the Papierkorb's history is
        // whatever is still in it.
        Self::record_audit(
            &mut *tx,
            Some(&principal.id),
            "document.restore",
            Some(path),
            Some(path),
            &json!({ "pages": restored }),
        )
        .await?;
        tx.commit().await?;

        Ok(TrashOutcome::Done(TrashSummary {
            path: access.document.path,
            title: access.document.title,
            pages: restored,
        }))
    }

    /// The Papierkorb as this caller may see it, newest first.
    ///
    /// One row per **act**: the entries are the self-rooted trashed pages, so a subtree that
    /// went down together is one line rather than forty. Every one of them is authorised
    /// through [`Store::trashed_document_access`] — the same body a page read ends in — and
    /// so is every page inside it, which is what makes the count beside an entry a fact about
    /// this caller rather than about the entry.
    pub async fn trash_for(&self, principal: &Principal) -> Result<Vec<TrashEntry>> {
        // Once, for the whole walk. `tree_for` hoists it for the same reason: the baseline is
        // a property of the caller, not of the row, and re-querying it per page would only
        // invite it to drift within one response.
        let baseline = self.baseline_for(principal).await?;
        let signed_in = principal.is_authenticated() && principal.active;

        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT id, path, deleted_at, deleted_by_name FROM documents \
             WHERE deleted_at IS NOT NULL AND deleted_root = id \
             ORDER BY deleted_at DESC, path",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for (root_id, path, deleted_at, deleted_by_name) in rows {
            let mut root: Option<StoredDocument> = None;
            let mut pages = 0;
            // Starts true and is only ever narrowed. A page in the entry the caller cannot
            // even read leaves it false, which is the fail-closed direction: an offer to
            // restore something they would be refused is a control that lies.
            let mut may_restore = signed_in;

            for member in self.entry_members(&root_id).await? {
                match self
                    .trashed_document_access(principal, &member, Action::Read, baseline)
                    .await?
                {
                    Some(access) => {
                        pages += 1;
                        may_restore &= access.may_write;
                        if access.document.path == path {
                            root = Some(access.document);
                        }
                    }
                    None => may_restore = false,
                }
            }

            // The entry itself is a page, and an entry whose own page this caller may not
            // read is not one they may know about. Its title comes from the authorised
            // document rather than from the listing query, so nothing reaches a caller
            // without having gone through the accessor.
            let Some(root) = root else { continue };
            out.push(TrashEntry {
                path: root.path,
                title: root.title,
                deleted_at,
                deleted_by_name,
                pages,
                may_restore,
            });
        }
        Ok(out)
    }

    /// Destroy a trashed page and everything under it, or describe doing so.
    ///
    /// **Authorisation is NOT decided here**, as everywhere in this crate an administrator
    /// acts — see [`crate::admin`]'s header. The gate is `path_admin` on this page's own
    /// path, in the API; ADR 0012 says why that gate and not the instance one, and what it
    /// means for a subpage carrying its own narrower grants. `actor` is recorded, never
    /// consulted.
    ///
    /// **Only what is already in the trash.** D-14 makes the trash the only way in, so this
    /// refuses a live page rather than deleting it — a purge is the second act, and there is
    /// no first-and-second in one press.
    ///
    /// **What it takes**: the whole subtree at that path, whatever put each page there. A
    /// page thrown away under its own entry cannot survive the destruction of its parent —
    /// there would be nowhere to restore it to — so it goes, and [`PurgeReport::pages`] names
    /// it. Everything hanging off those pages goes with them, by the cascades declared in
    /// `0008`, `0009`, `0010` and `0011`: revisions, the live editing state, links at either
    /// end, cards, projects homed there, and topic filings. A topic that no page carries any
    /// more is pruned in the same transaction, because `prune_empty_topics` treats a page in
    /// the trash as still carrying its topics — `deleted_at` is reversible and this is not.
    ///
    /// **What it does NOT take: the grants on the path.** An `acl` row is a fact about a
    /// path, not about a document — [`Store::set_visibility_audited`] says so, and a grant
    /// may be written on a path no page occupies so that access can be prepared before one
    /// arrives. Withdrawing it here would make a purge a change to the access policy of a
    /// space, which it is not.
    pub async fn purge_document(
        &self,
        actor: &str,
        path: &str,
        mode: Purge,
    ) -> Result<PurgeOutcome> {
        let mut tx = self.pool.begin().await?;

        let in_trash: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM documents WHERE path = ?1 AND deleted_at IS NOT NULL",
        )
        .bind(path)
        .fetch_one(&mut *tx)
        .await?;
        if in_trash == 0 {
            tx.rollback().await?;
            return Ok(PurgeOutcome::Refused);
        }

        // A live page inside the subtree is the hole this module exists to prevent, and a
        // purge would destroy it without it ever having been in the trash. It cannot happen
        // — `trash_document` moves the subtree as one and `refuse_a_hole_in_the_tree` checks
        // it — so this is here to fail closed if it ever does, rather than to be reachable.
        let live: i64 = sqlx::query_scalar(&format!(
            "SELECT count(*) FROM documents WHERE {SUBTREE} AND deleted_at IS NULL"
        ))
        .bind(path)
        .fetch_one(&mut *tx)
        .await?;
        if live > 0 {
            tx.rollback().await?;
            return Ok(PurgeOutcome::Blocked(format!(
                "{live} page(s) under {path} are not in the trash; a purge destroys only what \
                 has already been deleted"
            )));
        }

        let before = Totals::read(&mut tx).await?;

        // THE destroying statement, and the names come out of it rather than out of a
        // `SELECT` written to resemble it. See ADR 0012.
        let mut pages: Vec<PurgedPage> = sqlx::query_as::<_, (String, String)>(&format!(
            "DELETE FROM documents WHERE {SUBTREE} RETURNING path, title"
        ))
        .bind(path)
        .fetch_all(&mut *tx)
        .await?
        .into_iter()
        .map(|(path, title)| PurgedPage { path, title })
        .collect();
        // RETURNING makes no promise about order; a report is read by a person.
        pages.sort_by(|a, b| a.path.cmp(&b.path));

        prune_empty_topics(&mut tx).await?;
        let after = Totals::read(&mut tx).await?;

        // The integrity check that makes the report a measurement rather than a claim: what
        // the DELETE handed back has to be what disappeared. It is not theatre — the obvious
        // `deleted_root REFERENCES documents(id) ON DELETE CASCADE` would delete an entry's
        // members before the outer DELETE reached them, and this is the assertion that would
        // have caught it. `0012_trash.sql` records why that column carries no foreign key.
        let destroyed = before.documents - after.documents;
        anyhow::ensure!(
            destroyed == pages.len() as i64,
            "the purge destroyed {destroyed} pages and reported {}",
            pages.len()
        );

        let report = PurgeReport {
            committed: mode == Purge::Commit,
            revisions: before.revisions - after.revisions,
            tasks: before.tasks - after.tasks,
            projects: before.projects - after.projects,
            links: before.links - after.links,
            topic_filings: before.topic_filings - after.topic_filings,
            topics: before.topics - after.topics,
            attachments: before.attachments - after.attachments,
            // The other way round, and deliberately: orphans are what the purge CREATED.
            blobs_orphaned: after.orphan_blobs - before.orphan_blobs,
            pages,
        };

        if mode == Purge::Preview {
            // The preview IS the purge: it ran, it measured itself, and now none of it
            // happened. Nothing is recorded either — an audit row for a destruction that did
            // not occur is worse than none.
            tx.rollback().await?;
            return Ok(PurgeOutcome::Done(report));
        }

        // The only surviving trace. Every other record of these pages has just been
        // destroyed, which is what makes the paths part of the detail rather than a count.
        Self::record_audit(
            &mut *tx,
            Some(actor),
            "document.purge",
            Some(path),
            Some(path),
            &json!({
                "paths": report.pages.iter().map(|p| &p.path).collect::<Vec<_>>(),
                "revisions": report.revisions,
                "tasks": report.tasks,
                "projects": report.projects,
                "links": report.links,
                "topic_filings": report.topic_filings,
                "topics": report.topics,
                "attachments": report.attachments,
                "blobs_orphaned": report.blobs_orphaned,
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(PurgeOutcome::Done(report))
    }

    /// The live pages at and under `path`, nearest the root first.
    async fn live_subtree(&self, path: &str) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar(&format!(
            "SELECT path FROM documents WHERE {SUBTREE} AND deleted_at IS NULL ORDER BY path"
        ))
        .bind(path)
        .fetch_all(&self.pool)
        .await?)
    }

    /// The pages that went into the trash as one act, by the root's id.
    async fn entry_members(&self, root_id: &str) -> Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT path FROM documents WHERE deleted_root = ?1 ORDER BY path")
                .bind(root_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }
}

/// Every table a purge reaches, counted whole.
///
/// Whole totals rather than "rows matching the subtree", and that is the point: a count taken
/// with a predicate written to resemble the `DELETE`'s can be edited apart from it, and the
/// day they disagree the number an administrator confirmed is not the number of things that
/// went. A difference across the statement itself cannot disagree with the statement.
#[derive(Debug, Clone, Copy)]
struct Totals {
    documents: i64,
    revisions: i64,
    tasks: i64,
    projects: i64,
    links: i64,
    topic_filings: i64,
    topics: i64,
    attachments: i64,
    /// Blobs nothing references. The only figure here that is not a whole-table count, and it
    /// is still a whole-corpus one: a predicate written to resemble the `DELETE`'s would be
    /// the second statement ADR 0012 refuses, and there is no way to express "lost its last
    /// reference" as a table total.
    orphan_blobs: i64,
}

impl Totals {
    async fn read(tx: &mut sqlx::SqliteConnection) -> Result<Self> {
        async fn count(tx: &mut sqlx::SqliteConnection, table: &str) -> Result<i64> {
            Ok(sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
                .fetch_one(&mut *tx)
                .await?)
        }
        Ok(Self {
            documents: count(&mut *tx, "documents").await?,
            revisions: count(&mut *tx, "revisions").await?,
            tasks: count(&mut *tx, "tasks").await?,
            projects: count(&mut *tx, "projects").await?,
            links: count(&mut *tx, "links").await?,
            topic_filings: count(&mut *tx, "document_tags").await?,
            topics: count(&mut *tx, "tags").await?,
            attachments: count(&mut *tx, "attachments").await?,
            orphan_blobs: sqlx::query_scalar(
                "SELECT count(*) FROM blobs b WHERE NOT EXISTS ( \
                   SELECT 1 FROM attachments a WHERE a.sha256 = b.sha256)",
            )
            .fetch_one(&mut *tx)
            .await?,
        })
    }
}

/// Refuse to commit a live page whose parent is in the trash.
///
/// The invariant the whole module rests on, asserted inside the transaction that could break
/// it rather than promised in a comment. It is cheap — one indexed pass over a corpus of tens
/// of pages — and it is checked on the way OUT of both operations because both of them can
/// produce the state: a delete that missed a child, a restore that put a page back under a
/// parent still in the trash.
///
/// A hole is silent, which is why this is an error and not a warning: the orphan stays
/// readable at its own URL and on its board, and disappears only from the navigation and
/// from the markdown export — where "it was never written" and "it was deleted" look the same.
async fn refuse_a_hole_in_the_tree(tx: &mut sqlx::SqliteConnection) -> Result<()> {
    let orphans: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM documents c WHERE c.deleted_at IS NULL AND EXISTS ( \
           SELECT 1 FROM documents p \
            WHERE p.path = c.parent_path AND p.deleted_at IS NOT NULL)",
    )
    .fetch_one(&mut *tx)
    .await?;
    anyhow::ensure!(
        orphans == 0,
        "{orphans} live page(s) would be left under a page in the trash: unreachable in the \
         navigation and absent from the export, while still readable at their own address"
    );
    Ok(())
}

/// What a purge did, or would have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeOutcome {
    Done(PurgeReport),
    /// Nothing is in the trash at that path. Not the same as "no such page": purging a LIVE
    /// page is not an operation, because D-14 makes the trash the only way in.
    Refused,
    /// The subtree is not in the state a purge may act on. Carries what is wrong.
    Blocked(String),
}

#[cfg(test)]
mod tests {
    //! What a delete leaves behind, what a restore brings back, and what a purge takes.

    use super::{Purge, PurgeOutcome, TrashOutcome};
    use crate::{Author, NewDocument, Store};
    use gw_auth::{Action, Permission, Principal, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Visibility};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
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

    async fn page(store: &Store, parent: Option<&str>, title: &str, v: Visibility) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
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

    async fn filed(store: &Store, parent: Option<&str>, title: &str, topics: &[&str]) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body(),
                    sort_key: 0,
                    topics: topics.iter().map(|t| (*t).to_string()).collect(),
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

    async fn count(store: &Store, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    /// Every live page whose parent is in the trash. The hole this module exists to prevent.
    async fn orphans(store: &Store) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM documents c \
             WHERE c.deleted_at IS NULL AND EXISTS ( \
               SELECT 1 FROM documents p \
                WHERE p.path = c.parent_path AND p.deleted_at IS NOT NULL)",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap()
    }

    fn done(outcome: TrashOutcome) -> super::TrashSummary {
        match outcome {
            TrashOutcome::Done(summary) => summary,
            other => panic!("expected the change to be applied, got {other:?}"),
        }
    }

    fn report(outcome: PurgeOutcome) -> super::PurgeReport {
        match outcome {
            PurgeOutcome::Done(report) => report,
            other => panic!("expected a purge, got {other:?}"),
        }
    }

    // --- deleting -----------------------------------------------------------------------

    #[tokio::test]
    async fn a_deleted_page_leaves_the_tree_and_stays_in_the_database() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;

        let summary = done(store.trash_document(&chef, "/notiz").await.unwrap());
        assert_eq!(summary.path, "/notiz");
        assert_eq!(summary.pages, 1);

        assert!(store.tree_for(&chef).await.unwrap().is_empty());
        assert!(!store.document_exists("/notiz").await.unwrap());
        assert!(store
            .document_for(&chef, "/notiz", Action::Read)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            count(&store, "documents").await,
            1,
            "the trash is not a delete: the row and its history stay"
        );
        assert_eq!(count(&store, "revisions").await, 1);
    }

    #[tokio::test]
    async fn deleting_a_page_takes_its_subtree_with_it() {
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Onboarding", Visibility::Public).await;
        page(
            &store,
            Some("/handbuch/onboarding"),
            "Tag Eins",
            Visibility::Public,
        )
        .await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;

        let summary = done(store.trash_document(&chef, "/handbuch").await.unwrap());
        assert_eq!(
            summary.pages, 3,
            "a page in the trash whose children are not is a tree with a hole in it"
        );
        assert_eq!(orphans(&store).await, 0);
        assert!(store.tree_for(&chef).await.unwrap().is_empty());
        assert!(!store
            .document_exists("/handbuch/onboarding/tag-eins")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn deleting_needs_write_on_the_page() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let leser = who(&store, "leser", Permission::Read, &["/notiz"]).await;

        assert_eq!(
            store.trash_document(&leser, "/notiz").await.unwrap(),
            TrashOutcome::Refused
        );
        assert!(store.document_exists("/notiz").await.unwrap());

        // Anti-vacuity: the same page, the same call, somebody who may write it.
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());
    }

    #[tokio::test]
    async fn deleting_needs_write_on_every_page_that_moves() {
        // `/handbuch` is `chef`'s to write; `/handbuch/intern` carries its own grants and is
        // not. The subtree moves together or not at all, so the whole delete is refused —
        // and nothing moves.
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Intern", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        who(&store, "andere", Permission::Write, &["/handbuch/intern"]).await;

        match store.trash_document(&chef, "/handbuch").await.unwrap() {
            TrashOutcome::Blocked(reason) => assert!(
                reason.contains("subpage"),
                "the refusal must say what to do about it: {reason}"
            ),
            other => panic!("a subtree that is not all theirs was swept away: {other:?}"),
        }
        assert!(store.document_exists("/handbuch").await.unwrap());
        assert!(store.document_exists("/handbuch/intern").await.unwrap());
    }

    #[tokio::test]
    async fn deleting_needs_a_signed_in_account_even_where_anyone_may_write() {
        // A public share link makes a page writable by somebody who has not said who they
        // are — see `DocumentAccess::may_write`. Emptying the wiki into a trash that cannot
        // say who did it is not the same act as editing a paragraph.
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        store
            .add_grant("/notiz", Subject::Anyone, Permission::Write)
            .await
            .unwrap();
        let anon = Principal::anonymous();

        assert!(
            store
                .document_access(&anon, "/notiz", Action::Read)
                .await
                .unwrap()
                .unwrap()
                .may_write,
            "the fixture has to grant the write, or this test proves nothing"
        );
        assert_eq!(
            store.trash_document(&anon, "/notiz").await.unwrap(),
            TrashOutcome::Refused
        );
        assert!(store.document_exists("/notiz").await.unwrap());
    }

    #[tokio::test]
    async fn deleting_a_page_that_is_already_in_the_trash_is_not_success() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());

        assert_eq!(
            store.trash_document(&chef, "/notiz").await.unwrap(),
            TrashOutcome::Refused
        );
    }

    // --- the listing --------------------------------------------------------------------

    #[tokio::test]
    async fn a_page_you_could_not_see_is_not_one_you_can_see_in_the_trash() {
        let store = store().await;
        page(&store, None, "Geheim", Visibility::Restricted).await;
        let chef = who(&store, "chef", Permission::Write, &["/geheim"]).await;
        done(store.trash_document(&chef, "/geheim").await.unwrap());

        let fremder = Principal::test("fremder", &[], &[]);
        assert!(
            store.trash_for(&fremder).await.unwrap().is_empty(),
            "the trash listed a page its ACL was hiding"
        );
        let mine = store.trash_for(&chef).await.unwrap();
        assert_eq!(mine.len(), 1);
        assert_eq!(mine[0].path, "/geheim");
        assert_eq!(mine[0].pages, 1);
        assert_eq!(mine[0].deleted_by_name, "chef");
        assert!(mine[0].may_restore);
    }

    #[tokio::test]
    async fn the_count_beside_an_entry_is_the_pages_that_caller_may_read() {
        // `/handbuch` is public and `/handbuch/intern` is restricted to `chef`. `leser` may
        // read one of the two pages in the entry, and is told about one.
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Intern", Visibility::Restricted).await;
        let chef = who(
            &store,
            "chef",
            Permission::Write,
            &["/handbuch", "/handbuch/intern"],
        )
        .await;
        let leser = who(&store, "leser", Permission::Read, &["/handbuch"]).await;
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        let theirs = store.trash_for(&leser).await.unwrap();
        assert_eq!(theirs.len(), 1);
        assert_eq!(theirs[0].pages, 1, "the count leaked a page the ACL hides");
        assert!(
            !theirs[0].may_restore,
            "a restore they cannot perform must not be offered"
        );

        let mine = store.trash_for(&chef).await.unwrap();
        assert_eq!(mine[0].pages, 2);
        assert!(mine[0].may_restore);
    }

    #[tokio::test]
    async fn the_trash_lists_acts_and_not_the_pages_that_went_down_with_them() {
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Onboarding", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        let entries = store.trash_for(&chef).await.unwrap();
        assert_eq!(entries.len(), 1, "one delete is one entry");
        assert_eq!(entries[0].path, "/handbuch");
    }

    // --- restoring ----------------------------------------------------------------------

    #[tokio::test]
    async fn restoring_puts_back_exactly_what_went_down_with_it() {
        // `/handbuch/alt` was thrown away on its own first. Restoring `/handbuch` must not
        // resurrect it — it was somebody's deliberate act, and undoing it silently is
        // exactly "it came back" as against "it vanished".
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Alt", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Neu", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;

        done(store.trash_document(&chef, "/handbuch/alt").await.unwrap());
        let swept = done(store.trash_document(&chef, "/handbuch").await.unwrap());
        assert_eq!(swept.pages, 2, "the page already in the trash did not move");

        let back = done(store.restore_document(&chef, "/handbuch").await.unwrap());
        assert_eq!(back.pages, 2);
        assert!(store.document_exists("/handbuch").await.unwrap());
        assert!(store.document_exists("/handbuch/neu").await.unwrap());
        assert!(
            !store.document_exists("/handbuch/alt").await.unwrap(),
            "a restore brought back a page somebody had deliberately thrown away"
        );
        assert_eq!(store.trash_for(&chef).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn restoring_under_a_parent_still_in_the_trash_is_refused_and_names_it() {
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Alt", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        done(store.trash_document(&chef, "/handbuch/alt").await.unwrap());
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        match store
            .restore_document(&chef, "/handbuch/alt")
            .await
            .unwrap()
        {
            TrashOutcome::Blocked(reason) => {
                assert!(reason.contains("/handbuch"), "{reason}");
                assert!(reason.contains("restore"), "{reason}");
            }
            other => panic!("a live page was put under a parent in the trash: {other:?}"),
        }
        assert_eq!(orphans(&store).await, 0);
    }

    #[tokio::test]
    async fn restoring_needs_write_on_the_page() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());

        let leser = who(&store, "leser", Permission::Read, &["/notiz"]).await;
        assert_eq!(
            store.restore_document(&leser, "/notiz").await.unwrap(),
            TrashOutcome::Refused
        );
        assert!(!store.document_exists("/notiz").await.unwrap());
        done(store.restore_document(&chef, "/notiz").await.unwrap());
        assert!(store.document_exists("/notiz").await.unwrap());
    }

    #[tokio::test]
    async fn restoring_something_that_is_not_an_entry_is_refused() {
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Neu", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        assert_eq!(
            store
                .restore_document(&chef, "/handbuch/neu")
                .await
                .unwrap(),
            TrashOutcome::Refused,
            "a member of an entry is not restorable on its own"
        );
    }

    // --- the path a trashed page still occupies -----------------------------------------

    #[tokio::test]
    async fn the_path_of_a_page_in_the_trash_cannot_be_taken_and_the_refusal_says_why() {
        // `documents.path` is UNIQUE across every row, trashed ones included — which is what
        // makes a restore always safe, and what makes this refusal necessary.
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());

        let error = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Notiz".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body(),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .expect_err("a page was created on a path the trash still holds");
        let error = error.to_string();
        assert!(error.contains("trash"), "{error}");
        assert!(error.contains("/notiz"), "{error}");
    }

    // --- purging ------------------------------------------------------------------------

    #[tokio::test]
    async fn a_purge_names_every_page_it_destroys_and_counts_what_cascades() {
        let store = store().await;
        let handbuch = page(&store, None, "Handbuch", Visibility::Public).await;
        let onboarding = page(&store, Some("/handbuch"), "Onboarding", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;

        sqlx::query("INSERT INTO links (from_doc, to_doc) VALUES (?1, ?2)")
            .bind(&handbuch)
            .bind(&onboarding)
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tasks (id, doc_id, title) VALUES (?1, ?2, 'Zeile')")
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&onboarding)
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO projects (id, home_doc) VALUES (?1, ?2)")
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&handbuch)
            .execute(&store.pool)
            .await
            .unwrap();

        done(store.trash_document(&chef, "/handbuch").await.unwrap());
        let report = report(
            store
                .purge_document(&chef.id, "/handbuch", Purge::Commit)
                .await
                .unwrap(),
        );

        assert!(report.committed);
        assert_eq!(
            report
                .pages
                .iter()
                .map(|p| p.path.as_str())
                .collect::<Vec<_>>(),
            vec!["/handbuch", "/handbuch/onboarding"],
            "a purge that does not name what it destroys is not the second deliberate act"
        );
        assert_eq!(report.revisions, 2);
        assert_eq!(report.links, 1);
        assert_eq!(report.tasks, 1);
        assert_eq!(report.projects, 1);

        assert_eq!(count(&store, "documents").await, 0);
        assert_eq!(count(&store, "revisions").await, 0);
        assert_eq!(count(&store, "links").await, 0);
        assert_eq!(count(&store, "tasks").await, 0);
        assert_eq!(count(&store, "projects").await, 0);
    }

    #[tokio::test]
    async fn a_preview_reports_exactly_what_the_purge_does_and_destroys_nothing() {
        // The preview IS the purge, rolled back (ADR 0012). Asserting the two reports are
        // equal is asserting that no second query was written to describe the first.
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Onboarding", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        let preview = report(
            store
                .purge_document(&chef.id, "/handbuch", Purge::Preview)
                .await
                .unwrap(),
        );
        assert!(!preview.committed);
        assert_eq!(
            count(&store, "documents").await,
            2,
            "a preview destroyed data"
        );
        assert_eq!(count(&store, "revisions").await, 2);
        assert_eq!(
            store.trash_for(&chef).await.unwrap().len(),
            1,
            "the entry survived the description of its own destruction"
        );

        let real = report(
            store
                .purge_document(&chef.id, "/handbuch", Purge::Commit)
                .await
                .unwrap(),
        );
        assert_eq!(preview.pages, real.pages);
        assert_eq!(preview.revisions, real.revisions);
        assert_eq!(preview.tasks, real.tasks);
        assert_eq!(preview.links, real.links);
        assert_eq!(preview.projects, real.projects);
        assert_eq!(preview.topic_filings, real.topic_filings);
        assert_eq!(preview.topics, real.topics);
    }

    #[tokio::test]
    async fn a_purge_never_reaches_a_live_page() {
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, None, "Handbuch Extra", Visibility::Public).await;
        let chef = who(
            &store,
            "chef",
            Permission::Write,
            &["/handbuch", "/handbuch-extra"],
        )
        .await;
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        report(
            store
                .purge_document(&chef.id, "/handbuch", Purge::Commit)
                .await
                .unwrap(),
        );
        assert!(
            store.document_exists("/handbuch-extra").await.unwrap(),
            "a prefix match destroyed a page that is not inside the purged subtree"
        );
    }

    #[tokio::test]
    async fn purging_a_page_that_is_not_in_the_trash_is_refused() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;

        assert_eq!(
            store
                .purge_document(&chef.id, "/notiz", Purge::Commit)
                .await
                .unwrap(),
            PurgeOutcome::Refused,
            "the trash is the only way in"
        );
        assert!(store.document_exists("/notiz").await.unwrap());
    }

    #[tokio::test]
    async fn purging_a_trashed_parent_takes_a_separately_trashed_child_with_it() {
        // `/handbuch/alt` is its own entry. Once `/handbuch` is gone it can never be
        // restored — its parent would not be there — so it goes, and the report says so.
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Alt", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        done(store.trash_document(&chef, "/handbuch/alt").await.unwrap());
        done(store.trash_document(&chef, "/handbuch").await.unwrap());

        let report = report(
            store
                .purge_document(&chef.id, "/handbuch", Purge::Commit)
                .await
                .unwrap(),
        );
        assert_eq!(report.pages.len(), 2);
        assert!(report.pages.iter().any(|p| p.path == "/handbuch/alt"));
        assert_eq!(count(&store, "documents").await, 0);
        assert!(store.trash_for(&chef).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_purge_takes_the_files_off_the_page_and_says_what_it_orphaned() {
        // D-14: the report names what it destroys, "and the count includes the things that
        // cascade". Attachments cascade. The bytes do NOT go — ADR 0013 — so the report has
        // to say how many files nothing references any more, or a purge would silently claim
        // to have removed something it left on the mount.
        let store = store().await;
        let dir = tempfile::tempdir().unwrap();
        let blobs = crate::BlobStore::open(dir.path()).unwrap();
        page(&store, None, "Notiz", Visibility::Public).await;
        page(&store, None, "Andere", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Admin, &["/notiz", "/andere"]).await;

        // Two files on the doomed page. One of them is also on a page that survives, so
        // exactly one blob loses its last reference.
        for (path, name, tail) in [
            ("/notiz", "nur-hier.png", "a"),
            ("/notiz", "geteilt.png", "b"),
            ("/andere", "geteilt.png", "b"),
        ] {
            let mut writer = blobs.writer().unwrap();
            let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            bytes.extend_from_slice(tail.as_bytes());
            writer.push(&bytes).await.unwrap();
            let crate::BlobOutcome::Accepted(pending) = writer.finish().await.unwrap() else {
                panic!("a PNG must be acceptable");
            };
            assert!(matches!(
                store.attach(&chef, path, name, pending).await.unwrap(),
                crate::AttachOutcome::Done(_)
            ));
        }

        store.trash_document(&chef, "/notiz").await.unwrap();
        let preview = report(
            store
                .purge_document(&chef.id, "/notiz", Purge::Preview)
                .await
                .unwrap(),
        );
        assert_eq!(preview.attachments, 2);
        assert_eq!(
            preview.blobs_orphaned, 1,
            "only the file the surviving page does not also carry"
        );

        let done = report(
            store
                .purge_document(&chef.id, "/notiz", Purge::Commit)
                .await
                .unwrap(),
        );
        assert_eq!(
            (done.attachments, done.blobs_orphaned),
            (preview.attachments, preview.blobs_orphaned),
            "the preview IS the purge, so the numbers cannot differ"
        );
        assert_eq!(count(&store, "attachments").await, 1);
        assert_eq!(
            count(&store, "blobs").await,
            2,
            "the index keeps the orphan, so a sweep can find it"
        );
    }

    #[tokio::test]
    async fn purging_the_last_page_under_a_topic_takes_the_topic_with_it() {
        // `prune_empty_topics` says a document in the trash still counts as carrying its
        // topics, because `deleted_at` is reversible. A purge is not.
        let store = store().await;
        filed(&store, None, "Darmflora", &["Medizin/Darm"]).await;
        let chef = who(&store, "chef", Permission::Write, &["/darmflora"]).await;
        assert_eq!(count(&store, "tags").await, 2);

        done(store.trash_document(&chef, "/darmflora").await.unwrap());
        assert_eq!(
            count(&store, "tags").await,
            2,
            "a topic dropped while a page sits in the trash would not come back with it"
        );

        let report = report(
            store
                .purge_document(&chef.id, "/darmflora", Purge::Commit)
                .await
                .unwrap(),
        );
        assert_eq!(report.topic_filings, 1);
        assert_eq!(report.topics, 2);
        assert_eq!(count(&store, "tags").await, 0);
    }

    #[tokio::test]
    async fn purging_a_page_leaves_the_policy_on_its_path_alone() {
        // A grant is a fact about a PATH, not about a document — `set_visibility_audited`
        // says so, and `acl` rows may be written on a path no page occupies so that access
        // can be prepared before a page arrives. Destroying a page does not withdraw it.
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());
        report(
            store
                .purge_document(&chef.id, "/notiz", Purge::Commit)
                .await
                .unwrap(),
        );

        assert_eq!(
            store.grants_for_path("/notiz").await.unwrap().len(),
            1,
            "a purge withdrew a grant that outlives the page by design"
        );
    }

    // --- the invariants, forced ---------------------------------------------------------

    #[tokio::test]
    async fn a_row_cannot_be_half_in_the_trash() {
        // `deleted_at`, `deleted_root`, `deleted_by` and `deleted_by_name` are one fact.
        // A row with only the timestamp set is invisible in the tree and belongs to no
        // entry: unreachable, unrestorable, and unnoticeable.
        let store = store().await;
        let id = page(&store, None, "Notiz", Visibility::Public).await;

        let outcome =
            sqlx::query("UPDATE documents SET deleted_at = datetime('now') WHERE id = ?1")
                .bind(&id)
                .execute(&store.pool)
                .await;
        let error = outcome.expect_err("a row was written half-way into the trash");
        assert!(error.to_string().contains("one fact"), "{error}");

        let outcome = sqlx::query("UPDATE documents SET deleted_root = ?1 WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await;
        assert!(
            outcome.is_err(),
            "a live row was claimed by a trash entry that would restore it again"
        );
    }

    #[tokio::test]
    async fn a_hole_in_the_tree_refuses_the_next_delete_rather_than_committing_on_top_of_it() {
        // The state cannot be produced through this module, so it is produced by hand. What
        // is being asserted is the direction of the failure: a live page under a page in the
        // trash is invisible in the navigation and absent from the export while still
        // readable at its own address, and no further delete may be committed over it.
        let store = store().await;
        let handbuch = page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Neu", Visibility::Public).await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch", "/notiz"]).await;

        sqlx::query(
            "UPDATE documents SET deleted_at = datetime('now'), deleted_root = id, \
             deleted_by = 'x', deleted_by_name = 'X' WHERE id = ?1",
        )
        .bind(&handbuch)
        .execute(&store.pool)
        .await
        .unwrap();
        assert_eq!(
            orphans(&store).await,
            1,
            "the fixture has to build the hole"
        );

        let error = store
            .trash_document(&chef, "/notiz")
            .await
            .expect_err("a delete was committed over a tree with a hole in it");
        assert!(
            error.to_string().contains("under a page in the trash"),
            "{error}"
        );
        assert!(
            store.document_exists("/notiz").await.unwrap(),
            "the refusal has to roll back, or it is a report rather than a refusal"
        );
    }

    #[tokio::test]
    async fn a_purge_refuses_a_subtree_that_still_holds_a_live_page() {
        // The same hand-built state, asked of the operation that destroys. A purge takes a
        // whole subtree, so a live page inside one would be destroyed without ever having
        // been deleted — no trash, no restore, no second deliberate act.
        let store = store().await;
        let handbuch = page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Neu", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;

        sqlx::query(
            "UPDATE documents SET deleted_at = datetime('now'), deleted_root = id, \
             deleted_by = 'x', deleted_by_name = 'X' WHERE id = ?1",
        )
        .bind(&handbuch)
        .execute(&store.pool)
        .await
        .unwrap();

        match store
            .purge_document(&chef.id, "/handbuch", Purge::Commit)
            .await
            .unwrap()
        {
            PurgeOutcome::Blocked(reason) => {
                assert!(reason.contains("not in the trash"), "{reason}")
            }
            other => panic!("a live page was destroyed by a purge: {other:?}"),
        }
        assert_eq!(count(&store, "documents").await, 2);
    }

    #[tokio::test]
    async fn an_entry_holding_a_page_the_caller_cannot_even_see_offers_no_restore() {
        // `chef` may read AND write `/handbuch`, so every page they can see in this entry is
        // one they could put back — and the answer is still no, because the entry holds one
        // more. Fail closed: an offer that would be refused is a control that lies.
        let store = store().await;
        page(&store, None, "Handbuch", Visibility::Public).await;
        page(&store, Some("/handbuch"), "Intern", Visibility::Restricted).await;
        let chefin = who(
            &store,
            "chefin",
            Permission::Write,
            &["/handbuch", "/handbuch/intern"],
        )
        .await;
        let chef = who(&store, "chef", Permission::Write, &["/handbuch"]).await;
        done(store.trash_document(&chefin, "/handbuch").await.unwrap());

        let entries = store.trash_for(&chef).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].pages, 1);
        assert!(
            !entries[0].may_restore,
            "a restore was offered on an entry holding a page the caller cannot see"
        );
        assert!(
            matches!(
                store.restore_document(&chef, "/handbuch").await.unwrap(),
                TrashOutcome::Blocked(_)
            ),
            "and the restore itself has to refuse, or the bit was merely pessimistic"
        );
    }

    // --- the record ---------------------------------------------------------------------

    #[tokio::test]
    async fn a_purge_is_recorded_because_nothing_else_survives_to_say_it_happened() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());

        report(
            store
                .purge_document(&chef.id, "/notiz", Purge::Preview)
                .await
                .unwrap(),
        );
        let admin = Principal::test("admin", &["admins"], &[]);
        let actions: Vec<String> = store
            .audit_for(&admin, 100)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|e| e.action)
            .collect();
        assert!(
            !actions.contains(&"document.purge".to_string()),
            "a preview recorded a destruction that did not happen"
        );

        report(
            store
                .purge_document(&chef.id, "/notiz", Purge::Commit)
                .await
                .unwrap(),
        );
        let actions: Vec<String> = store
            .audit_for(&admin, 100)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|e| e.action)
            .collect();
        assert!(
            actions.contains(&"document.purge".to_string()),
            "{actions:?}"
        );
        assert!(
            actions.contains(&"document.trash".to_string()),
            "{actions:?}"
        );
    }

    #[tokio::test]
    async fn a_restore_is_recorded_because_the_row_stops_saying_it_was_deleted() {
        let store = store().await;
        page(&store, None, "Notiz", Visibility::Public).await;
        let chef = who(&store, "chef", Permission::Write, &["/notiz"]).await;
        done(store.trash_document(&chef, "/notiz").await.unwrap());
        done(store.restore_document(&chef, "/notiz").await.unwrap());

        let admin = Principal::test("admin", &["admins"], &[]);
        let actions: Vec<String> = store
            .audit_for(&admin, 100)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|e| e.action)
            .collect();
        assert!(
            actions.contains(&"document.restore".to_string()),
            "{actions:?}"
        );
    }
}
