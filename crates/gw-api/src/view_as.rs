//! "What can this person see?" — the permission engine, deliberately run as somebody else.
//!
//! D-M2-17 decides the shape of this file, and the decision is not about convenience:
//!
//! > View-as is blocked at the router, not in each handler. A persistent banner names
//! > whose view is being shown and offers an exit. Every non-GET request is refused while
//! > the mode is active, *before* reaching any handler — so an endpoint written next year
//! > cannot forget the check.
//!
//! So there is exactly one refusal, in [`enforce`], layered in `build_router` next to
//! `proxy_guard::enforce` and for the same reason: it runs before routing, which is what
//! makes it apply to routes that do not exist yet. A per-handler check would be more
//! flexible and would fail *open* for every handler written after it.
//!
//! Three properties, and each is a test in `tests/view_as.rs`:
//!
//! - **Read-only.** [`enforce`] refuses every method but `GET` and `HEAD`, with one
//!   exemption — the request that ENDS the mode, which only ever narrows what the caller
//!   can do. Nothing else passes, including paths the router knows nothing about.
//! - **No escalation.** The substituted principal is read from the store exactly as a real
//!   one is ([`crate::routes::AppState::principal`]), so `can()` and `baseline_for` run
//!   unchanged and the answer is *that person's* view rather than a filtered copy of the
//!   administrator's. Where the substitution cannot be completed — the target is gone,
//!   deactivated, or the viewer is no longer an administrator — the request continues as
//!   `Principal::anonymous()`, never as the administrator. Failing closed here means
//!   seeing less, not more.
//! - **Audited.** Activation writes a row naming both identities BEFORE the cookie is
//!   issued, so an unaudited activation cannot happen.
//!
//! ### Why a cookie, and why the mode is not in the cookie
//!
//! The reader interface has to render as that person across ordinary navigation — the
//! whole point is to look at the wiki through their eyes — so the mode must survive a link
//! click, which rules out a header or a query parameter that only a scripted client would
//! carry.
//!
//! What the cookie holds is a 256-bit token and nothing else. The mode itself — who is
//! viewing, as whom, until when — lives server-side in [`Registry`], and the record is
//! bound to the administrator who created it and re-checked against the caller on every
//! request. A cookie that merely *said* `view_as=<principal id>` would be a privilege
//! escalation written down: anything that can reach the port could set it.
//!
//! And it does not leave anybody stuck. The record expires after
//! [`VIEW_AS_TTL_SECONDS`]; the cookie carries the same lifetime, so the browser stops
//! presenting it exactly when the server stops honouring it; the registry is in memory, so
//! a restart ends every session of it; and the exit is a plain form POST that needs no
//! JavaScript and refuses nothing.

use crate::auth::session::{cleared_cookie, flow_cookie, hash_token, new_session_token};
use crate::error::ApiError;
use crate::routes::AppState;
use axum::extract::{Path, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::Principal;
use gw_store::Baseline;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The cookie that carries the token.
///
/// `__Host-` for the same reason the session cookie has it, and it matters more here: a
/// sibling host that could set this one could put an administrator into a mode they did
/// not ask for. The browser only accepts the prefix on a `Secure`, `Path=/`, domain-less
/// cookie, which no sibling host can produce.
pub const VIEW_AS_COOKIE: &str = "__Host-gw_view_as";

/// How long one activation lasts.
///
/// Short, because this is a question somebody asks and answers — "why can they not see the
/// handbook?" — not a mode to work in. It is also the bound the audit row records, so a
/// start entry with no matching stop entry still says how long the window could have been.
pub const VIEW_AS_TTL_SECONDS: i64 = 30 * 60;

/// The one request that is allowed through while the mode is active.
///
/// A `const` and an exact comparison, never a prefix: `/api/admin/view-as/` as a prefix
/// would exempt every future route under it, including ones nobody has thought of. POST
/// rather than DELETE so the banner's exit is a plain `<form method="post">` that works
/// with no JavaScript — an administrator who cannot leave is the failure this mode must
/// not have.
const EXIT_PATH: &str = "/api/admin/view-as/exit";

// -------------------------------------------------------------------------------------
// The registry.
// -------------------------------------------------------------------------------------

/// One active substitution, as the server remembers it.
#[derive(Debug, Clone)]
struct Record {
    /// The administrator who started it. The mode is theirs and nobody else's.
    viewer_id: String,
    target_id: String,
    expires_at: Instant,
}

/// Every live view-as, keyed by the SHA-256 of the token in the cookie.
///
/// In memory rather than in the database, deliberately. It holds no fact worth surviving a
/// restart — the audit log holds that — and losing it on restart is a property rather than
/// a cost: there is no way for an instance to come back up with somebody still silently
/// substituted. The digest is stored rather than the token for the same reason sessions do
/// it: a dump of process memory should not be a bag of live modes.
#[derive(Debug)]
pub struct Registry {
    live: Mutex<HashMap<String, Record>>,
    /// How long a substitution lasts. A field rather than a constant read at the point of
    /// use, for one reason that is not about testing: the record's deadline and the
    /// cookie's `Max-Age` have to be the same number, and two call sites reading the same
    /// constant is exactly how they stop being.
    ///
    /// It is also the only way the expiry can be PROVEN. With the deadline hard-wired to
    /// thirty minutes, no test can reach it — and a test that cannot reach it is a test
    /// that would not notice the filter in [`Registry::lookup`] being deleted, which is a
    /// mode that never ends. That mutation survived until this field existed.
    ttl: Duration,
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_ttl(Duration::from_secs(VIEW_AS_TTL_SECONDS as u64))
    }
}

