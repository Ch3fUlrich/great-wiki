//! Failed sign-in throttling, for the publicly reachable guest password form.
//!
//! Two counters per attempt — one for the submitted username, one for the client address —
//! and either one reaching the limit refuses the next try. See `0005_login_attempts.sql`
//! for why both exist and why neither subsumes the other.
//!
//! Nothing here knows anything about OpenID Connect, and that is deliberate: the Authelia
//! path never consults these counters, so somebody guessing at guest passwords cannot make
//! a homelab sign-in stop working.

use crate::Store;
use anyhow::Result;

/// Failures before a lockout. The owner's figure: high enough that a person who has
/// genuinely forgotten which password they used is not locked out mid-thought, low enough
/// that ten guesses per five minutes is not a rate anybody can search a keyspace at.
pub const LOGIN_FAILURE_LIMIT: i64 = 10;

/// How long a lockout lasts. Long enough to make guessing pointless, short enough that a
/// legitimate person waits rather than needing an administrator.
pub const LOGIN_LOCKOUT_SECONDS: i64 = 5 * 60;

/// Rows older than this are swept opportunistically. Never a security boundary — the
/// lockout comparison is what refuses an attempt, and it does so whether or not anything
/// has been swept.
const STALE_AFTER: &str = "-1 day";

/// Which of the two independent counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginScope {
    /// The submitted username, whether or not an account by that name exists.
    Account,
    /// The client address, as established by `gw_api::auth::client_address`.
    Address,
}

impl LoginScope {
    fn as_str(self) -> &'static str {
        match self {
            LoginScope::Account => "account",
            LoginScope::Address => "address",
        }
    }
}

impl Store {
    /// Whether this attempt must be refused before a password is even looked at.
    ///
    /// Checked BEFORE verification, not after: argon2id at Authelia's parameters costs
    /// 64 MiB and tens of milliseconds per call, so a form that verifies first and
    /// throttles afterwards has already done the expensive thing an attacker wanted.
    ///
    /// Either counter is enough. `OR` rather than `AND` is the whole design — see the
    /// migration for why one counter cannot stand in for the other.
    pub async fn login_locked(&self, username: &str, address: &str) -> Result<bool> {
        let (locked,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM login_attempts \
             WHERE locked_until > datetime('now') \
               AND ((scope = 'account' AND subject = ?1) \
                 OR (scope = 'address'  AND subject = ?2))",
        )
        .bind(username)
        .bind(address)
        .fetch_one(&self.pool)
        .await?;
        Ok(locked > 0)
    }

    /// Count one failed attempt against both the username and the address.
    ///
    /// Both are incremented in ONE transaction. Two statements would leave a window in
    /// which a concurrent attempt saw the account counter raised and the address counter
    /// not, and a race that loses a count is a race that widens the budget.
    ///
    /// `lockout_seconds` is a parameter rather than a constant read in here for the same
    /// reason [`Store::create_session`] takes a TTL: a negative value produces an
    /// already-expired lockout, which is how the expiry test builds its fixture without
    /// sleeping for five minutes. Production passes [`LOGIN_LOCKOUT_SECONDS`].
    pub async fn record_login_failure(
        &self,
        username: &str,
        address: &str,
        lockout_seconds: i64,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for (scope, subject) in [
            (LoginScope::Account, username),
            (LoginScope::Address, address),
        ] {
            // A lockout that has already run out resets the count to 1 rather than
            // continuing from the limit. Without this the eleventh failure would re-lock
            // instantly for ever, turning a five-minute measure into an account deletion
            // anybody could trigger.
            sqlx::query(
                "INSERT INTO login_attempts (scope, subject, failures, last_failure_at, locked_until) \
                 VALUES (?1, ?2, 1, datetime('now'), NULL) \
                 ON CONFLICT (scope, subject) DO UPDATE SET \
                     failures = CASE \
                         WHEN login_attempts.locked_until IS NOT NULL \
                          AND login_attempts.locked_until <= datetime('now') THEN 1 \
                         ELSE login_attempts.failures + 1 END, \
                     locked_until = CASE \
                         WHEN login_attempts.locked_until IS NOT NULL \
                          AND login_attempts.locked_until <= datetime('now') THEN NULL \
                         ELSE login_attempts.locked_until END, \
                     last_failure_at = datetime('now')",
            )
            .bind(scope.as_str())
            .bind(subject)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                "UPDATE login_attempts SET locked_until = datetime('now', ?3) \
                 WHERE scope = ?1 AND subject = ?2 AND failures >= ?4 AND locked_until IS NULL",
            )
            .bind(scope.as_str())
            .bind(subject)
            .bind(format!("{lockout_seconds} seconds"))
            .bind(LOGIN_FAILURE_LIMIT)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        self.delete_stale_login_attempts().await?;
        Ok(())
    }

