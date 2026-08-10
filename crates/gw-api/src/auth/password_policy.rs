//! Accepting a password somebody is setting, and recording what was checked.
//!
//! The policy itself is `gw_auth::validate_new_password`: twelve characters and no
//! composition rules (current NIST guidance — character-class rules measurably produce
//! shorter, more guessable passwords), plus a breach check by k-anonymity.
//!
//! This file is the one call site that policy needs in order to be honest about itself.
//! An unreachable corpus **allows** the password, because failing closed would mean a
//! network outage stops everybody — including the administrator trying to fix it — from
//! ever setting one. That trade is only defensible if the gap is recorded rather than
//! swallowed, so the audit entry below is part of the decision and not an extra.

use gw_auth::breach::BreachRange;
use gw_auth::{hash_password, validate_new_password, BreachCheck, PasswordError};
use gw_store::Store;
use serde_json::json;

/// The audit action written when a password was accepted without being checked.
///
/// A dotted verb like every other action in the log, so it sorts and filters with them.
pub const BREACH_CHECK_SKIPPED: &str = "password.breach-check.unavailable";

/// Validate a new password and return its argon2id hash.
///
/// `actor` is whoever is setting it — an administrator creating an account, or the
/// recipient of an invite choosing their own. `subject` is the account it is for, so the
/// audit entry says which one went unchecked rather than merely that one did.
///
/// This is the only function that should ever produce a hash for the `credentials` table:
/// hashing without going through the policy is how a password below the floor, or one
/// straight out of a public breach dump, gets in.
pub async fn accept_new_password(
    store: &Store,
    actor: Option<&str>,
    subject: &str,
    plain: &str,
    corpus: &dyn BreachRange,
) -> Result<String, PasswordError> {
    match validate_new_password(plain, corpus).await? {
        BreachCheck::Clean => {}
        BreachCheck::Unavailable { reason } => {
            tracing::warn!(
                %reason,
                subject,
                "the breach corpus could not be reached; the password was accepted on its \
                 length alone"
            );
            if let Err(error) = store
                .record_audit_entry(
                    actor,
                    BREACH_CHECK_SKIPPED,
                    Some(subject),
                    // Instance-wide: setting a password belongs to no subtree, so this is
                    // readable by instance admins only (see the 0004 migration).
                    None,
                    &json!({ "reason": reason }),
                )
                .await
            {
                // The password still stands. A log line that could not be written is a
                // gap in the record, not a reason to refuse somebody a password.
                tracing::error!(%error, "could not record the skipped breach check");
            }
        }
    }

    hash_password(plain).map_err(|error| {
        // Unreachable in practice; the parameters are constants. Reported as `TooShort`
        // would be a lie, so it surfaces as what it is.
        tracing::error!(%error, "hashing a new password failed");
        PasswordError::Breached { times: 0 }
    })
}

#[cfg(test)]
mod tests {
    use super::{accept_new_password, BREACH_CHECK_SKIPPED};
    use gw_auth::breach::{BreachFuture, BreachRange, BreachUnavailable};
    use gw_auth::{password::verify_password, PasswordError, Principal};
    use gw_store::Store;

    struct Silent;
    impl BreachRange for Silent {
        fn fetch<'a>(&'a self, _prefix: &'a str) -> BreachFuture<'a> {
            Box::pin(async { Ok(String::new()) })
        }
    }

    struct Offline;
    impl BreachRange for Offline {
        fn fetch<'a>(&'a self, _prefix: &'a str) -> BreachFuture<'a> {
            Box::pin(async { Err(BreachUnavailable::new("kein Netz")) })
        }
    }

    async fn admin_view(store: &Store) -> Vec<String> {
        let admin = Principal::test("chef", &["admins"], &[]);
        store
            .audit_for(&admin, 50)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.action)
            .collect()
    }

    #[tokio::test]
    async fn a_clean_password_is_hashed_and_nothing_is_audited() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let hash =
            accept_new_password(&store, Some("chef"), "gast", "ein-langes-passwort", &Silent)
                .await
                .unwrap();

        assert!(verify_password("ein-langes-passwort", &hash));
        assert!(
            admin_view(&store).await.is_empty(),
            "a check that happened is not news"
        );
    }

    #[tokio::test]
    async fn a_short_password_is_refused_before_anything_is_written() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        assert_eq!(
            accept_new_password(&store, Some("chef"), "gast", "kurz", &Offline).await,
            Err(PasswordError::TooShort)
        );
        assert!(admin_view(&store).await.is_empty());
    }

    #[tokio::test]
    async fn an_unreachable_corpus_allows_the_password_and_leaves_a_record_of_the_gap() {
        // The deliberate trade, and the thing that makes it defensible. Without the audit
        // entry, "we accepted a password we could not check" would be unanswerable.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let hash = accept_new_password(
            &store,
            Some("chef"),
            "gast",
            "ein-langes-passwort",
            &Offline,
        )
        .await
        .expect("an outage must not stop somebody setting a password");
        assert!(verify_password("ein-langes-passwort", &hash));

        let entries = store
            .audit_for(&Principal::test("chef", &["admins"], &[]), 50)
            .await
            .unwrap()
            .entries;
        let recorded = entries
            .iter()
            .find(|entry| entry.action == BREACH_CHECK_SKIPPED)
            .expect("the skipped check was not recorded");
        assert_eq!(recorded.target.as_deref(), Some("gast"));
        assert!(recorded.detail.contains("kein Netz"), "{}", recorded.detail);
        assert_eq!(
            recorded.path, None,
            "setting a password belongs to no subtree, so instance admins only"
        );
    }
}