impl Registry {
    /// A registry whose substitutions last `ttl`.
    ///
    /// Production uses [`Registry::default`]. A test uses a `ttl` it can outlive, which is
    /// the only way to assert that an expired record stops resolving.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            live: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// The lifetime, as the cookie and the response body need it.
    ///
    /// Both are derived from this rather than from the constant, so a cookie cannot outlive
    /// the record it names — a browser that kept sending a token the server has already
    /// stopped honouring is a banner that disappears while the mode looks live.
    pub fn ttl_seconds(&self) -> i64 {
        self.ttl.as_secs() as i64
    }

    /// A poisoned lock is recovered from rather than propagated.
    ///
    /// A panic somewhere else must not make it impossible to LEAVE this mode; refusing to
    /// read the registry would strand every administrator in it until a restart.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, Record>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Start one, and hand back the token the browser will carry.
    fn begin(&self, viewer_id: &str, target_id: &str) -> String {
        let token = new_session_token();
        let mut live = self.live();
        // Opportunistic, on the one path that is already rare and already writing.
        live.retain(|_, record| record.expires_at > Instant::now());
        live.insert(
            hash_token(&token),
            Record {
                viewer_id: viewer_id.to_string(),
                target_id: target_id.to_string(),
                expires_at: Instant::now() + self.ttl,
            },
        );
        token
    }

    /// The record a token names, if it is still live.
    ///
    /// Expiry is enforced here rather than by a sweeper, exactly as session expiry is: a
    /// record that has outlived its deadline stops resolving the instant it does, whether
    /// or not anything has got round to removing it.
    fn lookup(&self, token: &str) -> Option<Record> {
        let digest = hash_token(token);
        let live = self.live();
        live.get(&digest)
            .filter(|record| record.expires_at > Instant::now())
            .cloned()
    }

    /// End one. `None` when the token names nothing — leaving a mode nobody is in is not
    /// an error.
    fn end(&self, token: &str) -> Option<Record> {
        self.live().remove(&hash_token(token))
    }
}

// -------------------------------------------------------------------------------------
// What is reported.
// -------------------------------------------------------------------------------------

/// One person, as the banner needs to name them.
#[derive(Debug, Clone, Serialize)]
pub struct PersonRef {
    pub id: String,
    pub username: String,
    pub display_name: String,
}

impl PersonRef {
    fn of(principal: &Principal) -> Self {
        Self {
            id: principal.id.clone(),
            username: principal.username.clone(),
            display_name: principal.display_name.clone(),
        }
    }

    /// A target whose row can no longer be read. Named by id rather than blanked: an
    /// unresolvable id is information, and a banner that names nobody is a banner that
    /// cannot be acted on.
    fn unknown(id: &str) -> Self {
        Self {
            id: id.to_string(),
            username: id.to_string(),
            display_name: id.to_string(),
        }
    }
}

