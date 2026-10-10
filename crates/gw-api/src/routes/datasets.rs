//! Datasets over HTTP (ADR 0029).
//!
//! - `POST /api/datasets` with `{parent, title, slug?}` — a new, empty dataset page.
//!   201 with `{path}`.
//!
//! The same door as `POST /api/pages` ([`gw_store::Store::create_dataset_for`]): the store
//! decides, this file supplies the `path_admin` gate and the server's date.

use super::admin::path_admin;
use super::templates::{today, Created};
use super::AppState;
use crate::error::ApiError;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_store::{CreateOutcome, CreateRequest};
use serde::Deserialize;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/datasets", post(create))
}

#[derive(Debug, Deserialize)]
pub struct CreateDataset {
    #[serde(default)]
    pub parent: Option<String>,
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<CreateDataset>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let principal = state.principal(&jar).await;
    let request = CreateRequest {
        parent: body
            .parent
            .filter(|p| !p.trim().trim_matches('/').is_empty()),
        title: body.title,
        slug: body.slug,
        template: None,
    };
    let destination = request.parent.as_deref().unwrap_or("/").to_string();
    let administers_destination = match path_admin(&state, &jar, &destination).await {
        Ok(_) => true,
        Err(ApiError::Forbidden) => false,
        Err(other) => return Err(other),
    };
    match state
        .store
        .create_dataset_for(&principal, &request, administers_destination, &today())
        .await
        .map_err(ApiError::Internal)?
    {
        CreateOutcome::Created { path, .. } => Ok((StatusCode::CREATED, Json(Created { path }))),
        CreateOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
        CreateOutcome::Refused => Err(ApiError::Unauthorized),
    }
}
