//! Datasets over HTTP (ADR 0029).
//!
//! - `POST /api/datasets` with `{parent, title, slug?}` — a new, empty dataset page.
//!   201 with `{path}`.
//!
//! - `GET|POST|PATCH|PUT|DELETE /api/datasets/schema/{path}` - list, add, relabel, reorder and
//!   delete columns. Reading needs read; every change needs write.
//!
//! - `POST|GET|PATCH|DELETE /api/datasets/rows/{path}` - add a row (`{values}`, 201), read one
//!   (`?id=`), change cells (`{id, version, values}`; a stale `version` answers 409 with
//!   `{error, current}`), delete (`{id}`). Reading needs read; every change needs write.
//!   An unreadable dataset is the same bare 404 as an absent one.
//!
//! The same door as `POST /api/pages` ([`gw_store::Store::create_dataset_for`]): the store
//! decides, this file supplies the `path_admin` gate and the server's date.

use super::admin::path_admin;
use super::templates::{today, Created};
use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_core::FieldKind;
use gw_store::datasets::{DatasetField, DatasetRow, NewField, RowOutcome, SchemaOutcome};
use gw_store::{CreateOutcome, CreateRequest};
use serde::{Deserialize, Serialize};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/datasets", post(create))
        .route(
            "/api/datasets/schema/{*path}",
            get(fields)
                .post(add_field)
                .patch(rename_field)
                .put(reorder_fields)
                .delete(delete_field),
        )
        .route(
            "/api/datasets/rows/{*path}",
            get(get_row)
                .post(add_row)
                .patch(update_row)
                .delete(delete_row),
        )
}

type Values = serde_json::Map<String, serde_json::Value>;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRow {
    pub values: Values,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowPatch {
    pub id: String,
    pub version: i64,
    pub values: Values,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RowId {
    pub id: String,
}

/// Why a row call failed: an ordinary API error, or a stale version, the one answer with a
/// body of its own - the editor needs the row as it is now to show the difference and keep
/// their draft.
pub enum RowFail {
    Api(ApiError),
    Stale(Box<DatasetRow>),
}

impl From<ApiError> for RowFail {
    fn from(e: ApiError) -> Self {
        RowFail::Api(e)
    }
}

impl IntoResponse for RowFail {
    fn into_response(self) -> Response {
        match self {
            RowFail::Api(e) => e.into_response(),
            RowFail::Stale(current) => (
                StatusCode::CONFLICT,
                Json(serde_json::json!({
                    "error": "the row changed since you read it",
                    "current": current,
                })),
            )
                .into_response(),
        }
    }
}

fn row_answer<T>(outcome: RowOutcome<T>, signed_in: bool) -> Result<T, RowFail> {
    let api = |e: ApiError| Err(RowFail::Api(e));
    match outcome {
        RowOutcome::Done(v) => Ok(v),
        RowOutcome::NoDataset | RowOutcome::NoRow => api(ApiError::NotFound),
        RowOutcome::ReadOnly if signed_in => api(ApiError::Forbidden),
        RowOutcome::ReadOnly => api(ApiError::Unauthorized),
        RowOutcome::Invalid(m) => api(ApiError::Invalid(m)),
        RowOutcome::Full(m) => api(ApiError::Conflict(m)),
        RowOutcome::Stale(current) => Err(RowFail::Stale(Box::new(current))),
    }
}

async fn get_row(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Query(q): Query<RowId>,
) -> Result<Json<DatasetRow>, RowFail> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .dataset_row(&principal, &full(&path), &q.id)
        .await
        .map_err(|e| RowFail::Api(ApiError::Internal(e)))?;
    row_answer(out, principal.is_authenticated()).map(Json)
}

async fn add_row(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<NewRow>,
) -> Result<(StatusCode, Json<DatasetRow>), RowFail> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .create_dataset_row(&principal, &full(&path), &body.values)
        .await
        .map_err(|e| RowFail::Api(ApiError::Internal(e)))?;
    row_answer(out, principal.is_authenticated()).map(|r| (StatusCode::CREATED, Json(r)))
}

