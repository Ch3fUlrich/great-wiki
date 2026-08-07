pub mod docs;
pub mod tree;

use crate::identity::Identity;
use crate::proxy_guard::{self, ProxyGuard};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::Principal;
use gw_store::Store;
use std::sync::Arc;

/// The cookie a signed-in browser presents. Task 6 issues and validates it; the name is
/// here because [`AppState::principal`] already looks for it, and the order in which the
/// three sources are consulted is the part that matters.
pub const SESSION_COOKIE: &str = "gw_session";

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
        }
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
        if let Some(cookie) = jar.get(SESSION_COOKIE) {
            if let Some(principal) = self.principal_from_session(cookie.value()).await {
                return principal;
            }
        }
        if let Some(dev) = &self.dev_identity {
            if let Some(principal) = self.principal_from_dev_shim(dev).await {
                return principal;
            }
        }
        Principal::anonymous()
    }

    /// Resolve a session cookie to the principal that owns it.
    ///
    /// **Stub until Task 6**, which adds the session store. Until then there is nothing to
    /// resolve a cookie against, and the fail-closed answer is the only correct one: a
    /// presented cookie confers *nothing*, rather than being believed for whatever it
    /// claims. Returning `None` here drops the request through to the shim and then to
    /// anonymous.
    ///
    /// When the session store lands, this looks the session up and then RE-READS the
    /// principal from the database by username (D-M2-7) instead of trusting whatever was
    /// captured at sign-in, so revoking a grant or deactivating an account takes effect on
    /// the next click.
    async fn principal_from_session(&self, _session_id: &str) -> Option<Principal> {
        None
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
        // Last, so it wraps every route registered above *and* the 404 fallback: the guard
        // has to run before routing, or an unattested request would learn which paths
        // exist. Handlers read `AppState::principal` inside this layer, never outside it.
        .layer(axum::middleware::from_fn_with_state(
            state.proxy_guard.clone(),
            proxy_guard::enforce,
        ))
        .with_state(state)
}