/// What `/api/me` reports while the mode is active, and what the banner is rendered from.
///
/// Both identities, because either one alone is misleading: the target's name without the
/// viewer's does not say whose session this really is, and the viewer's without the
/// target's does not say what is being shown.
#[derive(Debug, Clone, Serialize)]
pub struct ViewAsView {
    /// The administrator who is really signed in.
    pub viewer: PersonRef,
    /// Whose view is being shown.
    pub target: PersonRef,
}

/// An active substitution, resolved for one request.
pub(crate) struct Active {
    pub viewer: Principal,
    /// The principal the request actually runs as.
    ///
    /// `Principal::anonymous()` when the substitution could not be completed. Never the
    /// viewer: a mode that fell back to the administrator's own reach while the banner
    /// still said "viewing as somebody" would show an administrator their own documents
    /// and let them conclude the other person can read them.
    pub principal: Principal,
    pub target: PersonRef,
}

impl Active {
    pub(crate) fn view(&self) -> ViewAsView {
        ViewAsView {
            viewer: PersonRef::of(&self.viewer),
            target: self.target.clone(),
        }
    }
}

// -------------------------------------------------------------------------------------
// Resolution.
// -------------------------------------------------------------------------------------

/// The substitution in force for this request, given the caller as they really are.
///
/// `real` must be the UNsubstituted caller — [`crate::routes::AppState::real_principal`] —
/// or this would be asking the cookie to vouch for itself.
///
/// The order below is the whole security argument:
///
/// 1. No cookie, or a token naming no live record: there is no mode. A forged value gets
///    exactly here and no further, which is why the cookie can be a bearer token at all.
/// 2. A record that belongs to somebody else: there is no mode *for this caller*. A copied
///    cookie confers nothing, and it does not end the mode for the person it was copied
///    from either.
/// 3. From here the mode IS active, and the only remaining question is whether the
///    substitution can be completed. Every way it cannot — the viewer is no longer an
///    instance admin, the target is deactivated or gone, the store cannot answer — resolves
///    to anonymous. There is deliberately no branch that resolves to the viewer.
pub(crate) async fn active_for(
    state: &AppState,
    jar: &CookieJar,
    real: &Principal,
) -> Option<Active> {
    let record = state.view_as.lookup(jar.get(VIEW_AS_COOKIE)?.value())?;

    if !real.is_authenticated() || !real.active || real.id != record.viewer_id {
        return None;
    }

    // Re-read on every request (D-M2-7). A demotion or a deactivation ends the mode on the
    // caller's next click rather than at their next sign-in.
    let still_admin = matches!(
        state.store.baseline_for(real).await,
        Ok(baseline) if baseline >= Baseline::Admin
    );

    let (principal, target) = match state.store.principal_by_id(&record.target_id).await {
        Ok(Some((stored, _))) => {
            let named = PersonRef::of(&stored);
            let usable = still_admin && stored.active;
            let running_as = if usable {
                stored
            } else {
                Principal::anonymous()
            };
            (running_as, named)
        }
        Ok(None) => (
            Principal::anonymous(),
            PersonRef::unknown(&record.target_id),
        ),
        Err(error) => {
            tracing::error!(%error, "could not resolve the view-as target");
            (
                Principal::anonymous(),
                PersonRef::unknown(&record.target_id),
            )
        }
    };

    Some(Active {
        viewer: real.clone(),
        principal,
        target,
    })
}

/// The same, resolving the real caller itself. For [`enforce`] and for `/api/me`, neither
/// of which has an unsubstituted principal in hand.
///
/// The cookie is checked before anything is read from the store, so the ordinary request —
/// which carries no such cookie — pays nothing for either of them.
pub(crate) async fn active(state: &AppState, jar: &CookieJar) -> Option<Active> {
    jar.get(VIEW_AS_COOKIE)?;
    let real = state.real_principal(jar).await;
    active_for(state, jar, &real).await
}

// -------------------------------------------------------------------------------------
// The refusal.
// -------------------------------------------------------------------------------------

/// Is this the one request that ends the mode?
///
/// Method and path both, compared exactly. `/api/admin/view-as/exit/` and
/// `/api/admin/view-as/exit/x` are different strings and are not exempt — the test
/// `the_exit_is_the_only_non_get_that_passes` pins that, because an exemption that
/// matches by prefix quietly exempts whatever gets mounted underneath it later.
fn is_exit(method: &Method, path: &str) -> bool {
    method == Method::POST && path == EXIT_PATH
}

