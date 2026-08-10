//! Breach checking, by k-anonymity.
//!
//! A password that has appeared in a public breach is guessable no matter how long it is,
//! so the length floor alone is not a policy. Have I Been Pwned publishes the corpus as a
//! range API, and the protocol is what makes consulting it acceptable at all:
//!
//! 1. SHA-1 the password locally.
//! 2. Send the first FIVE hexadecimal characters — and nothing else, ever.
//! 3. The service returns every suffix it holds under that prefix, some hundreds of them.
//! 4. Look for our own suffix in that list, locally.
//!
//! The service therefore learns that somebody, somewhere, is checking one of ~2^140
//! digests. It never sees the password and never sees a digest it could reverse.
//! `only_the_first_five_hex_characters_of_the_digest_ever_leave_this_machine` pins that
//! rather than trusting the description.
//!
//! The network call itself is behind [`BreachRange`] so the tests can drive every branch —
//! a hit, a miss, a malformed response, an outage — without touching the network. A test
//! that needs the internet is a test that fails on a plane, and a *security* test that
//! needs the internet is one that gets deleted the first time it does.

use sha1::{Digest, Sha1};
use std::fmt::Write as _;
use std::future::Future;
use std::pin::Pin;

/// Why the corpus could not answer. Never a reason to refuse a password — see
/// [`crate::password::validate_new_password`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreachUnavailable(String);

impl BreachUnavailable {
    pub fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }

    pub fn reason(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BreachUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// A boxed future, because [`BreachRange`] has to be usable as `dyn`: the caller holds one
/// of these behind a reference and does not care whether it is the real service or a stub.
pub type BreachFuture<'a> =
    Pin<Box<dyn Future<Output = Result<String, BreachUnavailable>> + Send + 'a>>;

/// The seam between the policy and the network.
///
/// Implementors fetch the range response for a five-character prefix. The contract is
/// narrow on purpose: an implementation is handed a prefix and cannot be handed anything
/// else, so no future edit can widen what leaves this machine without changing this
/// signature.
pub trait BreachRange: Send + Sync {
    fn fetch<'a>(&'a self, prefix: &'a str) -> BreachFuture<'a>;
}

/// SHA-1, upper-case hex — the form the range API's index is published in.
pub fn sha1_hex(password: &str) -> String {
    let digest = Sha1::digest(password.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Infallible: writing to a String cannot fail.
        let _ = write!(out, "{byte:02X}");
    }
    out
}

/// How many breach records the response holds for `suffix`, or zero.
///
/// Every unreadable line is skipped rather than failing the check. The alternative would
/// let one malformed byte from a third party refuse a password, which is a denial of
/// service operated by somebody else's server.
pub fn times_in_range(body: &str, suffix: &str) -> u64 {
    body.lines()
        .filter_map(|line| line.trim().split_once(':'))
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(suffix))
        .and_then(|(_, count)| count.trim().parse::<u64>().ok())
        .unwrap_or(0)
}

/// Ask the corpus how often it has seen this password.
///
/// The split at five characters is the k-anonymity boundary and the only thing that
/// crosses the network.
pub async fn times_seen(
    password: &str,
    corpus: &dyn BreachRange,
) -> Result<u64, BreachUnavailable> {
    let digest = sha1_hex(password);
    let (prefix, suffix) = digest.split_at(5);
    let body = corpus.fetch(prefix).await?;
    Ok(times_in_range(&body, suffix))
}

/// A corpus that reports every password unseen.
///
/// For tests and for any deployment that deliberately does not consult an outside service.
/// It is NOT the same as an unreachable corpus: this answers the question ("not breached"),
/// where an unreachable one reports that the question could not be asked. Collapsing the
/// two would make a deployment with no network indistinguishable from one that checked.
pub struct UnseenCorpus;

impl BreachRange for UnseenCorpus {
    fn fetch<'a>(&'a self, _prefix: &'a str) -> BreachFuture<'a> {
        Box::pin(async { Ok(String::new()) })
    }
}

