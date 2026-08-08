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
        let id = uuid::Uuid::now_v7().to_string();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO principals (id, kind, username, display_name, email) \
             VALUES (?1, 'local', ?2, ?3, ?4)",
        )
        .bind(&id)
        .bind(username)
        .bind(display_name)
        .bind(email)
        .execute(&mut *tx)
        .await?;

        sqlx::query("INSERT INTO credentials (principal_id, password_hash) VALUES (?1, ?2)")
            .bind(&id)
            .bind(password_hash)
            .execute(&mut *tx)
            .await?;

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

        sqlx::query("UPDATE principals SET active = ?2 WHERE id = ?1")
            .bind(id)
            .bind(i64::from(active))
            .execute(&mut *tx)
            .await?;

        if !active {
            sqlx::query("DELETE FROM sessions WHERE principal_id = ?1")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }

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

    pub async fn add_team_member(&self, team_slug: &str, principal_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO team_members (team_id, principal_id) \
             SELECT id, ?2 FROM teams WHERE slug = ?1",
        )
        .bind(team_slug)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
