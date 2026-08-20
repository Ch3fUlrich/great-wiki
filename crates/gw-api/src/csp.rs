//! The Content-Security-Policy for the pages this crate renders ITSELF.
//!
//! # Why there is a second policy at all
//!
//! The wiki's own policy is configured in `web/vite.config.ts` and issued by SvelteKit, per
//! response, with a nonce. It covers exactly the responses SvelteKit renders — and `/auth/*`
//! is not one of them. The internal proxy routes that prefix here rather than to `gw-web`
//! (`docker/Caddyfile`), because the OIDC hand-off has to reach the application and not
//! SvelteKit's router, so the sign-in page and the invitation page are HTML this crate
//! writes and ships with no policy on it at all.
//!
//! Those are the two worst pages in the deployment to leave uncovered:
//!
//! * `auth::page` is a **password form exposed to the public internet**.
//! * `auth::invite` renders `invited_by_name` — **content another account authored** — into
//!   the page somebody is asked to choose a password on. `invite::escape` is what stops
//!   that becoming script today, and its own doc comment says so; this is the layer that
//!   catches the day something else is interpolated there and nobody remembers.
//!
//! # Why it can be so much stricter than the front end's
//!
//! Because these pages have no JavaScript whatsoever. `default-src 'none'` therefore denies
//! script outright — not "only from this origin", not "only with a nonce", but no script,
//! from anywhere, by any means, including an inline event handler. Nothing else in this
//! deployment can say that.
//!
//! The single loosening is `style-src 'unsafe-inline'`, forced by both pages carrying their
//! stylesheet in a `<style>` block. That is deliberate and load-bearing rather than lazy:
//! `auth::invite`'s `STYLE` says outright that a page which must never fetch anything cannot
//! reach across to the front end for a stylesheet — a sign-in page has to render correctly
//! when the rest of the stack is broken, which is when people most need to look at it. A
//! `sha256-` hash would be exact, but the login page builds its stylesheet inside a
//! `format!` literal rather than from a constant, so the hash would have to be maintained by
//! hand against a string it cannot be derived from — and the failure mode of a stale hash is
//! an unstyled password form in production. With no script permitted anywhere on the page,
//! what inline CSS can still do is bounded, and neither page renders authored content into a
//! style.
//!
//! # What it does NOT cover
//!
//! JSON. The layer looks at the response's own content type and attaches nothing to
//! anything that is not `text/html`, so `/api/*` is untouched — a policy on a JSON body
//! protects nothing and would only be one more header to explain.

use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::Response,
};

/// The policy, in one place so the tests below assert the string that is actually sent.
///
/// `frame-ancestors 'self'` matches the `X-Frame-Options: SAMEORIGIN` the edge already sends
/// (`server/network/opnsense/caddy.d/00-snippets.conf`); both are kept, because the edge's
/// copy also covers responses that never reach this layer.
pub const POLICY: &str = "default-src 'none'; \
     style-src 'unsafe-inline'; \
     form-action 'self'; \
     base-uri 'none'; \
     frame-ancestors 'self'";

/// Attach [`POLICY`] to every HTML response this crate produces.
///
/// Applied in `build_router` as the outermost layer, for the same reason the proxy guard is
/// applied last: a layer added after every route wraps the 404 fallback and every error
/// response too, so a page added later is covered before anyone remembers to cover it.
///
/// An existing header is never replaced. Nothing sets one today; the rule exists so that a
/// handler which one day needs its own policy can state it and have it survive.
pub async fn attach(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;

    if !is_html(&response)
        || response
            .headers()
            .contains_key(header::CONTENT_SECURITY_POLICY)
    {
        return response;
    }

    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(POLICY),
    );
    response
}

/// Whether this response carries HTML, judged by the media type alone.
///
/// Case-insensitive and parameter-tolerant: the pages here send
/// `text/html; charset=utf-8`, and a comparison against that exact string would silently
/// stop matching the day somebody drops the charset.
fn is_html(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/html"))
        })
}

#[cfg(test)]
mod tests {
    use super::{attach, POLICY};
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route(
                "/html",
                get(|| async {
                    (
                        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                        "<!doctype html><p>hallo",
                    )
                }),
            )
            .route(
                "/json",
                get(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .route(
                "/own-policy",
                get(|| async {
                    (
                        [
                            (header::CONTENT_TYPE, "text/html"),
                            (header::CONTENT_SECURITY_POLICY, "default-src 'self'"),
                        ],
                        "<!doctype html>",
                    )
                }),
            )
            .layer(axum::middleware::from_fn(attach))
    }

    async fn header_of(path: &str) -> Option<String> {
        let response = app()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .map(|value| value.to_str().unwrap().to_string())
    }

    #[tokio::test]
    async fn an_html_response_carries_the_policy() {
        assert_eq!(header_of("/html").await.as_deref(), Some(POLICY));
    }

    #[tokio::test]
    async fn the_policy_permits_no_script_at_all() {
        // The whole point of this crate's policy being stricter than the front end's. If
        // `default-src` ever stops being `'none'`, or a `script-src` appears, this is the
        // test that has to be argued with.
        assert!(POLICY.contains("default-src 'none'"));
        assert!(!POLICY.contains("script-src"));
        assert!(!POLICY.contains("unsafe-eval"));
        // The one loosening, pinned so that widening it further is a deliberate edit.
        assert_eq!(POLICY.matches("'unsafe-inline'").count(), 1);
        assert!(POLICY.contains("style-src 'unsafe-inline'"));
    }

    #[tokio::test]
    async fn a_json_response_is_left_alone() {
        assert_eq!(header_of("/json").await, None);
    }

    #[tokio::test]
    async fn a_handler_that_states_its_own_policy_keeps_it() {
        assert_eq!(
            header_of("/own-policy").await.as_deref(),
            Some("default-src 'self'")
        );
    }

    #[tokio::test]
    async fn a_fallback_response_is_still_inside_the_outermost_layer() {
        // `app()`'s own 404 answers `text/plain`, which `is_html` correctly refuses
        // regardless of whether this layer ran at all — asserting only its status, as an
        // earlier version of this test did, passes whether or not `.layer()` wraps the
        // fallback. What that earlier version was trying to prove is that the layer being
        // added OUTSIDE `Router::new()` (as `build_router` does) covers routes that do not
        // exist yet, not only the ones registered above it — and the only way to observe
        // that is to give the fallback itself something HTML to answer with, and check
        // that the policy lands on it exactly as it would on a matched route.
        let fallback_app = Router::new()
            .fallback(|| async {
                (
                    StatusCode::NOT_FOUND,
                    [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                    "<!doctype html><p>nicht gefunden",
                )
            })
            .layer(axum::middleware::from_fn(attach));

        let response = fallback_app
            .oneshot(
                Request::builder()
                    .uri("/nichts")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .and_then(|value| value.to_str().ok()),
            Some(POLICY),
            "a route reached only through the fallback must still be inside the layer"
        );
    }
}
