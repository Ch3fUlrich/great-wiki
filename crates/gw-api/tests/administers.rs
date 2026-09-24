//! "Does this caller administer anything at all?" — `administers` on `/api/me`, and the
//! sign-in page the console sends everybody else to.
//!
//! The console at `/admin` used to render its whole frame for anybody, anonymous visitors
//! included, and let seven refused endpoints fill it with seven German error sentences.
//! Nothing leaked — every endpoint refused — but the page did not decide up front whether
//! the caller belonged there. It now asks `/api/me` once, before it fetches anything, and
//! this flag is what it asks.
//!
//! The flag is not a second definition of "administrator". It is the audit reader's gate —
//! the one question in the admin API that is already "may this person administer ANY
//! path?" — asked through the same function, so the console and the endpoints behind it
//! cannot disagree. [`the_flag_agrees_with_the_admin_api_for_every_caller`] pins that.
//!
//! The fixture:
//!
//! - `chef` — an `admins` member, so an instance admin by baseline (D-M2-1).
//! - `gast` — a local account with nothing at all.
//! - `leser` — `read` on `/raum`. Reading a space is not administering it.
//! - `lektor` — `admin` on `/raum/tief/unten` and nowhere else: a space admin whose only
//!   grant is one leaf deep in a subtree, so no prefix or top-level shortcut can find it.
//! - `redakteurin` — no grant of her own; a member of team `redaktion`, which holds `admin`
//!   on `/team-raum`. Administering through a team is administering.
//! - `mitglied` — a member of team `lesekreis`, which holds only `read`. The team half of
//!   `leser`, so the team case cannot pass merely because team membership counts for
//!   anything at all.
//! - `ehemalig` — `admin` on `/raum`, but deactivated. A grant on a closed account confers
//!   nothing.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use gw_api::auth::session::hash_token;
use gw_auth::{Permission, Subject};
use gw_store::Store;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();

    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();
    store
        .create_local_principal("gast", "Gast", None, "$argon2id$fake")
        .await
        .unwrap();
    let leser = store
        .create_local_principal("leser", "Leser", None, "$argon2id$fake")
        .await
        .unwrap();
    let lektor = store
        .create_local_principal("lektor", "Lektor", None, "$argon2id$fake")
        .await
        .unwrap();
    let redakteurin = store
        .create_local_principal("redakteurin", "Redakteurin", None, "$argon2id$fake")
        .await
        .unwrap();
    let mitglied = store
        .create_local_principal("mitglied", "Mitglied", None, "$argon2id$fake")
        .await
        .unwrap();
    let ehemalig = store
        .create_local_principal("ehemalig", "Ehemalig", None, "$argon2id$fake")
        .await
        .unwrap();

    store
        .add_grant("/raum", Subject::Principal(leser.id), Permission::Read)
        .await
        .unwrap();
    store
        .add_grant(
            "/raum/tief/unten",
            Subject::Principal(lektor.id),
            Permission::Admin,
        )
        .await
        .unwrap();

    store.create_team("redaktion", "Redaktion").await.unwrap();
    store
        .add_team_member("redaktion", &redakteurin.id)
        .await
        .unwrap();
    store
        .add_grant(
            "/team-raum",
            Subject::Team("redaktion".into()),
            Permission::Admin,
        )
        .await
        .unwrap();

    store.create_team("lesekreis", "Lesekreis").await.unwrap();
    store
        .add_team_member("lesekreis", &mitglied.id)
        .await
        .unwrap();
    store
        .add_grant(
            "/lese-raum",
            Subject::Team("lesekreis".into()),
            Permission::Read,
        )
        .await
        .unwrap();

    store
        .add_grant(
            "/raum",
            Subject::Principal(ehemalig.id.clone()),
            Permission::Admin,
        )
        .await
        .unwrap();
    store
        .set_principal_active(&ehemalig.id, false)
        .await
        .unwrap();

    Arc::new(store)
}

/// A router whose requests arrive as the stored principal called `who`, or anonymously.
async fn router(store: &Arc<Store>, who: Option<&str>) -> axum::Router {
    let state = match who {
        Some(username) => {
            let (principal, _) = store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
            gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
        }
        None => gw_api::AppState::for_test(Arc::clone(store), None),
    };
    gw_api::build_router(state)
}

async fn get(app: axum::Router, uri: &str, cookie: Option<&str>) -> (StatusCode, String) {
    let mut request = Request::builder().method(Method::GET).uri(uri);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8_lossy(&bytes).to_string())
}

