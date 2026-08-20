//! The admin API: principals, teams, path-scoped grants, the audit log, and the interlock
//! that keeps the instance from being left with nobody able to administer it.
//!
//! Two gates, and the difference between them is the whole design.
//!
//! [`instance_admin`] guards what concerns the instance — creating an account, creating a
//! team, listing everyone. It asks for the `admin` baseline, which under D-M2-1 comes from
//! the verified Authelia group, and additionally from the per-account promotion in 0006 —
//! the fallback for an instance whose Authelia administrators are gone, since great-wiki
//! never writes that database (ADR 0002). Both are `Store::baseline_for`'s answer; neither
//! is decided here.
//!
//! [`path_admin`] guards what concerns one subtree. It asks `can()` for `Action::Admin` on
//! **that path**, so whoever administers `/raum` administers `/raum` and its descendants
//! and nothing else. This is what makes D-M2-2 possible without decentralising instance
//! administration along with it.
//!
//! Neither gate decides anything itself. The baseline comes from `Store::baseline_for` and
//! the verdict from `gw_auth::can`; there is no permission rule written in this file, and
//! there must not be one.
//!
//! Both gates check authentication *first*, before any grant is consulted, and that check
//! is not redundant. `can()` answers an `Anyone` grant before it looks at whether the
//! caller is signed in — deliberately, because that is how a public share link works — so
//! on a path carrying `anyone: admin` the engine would hand the ACL editor to a request
//! that has not said who it is. An admin API is not a share link.

use super::AppState;
use crate::auth::password_policy::accept_new_password;
// The invite token and the session cookie share one generator and one digest. A second
// pair would be a second place for "random" to become "predictable" and for a plaintext
// credential to reach a table.
use crate::auth::session::{hash_token, new_session_token};
use crate::error::ApiError;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::{can, Action, Grant, Permission, Principal, Subject};
use gw_core::Visibility;
use gw_store::admin::{ActiveOutcome, InstanceAdminOutcome, VisibilityOutcome};
use gw_store::principals::AdminCandidate;
use gw_store::{
    AuditPage, Baseline, CreateInviteOutcome, InviteSummary, MembershipOutcome, NewInvite,
    RevokeInviteOutcome, TeamSummary, INVITE_TTL_SECONDS,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/admin/principals",
            get(list_principals).post(create_principal),
        )
        .route("/api/admin/principals/{id}/active", post(set_active))
        .route(
            "/api/admin/principals/{id}/instance-admin",
            post(set_instance_admin),
        )
        .route("/api/admin/admins/candidates", get(admin_candidates))
        .route("/api/admin/teams", get(list_teams).post(create_team))
        .route("/api/admin/teams/{slug}/members", post(add_member))
        .route(
            "/api/admin/teams/{slug}/members/{id}",
            delete(remove_member),
        )
        .route("/api/admin/acl", get(read_acl).post(grant).delete(revoke))
        .route("/api/admin/visibility", post(set_visibility))
        .route("/api/admin/roles", get(list_roles))
        .route("/api/admin/audit", get(read_audit))
        .route("/api/admin/invites", get(list_invites).post(create_invite))
        .route("/api/admin/invites/{id}", delete(revoke_invite))
}

// -------------------------------------------------------------------------------------
// Gates.
// -------------------------------------------------------------------------------------

/// The caller, if they administer the whole instance.
///
/// `baseline_for` already refuses an unauthenticated or deactivated caller, so the check
/// above it is defence in depth rather than the only thing standing here — but it is the
/// one gate that does not depend on the `group_roles` table being right, and this is not
/// a place to have exactly one.
pub(crate) async fn instance_admin(
    state: &AppState,
    jar: &CookieJar,
) -> Result<Principal, ApiError> {
    let principal = state.principal(jar).await;
    if !principal.is_authenticated() || !principal.active {
        return Err(ApiError::Forbidden);
    }

    let baseline = state
        .store
        .baseline_for(&principal)
        .await
        .map_err(ApiError::Internal)?;
    if baseline >= Baseline::Admin {
        Ok(principal)
    } else {
        Err(ApiError::Forbidden)
    }
}

/// The caller, if they administer `path`.
///
/// An instance admin passes as well, and that is not a shortcut: D-M2-1 gives `admins`
/// instance administration, and the first grant on a fresh subtree has to be writable by
/// somebody who holds no grant there yet — otherwise no space could ever be delegated.
async fn path_admin(state: &AppState, jar: &CookieJar, path: &str) -> Result<Principal, ApiError> {
    let principal = state.principal(jar).await;
    if !principal.is_authenticated() || !principal.active {
        return Err(ApiError::Forbidden);
    }

    if state
        .store
        .baseline_for(&principal)
        .await
        .map_err(ApiError::Internal)?
        >= Baseline::Admin
    {
        return Ok(principal);
    }

    // `Restricted` is how the rest of this codebase asks "is there a grant for this
    // caller?" without keeping a second copy of the subject-matching rules: at that
    // visibility `can()` consults grants and nothing else. A baseline must never stand in
    // for an admin grant here — administering a space is not something `internal` reach
    // confers.
    let grants = state
        .store
        .grants_for_path(path)
        .await
        .map_err(ApiError::Internal)?;
    if can(&principal, Action::Admin, Visibility::Restricted, &grants) {
        Ok(principal)
    } else {
        Err(ApiError::Forbidden)
    }
}

