//! Signing in, signing out, and answering "who am I?".
//!
//! These routes are deliberately not under `/api`. They are browser navigations — a
//! redirect out to the identity provider and a redirect back — and the edge treats them
//! the same way it treats a page, not the way it treats an XHR.

pub mod oidc;
pub mod session;

pub use oidc::{OidcClient, OidcConfig};
pub use session::SESSION_COOKIE;

use crate::routes::AppState;
use axum::routing::{get, post};
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(oidc::login))
        .route("/auth/callback", get(oidc::callback))
        // POST, because signing out changes server state — it deletes a row. A GET would
        // also mean any image tag on any page could sign somebody out.
        .route("/auth/logout", post(session::logout))
        .route("/api/me", get(session::me))
}
