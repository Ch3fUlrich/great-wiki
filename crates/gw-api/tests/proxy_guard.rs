//! The proxy attestation boundary, exercised through the real router.
//!
//! These go through `build_router` rather than calling the layer directly: the property
//! under test is that an unattested request cannot reach a handler, and a layer that is
//! written but never applied would pass any test that called it on its own.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_api::proxy_guard::ProxyGuard;
use gw_store::Store;
use std::sync::Arc;
use tower::ServiceExt;

/// A literal, never a real credential — this repository is public.
const SECRET: &str = "not-a-real-secret";

async fn app(guard: ProxyGuard) -> axum::Router {
    let store = Arc::new(Store::open("sqlite::memory:").await.unwrap());
    gw_api::build_router(gw_api::AppState::for_test_with_guard(store, None, guard))
}

/// `header` is `(name, value)` so each test can choose the spelling on the wire.
async fn get(app: axum::Router, uri: &str, header: Option<(&str, &str)>) -> StatusCode {
    let mut request = Request::builder().uri(uri);
    if let Some((name, value)) = header {
        request = request.header(name, value);
    }
    app.oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn a_loopback_bind_admits_a_request_with_no_header() {
    // Nothing is in front of `just dev` to add the header. Demanding one here would make
    // local development impossible, which is why enforcement follows the bind address.
    assert_eq!(
        get(app(ProxyGuard::disabled()).await, "/api/health", None).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_configured_secret_is_admitted() {
    assert_eq!(
        get(
            app(ProxyGuard::enforcing(SECRET)).await,
            "/api/health",
            Some(("X-GW-Proxy", SECRET))
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_wrong_secret_is_forbidden() {
    assert_eq!(
        get(
            app(ProxyGuard::enforcing(SECRET)).await,
            "/api/health",
            Some(("X-GW-Proxy", "not-the-secret"))
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_missing_header_is_forbidden() {
    // The case that matters in production: someone who found the port on the LAN and
    // spoke to it directly, knowing nothing about the header.
    assert_eq!(
        get(
            app(ProxyGuard::enforcing(SECRET)).await,
            "/api/health",
            None
        )
        .await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn the_lowercase_spelling_caddy_sends_is_admitted() {
    // Regression test for the canonicalisation trap. Caddy writes `X-Gw-Proxy` on the
    // wire and HTTP/2 lowercases names outright, so a case-sensitive match would reject
    // every production request while every test with a hand-written header passed.
    assert_eq!(
        get(
            app(ProxyGuard::enforcing(SECRET)).await,
            "/api/health",
            Some(("x-gw-proxy", SECRET))
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn an_empty_configured_secret_refuses_every_request() {
    // Fail closed. An unset secret must never mean "compare against nothing and admit
    // everyone" — 503 says the boundary is broken rather than quietly removing it.
    assert_eq!(
        get(
            app(ProxyGuard::enforcing("")).await,
            "/api/health",
            Some(("X-GW-Proxy", SECRET))
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn an_unattested_request_is_rejected_before_routing() {
    // 403 rather than 404 on a path that does not exist: the guard ran before the router
    // decided anything, which is the ordering the boundary depends on. A layer applied
    // inside the route table would answer 404 here and leak which paths exist.
    assert_eq!(
        get(
            app(ProxyGuard::enforcing(SECRET)).await,
            "/api/gibt-es-nicht",
            None
        )
        .await,
        StatusCode::FORBIDDEN
    );
}
