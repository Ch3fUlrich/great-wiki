use crate::Store;
use anyhow::Result;
use gw_auth::{Principal, PrincipalKind};
use sqlx::FromRow;

#[derive(FromRow)]
struct PrincipalRow {
    id: String,
    kind: String,
    username: String,
    display_name: String,
    email: Option<String>,
    groups: String,
    active: i64,
}

impl PrincipalRow {
    fn into_principal(self, teams: Vec<String>) -> Principal {
        Principal {
            id: self.id,
            // Fail closed on an unrecognised value: `local` is the kind with no OIDC
            // groups, so a corrupted row loses reach rather than gaining it.
            kind: if self.kind == "oidc" {
                PrincipalKind::Oidc
            } else {
                PrincipalKind::Local
            },
            username: self.username,
            display_name: self.display_name,
            email: self.email,
            groups: serde_json::from_str(&self.groups).unwrap_or_default(),
            teams,
            active: self.active != 0,
        }
    }
}

/// Write a local principal and its credential. Returns the new id.
///
/// Takes a connection rather than the pool so it can be part of a larger transaction —
/// [`crate::admin`] creates an account and its audit row together. One implementation for
/// both callers, because the credential row is the easy half to forget from a second one,
/// and an account with no credential cannot sign in at all.
pub(crate) async fn insert_local_principal(
    conn: &mut sqlx::SqliteConnection,
    username: &str,
    display_name: &str,
    email: Option<&str>,
    password_hash: &str,
) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();

    sqlx::query(
        "INSERT INTO principals (id, kind, username, display_name, email) \
         VALUES (?1, 'local', ?2, ?3, ?4)",
    )
    .bind(&id)
    .bind(username)
    .bind(display_name)
    .bind(email)
    .execute(&mut *conn)
    .await?;

    sqlx::query("INSERT INTO credentials (principal_id, password_hash) VALUES (?1, ?2)")
        .bind(&id)
        .bind(password_hash)
        .execute(&mut *conn)
        .await?;

    Ok(id)
}

/// Set the active flag, deleting every session the principal holds when deactivating
/// (D-M2-7). Returns whether a principal row was actually matched.
///
/// Same reason as [`insert_local_principal`] for taking a connection: the flag, the
/// sessions and the audit row belong to one transaction. Two statements would leave a
/// window in which the account is inactive but its cookies still resolve.
pub(crate) async fn apply_active(
    conn: &mut sqlx::SqliteConnection,
    id: &str,
    active: bool,
) -> Result<bool> {
    let result = sqlx::query("UPDATE principals SET active = ?2 WHERE id = ?1")
        .bind(id)
        .bind(i64::from(active))
        .execute(&mut *conn)
        .await?;

    if !active {
        sqlx::query("DELETE FROM sessions WHERE principal_id = ?1")
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }

    Ok(result.rows_affected() > 0)
}

/// Add or remove the per-account promotion (0006). Returns whether a row actually moved.
///
/// Takes a connection for the same reason [`apply_active`] does: the row, the check that
/// an administrator is left, and the audit entry belong to one transaction. The boolean is
/// what tells "already promoted" (nothing to do) apart from "promoted just now", so that
/// only the second is recorded.
pub(crate) async fn apply_instance_admin(
    conn: &mut sqlx::SqliteConnection,
    id: &str,
    admin: bool,
    granted_by: &str,
) -> Result<bool> {
    let result = if admin {
        sqlx::query(
            "INSERT OR IGNORE INTO instance_admins (principal_id, granted_by) VALUES (?1, ?2)",
        )
        .bind(id)
        .bind(granted_by)
        .execute(&mut *conn)
        .await?
    } else {
        sqlx::query("DELETE FROM instance_admins WHERE principal_id = ?1")
            .bind(id)
            .execute(&mut *conn)
            .await?
    };
    Ok(result.rows_affected() > 0)
}