/// A corpus that reports it could not answer.
///
/// The fallback when the real client cannot even be constructed. Deliberately NOT
/// [`UnseenCorpus`]: that one answers "this password is not breached", which would
/// silently disable the check and pass a breached password as clean. This one reports the
/// question could not be asked, which [`crate::password::validate_new_password`] turns
/// into [`BreachCheck::Unavailable`] — allowed, and audited, per D-M2-16. The difference
/// between "checked and clean" and "never checked" has to survive all the way to the
/// audit log or the degraded mode is invisible.
pub struct UnavailableCorpus;

impl BreachRange for UnavailableCorpus {
    fn fetch<'a>(&'a self, _prefix: &'a str) -> BreachFuture<'a> {
        Box::pin(async {
            Err(BreachUnavailable::new(
                "no breach-corpus client could be built",
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::breach::{sha1_hex, BreachFuture, BreachRange, BreachUnavailable};
    use crate::password::{validate_new_password, BreachCheck, PasswordError};
    use std::sync::Mutex;

    /// A corpus that answers from a canned table and records what it was asked.
    ///
    /// Recording the request is the point: it is what lets a test prove the password and
    /// its full digest never left this machine, rather than assuming it.
    #[derive(Default)]
    struct StubCorpus {
        body: String,
        asked: Mutex<Vec<String>>,
    }

    impl StubCorpus {
        fn holding(body: &str) -> Self {
            Self {
                body: body.to_string(),
                asked: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
    }

    impl BreachRange for StubCorpus {
        fn fetch<'a>(&'a self, prefix: &'a str) -> BreachFuture<'a> {
            Box::pin(async move {
                self.asked.lock().unwrap().push(prefix.to_string());
                Ok(self.body.clone())
            })
        }
    }

    /// A corpus that is simply not there — a network outage, DNS, a firewall.
    #[derive(Default)]
    struct UnreachableCorpus {
        asked: Mutex<Vec<String>>,
    }

    impl BreachRange for UnreachableCorpus {
        fn fetch<'a>(&'a self, prefix: &'a str) -> BreachFuture<'a> {
            Box::pin(async move {
                self.asked.lock().unwrap().push(prefix.to_string());
                Err(BreachUnavailable::new("kein Netz"))
            })
        }
    }

    /// SHA-1("password"), the most-breached password there is. A published vector rather
    /// than a round trip: computing the digest with the same code that consumes it would
    /// agree with itself while both were wrong, and it is Troy Hunt's server that has to
    /// agree. This one assertion is what makes it legitimate for the tests below to
    /// derive their fixtures from `sha1_hex` — the function has already been pinned.
    const PASSWORD_SHA1: &str = "5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8";

    /// Long enough to clear the length floor, so these tests exercise the corpus rather
    /// than tripping over the check in front of it. `password` itself is eight characters
    /// and never reaches the corpus at all.
    const LONG: &str = "correct horse battery staple";

    fn suffix_of(password: &str) -> String {
        sha1_hex(password)[5..].to_string()
    }

    #[test]
    fn the_digest_matches_the_published_vector() {
        assert_eq!(sha1_hex("password"), PASSWORD_SHA1);
    }

    #[tokio::test]
    async fn a_password_under_twelve_characters_is_refused() {
        let corpus = StubCorpus::default();
        assert_eq!(
            validate_new_password("kurz", &corpus).await,
            Err(PasswordError::TooShort)
        );
        // Twelve exactly is the floor, not the first value above it.
        assert!(validate_new_password("zwoelfzeich", &corpus).await.is_err());
        assert!(validate_new_password("zwoelfzeiche", &corpus).await.is_ok());
    }

    #[tokio::test]
    async fn a_short_password_never_reaches_the_corpus_at_all() {
        // The length floor is checked first, so a refusal costs no round trip and tells
        // Troy Hunt's server nothing about a password this instance never accepted.
        let corpus = StubCorpus::default();
        let _ = validate_new_password("kurz", &corpus).await;
        assert!(
            corpus.seen().is_empty(),
            "a refused password was sent anyway"
        );
    }

    #[tokio::test]
    async fn a_password_the_corpus_knows_is_refused() {
        // Long enough to pass every other rule, and refused anyway. Length is not a
        // defence against a password somebody already has a copy of.
        let corpus = StubCorpus::holding(&format!(
            "0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n{}:20361\r\n",
            suffix_of(LONG)
        ));
        assert_eq!(
            validate_new_password(LONG, &corpus).await,
            Err(PasswordError::Breached { times: 20361 })
        );
    }

    #[tokio::test]
    async fn a_password_the_corpus_does_not_list_is_accepted() {
        // A response with entries in it, none of them ours: the check must compare
        // suffixes rather than merely noticing that the corpus answered.
        let corpus = StubCorpus::holding("0018A45C4D1DEF81644B54AB7F969B88D65:1\r\n");
        assert_eq!(
            validate_new_password(LONG, &corpus).await,
            Ok(BreachCheck::Clean)
        );
    }

    #[tokio::test]
    async fn an_unreachable_corpus_allows_the_password_and_says_so() {
        // The deliberate trade, stated in `validate_new_password`: failing closed would
        // mean a network outage stops anybody setting a password at all. The length floor
        // still applies, and the caller records the gap in the audit log.
        let corpus = UnreachableCorpus::default();
        let verdict = validate_new_password("ein-langes-passwort", &corpus)
            .await
            .expect("an outage must not refuse a password");
        assert!(
            matches!(verdict, BreachCheck::Unavailable { .. }),
            "the caller must be told the check did not happen: {verdict:?}"
        );
    }

    #[tokio::test]
    async fn only_the_first_five_hex_characters_of_the_digest_ever_leave_this_machine() {
        // k-anonymity, proven rather than described. If this ever regresses, the password
        // — or a digest that a rainbow table reverses — is being handed to a third party.
        let corpus = StubCorpus::holding("");
        validate_new_password(LONG, &corpus).await.unwrap();

        let asked = corpus.seen();
        assert_eq!(asked.len(), 1, "one lookup per check: {asked:?}");
        let prefix = &asked[0];
        let digest = sha1_hex(LONG);
        assert_eq!(prefix.len(), 5, "sent `{prefix}`");
        assert_eq!(prefix, &digest[..5]);
        assert_ne!(prefix, &digest, "the whole digest was sent");
        assert!(!prefix.contains(LONG), "the password itself was sent");
    }

    #[tokio::test]
    async fn the_suffix_comparison_ignores_case_and_surrounding_whitespace() {
        // The service answers in upper case today. A response that arrived lower-cased, or
        // with the CRLF the API actually sends, must not read as "not breached".
        for body in [
            format!("{}:5\n", suffix_of(LONG).to_ascii_lowercase()),
            format!("  {}:5  \r\n", suffix_of(LONG)),
        ] {
            let corpus = StubCorpus::holding(&body);
            assert_eq!(
                validate_new_password(LONG, &corpus).await,
                Err(PasswordError::Breached { times: 5 }),
                "body was {body:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_zero_count_line_is_not_a_breach() {
        // HIBP's padding responses carry synthetic suffixes with a count of 0. Treating
        // one as a hit would refuse a password nothing has ever seen.
        let corpus = StubCorpus::holding(&format!("{}:0\r\n", suffix_of(LONG)));
        assert_eq!(
            validate_new_password(LONG, &corpus).await,
            Ok(BreachCheck::Clean)
        );
    }

    #[tokio::test]
    async fn a_malformed_line_is_ignored_rather_than_taken_as_a_hit() {
        let corpus = StubCorpus::holding("nonsense\r\n:::\r\nAAAA:nichtzahl\r\n");
        assert_eq!(
            validate_new_password("ein-langes-passwort", &corpus).await,
            Ok(BreachCheck::Clean)
        );
    }
}
