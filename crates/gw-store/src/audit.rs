//! The audit log: writing it, and reading it without leaking it.
//!
//! Reading is the interesting half. The log records who was granted access to what and
//! who looked at whom, so it is frequently *more* sensitive than the documents it
//! describes. D-M2-6 makes it readable by instance admins and, for the entries concerning
//! a subtree, by whoever administers that subtree.
//!
//! That means this file contains a retrieval path, and the architecture rule applies with
//! full force: **the filtering happens inside the retriever**. There is deliberately no
//! `all_entries()` for a caller to filter afterwards, because the first caller that forgot
//! to would leak the whole log and nothing would have failed.

use crate::Store;
use anyhow::Result;
use gw_auth::{can, Action, Principal};
use gw_core::Visibility;
use serde::Serialize;
use sqlx::FromRow;
use std::collections::HashMap;

/// One recorded action.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AuditEntry {
    pub id: String,
    pub at: String,
    /// `None` for an action taken by nobody in particular — a migration, or the system.
    pub principal_id: Option<String>,
    /// A dotted verb: `acl.grant`, `principal.deactivate`, `team.member.add`.
    pub action: String,
    /// What the action was applied to, in whatever terms the action uses.
    pub target: Option<String>,
    /// The subtree this concerns. `None` means instance-wide — see the 0004 migration.
    pub path: Option<String>,
    pub detail: String,
}

/// How many scoped entries a non-instance-admin read will examine before giving up.
///
/// A space admin's page is assembled by checking entries against their permissions one
/// distinct path at a time, so it cannot be expressed as a single indexed query. This
/// bounds that work. It is stated here, and reported by [`Store::audit_for`], because a
/// silently truncated audit log reads exactly like an audit log with nothing in it.
const SCOPED_SCAN_LIMIT: i64 = 2_000;

/// What a read of the audit log returned, and whether it saw everything.
#[derive(Debug, Clone, Serialize)]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    /// True when the scan hit [`SCOPED_SCAN_LIMIT`] before exhausting the log, so older
    /// entries this caller may be entitled to were not examined. Never silently dropped.
    pub truncated: bool,
}

