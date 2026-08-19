//! Proxy attestation — the boundary between "this request came through Caddy" and
//! "someone found the port on the LAN".
//!
//! Caddy runs on a different host, so production binds `0.0.0.0` and the port is reachable
//! by anything on the network. `config::validate` refuses to start such a bind without
//! `GW_PROXY_SECRET`, but a secret that merely *exists* is not a secret that is *checked*:
//! this layer is the check. It runs before routing and before any handler reads an
//! identity, so an unattested request never reaches code that decides what it may read.

use crate::config::Config;
use crate::error::ApiError;
use axum::extract::{Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use subtle::ConstantTimeEq;

/// The header Caddy injects.
///
/// Lowercase, and looked up as a `&str`: `HeaderMap` runs a `&str` key through the same
/// ASCII-lowercasing table (`http`'s `HEADER_CHARS`) that it applies to names arriving off
/// the wire, so this one spelling matches `X-Gw-Proxy` — what Caddy actually sends, having
/// canonicalised it — and `x-gw-proxy`, which is all HTTP/2 permits.
/// `tests::header_lookup_ignores_the_casing_caddy_sends` pins that rather than trusting
/// it: a case-sensitive match would reject every request in production while every test
/// written with a hand-spelled header passed, and present as an inexplicable
/// application-level 403.
const PROXY_HEADER: &str = "x-gw-proxy";

/// Whether proxy attestation is enforced, and the secret it is enforced against.
///
/// Enforcement is decided once, from the bind address at startup — never from the request.
/// Anything a caller controls is something a caller can arrange to be exempt from.
///
/// Deliberately no `Default`: the default would be "not enforced", and a future
/// `..Default::default()` in a state literal would then switch the boundary off in
/// production without anyone writing the word `disabled`.
#[derive(Clone)]
pub struct ProxyGuard {
    enforced: bool,
    secret: String,
}

// Hand-written, and redacting: `#[derive(Debug)]` would put the shared secret into any log
// line that ever formats the state, including one added years from now at debug level.
impl std::fmt::Debug for ProxyGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProxyGuard")
            .field("enforced", &self.enforced)
            .field("secret", &"<redacted>")
            .finish()
    }
}

/// The outcome of checking one request. `Denied` deliberately does not distinguish a
/// missing header from a wrong one: telling those apart hands a scanner an oracle for
/// "the header name is right, keep guessing the value".
enum Verdict {
    Pass,
    Denied,
    /// Enforcing with no secret configured. Never a pass — see `verdict`.
    Unconfigured,
}

/// Proof, carried on the request itself, that this particular request presented the shared
/// secret — that is, that it came through Caddy.
///
/// Inserted by [`enforce`] and by nothing else. It exists so a handler can ask about THIS
/// request rather than about the global configuration: "attestation is switched on" and
/// "this request was attested" are the same thing only for as long as every route sits
/// inside the layer, and a route mounted outside it one day would silently turn a
/// configuration question into a wrong answer.
///
/// What it licenses is narrow and specific: `X-Forwarded-For` is a header any client can
/// write, so it is worth reading only on a request that could not have come from a client
/// directly. See `auth::client_address`.
#[derive(Debug, Clone, Copy)]
pub struct Attested;

impl ProxyGuard {
    /// Nothing in front to add the header, so nothing to demand. This is `just dev`.
    pub fn disabled() -> Self {
        Self {
            enforced: false,
            secret: String::new(),
        }
    }

    pub fn enforcing(secret: impl Into<String>) -> Self {
        Self {
            enforced: true,
            secret: secret.into(),
        }
    }

    /// Derived from the bind address: a loopback bind has no proxy in front of it, and a
    /// non-loopback one is LAN-reachable and has nothing else guarding it.
    pub fn from_config(config: &Config) -> Self {
        if config.bind.ip().is_loopback() {
            Self::disabled()
        } else {
            // `unwrap_or_default` rather than "disable if unset": an absent secret on a
            // reachable bind is a misconfiguration, and `verdict` answers 503 to it.
            Self::enforcing(config.proxy_secret.clone().unwrap_or_default())
        }
    }

    fn verdict(&self, headers: &HeaderMap) -> Verdict {
        if !self.enforced {
            return Verdict::Pass;
        }

        // Fail closed. Comparing against an empty secret would admit every request that
        // sends an empty header value, which is every request that sends none at all.
        // Whitespace-only counts as empty for the same reason `Config` treats it as unset.
        if self.secret.trim().is_empty() {
            return Verdict::Unconfigured;
        }

        let presented = headers.get(PROXY_HEADER).map_or(&[][..], |v| v.as_bytes());

        // Constant time. `==` returns at the first differing byte, so response latency
        // reports how many leading bytes a guess got right and the secret can be
        // recovered one byte at a time. `ct_eq` reads every byte either way. It does
        // compare lengths up front, so the *length* is not hidden — the content is, and
        // an attacker who knows only the length has learnt nothing usable.
        if bool::from(presented.ct_eq(self.secret.as_bytes())) {
            Verdict::Pass
        } else {
            Verdict::Denied
        }
    }
}