async fn update_row(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<RowPatch>,
) -> Result<Json<DatasetRow>, RowFail> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .update_dataset_row(
            &principal,
            &full(&path),
            &body.id,
            body.version,
            &body.values,
        )
        .await
        .map_err(|e| RowFail::Api(ApiError::Internal(e)))?;
    row_answer(out, principal.is_authenticated()).map(Json)
}

async fn delete_row(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<RowId>,
) -> Result<StatusCode, RowFail> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .delete_dataset_row(&principal, &full(&path), &body.id)
        .await
        .map_err(|e| RowFail::Api(ApiError::Internal(e)))?;
    row_answer(out, principal.is_authenticated())?;
    Ok(StatusCode::OK)
}

#[derive(Debug, Serialize)]
pub struct Fields {
    pub fields: Vec<DatasetField>,
}

#[derive(Debug, Deserialize)]
pub struct AddField {
    pub key: String,
    pub label: String,
    pub kind: FieldKind,
    #[serde(default = "empty_config")]
    pub config: serde_json::Value,
}

fn empty_config() -> serde_json::Value {
    serde_json::json!({})
}

/// Key and label only: `kind` or any other member is rejected, so neither can be changed.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenameField {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reorder {
    pub keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyOnly {
    pub key: String,
}

fn full(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

/// One place that turns a store verdict into a status. An unreadable dataset and an absent
/// one are both `NoDataset` and leave as the same bare 404 (ADR 0022); a reader without write
/// gets the ordinary 403, and a visitor with no account 401.
fn answer<T>(outcome: SchemaOutcome<T>, signed_in: bool) -> Result<T, ApiError> {
    match outcome {
        SchemaOutcome::Done(v) => Ok(v),
        SchemaOutcome::NoDataset | SchemaOutcome::NoField => Err(ApiError::NotFound),
        SchemaOutcome::ReadOnly if signed_in => Err(ApiError::Forbidden),
        SchemaOutcome::ReadOnly => Err(ApiError::Unauthorized),
        SchemaOutcome::Invalid(m) => Err(ApiError::Invalid(m)),
        SchemaOutcome::Conflict(m) => Err(ApiError::Conflict(m)),
    }
}

async fn fields(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
) -> Result<Json<Fields>, ApiError> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .dataset_fields(&principal, &full(&path))
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(Fields {
        fields: answer(out, principal.is_authenticated())?,
    }))
}

async fn add_field(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<AddField>,
) -> Result<(StatusCode, Json<DatasetField>), ApiError> {
    let principal = state.principal(&jar).await;
    let new = NewField {
        key: body.key,
        label: body.label,
        kind: body.kind,
        config: body.config,
    };
    let out = state
        .store
        .add_dataset_field(&principal, &full(&path), &new)
        .await
        .map_err(ApiError::Internal)?;
    Ok((
        StatusCode::CREATED,
        Json(answer(out, principal.is_authenticated())?),
    ))
}

async fn rename_field(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<RenameField>,
) -> Result<Json<DatasetField>, ApiError> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .rename_dataset_field(&principal, &full(&path), &body.key, &body.label)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(answer(out, principal.is_authenticated())?))
}

async fn reorder_fields(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<Reorder>,
) -> Result<Json<Fields>, ApiError> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .reorder_dataset_fields(&principal, &full(&path), &body.keys)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(Fields {
        fields: answer(out, principal.is_authenticated())?,
    }))
}

async fn delete_field(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(body): Json<KeyOnly>,
) -> Result<StatusCode, ApiError> {
    let principal = state.principal(&jar).await;
    let out = state
        .store
        .delete_dataset_field(&principal, &full(&path), &body.key)
        .await
        .map_err(ApiError::Internal)?;
    answer(out, principal.is_authenticated())?;
    Ok(StatusCode::OK)
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