impl Store {
    /// Record an action.
    ///
    /// `path` is the subtree the action concerns; `None` means instance-wide and is
    /// therefore readable by instance admins only. `None` is the closed choice: an action
    /// that forgets to state its scope becomes invisible to space admins rather than
    /// visible to the wrong ones.
    ///
    /// Takes an executor so a caller can pass a transaction — an action must not be able
    /// to succeed while the record of it is rolled back, or is written and then lost.
    pub async fn record_audit<'e, E>(
        executor: E,
        actor: Option<&str>,
        action: &str,
        target: Option<&str>,
        path: Option<&str>,
        detail: &serde_json::Value,
    ) -> Result<()>
    where
        E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
    {
        sqlx::query(
            "INSERT INTO audit_log (id, principal_id, action, target, path, detail) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(actor)
        .bind(action)
        .bind(target)
        .bind(path)
        .bind(detail.to_string())
        .execute(executor)
        .await?;
        Ok(())
    }

    /// The audit entries `principal` is entitled to read, newest first.
    ///
    /// An instance admin sees everything. Anyone else sees only entries carrying a path
    /// they hold `admin` on — never the instance-wide ones, which is what keeps the
    /// existence of other people's accounts out of a space admin's console.
    ///
    /// Permission is evaluated per distinct path rather than by prefix, because grants do
    /// not union up the tree: the nearest ancestor holding any grants wins outright, so
    /// holding `admin` at `/a` says nothing about `/a/b` if `/a/b` carries its own grants.
    /// A prefix query would be faster and would quietly hand over subtrees that had been
    /// deliberately narrowed.
    pub async fn audit_for(&self, principal: &Principal, limit: i64) -> Result<AuditPage> {
        // An unauthenticated caller is refused before any row is read. `can()` would
        // reach the same answer, but not reading is a stronger guarantee than not
        // returning.
        if !principal.is_authenticated() {
            return Ok(AuditPage {
                entries: Vec::new(),
                truncated: false,
            });
        }

        if self.baseline_for(principal).await? >= crate::Baseline::Admin {
            let entries = sqlx::query_as::<_, AuditEntry>(
                "SELECT id, at, principal_id, action, target, path, detail \
                 FROM audit_log ORDER BY at DESC, id DESC LIMIT ?1",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;
            return Ok(AuditPage {
                entries,
                truncated: false,
            });
        }

        // `path IS NOT NULL` here and the `else { continue }` below are the same rule
        // twice, deliberately. Removing either one alone changes nothing observable —
        // mutation testing confirms it — so this is defence in depth rather than a
        // redundancy to tidy away. The SQL predicate is also what keeps instance-wide
        // entries from consuming the scan budget and skewing `truncated`.
        let candidates = sqlx::query_as::<_, AuditEntry>(
            "SELECT id, at, principal_id, action, target, path, detail \
             FROM audit_log WHERE path IS NOT NULL ORDER BY at DESC, id DESC LIMIT ?1",
        )
        .bind(SCOPED_SCAN_LIMIT)
        .fetch_all(&self.pool)
        .await?;

        let examined = candidates.len() as i64;
        let mut allowed: HashMap<String, bool> = HashMap::new();
        let mut entries = Vec::new();

        for entry in candidates {
            let Some(path) = entry.path.clone() else {
                continue;
            };
            let permitted = match allowed.get(&path) {
                Some(known) => *known,
                None => {
                    let grants = self.grants_for_path(&path).await?;
                    // `Restricted` is how this crate asks "is there a grant for this
                    // caller?" without duplicating the subject-matching rules: at that
                    // visibility `can()` consults grants and nothing else. A baseline
                    // must not stand in for an admin grant here — reading the audit log
                    // is not something `internal` reach confers.
                    let verdict = can(principal, Action::Admin, Visibility::Restricted, &grants);
                    allowed.insert(path, verdict);
                    verdict
                }
            };
            if permitted {
                entries.push(entry);
                if entries.len() as i64 >= limit {
                    return Ok(AuditPage {
                        entries,
                        truncated: false,
                    });
                }
            }
        }

        Ok(AuditPage {
            entries,
            truncated: examined >= SCOPED_SCAN_LIMIT,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;
    use gw_auth::{Permission, Principal, Subject};
    use serde_json::json;

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    async fn record(store: &Store, action: &str, path: Option<&str>) {
        Store::record_audit(&store.pool, Some("someone"), action, None, path, &json!({}))
            .await
            .unwrap();
    }

    fn actions(page: &super::AuditPage) -> Vec<&str> {
        page.entries.iter().map(|e| e.action.as_str()).collect()
    }

    #[tokio::test]
    async fn an_anonymous_caller_reads_nothing() {
        let store = store().await;

        // The `Anyone` grant is what makes this test mean something. Without it the
        // caller is refused because no grant matches ANY subject, so the assertion holds
        // whether or not authentication is checked at all — and a mutation that deleted
        // the authentication check entirely still passed. That is a vacuous test: it
        // asserts the right thing for the wrong reason.
        //
        // `Anyone` is the one subject an unauthenticated caller can match (it is what
        // public share links will use), so with this grant in place the ONLY thing
        // standing between an anonymous request and the audit log is the check being
        // tested here.
        store
            .add_grant("/raum", Subject::Anyone, Permission::Admin)
            .await
            .unwrap();
        record(&store, "acl.grant", Some("/raum")).await;
        record(&store, "principal.create", None).await;

        let page = store.audit_for(&Principal::anonymous(), 50).await.unwrap();
        assert!(
            page.entries.is_empty(),
            "an anonymous caller read the audit log: {:?}",
            actions(&page)
        );
    }

    #[tokio::test]
    async fn an_instance_admin_reads_everything_including_the_unscoped() {
        let store = store().await;
        record(&store, "acl.grant", Some("/raum")).await;
        record(&store, "principal.create", None).await;

        // `admins` maps to the admin baseline in 0002, so this is an instance admin.
        let admin = Principal::test("chef", &["admins"], &[]);
        let page = store.audit_for(&admin, 50).await.unwrap();

        let seen = actions(&page);
        assert!(seen.contains(&"acl.grant"), "got {seen:?}");
        assert!(seen.contains(&"principal.create"), "got {seen:?}");
        assert_eq!(seen.len(), 2, "got {seen:?}");
    }

    #[tokio::test]
    async fn a_space_admin_reads_their_subtree_and_nothing_else() {
        let store = store().await;
        let space_admin = Principal::test("lektor", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(space_admin.id.clone()),
                Permission::Admin,
            )
            .await
            .unwrap();

        record(&store, "meins", Some("/raum")).await;
        record(&store, "tiefer", Some("/raum/unterseite")).await;
        record(&store, "fremd", Some("/anderer-raum")).await;
        record(&store, "instanzweit", None).await;

        let page = store.audit_for(&space_admin, 50).await.unwrap();
        let seen = actions(&page);

        assert!(seen.contains(&"meins"), "own subtree missing: {seen:?}");
        assert!(
            seen.contains(&"tiefer"),
            "inherited descendant missing: {seen:?}"
        );
        // The two that must never appear. A prefix query gets these right too — it is
        // the next test that separates a correct implementation from a plausible one.
        assert!(
            !seen.contains(&"fremd"),
            "another space's entries leaked: {seen:?}"
        );
        assert!(
            !seen.contains(&"instanzweit"),
            "instance-wide entries leaked to a space admin: {seen:?}"
        );
        assert_eq!(seen.len(), 2, "got {seen:?}");
    }

    #[tokio::test]
    async fn a_subtree_that_narrows_access_is_not_readable_from_above() {
        // THE test. Grants do not union up the tree: the nearest ancestor holding any
        // grants wins outright, which is what makes narrowing possible. So holding admin
        // at /raum must NOT confer anything at /raum/geheim once that path carries its
        // own grants naming somebody else.
        //
        // A `WHERE path LIKE '/raum%'` implementation passes every other test in this
        // file and hands over the narrowed subtree here.
        let store = store().await;
        let space_admin = Principal::test("lektor", &[], &[]);
        let someone_else = Principal::test("andere", &[], &[]);

        store
            .add_grant(
                "/raum",
                Subject::Principal(space_admin.id.clone()),
                Permission::Admin,
            )
            .await
            .unwrap();
        store
            .add_grant(
                "/raum/geheim",
                Subject::Principal(someone_else.id.clone()),
                Permission::Admin,
            )
            .await
            .unwrap();

        record(&store, "offen", Some("/raum")).await;
        record(&store, "verengt", Some("/raum/geheim")).await;

        let page = store.audit_for(&space_admin, 50).await.unwrap();
        let seen = actions(&page);

        assert!(seen.contains(&"offen"), "got {seen:?}");
        assert!(
            !seen.contains(&"verengt"),
            "a narrowed subtree was readable from its parent: {seen:?}"
        );
    }

    #[tokio::test]
    async fn reading_the_log_needs_admin_and_not_merely_read() {
        // Being able to READ a space is not being able to read who was granted access to
        // it. Any weaker action here would turn every reader into an auditor.
        let store = store().await;
        let reader = Principal::test("leser", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(reader.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        record(&store, "acl.grant", Some("/raum")).await;

        let page = store.audit_for(&reader, 50).await.unwrap();
        assert!(page.entries.is_empty(), "read access exposed the audit log");
    }

    #[tokio::test]
    async fn a_team_grant_confers_the_same_reach_as_a_direct_one() {
        let store = store().await;
        let member = Principal::test("mitglied", &[], &["redaktion"]);
        store
            .add_grant(
                "/raum",
                Subject::Team("redaktion".into()),
                Permission::Admin,
            )
            .await
            .unwrap();
        record(&store, "acl.grant", Some("/raum")).await;

        let page = store.audit_for(&member, 50).await.unwrap();
        assert_eq!(actions(&page), vec!["acl.grant"]);
    }

    #[tokio::test]
    async fn a_deactivated_space_admin_reads_nothing() {
        let store = store().await;
        let mut suspended = Principal::test("gesperrt", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(suspended.id.clone()),
                Permission::Admin,
            )
            .await
            .unwrap();
        record(&store, "acl.grant", Some("/raum")).await;

        suspended.active = false;
        let page = store.audit_for(&suspended, 50).await.unwrap();
        assert!(
            page.entries.is_empty(),
            "a deactivated account still read the audit log"
        );
    }
}
