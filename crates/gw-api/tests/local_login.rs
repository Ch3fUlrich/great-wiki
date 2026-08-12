//! The one sign-in page (D-M2-11), the guest password form behind it, and the throttling
//! that form's public reachability requires.
//!
//! The tests that matter here are the negative ones, and each is written so that removing
//! the defence it covers makes it fail. Where a test could pass for a second reason —
//! because the flow would have died at a later check anyway — the fixture is set up so
//! that everything *except* the thing under test is correct.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use gw_api::auth::session::hash_token;
use gw_api::auth::OidcConfig;
use gw_api::proxy_guard::ProxyGuard;
use gw_api::AppState;
use gw_auth::password::{hash_password_at, HashingCost};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store, LOGIN_FAILURE_LIMIT};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

/// The shared secret a request has to carry for the proxy boundary to believe it came
/// through Caddy — and therefore for `X-Forwarded-For` to be worth reading at all.
const PROXY_SECRET: &str = "nicht-das-echte-proxy-geheimnis";

const GOOD_PASSWORD: &str = "ein-ausreichend-langes-passwort";

// -------------------------------------------------------------------------------------
// Fixtures.
// -------------------------------------------------------------------------------------

fn body_block() -> Block {
    serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap()
}

async fn seed_store(store: &Store) {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: "Öffentlich".into(),
                slug: None,
                language: "de".into(),
                visibility: Visibility::Public,
                body: body_block(),
                sort_key: 0,
            },
            None,
        )
        .await
        .unwrap();
}

/// A store holding one local guest account whose password is [`GOOD_PASSWORD`], hashed at
/// `cost`.
///
/// The cost has to be stated because a stored hash carries its own parameters: whatever
/// `create_local_principal` is handed here is what `verify_password` will later do the work
/// of. Everything in this file except the timing test is about *which* answer the form
/// gives rather than how long it takes to give it, so everything except the timing test
/// uses [`HashingCost::CHEAP_FOR_TESTS`].
async fn with_guest_at(cost: HashingCost) -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    seed_store(&store).await;
    store
        .create_local_principal(
            "gast",
            "Gast",
            None,
            &hash_password_at(GOOD_PASSWORD, cost).unwrap(),
        )
        .await
        .unwrap();
    Arc::new(store)
}

async fn with_guest() -> Arc<Store> {
    with_guest_at(HashingCost::CHEAP_FOR_TESTS).await
}

fn app(store: &Arc<Store>) -> Router {
    gw_api::build_router(AppState::for_test(Arc::clone(store), None))
}

/// The same application a server runs, as far as password work is concerned: argon2id at
/// Authelia's parameters, for both the stored hash and the stand-in the unknown-username
/// path verifies against.
fn app_at_production_cost(store: &Arc<Store>) -> Router {
    gw_api::build_router(AppState {
        hashing: HashingCost::PRODUCTION,
        ..AppState::for_test(Arc::clone(store), None)
    })
}

/// An application that enforces proxy attestation, which is the only configuration in
/// which `X-Forwarded-For` is read. Production binds `0.0.0.0`, so this — not the
/// unenforced one — is the shape the throttling actually ships in.
fn app_behind_the_proxy(store: &Arc<Store>) -> Router {
    gw_api::build_router(AppState::for_test_with_guard(
        Arc::clone(store),
        None,
        ProxyGuard::enforcing(PROXY_SECRET),
    ))
}

fn app_with_provider(store: &Arc<Store>, issuer: &str) -> Router {
    let config = OidcConfig {
        issuer: issuer.to_string(),
        client_id: "great-wiki".into(),
        client_secret: "nicht-das-echte-geheimnis".into(),
        redirect_uri: "https://wiki.example.invalid/auth/callback".into(),
    };
    // Struct update rather than a literal, so a new field on `AppState` does not break
    // this file.
    let state = AppState {
        oidc: Some(Arc::new(gw_api::OidcClient::new(config).unwrap())),
        proxy_guard: ProxyGuard::enforcing(PROXY_SECRET),
        ..AppState::for_test(Arc::clone(store), None)
    };
    gw_api::build_router(state)
}