/// The caller, if they have said who they are and the account is still active.
///
/// **Not an authorisation decision** — [`instance_admin`] and [`path_admin`] make those,
/// and nothing may be handed over on the strength of this alone. It exists for endpoints
/// whose authorisation depends on a stored row: the row has to be read before the right
/// gate is even known, and reading it first would make "does this id exist?" answerable by
/// somebody who has not signed in. Establishing that there is a caller closes that, and
/// costs one session lookup that the gate below it would have done anyway.
async fn signed_in(state: &AppState, jar: &CookieJar) -> Result<Principal, ApiError> {
    let principal = state.principal(jar).await;
    if principal.is_authenticated() && principal.active {
        Ok(principal)
    } else {
        Err(ApiError::Forbidden)
    }
}

/// The caller, if they may read any of the audit log at all.
///
/// The log itself is filtered by [`gw_store::Store::audit_for`], in the retriever, which
/// is where the architecture rule says it belongs: an instance admin sees everything, a
/// space admin sees the entries scoped to a subtree they administer. This gate only
/// decides whether there is any point asking, so that somebody who administers nothing is
/// refused rather than handed an empty list to keep polling.
///
/// "Administers something" is answered by asking `can()` once per path that carries a
/// grant — the console's own index. Not by a prefix query: grants do not union up the
/// tree, so holding `admin` at `/a` says nothing about `/a/b` once `/a/b` carries its own.
async fn audit_reader(state: &AppState, jar: &CookieJar) -> Result<Principal, ApiError> {
    let principal = state.principal(jar).await;
    if !principal.is_authenticated() || !principal.active {
        return Err(ApiError::Forbidden);
    }

    if state
        .store
        .baseline_for(&principal)
        .await
        .map_err(ApiError::Internal)?
        >= Baseline::Admin
    {
        return Ok(principal);
    }

    for (path, _) in state
        .store
        .paths_with_grants()
        .await
        .map_err(ApiError::Internal)?
    {
        let grants = state
            .store
            .grants_for_path(&path)
            .await
            .map_err(ApiError::Internal)?;
        if can(&principal, Action::Admin, Visibility::Restricted, &grants) {
            return Ok(principal);
        }
    }

    Err(ApiError::Forbidden)
}

// -------------------------------------------------------------------------------------
// Principals.
// -------------------------------------------------------------------------------------

