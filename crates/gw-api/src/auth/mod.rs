//! Signing in, signing out, and answering "who am I?".
//!
//! These routes are deliberately not under `/api`. They are browser navigations — a
//! rendered page, a redirect out to the identity provider, a redirect back, a form POST —
//! and the edge treats them the same way it treats a page, not the way it treats an XHR.
//!
//! D-M2-11 put a page in front of both mechanisms. `/auth/login` used to 302 straight to
//! Authelia; it now renders [`page`], and the redirect moved to `/auth/oidc` behind the
//! homelab button. `/auth/local` is the guest form's target, and the throttling in
//! `gw_store::login_attempts` exists because that form is reachable from a public
//! hostname by anyone.

pub mod breach;
pub mod client_address;
pub mod invite;
pub mod local;
pub mod oidc;
pub mod page;
pub mod password_policy;
pub mod session;

pub use client_address::client_address;
pub use oidc::{OidcClient, OidcConfig};
pub use page::CSRF_COOKIE;
pub use session::SESSION_COOKIE;

use crate::routes::AppState;
use axum::routing::{get, post};
use axum::Router;

pub fn routes() -> Router<AppState> {
    Router::new()
        // The page, not a redirect. One control, both mechanisms behind it.
        .route("/auth/login", get(page::show))
        // What `/auth/login` used to do, now behind the homelab button. It is a separate
        // route rather than a query parameter on the page so that the throttling on the
        // guest form cannot reach it: there is no code path from one to the other.
        .route("/auth/oidc", get(oidc::login))
        // POST, because it takes a password and changes server state — it creates a
        // session row. A GET would put the password in the access log.
        .route("/auth/local", post(local::sign_in))
        .route("/auth/callback", get(oidc::callback))
        // Following an invitation. Under `/auth/` and not `/api/` for the same reason
        // everything else here is: these are browser navigations and a form POST, and the
        // edge routes the whole prefix to this application. The page is reachable without
        // signing in — that is the point of it — and refuses every token it does not
        // recognise with one identical answer.
        .route("/auth/invite/{token}", get(invite::show))
        // POST, because it takes a password and creates an account. A GET would put the
        // password in the access log and let any image tag redeem somebody's invitation.
        .route("/auth/invite/{token}/accept", post(invite::accept))
        // POST, because signing out changes server state — it deletes a row. A GET would
        // also mean any image tag on any page could sign somebody out.
        .route("/auth/logout", post(session::logout))
        .route("/api/me", get(session::me))
}
