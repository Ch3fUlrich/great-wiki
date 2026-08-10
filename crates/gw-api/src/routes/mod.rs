pub mod admin;
pub mod docs;
pub mod tree;

use crate::auth::{OidcClient, OidcConfig};
use crate::identity::Identity;
use crate::proxy_guard::{self, ProxyGuard};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::breach::BreachRange;
use gw_auth::Principal;
use gw_store::Store;
use std::sync::Arc;

/// The cookie a signed-in browser presents. Defined next to the code that issues it; this
/// re-export exists because [`AppState::principal`] is the other end of the same contract.
pub use crate::auth::SESSION_COOKIE;

/// Which of the three sources established the caller's identity.
///
/// Reported by `/api/me`, never consulted by the permission engine: an identity is an
/// identity whatever produced it, and a rule that treated one source as more trustworthy
/// than another would be a second place where access is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PrincipalSource {
    /// A session cookie, resolved against the session table.
    Session,
    /// `GW_DEV_IDENTITY`. Only reachable on a loopback bind.
    DevShim,
    Anonymous,
}

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    /// When present, every request arrives as this user. Only reachable on a loopback
    /// bind — `config::validate` refuses to start otherwise.
    ///
    /// It names a user and their Authelia groups and nothing else: the principal it
    /// resolves to is read from the store on every request, so development exercises the
    /// real permission engine rather than a shortcut around it.
    pub dev_identity: Option<Identity>,
    /// Proxy attestation policy, resolved from the bind address once at startup. It lives
    /// in the state rather than being read from the environment inside the layer, so a
    /// test constructs an enforcing server directly instead of mutating process globals.
    pub proxy_guard: ProxyGuard,
    /// The identity provider, when one is configured.
    ///
    /// `None` is a legitimate deployment — a wiki with only local accounts — so
    /// `/auth/login` answers 503 rather than the process refusing to start. What must
    /// never happen is a *half*-configured provider, and `Config::from_env` refuses that
    /// at startup instead of leaving it to fail at the first sign-in.
    pub oidc: Option<Arc<OidcClient>>,
    /// The breach corpus consulted when somebody SETS a password (D-M2-16).
    ///
    /// In the state rather than constructed at the call site so a test can substitute one
    /// and never touch the network — a test that reaches the internet is a test that fails
    /// on a plane — and so that every password-setting path shares one instance rather
    /// than each deciding for itself whether to check at all. That is not hypothetical:
    /// the admin account-creation endpoint originally called the length-only validator
    /// directly, so accounts created by an administrator skipped the corpus entirely while
    /// the policy looked implemented.
    pub corpus: Arc<dyn BreachRange>,
}

impl AppState {
    /// Unenforced, matching the loopback bind a test implies. Tests that are about the
    /// boundary itself use `for_test_with_guard`.
    pub fn for_test(store: Arc<Store>, dev_identity: Option<Identity>) -> Self {
        Self::for_test_with_guard(store, dev_identity, ProxyGuard::disabled())
    }

    pub fn for_test_with_guard(
        store: Arc<Store>,
        dev_identity: Option<Identity>,
        proxy_guard: ProxyGuard,
    ) -> Self {
        Self {
            store,
            dev_identity,
            proxy_guard,
            oidc: None,
            // Never the real corpus in a test. A stub that reports every password unseen
            // keeps the length rule under test without a network call deciding whether
            // the suite passes.
            corpus: Arc::new(gw_auth::breach::UnseenCorpus),
        }
    }

    /// A state that talks OIDC to `config`'s issuer, with no development shim — the login
    /// tests must exercise the session branch and nothing else.
    pub fn for_test_oidc(store: Arc<Store>, config: OidcConfig) -> anyhow::Result<Self> {
        Ok(Self {
            oidc: Some(Arc::new(OidcClient::new(config)?)),
            ..Self::for_test(store, None)
        })
    }

    /// A state whose requests arrive as `principal`.
    ///
    /// It goes *through* the development shim rather than around it, so a test exercises
    /// the same resolution the running server does — including the per-request re-read
    /// from the store. That re-read is what makes "deactivate the account, then make one
    /// more request" testable at all: a pinned copy of the principal would still be
    /// active, and the test would prove nothing.
    pub fn for_test_principal(store: Arc<Store>, principal: &Principal) -> Self {
        Self::for_test(
            store,
            Some(Identity {
                user: Some(principal.username.clone()),
                groups: principal.groups.clone(),
            }),
        )
    }