    /// Forget the failures against one account, after that account signs in successfully.
    ///
    /// The ADDRESS counter is deliberately left alone. Clearing it too would hand anyone
    /// holding a single valid credential an unlimited budget: guess nine times, sign in as
    /// themselves, guess nine more, for ever. What a success proves is that this account's
    /// owner is at the keyboard — it proves nothing about the address they are at.
    pub async fn clear_login_failures(&self, username: &str) -> Result<()> {
        sqlx::query("DELETE FROM login_attempts WHERE scope = 'account' AND subject = ?1")
            .bind(username)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// How many failures stand against one subject. For the admin console, and for tests
    /// that need to assert a counter was cleared rather than merely stopped refusing.
    pub async fn login_failures(&self, scope: LoginScope, subject: &str) -> Result<i64> {
        let row: Option<(i64,)> =
            sqlx::query_as("SELECT failures FROM login_attempts WHERE scope = ?1 AND subject = ?2")
                .bind(scope.as_str())
                .bind(subject)
                .fetch_optional(&self.pool)
                .await?;
        Ok(row.map_or(0, |(n,)| n))
    }

    /// Drop counters nobody is counting any more. Hygiene, never a boundary.
    pub async fn delete_stale_login_attempts(&self) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM login_attempts \
             WHERE last_failure_at <= datetime('now', ?1) \
               AND (locked_until IS NULL OR locked_until <= datetime('now'))",
        )
        .bind(STALE_AFTER)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use crate::{LoginScope, Store, LOGIN_FAILURE_LIMIT, LOGIN_LOCKOUT_SECONDS};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    async fn fail_times(store: &Store, username: &str, address: &str, times: i64) {
        for _ in 0..times {
            store
                .record_login_failure(username, address, LOGIN_LOCKOUT_SECONDS)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn nobody_starts_out_locked() {
        let store = store().await;
        assert!(!store.login_locked("gast", "203.0.113.7").await.unwrap());
    }

    #[tokio::test]
    async fn the_limit_is_the_tenth_failure_and_the_ninth_is_not_enough() {
        let store = store().await;
        fail_times(&store, "gast", "203.0.113.7", LOGIN_FAILURE_LIMIT - 1).await;
        assert!(
            !store.login_locked("gast", "203.0.113.7").await.unwrap(),
            "nine failures must still leave somebody able to type their password"
        );

        fail_times(&store, "gast", "203.0.113.7", 1).await;
        assert!(store.login_locked("gast", "203.0.113.7").await.unwrap());
    }

    #[tokio::test]
    async fn an_account_lock_follows_the_account_across_addresses() {
        // A distributed guess at one account is still a guess at one account.
        let store = store().await;
        for n in 0..LOGIN_FAILURE_LIMIT {
            store
                .record_login_failure("gast", &format!("203.0.113.{n}"), LOGIN_LOCKOUT_SECONDS)
                .await
                .unwrap();
        }
        assert!(
            store.login_locked("gast", "198.51.100.1").await.unwrap(),
            "an address that has never failed must still not reach a locked account"
        );
    }

    #[tokio::test]
    async fn an_address_lock_follows_the_address_across_accounts() {
        // A spray across many accounts from one place is still one place.
        let store = store().await;
        for n in 0..LOGIN_FAILURE_LIMIT {
            store
                .record_login_failure(&format!("gast{n}"), "203.0.113.7", LOGIN_LOCKOUT_SECONDS)
                .await
                .unwrap();
        }
        assert!(
            store
                .login_locked("noch-jemand", "203.0.113.7")
                .await
                .unwrap(),
            "an account that has never failed must not be a way past an address lock"
        );
    }

    #[tokio::test]
    async fn a_success_clears_the_account_but_deliberately_not_the_address() {
        // Clearing the address counter on success would hand an attacker who holds ONE
        // valid credential an unlimited spray budget: guess nine times, sign in as
        // themselves, guess nine more. The account counter is the one a legitimate person
        // who mistyped needs back, and it is the only one a success returns.
        let store = store().await;
        fail_times(&store, "gast", "203.0.113.7", 5).await;

        store.clear_login_failures("gast").await.unwrap();

        assert_eq!(
            store
                .login_failures(LoginScope::Account, "gast")
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            store
                .login_failures(LoginScope::Address, "203.0.113.7")
                .await
                .unwrap(),
            5,
            "a success must not refill somebody else's spray budget"
        );
    }

    #[tokio::test]
    async fn a_lockout_that_has_run_out_lets_the_next_attempt_through_and_starts_again() {
        // A negative lockout builds an already-expired lock without sleeping, exactly as
        // a negative TTL builds an already-expired session.
        let store = store().await;
        for _ in 0..LOGIN_FAILURE_LIMIT {
            store
                .record_login_failure("gast", "203.0.113.7", -60)
                .await
                .unwrap();
        }

        assert!(
            !store.login_locked("gast", "203.0.113.7").await.unwrap(),
            "a five-minute lockout must not be a permanent one"
        );

        store
            .record_login_failure("gast", "203.0.113.7", LOGIN_LOCKOUT_SECONDS)
            .await
            .unwrap();
        assert_eq!(
            store
                .login_failures(LoginScope::Account, "gast")
                .await
                .unwrap(),
            1,
            "the window restarts rather than resuming at the limit"
        );
        assert!(!store.login_locked("gast", "203.0.113.7").await.unwrap());
    }

    #[tokio::test]
    async fn the_counters_survive_a_restart() {
        // The whole reason this lives in SQLite: an in-memory counter makes the lockout a
        // matter of how recently the process was restarted.
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("gw.db").display());

        {
            let store = Store::open(&url).await.unwrap();
            fail_times(&store, "gast", "203.0.113.7", LOGIN_FAILURE_LIMIT).await;
            assert!(store.login_locked("gast", "203.0.113.7").await.unwrap());
        }

        let restarted = Store::open(&url).await.unwrap();
        assert!(
            restarted.login_locked("gast", "203.0.113.7").await.unwrap(),
            "a restart must not be a way out of a lockout"
        );
    }
}