pub async fn list_principals(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<Principal>>, ApiError> {
    instance_admin(&state, &jar).await?;
    // `Principal` carries no credential, so there is no hash to forget to strip here.
    state
        .store
        .list_principals()
        .await
        .map(Json)
        .map_err(ApiError::Internal)
}

#[derive(Deserialize)]
pub struct NewPrincipal {
    username: String,
    display_name: String,
    email: Option<String>,
    /// Hashed before it reaches the store, and never logged, echoed or recorded.
    password: String,
}

pub async fn create_principal(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<NewPrincipal>,
) -> Result<(StatusCode, Json<Principal>), ApiError> {
    let actor = instance_admin(&state, &jar).await?;

    let username = body.username.trim();
    if username.is_empty() || body.display_name.trim().is_empty() {
        return Err(ApiError::Invalid(
            "username and display_name are required".into(),
        ));
    }
    // Checked before the insert so the answer is 409 rather than a UNIQUE violation
    // surfacing as 500. The constraint is still what actually guarantees it.
    if state
        .store
        .principal_by_username(username)
        .await
        .map_err(ApiError::Internal)?
        .is_some()
    {
        return Err(ApiError::Conflict("username already taken".into()));
    }

    // The FULL policy (D-M2-16), not the length rule alone. This originally called
    // `validate_password_strength` and `hash_password` directly, which is a length floor
    // and nothing else — so every account an administrator created skipped the breach
    // corpus while the policy looked implemented. `accept_new_password` is the single
    // audited call site: it applies the length rule, consults the corpus, refuses a
    // breached password, and writes an audit row when the corpus could not be reached so
    // that the degraded mode is visible rather than silent.
    //
    // It runs before the insert, so a refused password creates no account.
    let hash = accept_new_password(
        &state.store,
        Some(actor.id.as_str()),
        username,
        &body.password,
        state.corpus.as_ref(),
    )
    .await
    .map_err(|error| ApiError::Invalid(error.to_string()))?;
    let created = state
        .store
        .create_local_principal_audited(
            &actor.id,
            username,
            body.display_name.trim(),
            body.email
                .as_deref()
                .map(str::trim)
                .filter(|e| !e.is_empty()),
            &hash,
        )
        .await
        .map_err(ApiError::Internal)?;

    Ok((StatusCode::CREATED, Json(created)))
}

#[derive(Deserialize)]
pub struct ActiveFlag {
    active: bool,
}

/// What a refused deactivation or demotion says, and the only advice that helps.
///
/// The interlock is not a permission problem — the caller may do this, and would be
/// allowed to a moment after promoting somebody — so it is 409 rather than 403, and it
/// names the way out. Only instance admins reach either endpoint, and they can already
/// list every account, so saying how many administrators are left reveals nothing to them
/// that the console does not already show.
const LAST_ADMIN: &str = "this would leave the instance with no active administrator; \
                          promote somebody from /api/admin/admins/candidates first";

pub async fn set_active(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<ActiveFlag>,
) -> Result<Json<Principal>, ApiError> {
    let actor = instance_admin(&state, &jar).await?;

    // 404 rather than a silent 200: only an instance admin reaches this, and they can
    // already list every account, so naming an id that does not exist reveals nothing
    // they could not read directly.
    match state
        .store
        .set_principal_active_audited(&actor.id, &id, body.active)
        .await
        .map_err(ApiError::Internal)?
    {
        ActiveOutcome::Changed(principal) => Ok(Json(*principal)),
        ActiveOutcome::NoSuchPrincipal => Err(ApiError::NotFound),
        // Refused by the store, in the transaction that would have committed it. The rule
        // is not restated here: a second copy in this handler could disagree with the one
        // that actually decides, and it is the one in the transaction that is safe against
        // two administrators deactivating themselves at the same moment.
        ActiveOutcome::LastAdmin => Err(ApiError::Conflict(LAST_ADMIN.into())),
    }
}

#[derive(Deserialize)]
pub struct InstanceAdminFlag {
    admin: bool,
}

/// What a promotion or demotion did, and what the person can do afterwards.
#[derive(Serialize)]
pub struct InstanceAdminView {
    /// Whether a promotion row was written or removed. Same field name as every other
    /// mutating endpoint, so a client can ask the question generically.
    changed: bool,
    /// Whether they administer the instance NOW.
    ///
    /// Not implied by `changed`, and this is the one place where the difference is a
    /// disclosure rather than a nicety: somebody who administers through Authelia's
    /// `admins` group still does after a demotion here, because great-wiki never writes
    /// that database (ADR 0002). A console reading only `changed` would announce a
    /// revocation that did not happen.
    instance_admin: bool,
}

/// Promote or demote a single account (0006).
///
/// Instance admins only, and audited instance-wide. This is the fallback for an instance
/// whose Authelia administrators are gone; it is not a way to hand out space access, which
/// stays with teams and path grants.
pub async fn set_instance_admin(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<InstanceAdminFlag>,
) -> Result<Json<InstanceAdminView>, ApiError> {
    let actor = instance_admin(&state, &jar).await?;

    let changed = match state
        .store
        .set_instance_admin_audited(&actor.id, &id, body.admin)
        .await
        .map_err(ApiError::Internal)?
    {
        InstanceAdminOutcome::Changed => true,
        // The promotion already said what was asked. Not an error: unlike a revoked
        // grant, the state afterwards IS what the caller wanted — `instance_admin` below
        // is what tells them whether that state means what they think it does.
        InstanceAdminOutcome::Unchanged => false,
        InstanceAdminOutcome::NoSuchPrincipal => return Err(ApiError::NotFound),
        InstanceAdminOutcome::LastAdmin => return Err(ApiError::Conflict(LAST_ADMIN.into())),
    };

    // Asked of the same function every gate asks, rather than assumed from `body.admin`:
    // the answer for somebody in the `admins` group is different from the answer for
    // somebody who only ever had the promotion row.
    let subject = state
        .store
        .principal_by_id(&id)
        .await
        .map_err(ApiError::Internal)?
        .map(|(principal, _)| principal)
        .ok_or(ApiError::NotFound)?;
    let instance_admin = state
        .store
        .baseline_for(&subject)
        .await
        .map_err(ApiError::Internal)?
        >= Baseline::Admin;

    Ok(Json(InstanceAdminView {
        changed,
        instance_admin,
    }))
}

/// Who could be promoted, most recently active first.
///
/// Instance-wide by the same argument as listing principals: it names every active
/// account. The ordering is the point — this list is read by somebody who is about to
/// hand over, usually because they are losing their own access, and the useful first
/// answer is "the people who have actually been here lately".
pub async fn admin_candidates(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<AdminCandidate>>, ApiError> {
    instance_admin(&state, &jar).await?;
    state
        .store
        .admin_candidates()
        .await
        .map(Json)
        .map_err(ApiError::Internal)
}

// -------------------------------------------------------------------------------------
// Teams.
// -------------------------------------------------------------------------------------

pub async fn list_teams(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<TeamSummary>>, ApiError> {
    instance_admin(&state, &jar).await?;
    state
        .store
        .list_teams()
        .await
        .map(Json)
        .map_err(ApiError::Internal)
}

#[derive(Deserialize)]
pub struct NewTeam {
    slug: String,
    name: String,
}

pub async fn create_team(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<NewTeam>,
) -> Result<(StatusCode, Json<TeamSummary>), ApiError> {
    let actor = instance_admin(&state, &jar).await?;

    let slug = body.slug.trim();
    let name = body.name.trim();
    if slug.is_empty() || name.is_empty() {
        return Err(ApiError::Invalid("slug and name are required".into()));
    }
    if state
        .store
        .list_teams()
        .await
        .map_err(ApiError::Internal)?
        .iter()
        .any(|team| team.slug == slug)
    {
        return Err(ApiError::Conflict("team slug already taken".into()));
    }

    state
        .store
        .create_team_audited(&actor.id, slug, name)
        .await
        .map_err(ApiError::Internal)?;

    Ok((
        StatusCode::CREATED,
        Json(TeamSummary {
            slug: slug.to_string(),
            name: name.to_string(),
            members: Vec::new(),
        }),
    ))
}

#[derive(Deserialize)]
pub struct NewMember {
    principal_id: String,
}

/// Whether the request changed anything.
///
/// Reported rather than implied: adding something that is already there is success with
/// nothing done, and a console that showed "added" either way would be describing an
/// action that did not take place. One field name across every mutating endpoint, so a
/// client can ask the question generically rather than knowing a different key per route.
#[derive(Serialize)]
pub struct Changed {
    changed: bool,
}

pub async fn add_member(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(slug): Path<String>,
    Json(body): Json<NewMember>,
) -> Result<Json<Changed>, ApiError> {
    let actor = instance_admin(&state, &jar).await?;

    match state
        .store
        .add_team_member_audited(&actor.id, &slug, &body.principal_id)
        .await
        .map_err(ApiError::Internal)?
    {
        MembershipOutcome::Changed => Ok(Json(Changed { changed: true })),
        // Already a member: the state is what was asked for.
        MembershipOutcome::Unchanged => Ok(Json(Changed { changed: false })),
        // A slug or an id that names nothing. Answering 200 here is the specific lie this
        // endpoint exists to avoid: an administrator would read "added to team" while the
        // person gained nothing at all.
        MembershipOutcome::NoSuchTeam | MembershipOutcome::NoSuchPrincipal => {
            Err(ApiError::NotFound)
        }
    }
}

pub async fn remove_member(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((slug, id)): Path<(String, String)>,
) -> Result<Json<Changed>, ApiError> {
    let actor = instance_admin(&state, &jar).await?;

    match state
        .store
        .remove_team_member_audited(&actor.id, &slug, &id)
        .await
        .map_err(ApiError::Internal)?
    {
        MembershipOutcome::Changed => Ok(Json(Changed { changed: true })),
        // A removal that removed nothing is NOT reported as done. Unlike an add, the
        // state afterwards is not what the caller asked for — the person may still have
        // the access the administrator believes they just withdrew.
        MembershipOutcome::Unchanged
        | MembershipOutcome::NoSuchTeam
        | MembershipOutcome::NoSuchPrincipal => Err(ApiError::NotFound),
    }
}

// -------------------------------------------------------------------------------------
// Access control.
// -------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AclQuery {
    path: Option<String>,
}

/// What applies at a path, and where it comes from.
#[derive(Serialize)]
pub struct AclView {
    path: String,
    /// The ancestor the effective grants are written on. `None` means no ancestor carries
    /// any, so reach here is whatever the baseline confers and nothing more.
    inherited_from: Option<String>,
    /// The grants in force. The nearest ancestor with any wins outright — they do not
    /// union up the tree, which is what makes narrowing a subtree possible.
    effective: Vec<Grant>,
    /// The grants written on this path itself. What a revoke here can actually operate
    /// on; everything else in `effective` has to be changed where it lives.
    defined_here: Vec<Grant>,
    /// The nearest ancestor ABOVE this path that carries grants, and what they are.
    ///
    /// What would apply here if every grant on this path were removed — which is what
    /// revoking the last one does, here and across every page below that carries nothing
    /// of its own. `inherited_from` cannot answer it: a path is its own first ancestor,
    /// so it names this path as soon as this path holds a single row.
    ancestor_source: Option<String>,
    ancestor_grants: Vec<Grant>,
}

#[derive(Serialize)]
pub struct AclIndexEntry {
    path: String,
    grants: i64,
}

#[derive(Serialize)]
pub struct AclIndex {
    paths: Vec<AclIndexEntry>,
}

/// Either one path's grants, or — with no `path` — the index of every path that carries
/// one.
///
/// The index is instance-wide on purpose: it names subtrees the caller may administer
/// none of, and the existence of a space is itself something not everybody may learn.
pub async fn read_acl(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<AclQuery>,
) -> Result<axum::response::Response, ApiError> {
    use axum::response::IntoResponse;

    let Some(path) = query.path else {
        instance_admin(&state, &jar).await?;
        let paths = state
            .store
            .paths_with_grants()
            .await
            .map_err(ApiError::Internal)?
            .into_iter()
            .map(|(path, grants)| AclIndexEntry { path, grants })
            .collect();
        return Ok(Json(AclIndex { paths }).into_response());
    };

    path_admin(&state, &jar, &path).await?;
    let (inherited_from, effective) = state
        .store
        .effective_grants(&path)
        .await
        .map_err(ApiError::Internal)?;
    let defined_here = state
        .store
        .grants_defined_at(&path)
        .await
        .map_err(ApiError::Internal)?;
    let (ancestor_source, ancestor_grants) = state
        .store
        .grants_above(&path)
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(AclView {
        path,
        inherited_from,
        effective,
        defined_here,
        ancestor_source,
        ancestor_grants,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct GrantRequest {
    path: String,
    subject: Subject,
    permission: Permission,
}

/// Refuse a path a grant could never apply to.
///
/// Inheritance walks path *segments* upward from a document's own path and stops below
/// the root, so a grant written on `raum` (no leading slash), on `/raum/` (trailing slash)
/// or on `/` matches no document at all. Each of those is somebody believing they granted
/// access and getting a 200 for it.
///
/// They all fail closed — nobody gains anything — which is why this is input validation
/// and not a permission rule. It belongs on the write only: a grant already stored on such
/// a path has to remain removable, and validating the revoke the same way would trap it
/// there for ever.
fn validate_grant_path(path: &str) -> Result<(), ApiError> {
    if !path.starts_with('/') {
        return Err(ApiError::Invalid("path must start with `/`".into()));
    }
    if path == "/" {
        return Err(ApiError::Invalid(
            "grants do not inherit from the root; write one on a space or a page".into(),
        ));
    }
    if path.ends_with('/') {
        return Err(ApiError::Invalid("path must not end with `/`".into()));
    }
    Ok(())
}

pub async fn grant(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<GrantRequest>,
) -> Result<Json<Changed>, ApiError> {
    // Authorisation first, validation second. A caller who may not touch this path learns
    // 403 and nothing else — not even that their path was malformed, which is a small
    // thing to leak but a free one to withhold.
    let actor = path_admin(&state, &jar, &body.path).await?;
    validate_grant_path(&body.path)?;

    let changed = state
        .store
        .add_grant_audited(&actor.id, &body.path, &body.subject, body.permission)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(Changed { changed }))
}

pub async fn revoke(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<GrantRequest>,
) -> Result<Json<Changed>, ApiError> {
    // No `validate_grant_path` here, deliberately — see its doc comment. A grant stored on
    // a path that can never match still has to be removable.
    let actor = path_admin(&state, &jar, &body.path).await?;

    // 404 when nothing was removed, which is usually because the grant is inherited
    // rather than defined here. Reporting success would tell an administrator that access
    // has been withdrawn while the ancestor's grant is still in force — see
    // `Store::remove_grant`, whose boolean exists for exactly this.
    if state
        .store
        .remove_grant_audited(&actor.id, &body.path, &body.subject, body.permission)
        .await
        .map_err(ApiError::Internal)?
    {
        Ok(Json(Changed { changed: true }))
    } else {
        Err(ApiError::NotFound)
    }
}

// -------------------------------------------------------------------------------------
// Visibility: how open a page is.
// -------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct VisibilityRequest {
    path: String,
    /// Taken as a string and parsed here rather than deserialised into `Visibility`, so an
    /// unrecognised value is a 400 that NAMES the accepted ones instead of serde's
    /// "unknown variant" — and so that this handler, not the deserialiser, is visibly the
    /// place that refuses it.
    visibility: String,
}

/// Change how open one page is.
///
/// **Who may do this: whoever administers the page's own path** — `path_admin`, the same
/// gate as a grant, and instance admins through it as everywhere. Three things decide
/// that, and the third is the one that settles it:
///
///  1. `read` must never widen anything, and `write` must not either. `path_admin` asks
///     `can()` for `Action::Admin`, which no amount of read or write satisfies — being
///     able to edit a page is not being able to decide who may see it.
///  2. The authority is bounded the way every other path-scoped power in this API is
///     bounded: to a subtree somebody was deliberately given.
///  3. Anybody who passes this gate can ALREADY publish the page to the open internet, by
///     writing `anyone: read` on it through `/api/admin/acl` — `can()` answers an `Anyone`
///     grant before it looks at authentication at all. A stricter gate here would withhold
///     nothing; it would only push the same act onto the mechanism this console shows less
///     clearly, and leave the page's badge still reading "Eingeschränkt" while the world
///     reads it. Two doors to one room, with different locks, is not security.
///
/// This is emphatically NOT a relaxation of `seed --update`, which compares visibility and
/// refuses to change it. That refusal is about a BULK FILE DROP, where a stray
/// `visibility: public` in one of two hundred files publishes a page with nobody watching.
/// This is one path, chosen by one person, with an audit row naming them. The refusal
/// message in `seed.rs` has always pointed here in words; now it points somewhere.
pub async fn set_visibility(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<VisibilityRequest>,
) -> Result<Json<Changed>, ApiError> {
    // Authorisation first, then the value — for the same reason `grant` validates in that
    // order: a caller who may not touch this page learns 403 and nothing else, not even
    // whether a page is there.
    let actor = path_admin(&state, &jar, &body.path).await?;

    // Fail closed on a value this code does not understand — REFUSED, never defaulted.
    // `Visibility::default()` is `Restricted`, which is the safe direction today, and that
    // is exactly why a silent fallback would be the wrong habit: it is one reordering of
    // an enum away from publishing a page nobody asked to publish.
    //
    // The parse itself is `Visibility::from_str`, the same one the store applies when it
    // reads the column back, so nothing written here can later be read as something else.
    // It trims and lowercases, so `"PUBLIC "` is accepted and stored canonically; a
    // second, stricter parse in this handler would be a second definition of what counts
    // as public.
    let Ok(visibility) = Visibility::from_str(&body.visibility) else {
        return Err(ApiError::Invalid(
            "visibility must be one of `public`, `internal`, `restricted`".into(),
        ));
    };

    match state
        .store
        .set_visibility_audited(&actor.id, &body.path, visibility)
        .await
        .map_err(ApiError::Internal)?
    {
        VisibilityOutcome::Changed { .. } => Ok(Json(Changed { changed: true })),
        // Already what was asked for. Reported rather than dressed up as a change: the
        // console disables the control for the current value, so this answer means
        // somebody else got there first — which is worth seeing.
        VisibilityOutcome::Unchanged => Ok(Json(Changed { changed: false })),
        VisibilityOutcome::NoSuchDocument => Err(ApiError::NotFound),
    }
}

// -------------------------------------------------------------------------------------
// Default reach: which group confers what, before any grant.
// -------------------------------------------------------------------------------------

/// One row of the D-M2-1 mapping, as the console reads it.
#[derive(Serialize)]
pub struct GroupRoleView {
    group: String,
    baseline: &'static str,
}

/// The whole group-to-baseline mapping.
///
/// Instance-wide, and instance admins only, by the same argument as the grant index: it
/// describes reach over the WHOLE corpus rather than over any one subtree.
///
/// It exists because the access panel cannot otherwise be honest. A group mapped to
/// `admin` reads every `restricted` document in the wiki with no grant written anywhere,
/// and a table of grants shows no trace of that; a group mapped to `internal` reads every
/// `internal` one. The 0002 migration says this mapping "has to be inspectable in the
/// admin console alongside everything else that decides who sees what" — this is that,
/// and the panel is the reason it is now needed.
///
/// Read-only. Changing the mapping is a separate decision with a separate blast radius —
/// one row here can hand the whole corpus to a group — and it is not this task's.
pub async fn list_roles(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<GroupRoleView>>, ApiError> {
    instance_admin(&state, &jar).await?;
    let roles = state
        .store
        .group_roles()
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(|(group, baseline)| GroupRoleView {
            group,
            baseline: baseline.as_str(),
        })
        .collect();
    Ok(Json(roles))
}

// -------------------------------------------------------------------------------------
// The audit log.
// -------------------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AuditQuery {
    limit: Option<i64>,
}

/// The default page. Enough to answer "what changed recently?" without asking for the
/// whole log by accident.
const DEFAULT_AUDIT_LIMIT: i64 = 50;
const MAX_AUDIT_LIMIT: i64 = 500;

pub async fn read_audit(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<AuditQuery>,
) -> Result<Json<AuditPage>, ApiError> {
    let principal = audit_reader(&state, &jar).await?;

    // Clamped rather than trusted. A limit of zero or a negative one becomes "no LIMIT
    // clause behaviour I would have to reason about", and an enormous one is a way to ask
    // the space-admin scan to walk the whole table on every request.
    let limit = query
        .limit
        .unwrap_or(DEFAULT_AUDIT_LIMIT)
        .clamp(1, MAX_AUDIT_LIMIT);

    state
        .store
        .audit_for(&principal, limit)
        .await
        .map(Json)
        .map_err(ApiError::Internal)
}

// -------------------------------------------------------------------------------------
// Invites.
//
// An invite is a CREDENTIAL: whoever holds the link can create an account and walk into
// whatever the invite carries, with nobody watching. So these three endpoints are the ones
// in this file where getting the gate wrong is not a leak of information but a way in.
//
// WHO MAY CREATE ONE, AND WHY IT IS TWO RULES RATHER THAN ONE
//
// D-M2-2: an invite may grant only into spaces the inviter administers. What that means
// depends on what the invite carries, and the two halves are not the same question:
//
//   * A PATH GRANT is bounded by its path. Whoever holds `Action::Admin` there may write
//     it, exactly as they may write the same grant through `POST /api/admin/acl` — this is
//     the same act, arriving by a different door, and it is gated by the same function.
//   * A TEAM is bounded by nothing. `redaktion` may read a space its own members chose,
//     anywhere in the instance, and it may be granted more tomorrow by somebody else
//     entirely. A space admin who could attach a team would be handing out reach they do
//     not have and cannot see, which is precisely what D-M2-2 exists to stop — so a
//     team-carrying invite is INSTANCE ADMINS ONLY.
//
// Written as a match on what the request carries rather than as a flag, so an invite that
// grows a third kind of grant has to say which gate it belongs behind rather than
// inheriting the weaker one by default.
// -------------------------------------------------------------------------------------

/// What a request asks for. Everything but the username is optional, and what is present
/// decides both which gate applies and whether the invite is legal at all.
#[derive(Deserialize)]
pub struct InviteRequest {
    /// The account the invite will create. Fixed here rather than chosen by the recipient:
    /// the acceptance page asks for a display name and a password, and letting the link be
    /// redeemed as any name at all would let it become a name an ACL already trusts.
    username: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    path: Option<String>,
    /// Required when `path` is given and refused when it is not. Not defaulted to `read`:
    /// a grant is a security decision, and an API that guesses at one is an API that
    /// writes a grant nobody chose.
    #[serde(default)]
    permission: Option<Permission>,
    /// A team slug.
    #[serde(default)]
    team: Option<String>,
}

/// How the link reached the person it is for (D-M2-12).
///
/// A tagged enum with one variant today, and that is the delivery seam rather than a
/// placeholder for it: the console has to branch on `how` from the first version, so
/// adding `Emailed { to }` when SMTP exists is an addition to a contract clients already
/// read rather than a change to one they do not. Nothing about the invite itself differs
/// between the two — it is created, hashed and stored identically — and only the handing
/// over changes, which is exactly what D-M2-12 asks for.
#[derive(Serialize)]
#[serde(tag = "how", rename_all = "kebab-case")]
pub enum Delivery {
    /// Nobody sent it. The console shows the link once and the inviter passes it on by
    /// whatever channel they like.
    ShownOnce {
        /// **Origin-relative**, and deliberately so. There is no configured public base
        /// URL, and the one thing this process could derive an origin from is the `Host`
        /// header — which any client can set, so a link built from it could be made to
        /// point at somebody else's server and collect the token when the recipient
        /// clicks. The console is already on the right origin and prepends it.
        url: String,
    },
}

/// An invite, plus the one and only copy of its link.
#[derive(Serialize)]
pub struct CreatedInvite {
    /// Flattened so the created invite and a listed one are the same shape, and a console
    /// can render either with one component.
    #[serde(flatten)]
    invite: InviteSummary,
    delivery: Delivery,
}

/// Hand the link over.
///
/// The whole of the delivery decision, in one place, so that "send it by mail instead"
/// is a branch here rather than a change to the endpoint, the store or the table.
fn deliver(token: &str) -> Delivery {
    Delivery::ShownOnce {
        url: format!("/auth/invite/{token}"),
    }
}

/// Refused for carrying nothing (D-M2-20), and what to do about it.
const NOTHING_GRANTED: &str = "an invite must carry a path grant, a team, or both: one that \
                               carries neither creates an account that can sign in and see \
                               nothing";

pub async fn create_invite(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<InviteRequest>,
) -> Result<(StatusCode, Json<CreatedInvite>), ApiError> {
    let path = body
        .path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    let team = body
        .team
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());

    // Authorisation first, validation second — the rule `grant` follows, for the same
    // reason: a caller who may not do this learns 403 and nothing else, not even that
    // their path was malformed.
    let actor = match (path, team) {
        // Bounded by the path, so gated by the path.
        (Some(path), None) => path_admin(&state, &jar, path).await?,
        // A team reaches wherever it has been granted, so nothing short of instance
        // administration bounds it. This also covers path-AND-team: `instance_admin`
        // already passes `path_admin` on every path, so asking again would decide nothing.
        (_, Some(_)) => instance_admin(&state, &jar).await?,
        // There is no scope to authorise against. Refused (see `NOTHING_GRANTED`), but
        // only after establishing that the caller is somebody — otherwise this endpoint
        // would describe its own request shape to anybody who asked.
        (None, None) => {
            signed_in(&state, &jar).await?;
            return Err(ApiError::Invalid(NOTHING_GRANTED.into()));
        }
    };

    let username = body.username.trim();
    if username.is_empty() {
        return Err(ApiError::Invalid("username is required".into()));
    }
    match (path, body.permission) {
        (Some(path), Some(_)) => validate_grant_path(path)?,
        (Some(_), None) => {
            return Err(ApiError::Invalid(
                "permission is required when a path is given".into(),
            ))
        }
        (None, Some(_)) => {
            return Err(ApiError::Invalid(
                "a permission needs a path to apply to".into(),
            ))
        }
        (None, None) => {}
    }

    // 256 bits from the operating system's CSPRNG — the SAME generator the session cookie
    // uses, because there is no reason for a second one and every reason not to have two
    // places where "random" could quietly become "predictable". The store is handed the
    // SHA-256 and never the token, exactly as the session table is.
    let token = new_session_token();
    let outcome = state
        .store
        .create_invite_audited(
            &actor.id,
            &hash_token(&token),
            &NewInvite {
                username,
                email: body
                    .email
                    .as_deref()
                    .map(str::trim)
                    .filter(|e| !e.is_empty()),
                path,
                permission: body.permission,
                team,
                // D-M2-21. Thirty days, decided in the store next to the table it is
                // written into rather than here.
                ttl_seconds: INVITE_TTL_SECONDS,
            },
        )
        .await
        .map_err(ApiError::Internal)?;

    match outcome {
        CreateInviteOutcome::Created(invite) => Ok((
            StatusCode::CREATED,
            Json(CreatedInvite {
                invite: *invite,
                delivery: deliver(&token),
            }),
        )),
        CreateInviteOutcome::UsernameTaken => {
            Err(ApiError::Conflict("username already taken".into()))
        }
        // A field of the request names nothing, so this is a bad request rather than a
        // missing resource: the invite is what was being created, and it does not exist
        // either way.
        CreateInviteOutcome::NoSuchTeam => Err(ApiError::Invalid("no such team".into())),
        // Unreachable from here — the match above refuses it — and kept because the store
        // is entitled to its own opinion about D-M2-20 and this must not become a 500 if
        // the two ever disagree.
        CreateInviteOutcome::NothingGranted => Err(ApiError::Invalid(NOTHING_GRANTED.into())),
    }
}

/// Outstanding invites, filtered to the ones the caller is entitled to see.
///
/// The gate here establishes only that there IS a caller. **Which** invites they see is
/// decided by [`gw_store::Store::invites_for`], in the retriever, where the architecture
/// rule says it belongs — an instance admin sees everything, a space admin sees the
/// invites into subtrees they administer, and nobody sees a token or its digest, because
/// [`InviteSummary`] has no field to carry one.
pub async fn list_invites(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<InviteSummary>>, ApiError> {
    let principal = signed_in(&state, &jar).await?;
    state
        .store
        .invites_for(&principal)
        .await
        .map(Json)
        .map_err(ApiError::Internal)
}

/// Withdraw an invite that has not been redeemed.
///
/// Gated by the invite's own scope, which means the row has to be read before the gate is
/// known. Two consequences, both deliberate:
///
/// - [`signed_in`] runs first, so "does this id exist?" is not a question an
///   unauthenticated request can ask.
/// - A caller who may not administer this invite's scope gets **404, not 403**. To a space
///   admin, an invite into another space is not there; answering 403 would confirm the id
///   and, with it, that somebody has been invited somewhere.
pub async fn revoke_invite(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<Changed>, ApiError> {
    signed_in(&state, &jar).await?;

    let Some(invite) = state
        .store
        .invite_by_id(&id)
        .await
        .map_err(ApiError::Internal)?
    else {
        return Err(ApiError::NotFound);
    };

    let gate = match invite.path.as_deref() {
        Some(path) => path_admin(&state, &jar, path).await,
        // No subtree, so the same rule that governs creating one: instance admins only.
        None => instance_admin(&state, &jar).await,
    };
    let actor = match gate {
        Ok(actor) => actor,
        Err(ApiError::Forbidden) => return Err(ApiError::NotFound),
        Err(other) => return Err(other),
    };

    match state
        .store
        .revoke_invite_audited(&actor.id, &id)
        .await
        .map_err(ApiError::Internal)?
    {
        RevokeInviteOutcome::Revoked => Ok(Json(Changed { changed: true })),
        // A revoke that revoked nothing is NOT reported as done, for the reason every
        // removal in this file gives: an administrator would conclude that a live link has
        // been withdrawn. An already-spent invite in particular needs the ACCOUNT dealing
        // with, and telling them the link is closed would answer the wrong question.
        RevokeInviteOutcome::NotPending | RevokeInviteOutcome::NoSuchInvite => {
            Err(ApiError::NotFound)
        }
    }
}
