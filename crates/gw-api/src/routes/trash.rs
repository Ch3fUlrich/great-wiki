//! The Papierkorb over HTTP (D-14): deleting a page, listing what is in the trash, putting
//! one back, and the two-step purge.
//!
//! # Two gates, and the difference between them is the decision
//!
//! **Deleting is an edit**, so it is decided by `gw_store::Store::trash_document` — write on
//! the page, through the accessor every read goes through, per document. No handler here asks
//! a permission question about a delete or a restore, for the reason
//! [`super::topics`]'s header gives: a second filter in a handler is a second place for the
//! property to be wrong, and the one in the handler is always the one that gets it wrong.
//!
//! **Purging is not an edit**, and it is the one operation in this system that loses data.
//! Its gate is [`super::admin::path_admin`] — admin on the page's own path — called here,
//! from this layer, exactly as `set_visibility` calls it. Not a new check with the same
//! shape: the *same function*, so there is one answer to "who administers this page" and this
//! is a caller of it. `docs/decisions/0012-what-a-purge-destroys.md` is why that gate rather
//! than `instance_admin`.
//!
//! The store deliberately makes no purge decision at all, which is [`gw_store::admin`]'s
//! rule for every administrative mutation: a store method with an opinion of its own would be
//! a second rule to disagree with the first.
//!
//! # Confirm and then do, as two requests to one address
//!
//! `GET /api/trash/purge/{path}` describes the purge; `POST` to the same address performs it.
//! The description is not a query written to resemble the destruction — it **is** the
//! destruction, rolled back (ADR 0012), so the numbers an administrator confirms cannot be a
//! different number from the one that happens. The GET is gated exactly as the POST is,
//! because a report that names every page in a subtree is a disclosure whether or not
//! anything is destroyed.
//!
//! # `/api/trash/...` and its own two literal segments
//!
//! `restore` and `purge` each get a literal segment followed by their own catch-all, so
//! matchit never has to choose between them — the arrangement [`super::topics`] uses, and for
//! the reason `super::collab` records: a literal segment beats `{*path}`, so
//! `/api/trash/{*path}/purge` would be shadowed by a page whose slug happens to be `purge`.
//!
//! The delete itself is `DELETE /api/documents/{path}` rather than a fifth route under this
//! prefix. It is the same resource `GET /api/documents/{path}` reads, and the verb already
//! says which operation it is.

use super::admin::path_admin;
use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_store::{Purge, PurgeOutcome, PurgeReport, TrashEntry, TrashOutcome, TrashSummary};
use serde::Serialize;

/// What one delete or one restore moved.
///
/// Declared here rather than serialising `gw_store::TrashSummary`: a field added to the
/// store's own type must not appear on the API by itself, which is the division
/// [`super::topics::TopicView`] and [`super::tasks::ProjectView`] both make.
#[derive(Debug, Serialize)]
pub struct TrashSummaryView {
    pub path: String,
    pub title: String,
    /// Pages that moved, including the named one. A page goes to the trash with everything
    /// under it, so this is how an interface can say "3 Seiten" rather than "erledigt".
    pub pages: usize,
}

impl From<TrashSummary> for TrashSummaryView {
    fn from(summary: TrashSummary) -> Self {
        Self {
            path: summary.path,
            title: summary.title,
            pages: summary.pages,
        }
    }
}

/// One act in the Papierkorb.
#[derive(Debug, Serialize)]
pub struct TrashEntryView {
    pub path: String,
    pub title: String,
    pub deleted_at: String,
    pub deleted_by_name: String,
    /// Pages in this entry **the caller may read**. Never a total, and there is deliberately
    /// no field beside it that could count what was hidden — see ADR 0011.
    pub pages: usize,
    /// Whether this caller may put it back. The store's own verdict, carried rather than
    /// recomputed (ADR 0010).
    pub may_restore: bool,
}