/// Somebody who could be promoted to administer the instance.
///
/// Carries where the account comes from and what it already holds, because the person
/// choosing a successor is choosing under time pressure — usually because they are about
/// to lose their own access — and a list of bare usernames is how the wrong `m.schmidt`
/// gets the instance.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminCandidate {
    pub id: String,
    pub username: String,
    pub display_name: String,
    /// `oidc` for an Authelia account, `local` for a great-wiki guest account.
    pub kind: PrincipalKind,
    /// The verified Authelia groups, so `admins` next to a name is visible.
    pub groups: Vec<String>,
    /// When this account was last seen doing something, or `None` if never — see
    /// [`Store::admin_candidates`] for what that is derived from.
    pub last_active_at: Option<String>,
}

impl Store {
    async fn teams_of(&self, principal_id: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT t.slug FROM teams t
             JOIN team_members m ON m.team_id = t.id
             WHERE m.principal_id = ?1 ORDER BY t.slug",
        )
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    /// Create on first login, refresh on every subsequent one.
    ///
    /// Groups are REPLACED, not merged: they mirror the verified `groups` claim, so
    /// losing a group in Authelia must take effect here at the next login. Merging would
    /// make removal impossible.
    pub async fn upsert_oidc_principal(
        &self,
        username: &str,
        display_name: &str,
        email: Option<&str>,
        groups: &[String],
    ) -> Result<Principal> {
        let groups_json = serde_json::to_string(groups)?;
        let id = uuid::Uuid::now_v7().to_string();

        sqlx::query(
            r#"
            INSERT INTO principals (id, kind, username, display_name, email, groups, last_seen_at)
            VALUES (?1, 'oidc', ?2, ?3, ?4, ?5, datetime('now'))
            ON CONFLICT (username) DO UPDATE SET
                display_name = excluded.display_name,
                email        = excluded.email,
                groups       = excluded.groups,
                last_seen_at = datetime('now')
            "#,
        )
        .bind(&id)
        .bind(username)
        .bind(display_name)
        .bind(email)
        .bind(&groups_json)
        .execute(&self.pool)
        .await?;

        self.principal_by_username(username)
            .await?
            .map(|(p, _)| p)
            .ok_or_else(|| anyhow::anyhow!("principal vanished immediately after upsert"))
    }

    pub async fn create_local_principal(
        &self,
        username: &str,
        display_name: &str,
        email: Option<&str>,
        password_hash: &str,
    ) -> Result<Principal> {
        let mut tx = self.pool.begin().await?;
        insert_local_principal(&mut tx, username, display_name, email, password_hash).await?;
        tx.commit().await?;

        self.principal_by_username(username)
            .await?
            .map(|(p, _)| p)
            .ok_or_else(|| anyhow::anyhow!("principal vanished immediately after insert"))
    }

    /// Returns the principal and, for local accounts, its password hash.
    pub async fn principal_by_username(
        &self,
        username: &str,
    ) -> Result<Option<(Principal, Option<String>)>> {
        let row: Option<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, display_name, email, groups, active \
             FROM principals WHERE username = ?1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        self.hydrate(row).await
    }

    /// The same, by primary key. What a session resolves through: a session names an id,
    /// and the principal behind it is read fresh on every request (D-M2-7).
    ///
    /// Keyed by id rather than by username on purpose — a username is a display handle and
    /// could in principle be changed, whereas the id is what every grant and membership
    /// already points at.
    pub async fn principal_by_id(&self, id: &str) -> Result<Option<(Principal, Option<String>)>> {
        let row: Option<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, display_name, email, groups, active \
             FROM principals WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        self.hydrate(row).await
    }

    /// Attach the credential and the team memberships a bare row does not carry.
    ///
    /// One implementation for both lookups, so `principal_by_id` cannot drift into
    /// returning a principal with no teams while `principal_by_username` returns one with
    /// them — a difference that would show up as a permission bug, not as a data bug.
    async fn hydrate(
        &self,
        row: Option<PrincipalRow>,
    ) -> Result<Option<(Principal, Option<String>)>> {
        let Some(row) = row else { return Ok(None) };
        let hash: Option<(String,)> =
            sqlx::query_as("SELECT password_hash FROM credentials WHERE principal_id = ?1")
                .bind(&row.id)
                .fetch_optional(&self.pool)
                .await?;
        let teams = self.teams_of(&row.id).await?;
        Ok(Some((row.into_principal(teams), hash.map(|(h,)| h))))
    }