/// Reject anything that did not come through the proxy.
///
/// Applied in `build_router` as the outermost layer, so it runs before routing, before any
/// handler, and before `AppState::identity`.
pub async fn enforce(
    State(guard): State<ProxyGuard>,
    mut request: Request,
    next: Next,
) -> Response {
    // Bound in a `let` so the borrow of `request` ends before the `Pass` arm moves it.
    let verdict = guard.verdict(request.headers());

    match verdict {
        Verdict::Pass => {
            // Only when attestation was actually demanded and met. An unenforced pass is
            // "there is nothing in front of this server", which proves nothing about where
            // the request came from and must not be recorded as if it did.
            if guard.enforced {
                request.extensions_mut().insert(Attested);
            }
            next.run(request).await
        }
        Verdict::Denied => {
            // Method and path only. The value that was presented is never logged, at any
            // level: a near-miss in a log file is a secret in a log file.
            tracing::warn!(
                method = %request.method(),
                path = %request.uri().path(),
                "rejected: proxy attestation absent or wrong"
            );
            ApiError::Forbidden.into_response()
        }
        Verdict::Unconfigured => {
            tracing::error!(
                "proxy attestation is enforced but GW_PROXY_SECRET is empty — refusing every \
                 request rather than admitting them unchecked"
            );
            ApiError::Unavailable.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ProxyGuard, Verdict, PROXY_HEADER};
    use crate::config::Config;
    use axum::http::HeaderMap;
    use std::path::PathBuf;

    const SECRET: &str = "not-a-real-secret";

    fn headers(name: &str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        headers
    }

    fn config(bind: &str, secret: Option<&str>) -> Config {
        Config {
            database_url: "sqlite::memory:".into(),
            media_dir: PathBuf::from("./data/media"),
            bind: bind.parse().unwrap(),
            dev_identity: None,
            proxy_secret: secret.map(str::to_string),
            oidc: None,
            public_origin: None,
        }
    }

    fn passes(guard: &ProxyGuard, headers: &HeaderMap) -> bool {
        matches!(guard.verdict(headers), Verdict::Pass)
    }

    #[test]
    fn header_lookup_ignores_the_casing_caddy_sends() {
        // The claim the whole layer rests on, verified rather than assumed: one lowercase
        // `&str` key finds the header under every casing it can arrive in.
        let guard = ProxyGuard::enforcing(SECRET);
        for spelling in ["X-Gw-Proxy", "x-gw-proxy", "X-GW-PROXY", "x-GW-pRoXy"] {
            assert!(
                passes(&guard, &headers(spelling, SECRET)),
                "`{PROXY_HEADER}` must match a header spelled `{spelling}`"
            );
        }
    }

    #[test]
    fn a_loopback_bind_is_not_enforced() {
        // `just dev` binds loopback with no secret set and must keep working.
        let guard = ProxyGuard::from_config(&config("127.0.0.1:8092", None));
        assert!(passes(&guard, &HeaderMap::new()));
    }

    #[test]
    fn a_public_bind_is_enforced_against_the_configured_secret() {
        let guard = ProxyGuard::from_config(&config("0.0.0.0:8092", Some(SECRET)));
        assert!(!passes(&guard, &HeaderMap::new()));
        assert!(passes(&guard, &headers("X-Gw-Proxy", SECRET)));
    }

    #[test]
    fn a_public_bind_without_a_secret_is_unconfigured_not_open() {
        // `config::validate` refuses to start here, so this is defence in depth: if that
        // check is ever relaxed, the guard still must not degrade to admitting everyone.
        let guard = ProxyGuard::from_config(&config("0.0.0.0:8092", None));
        assert!(matches!(
            guard.verdict(&HeaderMap::new()),
            Verdict::Unconfigured
        ));
        assert!(matches!(
            guard.verdict(&headers("X-Gw-Proxy", "")),
            Verdict::Unconfigured
        ));
    }

    #[test]
    fn the_debug_rendering_does_not_carry_the_secret() {
        let rendered = format!("{:?}", ProxyGuard::enforcing(SECRET));
        assert!(!rendered.contains(SECRET), "{rendered}");
    }
}
