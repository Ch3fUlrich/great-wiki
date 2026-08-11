//! `POST /auth/local` — signing in with a great-wiki account rather than a homelab one.
//!
//! This is the half of D-M2-11 that changes the exposure. The earlier plan hid guest
//! sign-in behind an invite link, so nothing advertised that password authentication
//! existed at all; one visible button is better for the person signing in and strictly
//! worse for exposure. The throttling below is the price of that trade, not a later
//! hardening pass.
//!
//! Four properties, and the tests in `tests/local_login.rs` each break one to prove it:
//!
//! - **One refusal, whatever went wrong.** A wrong password, an unknown username, a
//!   deactivated account and a missing token all produce the same status and the same
//!   bytes. Anything else turns the form into a list of who has an account here.
//! - **The same refusal costs the same time.** An unknown username is verified against a
//!   stand-in hash, so it takes argon2's tens of milliseconds like everyone else. Skipping
//!   that is the same oracle, read off a stopwatch.
//! - **The throttle is consulted before the password is.** argon2id at Authelia's
//!   parameters costs 64 MiB and tens of milliseconds per call; a form that verifies first
//!   has already done the expensive thing an attacker came for.
//! - **The session is the SAME session the provider path issues.** One token generator,
//!   one hashing function, one cookie, one table. A second kind of session is a second
//!   place to get expiry, revocation and the `__Host-` attributes wrong.

use crate::error::ApiError;
use crate::proxy_guard::Attested;
use crate::routes::AppState;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Form};
use axum_extra::extract::CookieJar;
use gw_auth::password::{hash_password_at, verify_password, HashingCost};
use serde::Deserialize;
use subtle::ConstantTimeEq;

use super::client_address::client_address;
use super::oidc::random_secret;
use super::page::{self, Notice};
use super::session::{hash_token, new_session_token, session_cookie};

/// What the form submits.
///
/// Every field defaults, so a truncated or hand-made body lands on the ordinary refusal
/// path rather than on axum's 422 — which would be a response shape only malformed
/// requests ever see, and therefore something to probe with.
#[derive(Debug, Deserialize)]
pub struct Credentials {
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    csrf: String,
}

/// An argon2id hash of 256 random bits, made once and kept on the state.
///
/// Verified against when the submitted username has no account, or has one with no
/// password — an OIDC principal, which has no credential row at all. Its only job is to
/// make the work identical: without it, "no such user" returns in microseconds while a
/// real user costs tens of milliseconds, and the difference is measurable from anywhere.
///
/// Random rather than a constant, so there is no fixed input that would verify against it,
/// and made at [`AppState::hashing`] — the same cost as the stored hashes it stands in for,
/// which is the whole point of it.
fn placeholder_hash(state: &AppState) -> &str {
    state
        .placeholder
        .get_or_init(|| new_placeholder(state.hashing))
}

fn new_placeholder(cost: HashingCost) -> String {
    hash_password_at(&random_secret(), cost)
        .expect("both hashing costs are valid argon2 parameters and gw-auth's tests exercise them")
}

/// Constant-time comparison for the CSRF token. `==` on strings returns at the first
/// differing byte and so times how much of a guess was right.
fn constant_time_eq(a: &str, b: &str) -> bool {
    bool::from(a.as_bytes().ct_eq(b.as_bytes()))
}

