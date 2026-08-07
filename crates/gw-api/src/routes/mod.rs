pub mod docs;
pub mod tree;

use crate::identity::Identity;
use axum::routing::get;
use axum::{Json, Router};
use gw_store::Store;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    /// When present, every request is treated as this identity. Only reachable on a
    /// loopback bind — `config::validate` refuses to start otherwise.
    pub dev_identity: Option<Identity>,
}

impl AppState {
    pub fn for_test(store: Arc<Store>, dev_identity: Option<Identity>) -> Self {
        Self {
            store,
            dev_identity,
        }
    }

    /// The caller's identity. M1 has only the development shim and anonymous; Task 6 adds
    /// the OIDC session, and M2 adds local accounts — both by extending this one function.
    pub fn identity(&self) -> Identity {
        self.dev_identity
            .clone()
            .unwrap_or_else(Identity::anonymous)
    }
}

/// Whether `identity` may read something with this visibility.
///
/// M1 knows only the three visibility levels. M2 replaces this with the full permission
/// engine (teams, ACLs, tree inheritance) — and must replace it, not sit beside it.
pub fn may_read(identity: &Identity, visibility: &str) -> bool {
    match visibility {
        "public" => true,
        // Fail closed: an unrecognised value is treated as restricted, never as public.
        _ => identity.is_authenticated(),
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"status": "ok"})) }),
        )
        .route("/api/tree", get(tree::get_tree))
        .route("/api/documents/{*path}", get(docs::get_document))
        .with_state(state)
}
