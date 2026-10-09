//! The notification inbox over HTTP: a list to read, a badge count, and two ways to mark
//! things read.
//!
//! # This module makes no permission decision
//!
//! `gw_store::events` records at emit time only that somebody *might* want to hear about
//! something, and asks again at **delivery** whether the reader may still see the page it is
//! about (ADR 0025). Every function used here is that delivery: `notifications_for`,
//! `unread_count_for`, `mark_event_read` and `mark_all_read` all filter through the store's
//! permission-checked accessor. The handlers turn a request into those calls, turn the answer
//! into a status code, and drop the internal identifiers on the way out. A check written here
//! would be a second answer that can disagree with the one that decides.
//!
//! # No field here can count what was hidden
//!
//! The list is the newest `limit` events the caller may still see, and the count is the unread
//! length of that same filtered set. Neither response carries a total, a "hidden" figure or a
//! next-page cursor computed over unfiltered rows: any such number would tell a reader who lost
//! access that something was there. `a_list_response_carries_no_total_or_hidden_field` pins it.
//!
//! # One 404 for "not yours" and "withheld" and "no such id"
//!
//! `POST /api/notifications/{id}/read` answers 404 whenever the store reports `false`, and the
//! store reports `false` identically for an id that belongs to someone else, an id whose page
//! the caller may no longer read, and an id that does not exist (ADR 0022). An anonymous caller
//! is the exception and is told 401: they have no inbox, and that says nothing about any row.
//!
//! Routes live under their own prefix with literal segments, so nothing here shadows
//! `GET /api/documents/{*path}`.

use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::Principal;
use gw_store::events::{EventKind, Notification};
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

/// The page an event is about, as the caller may see it.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationPageView {
    pub path: String,
    pub title: Option<String>,
}

/// One notification. Only the event's own id leaves; the document, actor and recipient ids
/// stay inside.
#[derive(Debug, Clone, Serialize)]
pub struct NotificationView {
    pub id: String,
    pub kind: EventKind,
    pub created_at: String,
    pub read: bool,
    pub actor_name: Option<String>,
    pub page: NotificationPageView,
}

impl From<&Notification> for NotificationView {
    fn from(n: &Notification) -> Self {
        Self {
            id: n.id.clone(),
            kind: n.kind,
            created_at: n.created_at.clone(),
            read: n.read,
            actor_name: n.actor_name.clone(),
            page: NotificationPageView {
                path: n.page.path.clone(),
                title: n.page.title.clone(),
            },
        }
    }
}

/// Deliberately a single field: see the module header on counting what was hidden.
#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub notifications: Vec<NotificationView>,
}

#[derive(Debug, Serialize)]
pub struct CountResponse {
    pub count: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/notifications", get(list))
        .route("/api/notifications/unread-count", get(unread_count))
        .route("/api/notifications/read-all", post(read_all))
        .route("/api/notifications/{id}/read", post(read_one))
}

async fn caller(state: &AppState, jar: &CookieJar) -> Result<Principal, ApiError> {
    let principal = state.principal(jar).await;
    if principal.is_authenticated() {
        Ok(principal)
    } else {
        Err(ApiError::Unauthorized)
    }
}

async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<ListQuery>,
) -> Result<Json<ListResponse>, ApiError> {
    let principal = caller(&state, &jar).await?;
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let found = state
        .store
        .notifications_for(&principal, limit)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(ListResponse {
        notifications: found.iter().map(NotificationView::from).collect(),
    }))
}

async fn unread_count(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<CountResponse>, ApiError> {
    let principal = caller(&state, &jar).await?;
    let count = state
        .store
        .unread_count_for(&principal)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(CountResponse { count }))
}

async fn read_one(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let principal = caller(&state, &jar).await?;
    let marked = state
        .store
        .mark_event_read(&principal, &id)
        .await
        .map_err(ApiError::Internal)?;
    if marked {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

async fn read_all(State(state): State<AppState>, jar: CookieJar) -> Result<StatusCode, ApiError> {
    let principal = caller(&state, &jar).await?;
    state
        .store
        .mark_all_read(&principal)
        .await
        .map_err(ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}