    /// The calling principal.
    ///
    /// Session cookie first, then the development shim, then anonymous. Anonymous is the
    /// fall-through for every failure along the way, never an error: a request that cannot
    /// establish who it is has established that it is nobody.
    pub async fn principal(&self, jar: &CookieJar) -> Principal {
        self.principal_with_source(jar).await.0
    }

    /// The same, and where it came from.
    ///
    /// Nothing about authorisation depends on the source — `can()` sees a principal and
    /// only a principal, which is what keeps the shim honest. The interface needs it for
    /// one thing: a development shim has no session to end, so offering "sign out" there
    /// would be offering a button that quietly does nothing.
    pub async fn principal_with_source(&self, jar: &CookieJar) -> (Principal, PrincipalSource) {
        if let Some(cookie) = jar.get(SESSION_COOKIE) {
            if let Some(principal) = self.principal_from_session(cookie.value()).await {
                return (principal, PrincipalSource::Session);
            }
        }
        if let Some(dev) = &self.dev_identity {
            if let Some(principal) = self.principal_from_dev_shim(dev).await {
                return (principal, PrincipalSource::DevShim);
            }
        }
        (Principal::anonymous(), PrincipalSource::Anonymous)
    }

    /// Resolve a session cookie to the principal that owns it.
    ///
    /// The cookie carries the token; the database holds only its SHA-256, so what is
    /// looked up is the digest of what was presented. An unknown or expired session is
    /// `None` — indistinguishable from no cookie at all, which is the correct answer to
    /// both.
    ///
    /// The principal is RE-READ from the store here (D-M2-7) rather than being captured at
    /// sign-in. That is what makes revocation immediate: a removed grant, a changed team
    /// or a deactivated account takes effect on this request, not at the next sign-in.
    ///
    /// A store error is `None`, not a propagated failure. A store that cannot answer who
    /// this is has not authenticated anybody, and the request continues as nobody.
    async fn principal_from_session(&self, token: &str) -> Option<Principal> {
        match self
            .store
            .principal_for_session(&crate::auth::session::hash_token(token))
            .await
        {
            Ok(principal) => principal,
            Err(error) => {
                tracing::error!(%error, "could not resolve the session cookie");
                None
            }
        }
    }

    /// The development shim, resolved into a real principal.
    ///
    /// `GW_DEV_IDENTITY=sergej:admins` names a user and the Authelia groups they arrive
    /// with; *everything the engine then decides* — the baseline those groups confer, team
    /// membership, whether the account is still active — is read from the store, on every
    /// request (D-M2-7). The shim therefore exercises the real permission engine instead of
    /// bypassing it, which is the only way local development can catch a rule that is
    /// wrong.
    ///
    /// A principal that does not exist yet is created as an OIDC one carrying those groups.
    /// Without that, a fresh database would make local development silently anonymous —
    /// the shim would appear to be ignored, and the obvious next move is to weaken
    /// something until content shows up again.
    async fn principal_from_dev_shim(&self, dev: &Identity) -> Option<Principal> {
        let username = dev
            .user
            .as_deref()
            .map(str::trim)
            .filter(|u| !u.is_empty())?;

        match self.store.principal_by_username(username).await {
            Ok(Some((principal, _))) => Some(principal),
            Ok(None) => match self
                .store
                .upsert_oidc_principal(username, username, None, &dev.groups)
                .await
            {
                Ok(principal) => Some(principal),
                Err(error) => {
                    tracing::error!(%error, username, "could not create the dev shim principal");
                    None
                }
            },
            // Fail closed. A store that cannot answer who this is has not authenticated
            // anybody, and continuing as the shim would be trusting a name with no record
            // behind it.
            Err(error) => {
                tracing::error!(%error, username, "could not load the dev shim principal");
                None
            }
        }
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/api/tree", get(tree::get_tree))
        .route("/api/documents/{*path}", get(docs::get_document))
        .merge(admin::routes())
        .merge(crate::auth::routes())
        // Last, so it wraps every route registered above *and* the 404 fallback: the guard
        // has to run before routing, or an unattested request would learn which paths
        // exist. Handlers read `AppState::principal` inside this layer, never outside it.
        .layer(axum::middleware::from_fn_with_state(
            state.proxy_guard.clone(),
            proxy_guard::enforce,
        ))
        .with_state(state)
}
