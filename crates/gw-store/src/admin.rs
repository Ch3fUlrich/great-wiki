//! Administrative mutations, each written in the same transaction as its audit row.
//!
//! Every method here has a plain sibling elsewhere in the crate that performs the change
//! and nothing else. Those are for seeding and for tests, where there is no actor to
//! record. Anything reached from the admin API goes through this module instead, because
//! D-M2-4 requires the record and the change to stand or fall together: [`Store::record_audit`]
//! takes an executor precisely so the two can share one transaction, and "write the row
//! afterwards" is exactly the shape that lets an action succeed unrecorded.
//!
//! The booleans these return are load-bearing, and they are deliberately asymmetric:
//!
//! - **Adding** something that is already there is idempotent success. The state is what
//!   the caller asked for. Nothing changed, so nothing is recorded.
//! - **Removing** something that is not there is *not* success. A revoke that matched no
//!   row commonly means the grant lives on an ancestor and is still in force, so an
//!   administrator reading "done" has concluded the opposite of what happened.

use crate::acl::{permission_column, subject_columns};
use crate::principals::{apply_active, insert_local_principal};
use crate::Store;
use anyhow::Result;
use gw_auth::{Permission, Principal, Subject};
use serde_json::json;

/// What a membership change actually did.
///
/// Four outcomes rather than a boolean because the failures are different mistakes with
/// different fixes: a slug that names no team is a typo in the team name, an id that
/// names nobody is a typo in the person, and "already a member" is neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipOutcome {
    /// A row was written or removed.
    Changed,
    /// The membership already matched the request. Nothing written, nothing recorded.
    Unchanged,
    /// The slug names no team.
    NoSuchTeam,
    /// The id names no principal.
    NoSuchPrincipal,
}

async fn team_id(conn: &mut sqlx::SqliteConnection, slug: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM teams WHERE slug = ?1")
        .bind(slug)
        .fetch_optional(conn)
        .await?;
    Ok(row.map(|(id,)| id))
}

async fn principal_exists(conn: &mut sqlx::SqliteConnection, id: &str) -> Result<bool> {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM principals WHERE id = ?1")
        .bind(id)
        .fetch_optional(conn)
        .await?;
    Ok(row.is_some())
}

impl Store {
    /// Create a local account, recording it as an instance-wide action.
    ///
    /// `password_hash` is already hashed by the caller — this crate never sees a
    /// plaintext password — and neither the hash nor anything derived from it goes into
    /// the audit detail.
    pub async fn create_local_principal_audited(
        &self,
        actor: &str,
        username: &str,
        display_name: &str,
        email: Option<&str>,
        password_hash: &str,
    ) -> Result<Principal> {
        let mut tx = self.pool.begin().await?;
        let id =
            insert_local_principal(&mut tx, username, display_name, email, password_hash).await?;
        Self::record_audit(
            &mut *tx,
            Some(actor),
            "principal.create",
            Some(&id),
            // Instance-wide: an account belongs to no subtree, so only instance admins
            // read it (0004).
            None,
            &json!({ "username": username, "kind": "local" }),
        )
        .await?;
        tx.commit().await?;

        self.principal_by_id(&id)
            .await?
            .map(|(p, _)| p)
            .ok_or_else(|| anyhow::anyhow!("principal vanished immediately after insert"))
    }

    /// Activate or deactivate an account. `Ok(None)` when the id names nobody.
    ///
    /// Deactivating deletes every session that principal holds, in this same transaction
    /// (D-M2-7) — so the flag, the sessions and the record of it either all happen or
    /// none of them do.
    pub async fn set_principal_active_audited(
        &self,
        actor: &str,
        id: &str,
        active: bool,
    ) -> Result<Option<Principal>> {
        let mut tx = self.pool.begin().await?;
        if !apply_active(&mut tx, id, active).await? {
            // Nothing matched: roll back rather than recording an action that did not
            // happen.
            tx.rollback().await?;
            return Ok(None);
        }
        Self::record_audit(
            &mut *tx,
            Some(actor),
            if active {
                "principal.activate"
            } else {
                "principal.deactivate"
            },
            Some(id),
            None,
            &json!({ "active": active }),
        )
        .await?;
        tx.commit().await?;

        Ok(self.principal_by_id(id).await?.map(|(p, _)| p))
    }

