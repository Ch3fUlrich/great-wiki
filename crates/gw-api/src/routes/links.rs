//! Backlinks over HTTP: which pages point at the one being read.
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
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::Action;
use serde::Serialize;

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

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/links/backlinks/{*path}", get(get_backlinks))
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
