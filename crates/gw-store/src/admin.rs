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

use crate::acl::{active_instance_admins, permission_column, subject_columns};
use crate::principals::{apply_active, apply_instance_admin, insert_local_principal};
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

/// What a change to an account's active flag actually did.
///
/// Boxed principal because the other two variants carry nothing and this one is the whole
/// account.
#[derive(Debug, Clone)]
pub enum ActiveOutcome {
    /// The flag is now what the caller asked for.
    Changed(Box<Principal>),
    /// The id names nobody.
    NoSuchPrincipal,
    /// Refused: committing would have left the instance with no active administrator.
    LastAdmin,
}

/// What a change to the per-account promotion actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceAdminOutcome {
    /// A row was written or removed.
    Changed,
    /// The promotion already matched the request. Nothing written, nothing recorded.
    ///
    /// For a demotion this is NOT "administration withdrawn": the person may administer
    /// the instance through their Authelia group, which no row here can take away. The
    /// caller has to say so rather than report a revocation that did not happen.
    Unchanged,
    /// The id names nobody.
    NoSuchPrincipal,
    /// Refused: committing would have left the instance with no active administrator.
    LastAdmin,
}

/// Refuse a transaction that would leave nobody able to administer the instance.
///
/// **Counted after the change and inside the same transaction**, so what it asserts is the
/// state this commit would actually leave behind — not a number read beforehand, which two
/// concurrent requests can both read as two and both act on, ending at zero. Both callers
/// have already WRITTEN by the time they ask, so the transaction holds SQLite's write lock
/// and the second request cannot get past its own write until the first has committed or
/// rolled back; it then counts against the committed result.
///
/// It is the resulting count that decides, never who the caller is. "You may not
/// deactivate yourself" would be a different rule, and a weaker one: it lets two
/// administrators deactivate each other, and it refuses a self-deactivation that is
/// perfectly safe because somebody else is still there.
async fn leaves_an_administrator(tx: &mut sqlx::SqliteConnection) -> Result<bool> {
    Ok(active_instance_admins(tx).await? > 0)
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

    /// Activate or deactivate an account.
    ///
    /// Deactivating deletes every session that principal holds, in this same transaction
    /// (D-M2-7) — so the flag, the sessions and the record of it either all happen or
    /// none of them do.
    ///
    /// It is also where the safety interlock lives: a deactivation that would leave the
    /// instance with NO active administrator is refused and rolled back, whoever asked and
    /// whoever the target is. Without it, the last administrator can suspend their own
    /// account and lock everybody out of administration permanently — great-wiki cannot
    /// promote anybody in Authelia (ADR 0002), so there is no way back from inside.
    ///
    /// The rollback matters as much as the refusal: the session sweep happens before the
    /// count, so a refusal that only undid the flag would still have signed the last
    /// administrator out of the console they were just told they could keep.
    pub async fn set_principal_active_audited(
        &self,
        actor: &str,
        id: &str,
        active: bool,
    ) -> Result<ActiveOutcome> {
        let mut tx = self.pool.begin().await?;
        if !apply_active(&mut tx, id, active).await? {
            // Nothing matched: roll back rather than recording an action that did not
            // happen.
            tx.rollback().await?;
            return Ok(ActiveOutcome::NoSuchPrincipal);
        }
        // Only a deactivation can reduce the count, so only a deactivation is asked.
        if !active && !leaves_an_administrator(&mut tx).await? {
            tx.rollback().await?;
            return Ok(ActiveOutcome::LastAdmin);
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

        match self.principal_by_id(id).await? {
            Some((principal, _)) => Ok(ActiveOutcome::Changed(Box::new(principal))),
            // The row was there a moment ago inside the transaction, so this is a
            // concurrent deletion rather than a bad id. Reported as "nobody" either way.
            None => Ok(ActiveOutcome::NoSuchPrincipal),
        }
    }

    /// Promote or demote an account as an instance administrator (0006).
    ///
    /// The FALLBACK for an instance whose Authelia administrators are gone. It promotes
    /// exactly the principal named and has no effect on anybody else — in particular it is
    /// not a group mapping, so nobody sharing a group or a team with the promoted account
    /// gains anything.
    ///
    /// A demotion goes through the same floor check as a deactivation, in the same
    /// transaction, for the same reason: it is the other way to reach zero administrators.
    pub async fn set_instance_admin_audited(
        &self,
        actor: &str,
        id: &str,
        admin: bool,
    ) -> Result<InstanceAdminOutcome> {
        let mut tx = self.pool.begin().await?;

        // Looked up explicitly, so promoting an id that names nobody is a typo the caller
        // hears about rather than a row conferring administration on nothing. The foreign
        // key would refuse the insert but cannot be told apart from any other failure, and
        // the DELETE would report a clean no-op.
        if !principal_exists(&mut tx, id).await? {
            tx.rollback().await?;
            return Ok(InstanceAdminOutcome::NoSuchPrincipal);
        }

        if !apply_instance_admin(&mut tx, id, admin, actor).await? {
            tx.rollback().await?;
            return Ok(InstanceAdminOutcome::Unchanged);
        }

        // Only a demotion can reduce the count. A promotion that changed nothing never
        // reaches here, so a no-op can never be refused by the floor.
        if !admin && !leaves_an_administrator(&mut tx).await? {
            tx.rollback().await?;
            return Ok(InstanceAdminOutcome::LastAdmin);
        }

        Self::record_audit(
            &mut *tx,
            Some(actor),
            if admin {
                "principal.promote"
            } else {
                "principal.demote"
            },
            Some(id),
            // Instance-wide: administering the instance belongs to no subtree, so only
            // instance admins read it (0004).
            None,
            &json!({ "instance_admin": admin }),
        )
        .await?;
        tx.commit().await?;
        Ok(InstanceAdminOutcome::Changed)
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
    use super::{ActiveOutcome, InstanceAdminOutcome, MembershipOutcome};
    use crate::Store;
    use gw_auth::{Permission, Principal, Subject};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    /// An Authelia account in `admins`, which 0002 maps to the admin baseline. Returns
    /// its id.
    async fn admin(store: &Store, username: &str) -> String {
        store
            .upsert_oidc_principal(username, username, None, &["admins".into()])
            .await
            .unwrap()
            .id
    }

    /// The principal from an outcome that must have been a change.
    fn changed(outcome: ActiveOutcome) -> Principal {
        match outcome {
            ActiveOutcome::Changed(principal) => *principal,
            other => panic!("expected the change to be applied, got {other:?}"),
        }
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
        assert!(matches!(
            store
                .set_principal_active_audited("chef", "niemand", false)
                .await
                .unwrap(),
            ActiveOutcome::NoSuchPrincipal
        ));
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
        // Somebody has to be left administering the instance, or the interlock refuses
        // this — and rightly: a store with no administrator at all is the state the
        // interlock exists to prevent, not one to deactivate accounts from.
        admin(&store, "chef").await;
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&gast.id, "digest", crate::SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let updated = changed(
            store
                .set_principal_active_audited("chef", &gast.id, false)
                .await
                .unwrap(),
        );
        assert!(!updated.active);
        assert_eq!(store.session_count_for(&gast.id).await.unwrap(), 0);
    }

    // --- the safety interlock: the instance always keeps one administrator -------------

    #[tokio::test]
    async fn deactivating_the_last_administrator_is_refused_whoever_asks() {
        // Both directions of the rule, because it is a rule about the RESULT and not
        // about the caller. The API cannot reach the second case — a caller who is not an
        // administrator never gets this far, and one who is would still be counted — but
        // the store must not be the thing that relies on that.
        let store = store().await;
        let chef = admin(&store, "chef").await;

        assert!(
            matches!(
                store
                    .set_principal_active_audited(&chef, &chef, false)
                    .await
                    .unwrap(),
                ActiveOutcome::LastAdmin
            ),
            "the last administrator deactivated themselves"
        );
        assert!(
            matches!(
                store
                    .set_principal_active_audited("jemand-anders", &chef, false)
                    .await
                    .unwrap(),
                ActiveOutcome::LastAdmin
            ),
            "somebody else deactivated the last administrator"
        );

        let (loaded, _) = store.principal_by_id(&chef).await.unwrap().unwrap();
        assert!(loaded.active, "a refused deactivation took effect anyway");
        assert!(
            entries(&store).await.is_empty(),
            "a deactivation that did not happen was recorded"
        );
    }

    #[tokio::test]
    async fn an_administrator_is_deactivated_while_a_second_one_is_active() {
        let store = store().await;
        let chef = admin(&store, "chef").await;
        let zweite = admin(&store, "zweite").await;

        let updated = changed(
            store
                .set_principal_active_audited(&zweite, &chef, false)
                .await
                .unwrap(),
        );
        assert!(!updated.active);

        // And now the second one is the last, so the same call is refused.
        assert!(matches!(
            store
                .set_principal_active_audited(&zweite, &zweite, false)
                .await
                .unwrap(),
            ActiveOutcome::LastAdmin
        ));
    }

    #[tokio::test]
    async fn a_deactivated_administrator_does_not_hold_the_floor_open() {
        // The floor counts administrators who can still sign in. A suspended account
        // cannot, and D-M2-7 has already deleted its sessions.
        let store = store().await;
        let chef = admin(&store, "chef").await;
        let zweite = admin(&store, "zweite").await;

        changed(
            store
                .set_principal_active_audited(&chef, &zweite, false)
                .await
                .unwrap(),
        );
        assert!(
            matches!(
                store
                    .set_principal_active_audited(&chef, &chef, false)
                    .await
                    .unwrap(),
                ActiveOutcome::LastAdmin
            ),
            "a deactivated administrator was counted as one"
        );
    }

    #[tokio::test]
    async fn demoting_the_last_administrator_is_refused_by_the_same_rule() {
        let store = store().await;
        let chef = store
            .create_local_principal("chef", "Chef", None, "x")
            .await
            .unwrap()
            .id;
        assert_eq!(
            store
                .set_instance_admin_audited("system", &chef, true)
                .await
                .unwrap(),
            InstanceAdminOutcome::Changed
        );

        assert_eq!(
            store
                .set_instance_admin_audited(&chef, &chef, false)
                .await
                .unwrap(),
            InstanceAdminOutcome::LastAdmin
        );
        assert!(
            store.is_instance_admin(&chef).await.unwrap(),
            "a refused demotion took effect anyway"
        );
        assert_eq!(
            entries(&store).await.len(),
            1,
            "a demotion that did not happen was recorded"
        );
    }

    #[tokio::test]
    async fn a_promotion_that_changes_nothing_records_nothing() {
        let store = store().await;
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap()
            .id;

        assert_eq!(
            store
                .set_instance_admin_audited("chef", "niemand", true)
                .await
                .unwrap(),
            InstanceAdminOutcome::NoSuchPrincipal
        );
        assert_eq!(
            store
                .set_instance_admin_audited("chef", &gast, false)
                .await
                .unwrap(),
            InstanceAdminOutcome::Unchanged,
            "demoting somebody who was never promoted removes nothing"
        );
        assert!(entries(&store).await.is_empty());

        assert_eq!(
            store
                .set_instance_admin_audited("chef", &gast, true)
                .await
                .unwrap(),
            InstanceAdminOutcome::Changed
        );
        assert_eq!(
            store
                .set_instance_admin_audited("chef", &gast, true)
                .await
                .unwrap(),
            InstanceAdminOutcome::Unchanged,
            "promoting twice writes one row"
        );

        let log = entries(&store).await;
        assert_eq!(log.len(), 1, "{log:?}");
        assert_eq!(log[0].action, "principal.promote");
        assert_eq!(log[0].target.as_deref(), Some(gast.as_str()));
        assert_eq!(
            log[0].path, None,
            "administering the instance belongs to no subtree"
        );
    }
}
