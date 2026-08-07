use super::{may_read, AppState};
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::Json;
use gw_store::StoredDocument;

pub async fn get_document(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Json<StoredDocument>, ApiError> {
    let identity = state.identity();
    // Paths are stored with a leading slash; the route captures without one.
    let full = format!("/{}", path.trim_start_matches('/'));

    let doc = state
        .store
        .document_by_path(&full)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;

    if !may_read(&identity, &doc.visibility) {
        return Err(ApiError::Forbidden);
    }
    Ok(Json(doc))
}