/// A stand-in provider that answers discovery and nothing else. `/auth/oidc` needs only
/// the authorization endpoint to build its redirect, so this is the whole surface the
/// "throttling never touches the homelab path" test depends on.
async fn discovery_only_provider() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let issuer = format!("http://{}", listener.local_addr().unwrap());
    let document = json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks.json"),
    });
    let app = Router::new().route(
        "/.well-known/openid-configuration",
        get(move || async move { Json(document) }),
    );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    issuer
}

// -------------------------------------------------------------------------------------
// A browser, near enough.
// -------------------------------------------------------------------------------------

#[derive(Default, Clone)]
struct Jar(HashMap<String, String>);

impl Jar {
    fn header(&self) -> String {
        self.0
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }

    fn apply(&mut self, response: &Response) {
        for value in response.headers().get_all("set-cookie") {
            let raw = value.to_str().unwrap();
            let (pair, attrs) = raw.split_once(';').unwrap_or((raw, ""));
            let (name, value) = pair.split_once('=').unwrap();
            if attrs.to_ascii_lowercase().contains("max-age=0") {
                self.0.remove(name.trim());
            } else {
                self.0.insert(name.trim().into(), value.trim().into());
            }
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.0.get(name).map(String::as_str)
    }
}

/// One request. `forwarded` becomes `X-Forwarded-For`; `attested` decides whether the
/// proxy header goes with it, which is what decides whether the former is believed.
struct Call<'a> {
    method: &'a str,
    uri: &'a str,
    form: Option<String>,
    forwarded: Option<&'a str>,
    attested: bool,
}

impl<'a> Call<'a> {
    fn get(uri: &'a str) -> Self {
        Self {
            method: "GET",
            uri,
            form: None,
            forwarded: None,
            attested: false,
        }
    }

    fn post(uri: &'a str, form: String) -> Self {
        Self {
            method: "POST",
            uri,
            form: Some(form),
            forwarded: None,
            attested: false,
        }
    }

    /// Everything a request that genuinely came through Caddy carries: the shared secret
    /// the boundary checks, and the address Caddy observed appended to `X-Forwarded-For`.
    fn through_the_proxy(mut self, forwarded: &'a str) -> Self {
        self.forwarded = Some(forwarded);
        self.attested = true;
        self
    }

    /// A forged `X-Forwarded-For` with no attestation behind it: what any client on the
    /// network can send.
    fn claiming(mut self, forwarded: &'a str) -> Self {
        self.forwarded = Some(forwarded);
        self
    }
}

async fn send(app: &Router, call: Call<'_>, jar: &mut Jar) -> Response {
    let mut builder = Request::builder().method(call.method).uri(call.uri);
    if !jar.0.is_empty() {
        builder = builder.header("cookie", jar.header());
    }
    if let Some(forwarded) = call.forwarded {
        builder = builder.header("x-forwarded-for", forwarded);
    }
    if call.attested {
        builder = builder.header("x-gw-proxy", PROXY_SECRET);
    }
    let request = match call.form {
        Some(body) => builder
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };

    let response = app.clone().oneshot(request).await.unwrap();
    jar.apply(&response);
    response
}

async fn text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap()
}

/// Fetch the sign-in page so the browser holds a CSRF token, and return that token.
async fn open_the_page(app: &Router, jar: &mut Jar, attested: bool) -> String {
    let call = if attested {
        Call::get("/auth/login").through_the_proxy("203.0.113.1")
    } else {
        Call::get("/auth/login")
    };
    send(app, call, jar).await;
    jar.get("__Host-gw_login_csrf")
        .expect("the sign-in page must issue a CSRF token")
        .to_string()
}

fn form(username: &str, password: &str, csrf: &str) -> String {
    let encode = |s: &str| {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    (b as char).to_string()
                }
                b' ' => "+".to_string(),
                other => format!("%{other:02X}"),
            })
            .collect::<String>()
    };
    format!(
        "username={}&password={}&csrf={}",
        encode(username),
        encode(password),
        encode(csrf)
    )
}

