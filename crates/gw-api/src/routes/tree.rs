use super::{may_read, AppState};
use crate::error::ApiError;
use axum::extract::State;
use axum::Json;
use gw_store::TreeNode;

/// The navigable tree, filtered to what the caller may read.
///
/// Filtering happens HERE, in the retriever, not in the frontend. A restricted title in
/// the navigation is a disclosure even when the body is protected.
pub async fn get_tree(State(state): State<AppState>) -> Result<Json<Vec<TreeNode>>, ApiError> {
    let identity = state.identity();
    let tree = state.store.tree().await.map_err(ApiError::Internal)?;
    Ok(Json(filter(tree, &identity)))
}

fn filter(nodes: Vec<TreeNode>, identity: &crate::identity::Identity) -> Vec<TreeNode> {
    nodes
        .into_iter()
        .filter(|n| may_read(identity, &n.visibility))
        .map(|mut n| {
            n.children = filter(std::mem::take(&mut n.children), identity);
            n
        })
        .collect()
}