/// Refuse every mutating request while somebody is being viewed as (D-M2-17).
///
/// Applied in `build_router` as a layer, so it runs BEFORE routing: a path the router does
/// not know is refused here rather than answered 404, which is what makes this a property
/// of the server rather than of the routes that happen to exist today.
///
/// It sits inside `proxy_guard::enforce` and outside everything else. Inside, because an
/// unattested request must not reach code that reads the store; outside everything else,
/// because that is the point.
pub async fn enforce(
    State(state): State<AppState>,
    jar: CookieJar,
    request: Request,
    next: Next,
) -> Response {
    let Some(active) = active(&state, &jar).await else {
        return next.run(request).await;
    };

    let method = request.method();
    if method == Method::GET || method == Method::HEAD || is_exit(method, request.uri().path()) {
        return next.run(request).await;
    }

    // Both identities, because "somebody tried to write while viewing as somebody else" is
    // only actionable if it says who tried and as whom.
    tracing::warn!(
        method = %request.method(),
        path = %request.uri().path(),
        viewer = %active.viewer.username,
        target = %active.target.username,
        "refused: a mutating request while viewing as somebody else"
    );
    ApiError::Forbidden.into_response()
}

// -------------------------------------------------------------------------------------
// The endpoints.
// -------------------------------------------------------------------------------------

pub fn routes() -> Router<AppState> {
    Router::new()
        // The static path wins over the parameter beside it, and principal ids are UUIDs,
        // so `exit` can never be mistaken for one.
        .route(EXIT_PATH, post(exit))
        .route("/api/admin/view-as/{id}", post(start))
}

/// What activation answers. The same shape `/api/me` reports, so the console and the
/// banner read one thing.
#[derive(Debug, Serialize)]
pub struct Started {
    #[serde(flatten)]
    view: ViewAsView,
    /// How long this lasts. Stated rather than implied, so the console can say it.
    ttl_seconds: i64,
}

