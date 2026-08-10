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