/// `POST /auth/local`.
///
/// Returns a `Response` rather than a `Result` because every refusal is a rendered page
/// and they must all be the same page: routing one of them through `ApiError` would give
/// it a JSON body and a different status, which is the distinction this handler exists to
/// avoid.
pub async fn sign_in(
    State(state): State<AppState>,
    jar: CookieJar,
    attested: Option<Extension<Attested>>,
    headers: HeaderMap,
    Form(credentials): Form<Credentials>,
) -> Response {
    let homelab = state.oidc.is_some();
    let username = credentials.username.trim().to_string();
    let address = client_address(attested.as_deref(), &headers);

    // Before anything expensive, and before the credential is looked at. Either counter
    // is enough to refuse: see `0005_login_attempts.sql`.
    match state.store.login_locked(&username, &address).await {
        Ok(true) => {
            tracing::warn!(%address, "sign-in refused: throttled");
            return throttled(jar, homelab);
        }
        Ok(false) => {}
        // A store that cannot answer has not established that this attempt is allowed.
        // Fail closed rather than falling through to the password check.
        Err(error) => return ApiError::Internal(error).into_response(),
    }

    // The token this browser was issued, presented back in the form. A cross-site POST
    // cannot read the cookie to copy it, and `SameSite=Lax` withholds the cookie from
    // such a POST in any case. Deliberately NOT counted as a failed attempt: it is not a
    // guess at a password, and counting it would let a third-party page burn somebody
    // else's budget from an iframe.
    let expected = jar
        .get(page::CSRF_COOKIE)
        .map(|cookie| cookie.value().to_string())
        .unwrap_or_default();
    if expected.trim().is_empty() || !constant_time_eq(&expected, &credentials.csrf) {
        tracing::warn!("sign-in refused: the form carried no valid CSRF token");
        return failed(jar, homelab);
    }

    let found = match state.store.principal_by_username(&username).await {
        Ok(found) => found,
        Err(error) => return ApiError::Internal(error).into_response(),
    };
    let stored = found.as_ref().and_then(|(_, hash)| hash.clone());

    // Always a verification, hash or no hash. `spawn_blocking` because argon2id at these
    // parameters occupies a core for tens of milliseconds, and doing that on an async
    // worker stalls every other request sharing the thread.
    let candidate = stored
        .clone()
        .unwrap_or_else(|| placeholder_hash(&state).to_string());
    let password = credentials.password.clone();
    let verified =
        match tokio::task::spawn_blocking(move || verify_password(&password, &candidate)).await {
            Ok(verified) => verified,
            Err(error) => return ApiError::Internal(error.into()).into_response(),
        };

    // `stored.is_some()` is belt as well as braces: the placeholder is a hash of 256
    // random bits, so nothing can verify against it — but "the account has a password"
    // and "the password matched" are different claims and this says both.
    let principal = match found {
        // A deactivated account cannot sign in, whatever it typed. The check is here
        // rather than left to the permission engine because a session must not exist at
        // all: `set_principal_active(false)` deletes sessions, and issuing one afterwards
        // would put the account straight back where deactivating it took it from.
        Some((principal, _)) if verified && stored.is_some() && principal.active => principal,
        _ => {
            if let Err(error) = state
                .store
                .record_login_failure(&username, &address, gw_store::LOGIN_LOCKOUT_SECONDS)
                .await
            {
                // The refusal still stands. A counter that could not be written is a
                // weaker defence, not an open door, so this is logged and not surfaced.
                tracing::error!(%error, "could not record a failed sign-in");
            }
            tracing::warn!(%address, "sign-in refused: local credentials");
            return failed(jar, homelab);
        }
    };

    if let Err(error) = state.store.clear_login_failures(&username).await {
        tracing::error!(%error, "could not clear the failure counter after a sign-in");
    }

    let token = new_session_token();
    if let Err(error) = state
        .store
        .create_session(
            &principal.id,
            &hash_token(&token),
            gw_store::SESSION_TTL_SECONDS,
        )
        .await
    {
        return ApiError::Internal(error).into_response();
    }

    tracing::info!(username = %principal.username, "local sign-in completed");

    // 303 rather than 302: a POST answered with 302 is re-POSTed by some clients on
    // refresh, which would submit the password again. 303 says "GET this instead", which
    // is what a form flow with no JavaScript wants.
    let redirect = (StatusCode::SEE_OTHER, [(header::LOCATION, "/".to_string())]);
    (jar.add(session_cookie(token)), redirect).into_response()
}

fn failed(jar: CookieJar, homelab: bool) -> Response {
    page::respond(jar, homelab, Some(Notice::Failed), StatusCode::UNAUTHORIZED)
}

fn throttled(jar: CookieJar, homelab: bool) -> Response {
    page::respond(
        jar,
        homelab,
        Some(Notice::Throttled),
        StatusCode::TOO_MANY_REQUESTS,
    )
}

#[cfg(test)]
mod tests {
    use super::{constant_time_eq, new_placeholder};
    use gw_auth::password::{verify_password, HashingCost};

    #[test]
    fn the_placeholder_is_a_real_hash_that_nothing_verifies_against() {
        // If it were malformed, `verify_password` would return false immediately and the
        // timing floor it exists to provide would be gone.
        let hash = new_placeholder(HashingCost::CHEAP_FOR_TESTS);
        assert!(hash.starts_with("$argon2id$"), "{hash}");
        for guess in ["", "password", "gast", &hash] {
            assert!(!verify_password(guess, &hash), "verified `{guess}`");
        }
    }

    #[test]
    fn the_placeholder_a_server_uses_costs_what_a_stored_hash_costs() {
        // The one above is about the hash being well-formed and unguessable, so it is made
        // cheaply. This one is about the cost, so it is made the way a server makes it —
        // and asserts the parameters rather than timing them, which is the same claim
        // without a stopwatch in it.
        let hash = new_placeholder(HashingCost::PRODUCTION);
        assert!(
            hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"),
            "a stand-in cheaper than the hashes it stands in for is the timing oracle it \
             exists to close: {hash}"
        );
    }

    #[test]
    fn the_token_comparison_is_exact() {
        assert!(constant_time_eq("abc", "abc"));
        assert!(!constant_time_eq("abc", "abd"));
        assert!(!constant_time_eq("abc", "abcd"));
        assert!(!constant_time_eq("abc", ""));
        assert!(!constant_time_eq("", "abc"));
    }
}