// -------------------------------------------------------------------------------------
// The page itself.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_sign_in_page_offers_both_mechanisms() {
    // D-M2-11: one control, both ways in behind it. `/auth/login` no longer 302s.
    let store = with_guest().await;
    let issuer = discovery_only_provider().await;
    let app = app_with_provider(&store, &issuer);
    let mut jar = Jar::default();

    let response = send(
        &app,
        Call::get("/auth/login").through_the_proxy("203.0.113.1"),
        &mut jar,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the login page is a page"
    );
    let page = text(response).await;

    assert!(page.contains("lang=\"de\""), "{page}");
    assert!(page.contains("Mit Homelab-Konto anmelden"), "{page}");
    assert!(page.contains("href=\"/auth/oidc\""), "{page}");
    assert!(page.contains("action=\"/auth/local\""), "{page}");
    assert!(page.contains("method=\"post\""), "{page}");
    assert!(page.contains("name=\"username\""), "{page}");
    assert!(page.contains("name=\"password\""), "{page}");
    assert!(page.contains("Als Gast anmelden"), "{page}");
}

#[tokio::test]
async fn the_homelab_button_is_absent_rather_than_broken_when_no_provider_is_configured() {
    // The same precedent `/api/me`'s `login_available` set: offering a button that answers
    // 503 is worse than not offering one. The guest form stays, because local accounts
    // work with no identity provider at all.
    let store = with_guest().await;
    let app = app(&store);
    let mut jar = Jar::default();

    let page = text(send(&app, Call::get("/auth/login"), &mut jar).await).await;
    assert!(!page.contains("/auth/oidc"), "{page}");
    assert!(!page.contains("Homelab"), "{page}");
    assert!(page.contains("action=\"/auth/local\""), "{page}");
}

#[tokio::test]
async fn the_page_loads_nothing_from_anywhere_else() {
    // A sign-in page that fetches a font or a script from a third party hands that third
    // party the fact that this person is signing in, and a way to change the form.
    let store = with_guest().await;
    let app = app(&store);
    let page = text(send(&app, Call::get("/auth/login"), &mut Jar::default()).await).await;
    assert!(!page.contains("http://"), "{page}");
    assert!(!page.contains("https://"), "{page}");
    assert!(!page.contains("<script"), "{page}");
}

// -------------------------------------------------------------------------------------
// Signing in.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_correct_password_produces_a_session_the_api_accepts() {
    let store = with_guest().await;
    let app = app(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    let response = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;

    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
        "/"
    );

    let cookie = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_ascii_lowercase())
        .find(|v| v.starts_with("__host-gw_session="))
        .expect("no session cookie");
    assert!(cookie.contains("httponly"), "{cookie}");
    assert!(cookie.contains("secure"), "{cookie}");
    assert!(cookie.contains("samesite=lax"), "{cookie}");
    assert!(cookie.contains("path=/"), "{cookie}");
    assert!(!cookie.contains("domain="), "{cookie}");

    let me = json_body(send(&app, Call::get("/api/me"), &mut jar).await).await;
    assert_eq!(me["authenticated"], json!(true));
    assert_eq!(me["username"], json!("gast"));
    assert_eq!(
        me["source"],
        json!("session"),
        "a guest sign-in is a real session, not a shim"
    );
}

#[tokio::test]
async fn the_session_token_is_stored_hashed_and_the_plaintext_is_nowhere_in_the_database() {
    // On disk rather than in memory so the assertion can be made against the actual rows,
    // with a connection this test opened itself.
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("gw.db").display());
    let store = Arc::new(Store::open(&url).await.unwrap());
    seed_store(&store).await;
    store
        .create_local_principal(
            "gast",
            "Gast",
            None,
            &hash_password_at(GOOD_PASSWORD, HashingCost::CHEAP_FOR_TESTS).unwrap(),
        )
        .await
        .unwrap();

    let app = app(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;
    send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;
    let token = jar
        .get("__Host-gw_session")
        .expect("no session cookie")
        .to_string();

    let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
    let stored: Vec<(String,)> = sqlx::query_as("SELECT token_hash FROM sessions")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored.len(), 1, "exactly one session was created");
    let (row,) = &stored[0];

    assert_eq!(
        row,
        &hash_token(&token),
        "the stored value must be the digest of the cookie"
    );
    assert_ne!(row, &token, "the plaintext token is in the database");
    assert!(
        !row.contains(&token),
        "the plaintext token is embedded in the stored value"
    );

    // And the digest is what resolves — the plaintext is not a second key into the table.
    assert!(store
        .principal_for_session(&hash_token(&token))
        .await
        .unwrap()
        .is_some());
    assert!(store.principal_for_session(&token).await.unwrap().is_none());
}

