use crate::breach::{times_seen, BreachRange};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use thiserror::Error;

/// What one argon2id hash costs to compute, and therefore what one guess costs an
/// attacker.
///
/// A value rather than a global constant because the test suite needs a cheap one and a
/// running server must never get it. Deliberately **not** a Cargo feature: features unify
/// across a build graph, so anything anywhere in the tree asking for "cheap hashing" would
/// silently switch the server binary too, and no diff would say so. A value can only take
/// effect where somebody wrote it down.
///
/// The fields are private and there is no constructor that takes numbers, so exactly two
/// values of this type exist in the whole program — [`HashingCost::PRODUCTION`] and
/// [`HashingCost::CHEAP_FOR_TESTS`]. There is no third to invent by accident, and no
/// arithmetic anywhere that could produce a weaker one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HashingCost {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

impl HashingCost {
    /// Deliberately identical to Authelia's configured parameters, so a local account is no
    /// cheaper to attack than a homelab SSO account.
    ///
    /// This is what a running server hashes at, and `great-wiki serve` refuses to start on
    /// anything else. `the_hash_declares_argon2id_with_authelia_parameters` pins the three
    /// numbers against the hash string they actually produce.
    pub const PRODUCTION: Self = Self {
        memory_kib: 65536,
        iterations: 3,
        parallelism: 4,
    };

    /// The cheapest parameters argon2 will accept: one pass, one lane, and the 8 KiB floor
    /// that `m >= 8 * p` imposes.
    ///
    /// **Tests only, and named so it cannot be reached by accident.** It exists because
    /// argon2 at [`HashingCost::PRODUCTION`] is tens of milliseconds *by design*, and a
    /// suite that signs in a few hundred times pays that for a property — "the parameters
    /// are Authelia's" — that one test can pin directly and every other test merely
    /// inherits.
    ///
    /// Nothing that hashes at this cost is claiming anything about cost. The two tests that
    /// are about cost — the parameter assertion here, and
    /// `an_unknown_username_still_costs_a_password_verification` in gw-api — use
    /// [`HashingCost::PRODUCTION`] and always will.
    pub const CHEAP_FOR_TESTS: Self = Self {
        memory_kib: 8,
        iterations: 1,
        parallelism: 1,
    };

    fn hasher(self) -> Result<Argon2<'static>> {
        let params = Params::new(self.memory_kib, self.iterations, self.parallelism, None)
            .map_err(|e| anyhow!("invalid argon2 parameters: {e}"))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

/// The strong one. A forgotten field, a `..Default::default()`, or a future struct that
/// grows a cost it does not think about all land on Authelia's parameters rather than on
/// whatever happens to be cheapest.
impl Default for HashingCost {
    fn default() -> Self {
        Self::PRODUCTION
    }
}

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

/// Hash a password at Authelia's parameters.
///
/// The un-parameterised name is the production one on purpose: code that does not think
/// about cost gets the expensive answer, and only code that names a cost explicitly —
/// [`hash_password_at`] — can get anything else.
pub fn hash_password(plain: &str) -> Result<String> {
    hash_password_at(plain, HashingCost::PRODUCTION)
}

/// Hash a password at a stated cost.
pub fn hash_password_at(plain: &str, cost: HashingCost) -> Result<String> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(cost
        .hasher()?
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow!("hashing failed: {e}"))?
        .to_string())
}

/// Verify a password. Returns false for a malformed stored hash rather than erroring —
/// a corrupted row must deny access, not take the process down.
///
/// Takes no [`HashingCost`], and cannot: a PHC string carries `m`, `t` and `p` in its own
/// text, and argon2 verifies against *those* rather than against whatever this process
/// happens to be configured with. That is what makes changing the cost safe at all — a
/// hash written years ago at other parameters keeps verifying — and it is also why every
/// stored hash is self-describing evidence of the cost it was made at.
/// `a_hash_is_verified_at_the_parameters_it_carries_rather_than_the_ones_configured` is
/// the test that holds this property down.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    // The instance's parameters are not consulted for verification; only its algorithm
    // family and (absent) secret key are. Built from the production cost so that a future
    // argon2 release which *did* consult them would fail loudly on cheap test hashes
    // rather than quietly weaken production ones.
    let Ok(hasher) = HashingCost::PRODUCTION.hasher() else {
        return false;
    };
    hasher.verify_password(plain.as_bytes(), &parsed).is_ok()
}

#[cfg(test)]
mod tests {
    use crate::password::{
        hash_password, hash_password_at, validate_password_strength, verify_password, HashingCost,
    };

    /// Everything below that is about *correctness* rather than *cost* hashes here. The
    /// two tests that are about cost name [`HashingCost::PRODUCTION`] themselves.
    fn cheap(plain: &str) -> String {
        hash_password_at(plain, HashingCost::CHEAP_FOR_TESTS).unwrap()
    }

    #[test]
    fn a_hash_verifies_against_its_own_password() {
        let hash = cheap("correct horse battery staple");
        assert!(verify_password("correct horse battery staple", &hash));
    }

    #[test]
    fn a_hash_rejects_a_different_password() {
        let hash = cheap("correct horse battery staple");
        assert!(!verify_password("Correct horse battery staple", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn hashes_are_salted_so_the_same_password_hashes_differently() {
        let a = cheap("same password");
        let b = cheap("same password");
        assert_ne!(a, b, "identical hashes mean the salt is missing");
        assert!(verify_password("same password", &a));
        assert!(verify_password("same password", &b));
    }

    #[test]
    fn the_hash_declares_argon2id_with_authelia_parameters() {
        // The one that matters, and it runs against the REAL parameters: `hash_password`
        // takes no cost and cannot be given one, so this is the cost every caller that
        // does not think about cost gets.
        let hash = hash_password("x").unwrap();
        assert!(
            hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"),
            "got {hash}"
        );
    }

    #[test]
    fn the_cost_nobody_chooses_is_the_production_one() {
        // `Default` is the strong one, so a struct that grows a `HashingCost` it has not
        // thought about lands on Authelia's parameters rather than on the cheapest thing
        // in the file.
        assert_eq!(HashingCost::default(), HashingCost::PRODUCTION);
        assert_ne!(HashingCost::CHEAP_FOR_TESTS, HashingCost::PRODUCTION);
    }

    #[test]
    fn a_hash_is_verified_at_the_parameters_it_carries_rather_than_the_ones_configured() {
        // The property the whole arrangement rests on. `verify_password` takes no cost;
        // if it verified at some *configured* cost instead of the one written into the PHC
        // string, then hashing cheaply anywhere would make those hashes unverifiable — or,
        // far worse, a stored production hash would be checked against weaker work.
        let production = hash_password("ein-langes-passwort").unwrap();
        let cheap_hash = cheap("ein-langes-passwort");

        assert!(production.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"));
        assert!(cheap_hash.starts_with("$argon2id$v=19$m=8,t=1,p=1$"));

        assert!(verify_password("ein-langes-passwort", &production));
        assert!(verify_password("ein-langes-passwort", &cheap_hash));
        assert!(!verify_password("das-falsche-passwort", &production));
        assert!(!verify_password("das-falsche-passwort", &cheap_hash));
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