    pub async fn create_team_audited(&self, actor: &str, slug: &str, name: &str) -> Result<String> {
        let id = uuid::Uuid::now_v7().to_string();
        let mut tx = self.pool.begin().await?;

        sqlx::query("INSERT INTO teams (id, slug, name) VALUES (?1, ?2, ?3)")
            .bind(&id)
            .bind(slug)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        Self::record_audit(
            &mut *tx,
            Some(actor),
            "team.create",
            Some(slug),
            None,
            &json!({ "name": name }),
        )
        .await?;

        tx.commit().await?;
        Ok(id)
    }

    /// Put somebody in a team.
    ///
    /// The team and the principal are looked up explicitly rather than left to the insert,
    /// because `INSERT ... SELECT id FROM teams WHERE slug = ?` writes no rows for a slug
    /// that names no team and cannot be told apart from "already a member" afterwards.
    /// Those are a typo and a no-op respectively, and the caller must be able to answer
    /// differently.
    pub async fn add_team_member_audited(
        &self,
        actor: &str,
        team_slug: &str,
        principal_id: &str,
    ) -> Result<MembershipOutcome> {
        let mut tx = self.pool.begin().await?;

        let Some(team) = team_id(&mut tx, team_slug).await? else {
            tx.rollback().await?;
            return Ok(MembershipOutcome::NoSuchTeam);
        };
        if !principal_exists(&mut tx, principal_id).await? {
            tx.rollback().await?;
            return Ok(MembershipOutcome::NoSuchPrincipal);
        }

        let written = sqlx::query(
            "INSERT OR IGNORE INTO team_members (team_id, principal_id) VALUES (?1, ?2)",
        )
        .bind(&team)
        .bind(principal_id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        if !written {
            tx.rollback().await?;
            return Ok(MembershipOutcome::Unchanged);
        }

        Self::record_audit(
            &mut *tx,
            Some(actor),
            "team.member.add",
            Some(team_slug),
            None,
            &json!({ "principal_id": principal_id }),
        )
        .await?;
        tx.commit().await?;
        Ok(MembershipOutcome::Changed)
    }

    pub async fn remove_team_member_audited(
        &self,
        actor: &str,
        team_slug: &str,
        principal_id: &str,
    ) -> Result<MembershipOutcome> {
        let mut tx = self.pool.begin().await?;

        let Some(team) = team_id(&mut tx, team_slug).await? else {
            tx.rollback().await?;
            return Ok(MembershipOutcome::NoSuchTeam);
        };

        let removed =
            sqlx::query("DELETE FROM team_members WHERE team_id = ?1 AND principal_id = ?2")
                .bind(&team)
                .bind(principal_id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                > 0;

        if !removed {
            tx.rollback().await?;
            return Ok(MembershipOutcome::Unchanged);
        }

        Self::record_audit(
            &mut *tx,
            Some(actor),
            "team.member.remove",
            Some(team_slug),
            None,
            &json!({ "principal_id": principal_id }),
        )
        .await?;
        tx.commit().await?;
        Ok(MembershipOutcome::Changed)
    }

    /// Write a grant on `path`. Returns whether a row was actually inserted.
    ///
    /// The audit row is scoped to `path`, so whoever administers that subtree can see it
    /// (D-M2-6) — a space admin needs to know who was granted access to their space, and
    /// that is the entry that says so.
    pub async fn add_grant_audited(
        &self,
        actor: &str,
        path: &str,
        subject: &Subject,
        permission: Permission,
    ) -> Result<bool> {
        let (kind, id) = subject_columns(subject);
        let perm = permission_column(permission);
        let mut tx = self.pool.begin().await?;

        // Looked up rather than left to `INSERT OR IGNORE` and the UNIQUE constraint.
        // SQLite treats NULLs as distinct in a UNIQUE index, and `subject_id` is NULL for
        // exactly the two subjects that reach the most people — `anyone` and
        // `authenticated` — so the constraint does not deduplicate them and the insert
        // would report a fresh grant every time. `IS` is the null-safe comparison the
        // revoke already uses.
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT id FROM acl WHERE path = ?1 AND subject_kind = ?2 \
             AND subject_id IS ?3 AND permission = ?4",
        )
        .bind(path)
        .bind(kind)
        .bind(&id)
        .bind(perm)
        .fetch_optional(&mut *tx)
        .await?;

        if existing.is_some() {
            tx.rollback().await?;
            return Ok(false);
        }

        sqlx::query(
            "INSERT INTO acl (id, path, subject_kind, subject_id, permission) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(path)
        .bind(kind)
        .bind(&id)
        .bind(perm)
        .execute(&mut *tx)
        .await?;

        Self::record_audit(
            &mut *tx,
            Some(actor),
            "acl.grant",
            Some(path),
            Some(path),
            &json!({ "subject_kind": kind, "subject_id": id, "permission": perm }),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Remove a grant defined at `path`. Returns whether a row was actually removed.
    ///
    /// `false` is the answer for an inherited grant as much as for one that was never
    /// there — the row lives on an ancestor, and nothing here can remove it.
    pub async fn remove_grant_audited(
        &self,
        actor: &str,
        path: &str,
        subject: &Subject,
        permission: Permission,
    ) -> Result<bool> {
        let (kind, id) = subject_columns(subject);
        let perm = permission_column(permission);
        let mut tx = self.pool.begin().await?;

        let removed = sqlx::query(
            "DELETE FROM acl WHERE path = ?1 AND subject_kind = ?2 \
             AND subject_id IS ?3 AND permission = ?4",
        )
        .bind(path)
        .bind(kind)
        .bind(&id)
        .bind(perm)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;

        if !removed {
            tx.rollback().await?;
            return Ok(false);
        }

        Self::record_audit(
            &mut *tx,
            Some(actor),
            "acl.revoke",
            Some(path),
            Some(path),
            &json!({ "subject_kind": kind, "subject_id": id, "permission": perm }),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::MembershipOutcome;
    use crate::Store;
    use gw_auth::{Permission, Principal, Subject};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    /// The audit log as an instance admin sees it: everything.
    async fn entries(store: &Store) -> Vec<crate::AuditEntry> {
        let admin = Principal::test("chef", &["admins"], &[]);
        store.audit_for(&admin, 100).await.unwrap().entries
    }

    #[tokio::test]
    async fn a_change_and_its_record_are_one_transaction() {
        // The property D-M2-4 asks for, checked from the outside: after the call, both
        // the change and the row exist. The rollback paths below are the other half —
        // when nothing changed, nothing is recorded either.
        let store = store().await;
        store
            .create_team_audited("chef", "redaktion", "Redaktion")
            .await
            .unwrap();

        let log = entries(&store).await;
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].action, "team.create");
        assert_eq!(log[0].target.as_deref(), Some("redaktion"));
        assert_eq!(log[0].principal_id.as_deref(), Some("chef"));
        assert_eq!(log[0].path, None, "a team belongs to no subtree");
    }

    #[tokio::test]
    async fn a_change_that_fails_leaves_no_record_of_having_happened() {
        // The other direction of the same property. The UNIQUE constraint on `username`
        // fails the insert, and everything in the transaction goes with it — including
        // the audit row, which is precisely what a second statement written after the
        // commit would have left behind.
        let store = store().await;
        store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();

        assert!(store
            .create_local_principal_audited("chef", "gast", "Gast", None, "$argon2id$x")
            .await
            .is_err());
        assert!(
            entries(&store).await.is_empty(),
            "an account that was never created was recorded as created"
        );
    }

    #[tokio::test]
    async fn a_membership_that_changes_nothing_records_nothing() {
        let store = store().await;
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store.create_team("gaeste", "Gäste").await.unwrap();

        assert_eq!(
            store
                .add_team_member_audited("chef", "tippfehler", &gast.id)
                .await
                .unwrap(),
            MembershipOutcome::NoSuchTeam
        );
        assert_eq!(
            store
                .add_team_member_audited("chef", "gaeste", "niemand")
                .await
                .unwrap(),
            MembershipOutcome::NoSuchPrincipal
        );
        assert_eq!(
            store
                .remove_team_member_audited("chef", "gaeste", &gast.id)
                .await
                .unwrap(),
            MembershipOutcome::Unchanged
        );
        assert!(
            entries(&store).await.is_empty(),
            "an action that did not happen was recorded"
        );

        assert_eq!(
            store
                .add_team_member_audited("chef", "gaeste", &gast.id)
                .await
                .unwrap(),
            MembershipOutcome::Changed
        );
        assert_eq!(
            store
                .add_team_member_audited("chef", "gaeste", &gast.id)
                .await
                .unwrap(),
            MembershipOutcome::Unchanged,
            "adding twice is idempotent"
        );
        assert_eq!(entries(&store).await.len(), 1);
    }

    #[tokio::test]
    async fn a_grant_that_changes_nothing_records_nothing() {
        let store = store().await;
        assert!(store
            .add_grant_audited("chef", "/raum", &Subject::Anyone, Permission::Read)
            .await
            .unwrap());
        assert!(
            !store
                .add_grant_audited("chef", "/raum", &Subject::Anyone, Permission::Read)
                .await
                .unwrap(),
            "the same grant twice writes one row"
        );
        assert!(
            !store
                .remove_grant_audited(
                    "chef",
                    "/raum/unterseite",
                    &Subject::Anyone,
                    Permission::Read
                )
                .await
                .unwrap(),
            "an inherited grant cannot be revoked from the child"
        );

        let log = entries(&store).await;
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!(log[0].action, "acl.grant");
        assert_eq!(
            log[0].path.as_deref(),
            Some("/raum"),
            "a grant is scoped to the subtree it concerns, or a space admin cannot see it"
        );
    }

    #[tokio::test]
    async fn deactivating_an_account_that_does_not_exist_changes_and_records_nothing() {
        let store = store().await;
        assert!(store
            .set_principal_active_audited("chef", "niemand", false)
            .await
            .unwrap()
            .is_none());
        assert!(entries(&store).await.is_empty());
    }

    #[tokio::test]
    async fn creating_an_account_records_no_credential() {
        let store = store().await;
        let created = store
            .create_local_principal_audited(
                "chef",
                "gast",
                "Gast",
                None,
                "$argon2id$v=19$m=65536,t=3,p=4$abc$def",
            )
            .await
            .unwrap();
        assert!(created.active);

        let log = entries(&store).await;
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].action, "principal.create");
        assert_eq!(log[0].target.as_deref(), Some(created.id.as_str()));
        assert!(
            !log[0].detail.contains("argon2"),
            "a password hash reached the audit log: {}",
            log[0].detail
        );

        // The credential really is there — the audit row omitting it is not the insert
        // having been skipped.
        let (_, hash) = store.principal_by_username("gast").await.unwrap().unwrap();
        assert!(hash.unwrap().starts_with("$argon2id$"));
    }

    #[tokio::test]
    async fn deactivating_through_the_audited_path_still_ends_the_sessions() {
        // The audited variant shares one implementation with the plain one, so this pins
        // that it did not quietly become an UPDATE with no session sweep (D-M2-7).
        let store = store().await;
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&gast.id, "digest", crate::SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let updated = store
            .set_principal_active_audited("chef", &gast.id, false)
            .await
            .unwrap()
            .expect("the account exists");
        assert!(!updated.active);
        assert_eq!(store.session_count_for(&gast.id).await.unwrap(), 0);
    }
}
