use crate::breach::{times_seen, BreachRange};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use thiserror::Error;

/// Deliberately identical to Authelia's configured parameters, so a local account is no
/// cheaper to attack than a homelab SSO account.
const MEMORY_KIB: u32 = 65536;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 4;

/// The whole composition policy, current NIST guidance (SP 800-63B): a length floor and
/// nothing else. Character-class rules measurably produce *shorter* and more predictable
/// passwords — "Passw0rd!" satisfies every one of them — so they trade real strength for
/// the appearance of it.
pub const MIN_PASSWORD_LENGTH: usize = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PasswordError {
    #[error("password must be at least {MIN_PASSWORD_LENGTH} characters")]
    TooShort,
    /// The breach corpus holds this password. Length does not help here: a long passphrase
    /// that has appeared in a public dump is one entry in a wordlist.
    #[error("this password appears in {times} known breaches and cannot be used")]
    Breached { times: u64 },
}

/// What the breach half of the policy concluded.
///
/// An enum rather than a bare `Ok(())` because [`BreachCheck::Unavailable`] is a fact the
/// caller has to act on — it means the password was accepted WITHOUT the corpus having
/// been consulted, and D-M2-11's audit trail is where that gets recorded. Returning unit
/// would make "the check did not happen" indistinguishable from "the check passed", and
/// the difference is exactly what an auditor asks about afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreachCheck {
    /// The corpus was reached and does not hold this password.
    Clean,
    /// The corpus could not be reached. The password was ALLOWED.
    Unavailable { reason: String },
}

fn argon2() -> Result<Argon2<'static>> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, None)
        .map_err(|e| anyhow!("invalid argon2 parameters: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// The length floor on its own. [`validate_new_password`] is the whole policy.
pub fn validate_password_strength(plain: &str) -> Result<(), PasswordError> {
    // Length only. Composition rules push people toward "Passw0rd!" and are worse than
    // a length floor; strength beyond this is the user's judgement.
    if plain.chars().count() < MIN_PASSWORD_LENGTH {
        return Err(PasswordError::TooShort);
    }
    Ok(())
}

/// The full policy for a password somebody is *setting*: long enough, and not one the
/// world already has a copy of.
///
/// Order matters. Length is checked first, so a password this instance would refuse
/// anyway is never sent anywhere — not even as a five-character prefix.
///
/// **An unreachable corpus ALLOWS the password**, and that is a deliberate trade rather
/// than an oversight. Failing closed would mean a DNS hiccup, an expired certificate on
/// somebody else's server, or a homelab with no internet stops every person in the
/// building from setting a password — including the administrator trying to fix it. The
/// length floor still applies, the failure is reported to the caller as
/// [`BreachCheck::Unavailable`] rather than swallowed, and the caller writes it to the
/// audit log, so "we accepted a password we could not check" is a question the log can
/// answer months later. The alternative trades a rare, small increase in risk for a
/// frequent, total loss of function.
pub async fn validate_new_password(
    plain: &str,
    corpus: &dyn BreachRange,
) -> Result<BreachCheck, PasswordError> {
    validate_password_strength(plain)?;

    match times_seen(plain, corpus).await {
        Ok(0) => Ok(BreachCheck::Clean),
        Ok(times) => Err(PasswordError::Breached { times }),
        Err(unavailable) => Ok(BreachCheck::Unavailable {
            reason: unavailable.reason().to_string(),
        }),
    }
}

pub fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(argon2()?
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow!("hashing failed: {e}"))?
        .to_string())
}

/// Verify a password. Returns false for a malformed stored hash rather than erroring —
/// a corrupted row must deny access, not take the process down.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    let Ok(hasher) = argon2() else {
        return false;
    };
    hasher.verify_password(plain.as_bytes(), &parsed).is_ok()
}

#[cfg(test)]
mod tests {
    use crate::password::{hash_password, validate_password_strength, verify_password};

    #[test]
    fn a_hash_verifies_against_its_own_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
    }

    #[test]
    fn a_hash_rejects_a_different_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(!verify_password("Correct horse battery staple", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn hashes_are_salted_so_the_same_password_hashes_differently() {
        let a = hash_password("same password").unwrap();
        let b = hash_password("same password").unwrap();
        assert_ne!(a, b, "identical hashes mean the salt is missing");
        assert!(verify_password("same password", &a));
        assert!(verify_password("same password", &b));
    }

    #[test]
    fn the_hash_declares_argon2id_with_authelia_parameters() {
        let hash = hash_password("x").unwrap();
        assert!(
            hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"),
            "got {hash}"
        );
    }

    #[test]
    fn a_malformed_hash_verifies_to_false_rather_than_panicking() {
        // A corrupted row must deny access, not take the process down.
        assert!(!verify_password("x", "not-a-hash"));
        assert!(!verify_password("x", ""));
    }

    #[test]
    fn short_passwords_are_rejected() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("a-perfectly-fine-passphrase").is_ok());
    }
}