#[tokio::test]
async fn a_wrong_password_and_an_unknown_username_are_indistinguishable() {
    // The whole reason the message is generic. Anything that differs — the status, a word,
    // the length of the body — turns the form into a list of who has an account here.
    let store = with_guest().await;
    let app = app(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    let wrong = send(
        &app,
        Call::post("/auth/local", form("gast", "das-falsche-passwort", &csrf)),
        &mut jar,
    )
    .await;
    let unknown = send(
        &app,
        Call::post("/auth/local", form("gibt-es-nicht", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;

    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.status(), unknown.status());
    let wrong_body = text(wrong).await;
    let unknown_body = text(unknown).await;
    assert_eq!(
        wrong_body, unknown_body,
        "the two refusals differ, so the form says which usernames exist"
    );
    assert!(
        wrong_body.contains("Anmeldung fehlgeschlagen"),
        "{wrong_body}"
    );
    assert!(
        !wrong_body.contains("gast"),
        "the submitted username must not be echoed: {wrong_body}"
    );
}

#[tokio::test]
async fn an_unknown_username_still_costs_a_password_verification() {
    // Same body, same status, and it must also take the same kind of time. Without a
    // verification against a stand-in hash, an unknown username returns in microseconds
    // and a known one takes argon2's tens of milliseconds — which is the same enumeration
    // oracle, read off a stopwatch instead of the screen.
    //
    // A lower bound only. An upper bound would be flaky on a loaded machine; this one can
    // fail for exactly one reason, which is the check being skipped.
    //
    // The rest of this file hashes at `HashingCost::CHEAP_FOR_TESTS`, because a suite that
    // signs in a few hundred times cannot afford argon2 at Authelia's parameters and
    // almost none of it is about what a hash costs. This test IS about what a hash costs,
    // so it takes the production parameters — for the stored hash *and* for the stand-in,
    // which `AppState` derives from the same `hashing` value. A ten-millisecond floor
    // against eight-kilobyte hashes would be a floor nothing could ever hit, so it would
    // have had to be lowered until it measured nothing; instead the one test that needs
    // the real cost pays for it, once.
    let store = with_guest_at(HashingCost::PRODUCTION).await;
    let app = app_at_production_cost(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    // The stand-in hash is built on first use, so the first unknown-username request pays
    // for a hash as well as a verification. That would satisfy the bound below for the
    // wrong reason; this warms it so what is measured is what an attacker would measure —
    // the steady-state cost of a request for a username that does not exist. (Two failures
    // for one account, well inside `LOGIN_FAILURE_LIMIT`, so nothing here is throttled.)
    send(
        &app,
        Call::post("/auth/local", form("gibt-es-nicht", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;

    let started = std::time::Instant::now();
    send(
        &app,
        Call::post("/auth/local", form("gibt-es-nicht", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;
    let elapsed = started.elapsed();

    assert!(
        elapsed >= std::time::Duration::from_millis(10),
        "an unknown username returned in {elapsed:?}, so no hash was verified"
    );
}

#[tokio::test]
async fn a_deactivated_account_cannot_sign_in_even_with_the_right_password() {
    let store = with_guest().await;
    let (guest, _) = store.principal_by_username("gast").await.unwrap().unwrap();
    store.set_principal_active(&guest.id, false).await.unwrap();

    let app = app(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    let response = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        jar.get("__Host-gw_session").is_none(),
        "a deactivated account was issued a session"
    );
    assert_eq!(
        store.session_count_for(&guest.id).await.unwrap(),
        0,
        "no session row may exist for a deactivated account"
    );
}

#[tokio::test]
async fn an_account_that_signs_in_through_the_provider_has_no_password_to_present() {
    // An OIDC principal has no credential row at all. The form must refuse it rather than
    // comparing against something absent.
    let store = with_guest().await;
    store
        .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
        .await
        .unwrap();

    let app = app(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    for password in ["", GOOD_PASSWORD, "sergej"] {
        let response = send(
            &app,
            Call::post("/auth/local", form("sergej", password, &csrf)),
            &mut jar,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    assert!(jar.get("__Host-gw_session").is_none());
}

#[tokio::test]
async fn a_post_without_the_csrf_token_this_browser_was_issued_is_refused() {
    // Everything else about this attempt is correct — the right username, the right
    // password — so this fails on the CSRF check and on nothing else. Without it, a form
    // on another site can sign somebody into an account they did not choose.
    let store = with_guest().await;
    let app = app(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    for presented in ["", "der-falsche-token", &format!("{csrf}x")] {
        let response = send(
            &app,
            Call::post("/auth/local", form("gast", GOOD_PASSWORD, presented)),
            &mut jar,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "presented `{presented}`"
        );
        assert!(jar.get("__Host-gw_session").is_none());
    }

    // And the same request with the real token succeeds, which is what proves the three
    // refusals above were the CSRF check rather than anything else being wrong.
    let ok = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf)),
        &mut jar,
    )
    .await;
    assert_eq!(ok.status(), StatusCode::SEE_OTHER);
}

// -------------------------------------------------------------------------------------
// Throttling.
// -------------------------------------------------------------------------------------

/// Fail `times` times as `username` from `address`, through the proxy boundary.
async fn fail_from(
    app: &Router,
    jar: &mut Jar,
    csrf: &str,
    username: &str,
    address: &str,
    times: i64,
) {
    for _ in 0..times {
        let response = send(
            app,
            Call::post("/auth/local", form(username, "das-falsche-passwort", csrf))
                .through_the_proxy(address),
            jar,
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "a failure inside the budget must be a plain refusal, not a lockout"
        );
    }
}

#[tokio::test]
async fn ten_failures_for_one_account_refuse_the_eleventh_from_a_fresh_address() {
    // A distributed guess at one account. Counting per address alone would miss it.
    let store = with_guest().await;
    let app = app_behind_the_proxy(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, true).await;

    for n in 0..LOGIN_FAILURE_LIMIT {
        fail_from(&app, &mut jar, &csrf, "gast", &format!("203.0.113.{n}"), 1).await;
    }

    // The right password, from an address that has never failed once.
    let response = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf))
            .through_the_proxy("198.51.100.42"),
        &mut jar,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        jar.get("__Host-gw_session").is_none(),
        "a locked account signed in anyway"
    );
}

#[tokio::test]
async fn ten_failures_from_one_address_refuse_the_eleventh_for_a_fresh_account() {
    // A spray across accounts from one place. Counting per account alone would miss it.
    let store = with_guest().await;
    let app = app_behind_the_proxy(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, true).await;

    for n in 0..LOGIN_FAILURE_LIMIT {
        fail_from(
            &app,
            &mut jar,
            &csrf,
            &format!("konto{n}"),
            "203.0.113.7",
            1,
        )
        .await;
    }

    // `gast` has never failed, and this is its real password.
    let response = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf))
            .through_the_proxy("203.0.113.7"),
        &mut jar,
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(jar.get("__Host-gw_session").is_none());
}

#[tokio::test]
async fn a_successful_sign_in_clears_that_accounts_counter() {
    let store = with_guest().await;
    let app = app_behind_the_proxy(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, true).await;

    fail_from(
        &app,
        &mut jar,
        &csrf,
        "gast",
        "203.0.113.7",
        LOGIN_FAILURE_LIMIT - 1,
    )
    .await;
    let ok = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf))
            .through_the_proxy("203.0.113.7"),
        &mut jar,
    )
    .await;
    assert_eq!(ok.status(), StatusCode::SEE_OTHER);

    // Nine more failures would trip the limit if the counter had merely been left alone.
    fail_from(
        &app,
        &mut jar,
        &csrf,
        "gast",
        "198.51.100.9",
        LOGIN_FAILURE_LIMIT - 1,
    )
    .await;
    assert_eq!(
        store
            .login_failures(gw_store::LoginScope::Account, "gast")
            .await
            .unwrap(),
        LOGIN_FAILURE_LIMIT - 1,
        "the counter resumed from where it was rather than restarting"
    );
}

#[tokio::test]
async fn throttling_never_reaches_the_homelab_path() {
    // Somebody guessing at guest passwords must not be able to stop the owner signing in
    // with their homelab account. The lock is comprehensively tripped first — the same
    // account name AND the same address — and both the sign-in page and the redirect out
    // to Authelia must carry on working.
    let store = with_guest().await;
    let issuer = discovery_only_provider().await;
    let app = app_with_provider(&store, &issuer);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, true).await;

    fail_from(
        &app,
        &mut jar,
        &csrf,
        "gast",
        "203.0.113.1",
        LOGIN_FAILURE_LIMIT,
    )
    .await;
    let locked = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf))
            .through_the_proxy("203.0.113.1"),
        &mut jar,
    )
    .await;
    assert_eq!(
        locked.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the fixture is wrong: nothing is actually locked"
    );

    let redirect = send(
        &app,
        Call::get("/auth/oidc").through_the_proxy("203.0.113.1"),
        &mut jar,
    )
    .await;
    assert_eq!(
        redirect.status(),
        StatusCode::FOUND,
        "a locked-out guest form blocked the homelab sign-in"
    );
    assert!(redirect
        .headers()
        .get("location")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with(&format!("{issuer}/authorize")));

    let page = send(
        &app,
        Call::get("/auth/login").through_the_proxy("203.0.113.1"),
        &mut jar,
    )
    .await;
    assert_eq!(
        page.status(),
        StatusCode::OK,
        "the sign-in page must keep rendering while somebody is locked out of the form"
    );
}

