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
use gw_auth::password::HashingCost;
use gw_auth::Principal;
use gw_store::Store;
use std::sync::{Arc, OnceLock};

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
    /// What one argon2id hash costs here.
    ///
    /// In the state for the same reason `corpus` is: it is a value the composition root
    /// supplies, not a constant compiled into the code that uses it. [`AppState::serving`]
    /// — the only constructor a running server has — writes
    /// [`HashingCost::PRODUCTION`] itself and takes no argument that could say otherwise,
    /// and `great-wiki serve` re-checks it before it binds a listener.
    ///
    /// The `for_test*` constructors supply [`HashingCost::CHEAP_FOR_TESTS`], which is why
    /// a suite that signs in a few hundred times no longer spends a minute doing it. What
    /// the cheap cost never buys is a claim about cost: the two tests that assert one —
    /// `the_hash_declares_argon2id_with_authelia_parameters` in gw-auth, and
    /// `an_unknown_username_still_costs_a_password_verification` in `tests/local_login.rs`
    /// — name the production cost themselves.
    pub hashing: HashingCost,
    /// The stand-in hash the unknown-username path verifies against, built once on first
    /// use from 256 random bits at [`AppState::hashing`].
    ///
    /// Here rather than in a `static` because it has to be made at *this* state's cost:
    /// a placeholder cheaper than the stored hashes would hand back exactly the timing
    /// oracle it exists to close, and a process-wide one would be whichever cost happened
    /// to ask first. A server has one state, so "once per state" and "once per process"
    /// are the same thing where it matters.
    pub placeholder: Arc<OnceLock<String>>,
    /// Every live "view as somebody else" (D-M2-17).
    ///
    /// In the state rather than in a `static` for the same reason `placeholder` is: a test
    /// builds a server and gets its own, and two servers in one process are two servers.
    /// It is deliberately NOT in the database — see [`crate::view_as::Registry`]: losing
    /// every session of it on restart is a property, not a cost.
    pub view_as: Arc<crate::view_as::Registry>,
}

impl AppState {
    /// The state a running server uses.
    ///
    /// There is no hashing-cost parameter and there will not be one: a server hashes at
    /// [`HashingCost::PRODUCTION`], so there is nothing here for a caller to get wrong.
    pub fn serving(
        store: Arc<Store>,
        dev_identity: Option<Identity>,
        proxy_guard: ProxyGuard,
        oidc: Option<Arc<OidcClient>>,
        corpus: Arc<dyn BreachRange>,
    ) -> Self {
        Self {
            store,
            dev_identity,
            proxy_guard,
            oidc,
            corpus,
            hashing: HashingCost::PRODUCTION,
            placeholder: Arc::new(OnceLock::new()),
            view_as: Arc::new(crate::view_as::Registry::default()),
        }
    }

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
            // Cheap, because almost no test is about what a hash costs — and the two that
            // are ask for the production cost by name. See `HashingCost` for why this is a
            // value here rather than a Cargo feature.
            hashing: HashingCost::CHEAP_FOR_TESTS,
            placeholder: Arc::new(OnceLock::new()),
            view_as: Arc::new(crate::view_as::Registry::default()),
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
    ///
    /// While an administrator is viewing as somebody else (D-M2-17) this is the
    /// SUBSTITUTED principal — read from the store exactly as a real one is, so `can()`
    /// and `baseline_for` run unchanged and nothing downstream has to know. That is the
    /// only way the answer can be "what that person sees" rather than "what we believe
    /// that person sees".
    pub async fn principal(&self, jar: &CookieJar) -> Principal {
        self.principal_with_source(jar).await.0
    }

    /// The same, and where it came from.
    ///
    /// Nothing about authorisation depends on the source — `can()` sees a principal and
    /// only a principal, which is what keeps the shim honest. The interface needs it for
    /// one thing: a development shim has no session to end, so offering "sign out" there
    /// would be offering a button that quietly does nothing.
    ///
    /// The source is the REAL one even under a substitution: it describes how this request
    /// authenticated, which view-as does not change. There is still a session to end, and
    /// the banner — not the source field — is what says whose view is being shown.
    pub async fn principal_with_source(&self, jar: &CookieJar) -> (Principal, PrincipalSource) {
        let (real, source) = self.real_principal_with_source(jar).await;
        match crate::view_as::active_for(self, jar, &real).await {
            Some(active) => (active.principal, source),
            None => (real, source),
        }
    }

    /// The caller as they actually authenticated, before any substitution.
    ///
    /// Used by the two places that must not be fooled by the mode: [`crate::view_as`],
    /// which would otherwise be asking the cookie to vouch for itself, and the activation
    /// endpoint, which has to know who is really asking. Everything else wants
    /// [`AppState::principal`].
    pub(crate) async fn real_principal(&self, jar: &CookieJar) -> Principal {
        self.real_principal_with_source(jar).await.0
    }

    /// Whose view is being shown, when one is (D-M2-17). `None` is the ordinary case.
    ///
    /// Resolved through the same function every request uses, so `/api/me` cannot report a
    /// mode the permission engine is not actually in — and cannot fail to report one it is
    /// in, which is the direction that matters: a banner that does not appear is an
    /// administrator who does not know whose view they are reading.
    pub async fn viewing_as(&self, jar: &CookieJar) -> Option<crate::view_as::ViewAsView> {
        crate::view_as::active(self, jar)
            .await
            .map(|active| active.view())
    }

    async fn real_principal_with_source(&self, jar: &CookieJar) -> (Principal, PrincipalSource) {
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
    // The view-as layer needs the whole state (it reads the store); the proxy layer needs
    // only its own policy. Cloned here because `with_state` consumes the original.
    let view_as_state = state.clone();

    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/api/tree", get(tree::get_tree))
        .route("/api/documents/{*path}", get(docs::get_document))
        .merge(admin::routes())
        .merge(crate::auth::routes())
        .merge(crate::view_as::routes())
        // D-M2-17, and it is applied HERE rather than in each handler for one reason: a
        // layer added after every route wraps the 404 fallback too, so a path that does
        // not exist yet is already covered. Per-handler checks fail open for code that has
        // not been written, which is the class of bug this project keeps finding.
        //
        // Inside the proxy guard below, never outside it: this layer reads the store, and
        // an unattested request must not reach anything that does.
        .layer(axum::middleware::from_fn_with_state(
            view_as_state,
            crate::view_as::enforce,
        ))
        // Last, so it wraps every route registered above *and* the 404 fallback: the guard
        // has to run before routing, or an unattested request would learn which paths
        // exist. Handlers read `AppState::principal` inside this layer, never outside it.
        .layer(axum::middleware::from_fn_with_state(
            state.proxy_guard.clone(),
            proxy_guard::enforce,
        ))
        .with_state(state)
}
