//! Page templates and creating a page (ADR 0028).
//!
//! - `GET /api/templates` — the templates this caller may read.
//! - `POST /api/pages` with `{parent, title, slug?, template?}` — a new page, optionally a
//!   copy of a template. 201 with `{path}`.
//!
//! Every decision is the store's ([`gw_store::Store::create_page_for`]); this file adds the
//! one gate the store takes as input, `path_admin` on the destination (as `routes::moves`
//! does), and the server's date for `{{datum}}`.

use super::admin::path_admin;
use super::AppState;
use crate::error::ApiError;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_store::{CreateOutcome, CreateRequest, TemplateEntry};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/templates", get(list))
        .route("/api/pages", post(create))
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    #[serde(default)]
    pub parent: Option<String>,
    pub title: String,
    #[serde(default)]
    pub slug: Option<String>,
    /// Blank is no template.
    #[serde(default)]
    pub template: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Created {
    pub path: String,
}

/// TT.MM.JJJJ, UTC — the server's day, the same for everyone who creates a page today.
fn today() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!("{:02}.{:02}.{}", t.day(), u8::from(t.month()), t.year())
}

pub async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<TemplateEntry>>, ApiError> {
    let principal = state.principal(&jar).await;
    Ok(Json(
        state
            .store
            .templates_for(&principal)
            .await
            .map_err(ApiError::Internal)?,
    ))
}

pub async fn create(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<CreateBody>,
) -> Result<(StatusCode, Json<Created>), ApiError> {
    let principal = state.principal(&jar).await;
    let request = CreateRequest {
        parent: body
            .parent
            .filter(|p| !p.trim().trim_matches('/').is_empty()),
        title: body.title,
        slug: body.slug,
        template: body.template.filter(|t| !t.trim().is_empty()),
    };
    let destination = request.parent.as_deref().unwrap_or("/").to_string();
    let administers_destination = match path_admin(&state, &jar, &destination).await {
        Ok(_) => true,
        Err(ApiError::Forbidden) => false,
        Err(other) => return Err(other),
    };
    match state
        .store
        .create_page_for(&principal, &request, administers_destination, &today())
        .await
        .map_err(ApiError::Internal)?
    {
        CreateOutcome::Created { path, .. } => Ok((StatusCode::CREATED, Json(Created { path }))),
        CreateOutcome::Blocked(reason) => Err(ApiError::Conflict(reason)),
        CreateOutcome::Refused => Err(ApiError::Unauthorized),
    }
}