impl From<TrashEntry> for TrashEntryView {
    fn from(entry: TrashEntry) -> Self {
        Self {
            path: entry.path,
            title: entry.title,
            deleted_at: entry.deleted_at,
            deleted_by_name: entry.deleted_by_name,
            pages: entry.pages,
            may_restore: entry.may_restore,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TrashResponse {
    pub entries: Vec<TrashEntryView>,
}

#[derive(Debug, Serialize)]
pub struct PurgedPageView {
    pub path: String,
    pub title: String,
}

/// What a purge destroyed, or would.
///
/// Every number here was measured across the DELETE itself rather than counted beside it, so
/// a preview and the purge it describes cannot report different totals. `committed` says
/// which of the two this was.
#[derive(Debug, Serialize)]
pub struct PurgeReportView {
    pub committed: bool,
    pub pages: Vec<PurgedPageView>,
    pub revisions: i64,
    pub tasks: i64,
    pub projects: i64,
    pub links: i64,
    pub topic_filings: i64,
    pub topics: i64,
}

impl From<PurgeReport> for PurgeReportView {
    fn from(report: PurgeReport) -> Self {
        Self {
            committed: report.committed,
            pages: report
                .pages
                .into_iter()
                .map(|page| PurgedPageView {
                    path: page.path,
                    title: page.title,
                })
                .collect(),
            revisions: report.revisions,
            tasks: report.tasks,
            projects: report.projects,
            links: report.links,
            topic_filings: report.topic_filings,
            topics: report.topics,
        }
    }
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/trash", get(list_trash))
        .route("/api/trash/restore/{*path}", post(restore_document))
        .route(
            "/api/trash/purge/{*path}",
            get(preview_purge).post(purge_document),
        )
}

/// Paths are stored with a leading slash; a route captures without one.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// Move a page and everything under it to the trash. Needs **write** on every page that moves.
///
/// The store decides first and this handler only chooses a status code afterwards — the order
/// [`super::topics::set_document_topics`] uses, and for the same reason: asking first and
/// writing second would be two authorisation decisions on one action, and the second is
/// always the one that gets it wrong. A refused delete changes nothing, so asking afterwards
/// is free.
///
/// **409 and not 403 for a subpage that is not the caller's.** They hold write on the page
/// they named, so "forbidden" would read as "you have lost access here" and send them to an
/// administrator with the wrong question. The refusal says what is actually in the way, which
/// is the shape `LAST_ADMIN` established.
pub async fn delete_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<TrashSummaryView>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    match state
        .store
        .trash_document(&principal, &path)
        .await
        .map_err(ApiError::Internal)?
    {
        TrashOutcome::Done(summary) => Ok(Json(summary.into())),
        TrashOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
        // 404 for an absent page, 403 for one this caller may not delete. The store conflates
        // them; this is where they are told apart, and `document_exists` answers exactly the
        // one bit needed to choose — the same call `super::docs::get_document` makes.
        TrashOutcome::Refused => {
            if !state
                .store
                .document_exists(&path)
                .await
                .map_err(ApiError::Internal)?
            {
                return Err(ApiError::NotFound);
            }
            Err(ApiError::Forbidden)
        }
    }
}

/// The Papierkorb, filtered to what the caller may see.
///
/// **No 403 and no 404**, for the reason [`super::topics::list_topics`] has neither: this
/// endpoint asks about no particular page, so there is no existence a status code could
/// confirm. Somebody entitled to nothing gets `{"entries": []}` — the same answer an empty
/// trash gives, and the conflation is the point.
pub async fn list_trash(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<TrashResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let entries = state
        .store
        .trash_for(&principal)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(TrashResponse { entries }))
}

/// Put a trash entry back. Needs **write** on every page it restores.
pub async fn restore_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<TrashSummaryView>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    match state
        .store
        .restore_document(&principal, &path)
        .await
        .map_err(ApiError::Internal)?
    {
        TrashOutcome::Done(summary) => Ok(Json(summary.into())),
        TrashOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
        // Telling 404 from 403 here is a question about the TRASH, not about the wiki:
        // `document_exists` answers only about live pages and would say "not found" for
        // everything in it. The listing this caller is entitled to is what decides — the same
        // filtered answer `GET /api/trash` gives them, so a page they may not see in the
        // Papierkorb is one whose existence a status code does not confirm either.
        TrashOutcome::Refused => {
            let visible = state
                .store
                .trash_for(&principal)
                .await
                .map_err(ApiError::Internal)?
                .iter()
                .any(|entry| entry.path == path);
            if visible {
                Err(ApiError::Forbidden)
            } else {
                Err(ApiError::NotFound)
            }
        }
    }
}

/// What a purge of this page would destroy. Needs **admin on the page's own path**.
///
/// Gated exactly as the purge is, and deliberately so: the report names every page in the
/// subtree, including ones carrying their own narrower grants, so it discloses precisely what
/// the destruction would. A read-only preview is not a read-only disclosure.
pub async fn preview_purge(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<PurgeReportView>, ApiError> {
    run_purge(state, jar, captured, Purge::Preview).await
}

/// Destroy a trashed page and everything under it. Needs **admin on the page's own path**.
pub async fn purge_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<PurgeReportView>, ApiError> {
    run_purge(state, jar, captured, Purge::Commit).await
}

/// One body for both, so the preview cannot be gated differently from the thing it describes.
async fn run_purge(
    state: AppState,
    jar: CookieJar,
    captured: String,
    mode: Purge,
) -> Result<Json<PurgeReportView>, ApiError> {
    let path = full_path(&captured);
    // The gate FIRST, before anything is read: a 404 for "nothing in the trash there" is an
    // answer about what exists, and it is not owed to somebody who does not administer the
    // path in the first place.
    let actor = path_admin(&state, &jar, &path).await?;

    match state
        .store
        .purge_document(&actor.id, &path, mode)
        .await
        .map_err(ApiError::Internal)?
    {
        PurgeOutcome::Done(report) => Ok(Json(report.into())),
        // Nothing is in the trash there — including the case of a page that is perfectly
        // alive. D-14 makes the trash the only way in, so "delete it first" is the answer and
        // 404 is how this API already says "there is nothing here to act on".
        PurgeOutcome::Refused => Err(ApiError::NotFound),
        PurgeOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
    }
}
