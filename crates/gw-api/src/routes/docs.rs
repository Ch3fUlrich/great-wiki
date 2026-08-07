use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::Json;
use axum_extra::extract::CookieJar;
use gw_auth::Action;
use gw_store::StoredDocument;

/// One document, if the caller may read it.
///
/// The handler makes no authorisation decision of its own. It asks the store for the
/// document *as this principal*, and the store consults the permission engine; there is no
/// unfiltered variant it could reach for by mistake.
pub async fn get_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
) -> Result<Json<StoredDocument>, ApiError> {
    let principal = state.principal(&jar).await;
    // Paths are stored with a leading slash; the route captures without one.
    let full = format!("/{}", path.trim_start_matches('/'));

    // Existence is checked first, so an absent path is 404 and a forbidden one is 403.
    // `document_for` returns `None` for both, deliberately — it is this layer that decides
    // which to reveal. Collapsing both to 404 would hide configuration mistakes behind a
    // status code that says "you spelled it wrong"; collapsing both to 403 would confirm
    // the existence of every path somebody guesses. `document_exists` answers exactly the
    // one bit needed to choose, and nothing about the document itself.
    if !state
        .store
        .document_exists(&full)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }

    state
        .store
        .document_for(&principal, &full, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .map(Json)
        .ok_or(ApiError::Forbidden)
}
