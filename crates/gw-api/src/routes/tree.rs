use super::AppState;
use crate::error::ApiError;
use axum::extract::State;
use axum::Json;
use axum_extra::extract::CookieJar;
use gw_store::TreeNode;

/// The navigable tree, filtered to what the caller may read.
///
/// Filtering happens in the RETRIEVER — `tree_for` never builds the branches this
/// principal cannot see — not here as a post-filter and not in the frontend. A restricted
/// title in the navigation is a disclosure even when the body is protected, and the
/// unfiltered tree is not reachable from this crate at all.
pub async fn get_tree(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<Vec<TreeNode>>, ApiError> {
    let principal = state.principal(&jar).await;
    let tree = state
        .store
        .tree_for(&principal)
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(tree))
}
