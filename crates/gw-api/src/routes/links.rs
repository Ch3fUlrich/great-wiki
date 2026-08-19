//! Links over HTTP: which pages point at the one being read, and the whole graph of them.
//!
//! **Own prefix, not a suffix under `/api/documents`.** `crates/gw-api/src/routes/collab.rs`
//! documents why: matchit prefers a literal segment over `{*path}`, so a route shaped like
//! `/api/documents/{*path}/backlinks` would be shadowed by a real page whose slug happens to
//! be `backlinks`. `/api/links/backlinks/{*path}` puts the catch-all last instead, exactly
//! as collab's `/api/collab/{*path}` does.
//!
//! **The permission decision is made exactly once, and not here.** `Store::backlinks_for`
//! already gates every candidate on its own visibility (AGENTS.md rule 2) — that property is
//! the store's, and it is mutation-tested there. This handler's own job is smaller: resolve
//! `path` to the id `backlinks_for` actually takes, through `Store::document_for` — the
//! crate's one permission-checked document accessor, the same one `docs::get_document` and
//! `collab::authorise` use — and hand back exactly what the store returns. A second filter
//! applied to the list here would just be a second place for the same property to be wrong.

use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::Action;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One backlink, as the wire contract promises: a path to follow and a title to show.
///
/// Deliberately not `gw_store::Backlink` reused directly — that type also carries the
/// linking document's id, which is an internal identifier with no reason to leave this
/// crate over an endpoint the frontend reads straight off the wire.
#[derive(Debug, Serialize)]
pub struct BacklinkView {
    pub path: String,
    pub title: String,
}

impl From<gw_store::Backlink> for BacklinkView {
    fn from(backlink: gw_store::Backlink) -> Self {
        Self {
            path: backlink.path,
            title: backlink.title,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct BacklinksResponse {
    pub backlinks: Vec<BacklinkView>,
}

/// One page in the graph. `path` is both where it lives and how an edge names it.
#[derive(Debug, Serialize)]
pub struct GraphNodeView {
    pub path: String,
    pub title: String,
}

/// One link, by path at both ends.
///
/// The store's own [`gw_store::GraphEdge`] names its ends by document **id**, which is the
/// right identifier inside the database and the wrong one on the wire — for exactly the
/// reason [`BacklinkView`] drops the same field. A path identifies a document just as
/// uniquely, is already public, and is what the interface has to link to anyway, so
/// translating here saves the frontend a lookup table it would otherwise have to build.
#[derive(Debug, Serialize)]
pub struct GraphEdgeView {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Serialize)]
pub struct GraphResponse {
    pub nodes: Vec<GraphNodeView>,
    pub edges: Vec<GraphEdgeView>,
}

impl From<gw_store::Graph> for GraphResponse {
    fn from(graph: gw_store::Graph) -> Self {
        // Every edge's ends are guaranteed to be in `nodes` — that is the property
        // `Store::graph_for` carries — so an edge whose end is missing from this map cannot
        // happen. It is dropped rather than unwrapped anyway: if that guarantee ever broke,
        // the failure should be a missing line on a diagram, not a panic in the API, and
        // certainly not an edge naming a document by an id nothing else on the wire uses.
        let paths: HashMap<&str, &str> = graph
            .nodes
            .iter()
            .map(|node| (node.id.as_str(), node.path.as_str()))
            .collect();
        let edges = graph
            .edges
            .iter()
            .filter_map(|edge| {
                Some(GraphEdgeView {
                    from: paths.get(edge.from.as_str())?.to_string(),
                    to: paths.get(edge.to.as_str())?.to_string(),
                })
            })
            .collect();
        Self {
            nodes: graph
                .nodes
                .iter()
                .map(|node| GraphNodeView {
                    path: node.path.clone(),
                    title: node.title.clone(),
                })
                .collect(),
            edges,
        }
    }
}

/// `?root=/darm` — the subtree to draw. Absent means the whole wiki.
#[derive(Debug, Deserialize)]
pub struct GraphQuery {
    pub root: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/links/backlinks/{*path}", get(get_backlinks))
        // A literal segment beside `backlinks/{*path}`, so matchit has nothing to prefer:
        // the two never overlap. Registered here rather than in `mod.rs` so that everything
        // reading the `links` table is served from one module.
        .route("/api/links/graph", get(get_graph))
}

/// Paths are stored with a leading slash; the route captures without one.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// Which pages link to the page at `path`, filtered to what the caller may read.
///
/// Existence is checked before permission, exactly as `docs::get_document` and
/// `collab::authorise` do it: an absent path is 404 and a forbidden one is 403, because
/// collapsing either into the other either hides a configuration mistake or confirms the
/// existence of every path somebody guesses.
pub async fn get_backlinks(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<BacklinksResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    if !state
        .store
        .document_exists(&path)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }

    let document = state
        .store
        .document_for(&principal, &path, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    let backlinks = state
        .store
        .backlinks_for(&principal, &document.id)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(BacklinkView::from)
        .collect();

    Ok(Json(BacklinksResponse { backlinks }))
}

/// The pages the caller may read and the links between them, optionally narrowed to a
/// subtree.
///
/// **There is no 403 here and no 404, and both absences are deliberate.** This endpoint asks
/// about no particular page, so there is no page whose existence a status code could confirm
/// — every caller gets 200 and the graph they are entitled to, which for somebody entitled
/// to nothing is `{"nodes": [], "edges": []}`. That is the same answer an instance with no
/// links at all gives, and the conflation is the point: a 403 on `?root=/geheim` would say
/// `/geheim` exists, which is precisely what the graph is filtered to avoid saying.
///
/// The filtering itself is `Store::graph_for`'s and is not repeated here — same division as
/// [`get_backlinks`] above, and the same reason: a second filter in the handler is a second
/// place for the property to be wrong. It is mutation-tested in `gw-store`.
pub async fn get_graph(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<GraphQuery>,
) -> Result<Json<GraphResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let graph = state
        .store
        .graph_for(&principal, query.root.as_deref())
        .await
        .map_err(ApiError::Internal)?;
    Ok(Json(graph.into()))
}
