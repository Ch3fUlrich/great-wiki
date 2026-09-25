//! Renaming and moving a page over HTTP, and following an address a page has left.
//!
//! - `GET /api/move/{*path}?parent=…&title=…&slug=…` — the move, measured and not made.
//! - `POST /api/move/{*path}` with `{parent, title, slug}` — the move.
//! - `GET /api/forwards/{*path}` — where the page that used to be at `path` is now.
//!
//! **Its own prefix, not a suffix under `/api/documents`**, for the reason
//! `routes::topics` gives: matchit prefers a literal segment over a catch-all, so
//! `/api/documents/{*path}/move` would be shadowed by a real page slugged `move`.
//!
//! **One gate is decided here, and only one**: whether the caller administers the
//! destination. It is `path_admin` — the gate `set_visibility` and the purge use — asked of
//! the new parent, or of `/` for the top level, and handed to the store, which decides
//! everything else ([`gw_store::Store::move_document`]). A preview asks it too, so the
//! preview can say in advance that a widening move would be refused.

use super::admin::path_admin;
use super::docs::withheld_or_absent;
use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_store::{MoveMode, MoveOutcome, MovePlan, MoveRequest};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/move/{*path}", get(preview_move).post(move_document))
        .route("/api/forwards/{*path}", get(forward))
}

fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// Where the page should go. Query string for the preview, JSON body for the move.
#[derive(Debug, Default, Deserialize)]
pub struct MoveBody {
    /// The new parent. Absent or empty is the top level.
    #[serde(default)]
    pub parent: Option<String>,
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
}

impl MoveBody {
    fn request(self) -> MoveRequest {
        MoveRequest {
            parent: self
                .parent
                .filter(|p| !p.trim().trim_matches('/').is_empty()),
            title: self.title,
            slug: self.slug,
        }
    }
}

/// `{ "path": "/where/it/is/now" }`.
#[derive(Debug, Serialize)]
pub struct ForwardView {
    pub path: String,
}

async fn plan(
    state: &AppState,
    jar: &CookieJar,
    captured: &str,
    body: MoveBody,
    mode: MoveMode,
) -> Result<MovePlan, ApiError> {
    let principal = state.principal(jar).await;
    let path = full_path(captured);
    let request = body.request();
    let destination = request.parent.as_deref().unwrap_or("/").to_string();
    let administers_destination = match path_admin(state, jar, &destination).await {
        Ok(_) => true,
        Err(ApiError::Forbidden) => false,
        Err(other) => return Err(other),
    };

    match state
        .store
        .move_document(&principal, &path, &request, administers_destination, mode)
        .await
        .map_err(ApiError::Internal)?
    {
        MoveOutcome::Planned(plan) => Ok(plan),
        MoveOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
        MoveOutcome::Refused => Err(withheld_or_absent(state, &principal, &path).await),
    }
}

/// The move, measured and rolled back. `refusal` says whether this caller could make it.
pub async fn preview_move(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
    Query(body): Query<MoveBody>,
) -> Result<Json<MovePlan>, ApiError> {
    Ok(Json(
        plan(&state, &jar, &captured, body, MoveMode::Preview).await?,
    ))
}

/// The move. A plan this caller may not carry out is a 409 carrying the reason, and nothing
/// has changed.
pub async fn move_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
    Json(body): Json<MoveBody>,
) -> Result<Json<MovePlan>, ApiError> {
    let plan = plan(&state, &jar, &captured, body, MoveMode::Commit).await?;
    match (&plan.refusal, plan.committed) {
        (Some(reason), _) => Err(ApiError::Conflict(reason.clone())),
        (None, true) => Ok(Json(plan)),
        (None, false) => Err(ApiError::Internal(anyhow::anyhow!(
            "a move with no refusal was not committed"
        ))),
    }
}

/// Where the page that used to live at `path` lives now.
///
/// 404 for everything else — an address nothing left, a page this caller may not read, one
/// in the trash — and all of those are the same bytes, because they are the same
/// `ApiError::NotFound` (ADR 0022). The store's answer already made them one.
pub async fn forward(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<ForwardView>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);
    match state
        .store
        .forward_for(&principal, &path)
        .await
        .map_err(ApiError::Internal)?
    {
        Some(path) => Ok(Json(ForwardView { path })),
        None => Err(ApiError::NotFound),
    }
}