    /// Activate or deactivate an account.
    ///
    /// Deactivating additionally **deletes every session that principal holds**, in the
    /// SAME transaction as the flag (D-M2-7). Two statements would leave a window in which
    /// the account is inactive but its cookies still resolve; one transaction leaves none.
    ///
    /// Reactivating deliberately does not restore anything — sessions are not recoverable,
    /// and the person signs in again. Restoring them would mean keeping deleted rows,
    /// which is the opposite of what "invalidate everywhere" promises.
    pub async fn set_principal_active(&self, id: &str, active: bool) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        apply_active(&mut tx, id, active).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Promote or demote an account, with no actor and no record.
    ///
    /// The plain sibling of [`Store::set_instance_admin_audited`], for seeding and for
    /// tests. It does NOT enforce the floor of one administrator — the audited variant
    /// does, in the transaction that also writes the audit row, which is the only place
    /// the count can be trusted. Nothing reachable from the API may call this.
    pub async fn set_instance_admin(&self, id: &str, admin: bool) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let changed = apply_instance_admin(&mut tx, id, admin, "system").await?;
        tx.commit().await?;
        Ok(changed)
    }

    /// Who could be promoted to administer the instance, most recently active first.
    ///
    /// **Activity is derived, because nothing records it directly.** `principals` does
    /// carry a `last_seen_at`, but only [`Store::upsert_oidc_principal`] ever writes it —
    /// it is NULL for every local guest account, which is precisely the population this
    /// list exists to choose from. So the answer here is the more recent of:
    ///
    ///   * the newest session the account holds (`sessions.created_at`), which is the last
    ///     time it signed in and still has a live session; and
    ///   * the newest audit entry the account is the ACTOR of (`audit_log.at`), which is
    ///     the last time it did something administrative.
    ///
    /// Both are approximations and the comment is here so nobody mistakes them for more:
    /// signing out or being deactivated deletes the sessions, and expired ones are swept,
    /// so an account that was busy last year and has since signed out reads as never seen.
    /// It is enough for the job — sorting the plausible successors above the dormant ones
    /// — and it invents nothing.
    ///
    /// The list excludes anyone who ALREADY holds the admin baseline, by promotion or by
    /// Authelia group: a candidate is somebody whose promotion would add an administrator,
    /// and offering the ones who would add none is how a handover ends with nobody in
    /// charge. Deactivated accounts are excluded for the same reason.
    pub async fn admin_candidates(&self) -> Result<Vec<AdminCandidate>> {
        // The empty string is what "never" sorts as, and it sorts BELOW every timestamp
        // in the `datetime('now')` format, so accounts with no evidence land at the
        // bottom rather than at the top. Ties break on username so the order is stable.
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT p.id, p.kind, p.username, p.display_name, p.groups,
                    MAX(
                      COALESCE((SELECT MAX(s.created_at) FROM sessions s
                                WHERE s.principal_id = p.id), ''),
                      COALESCE((SELECT MAX(a.at) FROM audit_log a
                                WHERE a.principal_id = p.id), '')
                    ) AS last_active_at
             FROM principals p
             WHERE p.active = 1
             ORDER BY last_active_at DESC, p.username ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut conn = self.pool.acquire().await?;
        let mut out = Vec::new();
        for (id, kind, username, display_name, groups, last_active_at) in rows {
            let groups: Vec<String> = serde_json::from_str(&groups).unwrap_or_default();
            if crate::acl::baseline_on(&mut conn, &id, &groups).await? >= crate::Baseline::Admin {
                continue;
            }
            out.push(AdminCandidate {
                id,
                username,
                display_name,
                // Fail closed on an unrecognised value, exactly as `PrincipalRow` does.
                kind: if kind == "oidc" {
                    PrincipalKind::Oidc
                } else {
                    PrincipalKind::Local
                },
                groups,
                last_active_at: (!last_active_at.is_empty()).then_some(last_active_at),
            });
        }
        Ok(out)
    }

    pub async fn create_team(&self, slug: &str, name: &str) -> Result<String> {
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO teams (id, slug, name) VALUES (?1, ?2, ?3)")
            .bind(&id)
            .bind(slug)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    /// Put somebody in a team. Returns whether a row was actually written.
    ///
    /// The boolean matters because of how this is expressed: the insert selects the team
    /// id by slug, so a slug that names no team inserts NO ROWS and reports success. An
    /// administrator would see "added to team" and the person would have gained nothing —
    /// the failure mode being that a typo in a team name silently withholds access
    /// instead of announcing itself.
    pub async fn add_team_member(&self, team_slug: &str, principal_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO team_members (team_id, principal_id) \
             SELECT id, ?2 FROM teams WHERE slug = ?1",
        )
        .bind(team_slug)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// A team and who is in it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TeamSummary {
    pub slug: String,
    pub name: String,
    /// Principal ids. Resolving them to names is the caller's job, so this stays one
    /// query regardless of how many teams there are.
    pub members: Vec<String>,
}