async fn me(store: &Arc<Store>, who: Option<&str>) -> Value {
    let (status, body) = get(router(store, who).await, "/api/me", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    serde_json::from_str(&body).unwrap()
}

/// `administers`, asserted to be a real boolean. A missing key reads as `null`, and the
/// console treats that as "no" — so a test comparing against `false` alone would pass
/// against a server that never sends the flag at all.
async fn administers(store: &Arc<Store>, who: Option<&str>) -> bool {
    let me = me(store, who).await;
    me["administers"]
        .as_bool()
        .unwrap_or_else(|| panic!("`administers` must be a boolean on /api/me: {me}"))
}

#[tokio::test]
async fn an_anonymous_caller_administers_nothing() {
    let store = fixture().await;
    assert!(!administers(&store, None).await);
}

#[tokio::test]
async fn an_account_with_no_grants_administers_nothing() {
    let store = fixture().await;
    assert!(!administers(&store, Some("gast")).await);
}

#[tokio::test]
async fn reading_a_space_is_not_administering_it() {
    let store = fixture().await;
    assert!(!administers(&store, Some("leser")).await);
    assert!(
        !administers(&store, Some("mitglied")).await,
        "a team's read grant must not count as administering"
    );
}

#[tokio::test]
async fn an_instance_admin_administers() {
    let store = fixture().await;
    assert!(administers(&store, Some("chef")).await);
}

#[tokio::test]
async fn an_admin_grant_on_one_deep_leaf_is_enough() {
    let store = fixture().await;
    assert!(administers(&store, Some("lektor")).await);
}

#[tokio::test]
async fn an_admin_grant_held_through_a_team_is_enough() {
    let store = fixture().await;
    assert!(administers(&store, Some("redakteurin")).await);
}

#[tokio::test]
async fn a_deactivated_account_administers_nothing_whatever_it_was_granted() {
    let store = fixture().await;
    assert!(!administers(&store, Some("ehemalig")).await);
}

/// The flag is the audit reader's gate, asked through the same function — this is what
/// holds it there. `GET /api/admin/audit` is the one admin endpoint whose gate is exactly
/// "administers any path", so for every caller the two answers must be the same. A second
/// definition written for `/api/me` would drift from it, and this is where that shows.
#[tokio::test]
async fn the_flag_agrees_with_the_admin_api_for_every_caller() {
    let store = fixture().await;
    let callers = [
        None,
        Some("chef"),
        Some("gast"),
        Some("leser"),
        Some("lektor"),
        Some("redakteurin"),
        Some("mitglied"),
        Some("ehemalig"),
    ];
    let mut seen = (false, false);
    for who in callers {
        let flag = administers(&store, who).await;
        let (status, _) = get(router(&store, who).await, "/api/admin/audit", None).await;
        assert_eq!(
            flag,
            status == StatusCode::OK,
            "{who:?}: /api/me says administers={flag}, /api/admin/audit answered {status}"
        );
        if flag {
            seen.0 = true;
        } else {
            seen.1 = true;
        }
    }
    assert!(seen.0 && seen.1, "the fixture must hold both answers");
}

// -------------------------------------------------------------------------------------
// The sign-in page, as the console's redirect lands on it.
// -------------------------------------------------------------------------------------

/// A real session for `username`, as a `Cookie` header value. The dev shim is not used
/// here: the page offers a sign-out only for a session, because a shim has none to end.
async fn session_for(store: &Arc<Store>, username: &str) -> String {
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap();
    let token = format!("sitzung-{username}");
    store
        .create_session(&principal.id, &hash_token(&token), 3600)
        .await
        .unwrap();
    format!("__Host-gw_session={token}")
}

#[tokio::test]
async fn the_sign_in_page_tells_a_signed_in_non_admin_who_they_are_and_how_to_switch() {
    let store = fixture().await;
    let cookie = session_for(&store, "leser").await;
    let app = gw_api::build_router(gw_api::AppState::for_test(Arc::clone(&store), None));

    let (status, page) = get(app, "/auth/login", Some(&cookie)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Angemeldet als"), "{page}");
    assert!(page.contains("Leser"), "the page does not name who: {page}");
    assert!(
        page.contains("Verwaltungsrechte"),
        "the page does not say what the Verwaltung needs: {page}"
    );
    assert!(
        page.contains(r#"<form method="post" action="/auth/logout">"#),
        "no way to switch accounts: {page}"
    );
    // The two ways in are still there: switching accounts means signing in again.
    assert!(page.contains("/auth/local"), "{page}");
}

#[tokio::test]
async fn the_sign_in_page_says_nothing_about_an_account_to_somebody_without_one() {
    let store = fixture().await;
    let app = gw_api::build_router(gw_api::AppState::for_test(Arc::clone(&store), None));

    let (status, page) = get(app, "/auth/login", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!page.contains("Angemeldet als"), "{page}");
    assert!(!page.contains("/auth/logout"), "{page}");
    // And a forged cookie is nobody, not somebody whose name the page repeats.
    let app = gw_api::build_router(gw_api::AppState::for_test(Arc::clone(&store), None));
    let (_, page) = get(app, "/auth/login", Some("__Host-gw_session=erfunden")).await;
    assert!(!page.contains("Angemeldet als"), "{page}");
}

#[tokio::test]
async fn a_display_name_is_text_on_the_sign_in_page_never_markup() {
    let store = fixture().await;
    let principal = store
        .create_local_principal("frech", "<script>alert(1)</script>", None, "$argon2id$fake")
        .await
        .unwrap();
    store
        .create_session(&principal.id, &hash_token("frech-token"), 3600)
        .await
        .unwrap();
    let app = gw_api::build_router(gw_api::AppState::for_test(Arc::clone(&store), None));

    let (_, page) = get(app, "/auth/login", Some("__Host-gw_session=frech-token")).await;
    assert!(!page.contains("<script>alert(1)"), "{page}");
    assert!(page.contains("&lt;script&gt;alert(1)"), "{page}");
}

#[tokio::test]
async fn the_development_shim_is_named_but_offered_no_sign_out_that_would_do_nothing() {
    let store = fixture().await;
    let (status, page) = get(router(&store, Some("leser")).await, "/auth/login", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("Angemeldet als"), "{page}");
    assert!(!page.contains("/auth/logout"), "{page}");
}