/// Begin viewing as `id`.
///
/// Instance admins only, through [`crate::routes::admin::instance_admin`] — the same gate
/// that guards creating an account, not a second one written here. A second gate is a
/// second place for the answer to be wrong.
pub async fn start(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    // Never from inside the mode. `enforce` already refuses every non-GET while it is
    // active, so this is unreachable today — and it stays, because "unreachable" here is a
    // property of another file's layering rather than of this handler.
    let real = state.real_principal(&jar).await;
    if active_for(&state, &jar, &real).await.is_some() {
        return Err(ApiError::Forbidden);
    }

    let actor = crate::routes::admin::instance_admin(&state, &jar).await?;

    // 404 rather than a silent success: only an instance admin reaches this, and they can
    // already list every account, so naming an id that does not exist reveals nothing they
    // could not read directly. This is not a denied request — those are 403.
    let (target, _) = state
        .store
        .principal_by_id(&id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;

    // A deactivated account sees what an anonymous visitor sees, so this would answer the
    // question with something true but useless while the banner claimed otherwise. 409
    // rather than 403 for the same reason the last-admin interlock is: the caller may do
    // this, and could a moment after reactivating them.
    if !target.active {
        return Err(ApiError::Conflict(
            "this account is deactivated; reactivate it before viewing as them".into(),
        ));
    }

    // BEFORE the cookie exists. There is no row to share a transaction with — the mode
    // lives in memory — so the ordering is what guarantees "audited or not activated": a
    // failed write returns 500 and nothing has been started.
    state
        .store
        .record_audit_entry(
            Some(&actor.id),
            "view-as.start",
            Some(&target.id),
            None,
            &json!({
                "viewer": actor.username,
                "viewer_id": actor.id,
                "target": target.username,
                "target_id": target.id,
                "ttl_seconds": state.view_as.ttl_seconds(),
            }),
        )
        .await
        .map_err(ApiError::Internal)?;

    let token = state.view_as.begin(&actor.id, &target.id);
    let jar = jar.add(flow_cookie(
        VIEW_AS_COOKIE,
        token,
        state.view_as.ttl_seconds(),
    ));

    Ok((
        jar,
        Json(Started {
            view: ViewAsView {
                viewer: PersonRef::of(&actor),
                target: PersonRef::of(&target),
            },
            ttl_seconds: state.view_as.ttl_seconds(),
        }),
    )
        .into_response())
}

/// Leave the mode.
///
/// Refuses nothing and requires nothing. Ending a substitution only ever narrows what the
/// caller can do, and a gate here would be a way to be trapped in it — which is the one
/// failure this mode must not have.
///
/// 303 rather than 204 so a plain `<form method="post">` works with no JavaScript: the
/// browser follows it with a GET and lands on the home page, already back in its own view.
pub async fn exit(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(VIEW_AS_COOKIE) {
        if let Some(record) = state.view_as.end(cookie.value()) {
            record_exit(&state, &record).await;
        }
    }

    let jar = jar.remove(cleared_cookie(VIEW_AS_COOKIE));
    (
        jar,
        (StatusCode::SEE_OTHER, [(header::LOCATION, "/".to_string())]),
    )
        .into_response()
}

/// Record that the mode ended, best effort.
///
/// Deactivation IS audited, and the reasoning is worth writing down because the obvious
/// argument goes the other way: leaving is de-escalating, so there is no privilege event
/// to record, and a `view-as.stop` row is absent for most endings anyway — the token
/// expires, the process restarts, the browser is closed — so its absence would be read as
/// "still active".
///
/// What settles it is that the start row carries `ttl_seconds`, so the window is already
/// bounded without this: a missing stop row means "until the deadline at the latest", not
/// "possibly for ever". Given that, a stop row is strictly more information — it narrows a
/// bounded window to a known one — and costs one row per deliberate exit.
///
/// A failure here is logged and swallowed, which is the opposite of the rule on the way in.
/// That asymmetry is deliberate: refusing to activate when the log is unwritable is safe,
/// and refusing to LEAVE when the log is unwritable would strand an administrator inside a
/// mode because of a database problem.
async fn record_exit(state: &AppState, record: &Record) {
    let name_of = |resolved: &anyhow::Result<Option<(Principal, Option<String>)>>| match resolved {
        Ok(Some((principal, _))) => principal.username.clone(),
        _ => String::new(),
    };
    let viewer = state.store.principal_by_id(&record.viewer_id).await;
    let target = state.store.principal_by_id(&record.target_id).await;

    if let Err(error) = state
        .store
        .record_audit_entry(
            Some(&record.viewer_id),
            "view-as.stop",
            Some(&record.target_id),
            None,
            &json!({
                "viewer": name_of(&viewer),
                "viewer_id": record.viewer_id,
                "target": name_of(&target),
                "target_id": record.target_id,
            }),
        )
        .await
    {
        tracing::error!(%error, "could not record the end of a view-as session");
    }
}

#[cfg(test)]
mod tests {
    use super::{is_exit, Registry, EXIT_PATH, VIEW_AS_COOKIE};
    use axum::http::Method;

    #[test]
    fn the_cookie_carries_the_host_prefix() {
        // The prefix is what stops a sibling host putting an administrator into a mode
        // they did not ask for.
        assert!(VIEW_AS_COOKIE.starts_with("__Host-"));
    }

    #[test]
    fn only_the_exact_exit_request_is_exempt() {
        assert!(is_exit(&Method::POST, EXIT_PATH));
        assert!(!is_exit(&Method::DELETE, EXIT_PATH));
        assert!(!is_exit(&Method::POST, "/api/admin/view-as/exit/"));
        assert!(!is_exit(&Method::POST, "/api/admin/view-as/exitx"));
        assert!(!is_exit(&Method::POST, "/api/admin/view-as/exit/more"));
        assert!(!is_exit(&Method::POST, "/api/admin/view-as"));
    }

    #[test]
    fn a_token_names_one_record_and_nothing_else() {
        let registry = Registry::default();
        let token = registry.begin("chef", "gast");
        let other = registry.begin("chef", "niemand");

        assert_eq!(registry.lookup(&token).unwrap().target_id, "gast");
        assert_eq!(registry.lookup(&other).unwrap().target_id, "niemand");
        assert!(registry.lookup("nicht-ausgegeben").is_none());

        // Ending one does not end the other, and ending it twice is not an error.
        assert!(registry.end(&token).is_some());
        assert!(registry.end(&token).is_none());
        assert!(registry.lookup(&other).is_some());
    }

    #[test]
    fn the_registry_holds_the_digest_and_not_the_token() {
        // A record keyed by the token itself would make a memory dump a bag of live modes.
        let registry = Registry::default();
        let token = registry.begin("chef", "gast");
        assert!(!registry.live().contains_key(&token));
        assert_eq!(registry.live().len(), 1);
    }
}