impl Store {
    /// Everyone great-wiki knows about, active and deactivated alike.
    ///
    /// Deactivated accounts are included deliberately: an administrator needs to see that
    /// an account exists and is suspended. Hiding it would make "this person cannot sign
    /// in" indistinguishable from "this person was never here", and the second invites
    /// creating a duplicate.
    pub async fn list_principals(&self) -> Result<Vec<Principal>> {
        let rows: Vec<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, display_name, email, groups, active \
             FROM principals ORDER BY username",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let teams = self.teams_of(&row.id).await?;
            out.push(row.into_principal(teams));
        }
        Ok(out)
    }

    pub async fn list_teams(&self) -> Result<Vec<TeamSummary>> {
        let teams: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, slug, name FROM teams ORDER BY slug")
                .fetch_all(&self.pool)
                .await?;

        let mut out = Vec::with_capacity(teams.len());
        for (id, slug, name) in teams {
            let members: Vec<(String,)> =
                sqlx::query_as("SELECT principal_id FROM team_members WHERE team_id = ?1")
                    .bind(&id)
                    .fetch_all(&self.pool)
                    .await?;
            out.push(TeamSummary {
                slug,
                name,
                members: members.into_iter().map(|(m,)| m).collect(),
            });
        }
        Ok(out)
    }

    /// Remove somebody from a team. Returns whether a row was actually removed.
    ///
    /// Same reason [`Store::remove_grant`] returns a boolean: a removal that matched
    /// nothing must not report success to an administrator who is about to conclude that
    /// access has been withdrawn.
    pub async fn remove_team_member(&self, team_slug: &str, principal_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM team_members WHERE principal_id = ?2 \
             AND team_id = (SELECT id FROM teams WHERE slug = ?1)",
        )
        .bind(team_slug)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod admin_tests {
    use crate::Store;
    use gw_auth::{Permission, Subject};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn adding_to_a_team_that_does_not_exist_reports_failure() {
        // The insert selects the team id by slug, so a slug naming no team writes no
        // rows. Before this returned a boolean it reported success, and a typo in a team
        // name silently withheld access instead of announcing itself.
        let store = store().await;
        let p = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap()
            .id;

        assert!(
            !store.add_team_member("tippfehler", &p).await.unwrap(),
            "a nonexistent team reported a successful add"
        );

        store.create_team("redaktion", "Redaktion").await.unwrap();
        assert!(store.add_team_member("redaktion", &p).await.unwrap());
    }

    #[tokio::test]
    async fn removing_a_membership_that_is_not_there_reports_failure() {
        let store = store().await;
        let p = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap()
            .id;
        store.create_team("redaktion", "Redaktion").await.unwrap();

        assert!(!store.remove_team_member("redaktion", &p).await.unwrap());
        store.add_team_member("redaktion", &p).await.unwrap();
        assert!(store.remove_team_member("redaktion", &p).await.unwrap());
        assert!(!store.remove_team_member("redaktion", &p).await.unwrap());
    }

    #[tokio::test]
    async fn revoking_an_inherited_grant_from_the_child_reports_failure() {
        // The grant lives on the ancestor. A console offering a revoke control here would
        // be claiming an effect it cannot have — the boolean is what lets it refuse.
        let store = store().await;
        store
            .add_grant("/raum", Subject::Anyone, Permission::Read)
            .await
            .unwrap();

        assert!(
            !store
                .remove_grant("/raum/unterseite", &Subject::Anyone, Permission::Read)
                .await
                .unwrap(),
            "revoking an inherited grant from the child claimed success"
        );
        assert!(store
            .remove_grant("/raum", &Subject::Anyone, Permission::Read)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn effective_grants_name_the_ancestor_they_come_from() {
        let store = store().await;
        store
            .add_grant("/raum", Subject::Anyone, Permission::Read)
            .await
            .unwrap();

        let (source, grants) = store.effective_grants("/raum/tief/tiefer").await.unwrap();
        assert_eq!(source.as_deref(), Some("/raum"));
        assert_eq!(grants.len(), 1);

        // Nothing anywhere above: reach is whatever the baseline confers, and the console
        // must say so rather than implying an empty grant list was configured here.
        let (none, empty) = store.effective_grants("/woanders").await.unwrap();
        assert_eq!(none, None);
        assert!(empty.is_empty());
    }

    /// Rewrite a stored timestamp.
    ///
    /// `datetime('now')` has one-second resolution, so a test that created two sessions in
    /// a row would compare two identical timestamps and pass whatever the ORDER BY said.
    /// Backdating is the only way to assert an ORDER without sleeping through a real
    /// second, and it is available here because the pool is crate-private — which is also
    /// why this test lives in the store rather than against the HTTP API.
    async fn backdate(store: &Store, table: &str, column: &str, id: &str, at: &str) {
        let affected = sqlx::query(&format!(
            "UPDATE {table} SET {column} = ?2 WHERE principal_id = ?1"
        ))
        .bind(id)
        .bind(at)
        .execute(&store.pool)
        .await
        .unwrap()
        .rows_affected();
        assert!(affected > 0, "nothing to backdate in {table}");
    }

    #[tokio::test]
    async fn candidates_come_back_most_recently_active_first() {
        // Activity is derived from two sources and neither is authoritative on its own, so
        // this pins that BOTH are consulted and that the more recent of them wins.
        let store = store().await;
        let mut ids = std::collections::HashMap::new();
        for username in ["sitzung-neu", "sitzung-alt", "protokoll", "nie-gesehen"] {
            let id = store
                .create_local_principal(username, username, None, "x")
                .await
                .unwrap()
                .id;
            ids.insert(username, id);
        }

        // Two accounts whose only evidence is a session, one recent and one ancient.
        for (username, at) in [
            ("sitzung-neu", "2026-08-01 09:00:00"),
            ("sitzung-alt", "2019-01-01 09:00:00"),
        ] {
            store
                .create_session(&ids[username], username, crate::SESSION_TTL_SECONDS)
                .await
                .unwrap();
            backdate(&store, "sessions", "created_at", &ids[username], at).await;
        }

        // One whose only evidence is something they did, dated between the two — so an
        // implementation that consulted sessions alone would sort it last, and one that
        // consulted the audit log alone would lose the other two.
        store
            .record_audit_entry(
                Some(&ids["protokoll"]),
                "acl.grant",
                Some("/raum"),
                Some("/raum"),
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        backdate(
            &store,
            "audit_log",
            "at",
            &ids["protokoll"],
            "2023-05-05 09:00:00",
        )
        .await;

        let names: Vec<String> = store
            .admin_candidates()
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.username)
            .collect();
        assert_eq!(
            names,
            vec!["sitzung-neu", "protokoll", "sitzung-alt", "nie-gesehen"],
            "candidates are not ordered by the most recent of session and audit activity"
        );
    }

    #[tokio::test]
    async fn the_more_recent_of_a_session_and_an_audit_entry_wins() {
        // The MAX across the two sources, rather than a preference for one of them.
        let store = store().await;
        let alt = store
            .create_local_principal("alt", "Alt", None, "x")
            .await
            .unwrap()
            .id;
        let beides = store
            .create_local_principal("beides", "Beides", None, "x")
            .await
            .unwrap()
            .id;

        // `beides` has an ancient session and a recent audit entry; `alt` has a session
        // in between. Taking the session alone would put `alt` first.
        store
            .create_session(&beides, "beides", crate::SESSION_TTL_SECONDS)
            .await
            .unwrap();
        backdate(
            &store,
            "sessions",
            "created_at",
            &beides,
            "2019-01-01 09:00:00",
        )
        .await;
        store
            .record_audit_entry(
                Some(&beides),
                "team.create",
                None,
                None,
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        backdate(&store, "audit_log", "at", &beides, "2026-08-01 09:00:00").await;

        store
            .create_session(&alt, "alt", crate::SESSION_TTL_SECONDS)
            .await
            .unwrap();
        backdate(
            &store,
            "sessions",
            "created_at",
            &alt,
            "2022-01-01 09:00:00",
        )
        .await;

        let candidates = store.admin_candidates().await.unwrap();
        let names: Vec<&str> = candidates.iter().map(|c| c.username.as_str()).collect();
        assert_eq!(names, vec!["beides", "alt"]);
        assert_eq!(
            candidates[0].last_active_at.as_deref(),
            Some("2026-08-01 09:00:00")
        );
    }

    #[tokio::test]
    async fn candidates_exclude_administrators_and_deactivated_accounts() {
        // A candidate is somebody whose promotion would ADD an administrator. Offering the
        // ones who would add none is how a handover ends with nobody in charge.
        let store = store().await;
        let by_group = store
            .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
            .await
            .unwrap();
        let promoted = store
            .create_local_principal("befoerdert", "Befördert", None, "x")
            .await
            .unwrap();
        let gesperrt = store
            .create_local_principal("gesperrt", "Gesperrt", None, "x")
            .await
            .unwrap();
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        let kollege = store
            .upsert_oidc_principal("kollege", "Kollege", None, &["users".into()])
            .await
            .unwrap();

        store.set_instance_admin(&promoted.id, true).await.unwrap();
        store
            .set_principal_active(&gesperrt.id, false)
            .await
            .unwrap();

        let candidates = store.admin_candidates().await.unwrap();
        let names: Vec<&str> = candidates.iter().map(|c| c.username.as_str()).collect();
        assert_eq!(names, vec!["gast", "kollege"], "{candidates:?}");
        assert!(!names.contains(&by_group.username.as_str()));

        // The console needs the source and the groups to tell two people apart.
        let kollege_row = candidates
            .iter()
            .find(|c| c.id == kollege.id)
            .expect("listed");
        assert_eq!(kollege_row.kind, gw_auth::PrincipalKind::Oidc);
        assert_eq!(kollege_row.groups, vec!["users".to_string()]);
        assert_eq!(kollege_row.display_name, "Kollege");
        let gast_row = candidates.iter().find(|c| c.id == gast.id).expect("listed");
        assert_eq!(gast_row.kind, gw_auth::PrincipalKind::Local);
        assert!(gast_row.groups.is_empty());
        assert_eq!(
            gast_row.last_active_at, None,
            "an account with no session and no audit entry has no derived activity"
        );
    }

    #[tokio::test]
    async fn deactivated_accounts_are_still_listed() {
        // Hiding them would make "cannot sign in" indistinguishable from "never existed",
        // and the second invites creating a duplicate account.
        let store = store().await;
        let p = store
            .create_local_principal("gesperrt", "Gesperrt", None, "x")
            .await
            .unwrap()
            .id;
        store.set_principal_active(&p, false).await.unwrap();

        let all = store.list_principals().await.unwrap();
        let found = all.iter().find(|x| x.id == p).expect("not listed");
        assert!(!found.active);
    }
}
