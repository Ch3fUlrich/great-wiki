//! Session storage.
//!
//! The store never sees a session token. It is given the token's SHA-256 and stores that,
//! and every lookup hashes the presented token the same way — so this module's whole
//! surface is expressed in hashes, and there is no method that could accidentally persist
//! the plaintext. Hashing itself belongs to the caller, next to the code that generates
//! the token; see `gw_api::auth::session`.

use crate::Store;
use anyhow::Result;
use gw_auth::Principal;

/// How long a new session lives. Long enough that signing in is not a daily chore, short
/// enough that an abandoned browser is not a standing key.
pub const SESSION_TTL_SECONDS: i64 = 60 * 60 * 24 * 14;

impl Store {
    /// Record a session for `principal_id`, expiring `ttl_seconds` from now.
    ///
    /// `token_hash` is the SHA-256 of the token the browser will hold. Passing the token
    /// itself would store a working credential, which is the one thing this table exists
    /// to avoid.
    ///
    /// A negative `ttl_seconds` produces an already-expired row. That is not a quirk to
    /// work around — it is how the expiry test builds its fixture without sleeping.
    pub async fn create_session(
        &self,
        principal_id: &str,
        token_hash: &str,
        ttl_seconds: i64,
    ) -> Result<()> {
        // Cheap opportunistic housekeeping, on the one path that is already rare and
        // already writing: without it the table only ever grows. Indexed on expires_at.
        self.delete_expired_sessions().await?;

        sqlx::query(
            "INSERT INTO sessions (token_hash, principal_id, expires_at) \
             VALUES (?1, ?2, datetime('now', ?3))",
        )
        .bind(token_hash)
        .bind(principal_id)
        .bind(format!("{ttl_seconds} seconds"))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The principal a session token belongs to, or `None` if the session is unknown,
    /// expired, or points at a principal that no longer exists.
    ///
    /// The principal is RE-READ from `principals` on every call rather than being captured
    /// at sign-in (D-M2-7). That is the whole point: a grant removed, a team changed or an
    /// account deactivated takes effect on the caller's next click, not at their next
    /// sign-in. It costs one indexed lookup per request.
    ///
    /// Expiry is enforced here, in the `WHERE` clause, rather than by a sweeper. A row
    /// that has outlived `expires_at` resolves to nobody the instant it does, whether or
    /// not anything has got round to deleting it.
    pub async fn principal_for_session(&self, token_hash: &str) -> Result<Option<Principal>> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT principal_id FROM sessions \
             WHERE token_hash = ?1 AND expires_at > datetime('now')",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        let Some((principal_id,)) = row else {
            return Ok(None);
        };
        Ok(self.principal_by_id(&principal_id).await?.map(|(p, _)| p))
    }

    /// End one session. Returns whether a row was actually removed.
    ///
    /// Signing out must delete the server-side record, not merely clear the cookie: a
    /// cookie that has already been copied is not recalled by asking the browser to
    /// forget it.
    pub async fn delete_session(&self, token_hash: &str) -> Result<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE token_hash = ?1")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    /// End every session a principal holds, everywhere.
    pub async fn delete_sessions_for_principal(&self, principal_id: &str) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE principal_id = ?1")
            .bind(principal_id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Remove rows that have already stopped resolving. Hygiene, never a security
    /// boundary — `principal_for_session` refuses an expired row whether this has run
    /// or not.
    pub async fn delete_expired_sessions(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM sessions WHERE expires_at <= datetime('now')")
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// How many session rows a principal has. For the admin console and for tests that
    /// need to assert a session was *deleted* rather than merely stopped working.
    pub async fn session_count_for(&self, principal_id: &str) -> Result<i64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE principal_id = ?1")
                .bind(principal_id)
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }
}