// -------------------------------------------------------------------------------------
// Where the address comes from.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_forwarded_for_header_is_ignored_on_a_request_the_proxy_did_not_attest() {
    // `X-Forwarded-For` is a header any client can set. If it were believed unattested,
    // an attacker would rotate it and the per-address counter would be a decoration.
    let store = with_guest().await;
    let app = app(&store); // not enforcing: no attestation is possible
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, false).await;

    for n in 0..LOGIN_FAILURE_LIMIT {
        let response = send(
            &app,
            Call::post("/auth/local", form(&format!("konto{n}"), "falsch", &csrf))
                .claiming(&format!("203.0.113.{n}")),
            &mut jar,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    let response = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf)).claiming("198.51.100.99"),
        &mut jar,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "a forged X-Forwarded-For bought a fresh budget"
    );
}

#[tokio::test]
async fn only_the_rightmost_forwarded_for_entry_is_counted() {
    // Caddy APPENDS the address it observed, so the last entry is the only one no client
    // controls. Everything to its left arrived from the client and is theirs to invent —
    // reading the leftmost entry, which is the common mistake, would hand an attacker an
    // unlimited supply of addresses.
    let store = with_guest().await;
    let app = app_behind_the_proxy(&store);
    let mut jar = Jar::default();
    let csrf = open_the_page(&app, &mut jar, true).await;

    for n in 0..LOGIN_FAILURE_LIMIT {
        fail_from(
            &app,
            &mut jar,
            &csrf,
            &format!("konto{n}"),
            &format!("10.0.0.{n}, 203.0.113.7"),
            1,
        )
        .await;
    }

    let response = send(
        &app,
        Call::post("/auth/local", form("gast", GOOD_PASSWORD, &csrf))
            .through_the_proxy("10.0.0.250, 203.0.113.7"),
        &mut jar,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "the invented left-hand entry was counted instead of the one Caddy appended"
    );
}
