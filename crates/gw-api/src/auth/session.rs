//! Session tokens, the cookie that carries one, and the endpoints that read it.

use crate::error::ApiError;
use crate::routes::{AppState, PrincipalSource};
use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use axum_extra::extract::cookie::{Cookie, SameSite};
use axum_extra::extract::CookieJar;
use base64::Engine;
use gw_store::Baseline;
use rand::RngCore;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;

/// The cookie a signed-in browser presents.
///
/// The `__Host-` prefix is not decoration; the browser enforces it. A cookie by this name
/// is only accepted when it is `Secure`, has `Path=/` and carries **no** `Domain`, which
/// is exactly what makes it impossible for a sibling host under `ohje.ooguy.com` — or
/// anything that manages to answer for one — to set or overwrite the wiki's session.
/// Without the prefix that is a same-site cookie-injection away.
pub const SESSION_COOKIE: &str = "__Host-gw_session";

/// A fresh session token: 32 bytes — 256 bits — from the operating system's CSPRNG.
///
/// `OsRng` and not `thread_rng`, and certainly not a seeded generator: this value is the
/// entire credential. Anything an attacker could reproduce is not a session token, it is a
/// user identifier with extra steps.
pub fn new_session_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// The value that is stored for a token. SHA-256, hex.
///
/// A plain digest with no salt or stretching is right here and would be wrong for a
/// password: the input is 256 random bits, so there is no dictionary to run and no
/// guessing to slow down. What the hash buys is that a copy of the database is not a
/// bag of live sessions.
pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Infallible: writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The attributes every cookie this module issues carries, and the ones `__Host-` demands.
///
/// `SameSite=Lax` rather than `Strict`: the OIDC callback is a top-level navigation from
/// the identity provider, and `Strict` would withhold the flow cookies on exactly that
/// request — the sign-in would fail with no way to tell it from an attack. `Lax` still
/// withholds them from cross-site POSTs and sub-resource loads, which is the traffic that
/// matters for CSRF.
fn base_cookie(name: &'static str, value: String) -> Cookie<'static> {
    Cookie::build((name, value))
        .path("/")
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .build()
}

pub(crate) fn session_cookie(token: String) -> Cookie<'static> {
    let mut cookie = base_cookie(SESSION_COOKIE, token);
    cookie.set_max_age(time::Duration::seconds(gw_store::SESSION_TTL_SECONDS));
    cookie
}

/// A cookie shaped to *remove* the one of the same name.
///
/// It has to carry the same `Path` and attributes as the cookie it replaces, or the
/// browser treats it as a different cookie and keeps the original.
pub(crate) fn cleared_cookie(name: &'static str) -> Cookie<'static> {
    base_cookie(name, String::new())
}

pub(crate) fn flow_cookie(name: &'static str, value: String, ttl_seconds: i64) -> Cookie<'static> {
    let mut cookie = base_cookie(name, value);
    cookie.set_max_age(time::Duration::seconds(ttl_seconds));
    cookie
}

/// A 302 to `location`. Used for both legs of the login redirect.
pub(crate) fn found(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [(header::LOCATION, location.to_string())],
    )
        .into_response()
}

/// Who the caller is, as the interface needs it.
///
/// Every field is derived from the principal the permission engine would use, so the
/// header cannot show a name the server would not honour. When nobody is signed in, the
/// identity-bearing fields are empty rather than merely ignored: `/api/me` must not be a
/// place where an unverified claim can be echoed back and believed.
#[derive(Debug, Serialize)]
pub struct Me {
    pub authenticated: bool,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub groups: Vec<String>,
    pub teams: Vec<String>,
    /// The default reach the groups confer (D-M2-1). Shown so someone can see *why* they
    /// can or cannot reach something, rather than guessing.
    pub baseline: Baseline,
    /// Whether an identity provider is configured at all. The interface offers a sign-in
    /// link only when there is somewhere to send the person.
    pub login_available: bool,
    /// What established this identity. `dev-shim` has no session to end, so the interface
    /// does not offer a sign-out that would do nothing.
    pub source: PrincipalSource,
}

pub async fn me(State(state): State<AppState>, jar: CookieJar) -> Result<Json<Me>, ApiError> {
    let (principal, source) = state.principal_with_source(&jar).await;
    let baseline = state
        .store
        .baseline_for(&principal)
        .await
        .map_err(ApiError::Internal)?;

    let signed_in = principal.is_authenticated() && principal.active;
    Ok(Json(Me {
        authenticated: signed_in,
        username: signed_in.then(|| principal.username.clone()),
        display_name: signed_in.then(|| principal.display_name.clone()),
        email: signed_in.then(|| principal.email.clone()).flatten(),
        groups: if signed_in {
            principal.groups
        } else {
            Vec::new()
        },
        teams: if signed_in {
            principal.teams
        } else {
            Vec::new()
        },
        baseline,
        login_available: state.oidc.is_some(),
        source,
    }))
}

/// Sign out.
///
/// The server-side row is deleted, not merely the cookie cleared. Clearing a cookie asks
/// the browser to forget a credential; it does nothing about a copy taken from that
/// browser earlier, and "sign out" has to mean the token stops working.
///
/// 303 rather than 204 so a plain `<form method="post">` works with no JavaScript: the
/// browser follows it with a GET and lands on the home page.
pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Result<Response, ApiError> {
    if let Some(cookie) = jar.get(SESSION_COOKIE) {
        state
            .store
            .delete_session(&hash_token(cookie.value()))
            .await
            .map_err(ApiError::Internal)?;
    }

    let jar = jar.remove(cleared_cookie(SESSION_COOKIE));
    let redirect = (StatusCode::SEE_OTHER, [(header::LOCATION, "/".to_string())]);
    Ok((jar, redirect).into_response())
}

#[cfg(test)]
mod tests {
    use super::{hash_token, new_session_token, SESSION_COOKIE};
    use base64::Engine;
    use std::collections::HashSet;

    #[test]
    fn the_session_cookie_carries_the_host_prefix() {
        assert!(
            SESSION_COOKIE.starts_with("__Host-"),
            "the prefix is what stops a sibling host setting this cookie"
        );
    }

    #[test]
    fn a_session_token_carries_at_least_256_bits() {
        let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(new_session_token())
            .expect("tokens are base64url");
        assert!(raw.len() >= 32, "got {} bytes", raw.len());
    }

    #[test]
    fn session_tokens_do_not_repeat() {
        // A weak generator shows up here first: a thousand draws from anything with less
        // than 256 bits of state would be very unlikely to stay distinct by accident.
        let tokens: HashSet<String> = (0..1000).map(|_| new_session_token()).collect();
        assert_eq!(tokens.len(), 1000);
    }

    #[test]
    fn hashing_is_sha256_and_not_something_that_merely_looks_like_it() {
        // The published digest of "abc". Pinning it means a future change of algorithm is
        // a deliberate act with a failing test, not a silent one.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(hash_token("abc").len(), 64);
    }

    #[test]
    fn the_hash_is_not_the_token() {
        let token = new_session_token();
        assert_ne!(hash_token(&token), token);
    }
}
