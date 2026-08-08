//! The OpenID Connect login flow, end to end, against a stand-in provider.
//!
//! The stand-in is a real HTTP server signing real RS256 tokens with a real RSA key that
//! is generated per test run — no private key is committed to this public repository. It
//! exists because the properties worth testing here are exactly the ones a mock cannot
//! reach: that the `code_verifier` the provider receives is the pre-image of the
//! `code_challenge` it was sent, that a token signed by the wrong key is refused, that a
//! nonce which does not match the one this browser was issued is refused.
//!
//! What is NOT covered, and cannot be headlessly: the human half. Nobody types a password
//! into Authelia here, and nothing proves Authelia's own consent screen, its two-factor
//! step or the exact shape of its `groups` claim. Those need a browser.

use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use base64::Engine;
use gw_api::auth::session::hash_token;
use gw_api::auth::OidcConfig;
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{NewDocument, Store};
use jsonwebtoken::jwk::{Jwk, JwkSet};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

// -------------------------------------------------------------------------------------
// The stand-in identity provider.
// -------------------------------------------------------------------------------------

/// One RSA keypair per test process. Generating it is the slow part, and every test wants
/// the same one; `Cargo.toml` optimises `rsa` even in a debug build so this stays well
/// under a second.
fn signing_key() -> &'static (EncodingKey, Jwk) {
    static KEY: OnceLock<(EncodingKey, Jwk)> = OnceLock::new();
    KEY.get_or_init(|| generate_key("main"))
}

/// A second keypair, never published in the stand-in's JWKS. A token signed with it is
/// how "the signature is actually checked" is proven rather than assumed.
fn foreign_key() -> &'static (EncodingKey, Jwk) {
    static KEY: OnceLock<(EncodingKey, Jwk)> = OnceLock::new();
    KEY.get_or_init(|| generate_key("fremd"))
}

fn generate_key(kid: &str) -> (EncodingKey, Jwk) {
    use rsa::pkcs1::EncodeRsaPrivateKey;
    let private = rsa::RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
    let pem = private.to_pkcs1_pem(rsa::pkcs1::LineEnding::LF).unwrap();
    let encoding = EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap();
    let mut jwk = Jwk::from_encoding_key(&encoding, Algorithm::RS256).unwrap();
    jwk.common.key_id = Some(kid.to_string());
    (encoding, jwk)
}

/// What the stand-in should say next. Every field exists because some test needs to bend
/// exactly one of them and leave the rest correct — a test that changed two things at once
/// would not identify which check refused it.
#[derive(Clone)]
struct IdpScript {
    nonce: Option<String>,
    sub: String,
    preferred_username: Option<String>,
    email: Option<String>,
    /// `None` puts no `groups` claim in the id token at all, which is a different thing
    /// from an empty list and sends the code to the userinfo endpoint.
    groups: Option<Vec<String>>,
    userinfo_groups: Option<Vec<String>>,
    userinfo_sub: Option<String>,
    issuer_claim: Option<String>,
    audience_claim: Option<String>,
    /// Seconds from now. Negative produces an already-expired token.
    expires_in: i64,
    sign_with_foreign_key: bool,
}

impl Default for IdpScript {
    fn default() -> Self {
        Self {
            nonce: None,
            sub: "abc-123".into(),
            preferred_username: Some("sergej".into()),
            email: Some("sergej@example.invalid".into()),
            groups: Some(vec!["admins".into()]),
            userinfo_groups: None,
            userinfo_sub: None,
            issuer_claim: None,
            audience_claim: None,
            expires_in: 300,
            sign_with_foreign_key: false,
        }
    }
}

#[derive(Clone, Default)]
struct IdpState {
    /// What this provider genuinely is, as announced by its own discovery document.
    issuer: String,
    script: IdpScript,
    /// The parameters the token endpoint was last called with, so a test can assert on
    /// what the application actually sent rather than on what it meant to send.
    last_token_request: HashMap<String, String>,
    last_authorization_header: Option<String>,
}

#[derive(Clone)]
struct Idp {
    issuer: String,
    state: Arc<Mutex<IdpState>>,
}

impl Idp {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(IdpState {
            issuer: issuer.clone(),
            ..IdpState::default()
        }));

        let app = Router::new()
            .route(
                "/.well-known/openid-configuration",
                get(idp_discovery).with_state(issuer.clone()),
            )
            .route("/jwks.json", get(idp_jwks))
            .route("/authorize", get(|| async { "the consent screen" }))
            .route("/token", post(idp_token).with_state(state.clone()))
            .route("/userinfo", get(idp_userinfo).with_state(state.clone()));

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        Self { issuer, state }
    }

    fn script(&self, edit: impl FnOnce(&mut IdpScript)) {
        edit(&mut self.state.lock().unwrap().script);
    }

    fn last_token_request(&self) -> HashMap<String, String> {
        self.state.lock().unwrap().last_token_request.clone()
    }

    fn last_authorization_header(&self) -> Option<String> {
        self.state.lock().unwrap().last_authorization_header.clone()
    }
}

async fn idp_discovery(State(issuer): State<String>) -> Json<Value> {
    Json(json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks.json"),
        "userinfo_endpoint": format!("{issuer}/userinfo"),
    }))
}

async fn idp_jwks() -> Json<JwkSet> {
    Json(JwkSet {
        keys: vec![signing_key().1.clone()],
    })
}

async fn idp_token(
    State(state): State<Arc<Mutex<IdpState>>>,
    headers: HeaderMap,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    let (script, issuer) = {
        let mut guard = state.lock().unwrap();
        guard.last_token_request = form;
        guard.last_authorization_header = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        (guard.script.clone(), guard.issuer.clone())
    };

    let now = jsonwebtoken::get_current_timestamp() as i64;
    let mut claims = json!({
        "iss": script.issuer_claim.clone().unwrap_or(issuer),
        "aud": script.audience_claim.clone().unwrap_or_else(|| "great-wiki".into()),
        "sub": script.sub,
        "iat": now,
        "exp": now + script.expires_in,
    });
    if let Some(nonce) = &script.nonce {
        claims["nonce"] = json!(nonce);
    }
    if let Some(username) = &script.preferred_username {
        claims["preferred_username"] = json!(username);
        claims["name"] = json!(username);
    }
    if let Some(email) = &script.email {
        claims["email"] = json!(email);
    }
    if let Some(groups) = &script.groups {
        claims["groups"] = json!(groups);
    }

    let (key, jwk) = if script.sign_with_foreign_key {
        foreign_key()
    } else {
        signing_key()
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = jwk.common.key_id.clone();
    let id_token = jsonwebtoken::encode(&header, &claims, key).unwrap();

    Json(json!({
        "access_token": "stand-in-access-token",
        "token_type": "Bearer",
        "expires_in": 300,
        "id_token": id_token,
    }))
}

async fn idp_userinfo(State(state): State<Arc<Mutex<IdpState>>>) -> Json<Value> {
    let script = state.lock().unwrap().script.clone();
    Json(json!({
        "sub": script.userinfo_sub.unwrap_or(script.sub),
        "preferred_username": script.preferred_username,
        "groups": script.userinfo_groups.unwrap_or_default(),
    }))
}

// -------------------------------------------------------------------------------------
// The application under test.
// -------------------------------------------------------------------------------------

fn body_block() -> Block {
    serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap()
}

async fn seed() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    for (title, visibility) in [
        ("Öffentlich", Visibility::Public),
        ("Geheim", Visibility::Restricted),
    ] {
        store
            .insert_document(&NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: None,
                language: "de".into(),
                visibility,
                body: body_block(),
                sort_key: 0,
            })
            .await
            .unwrap();
    }
    Arc::new(store)
}

fn config_for(idp: &Idp) -> OidcConfig {
    OidcConfig {
        issuer: idp.issuer.clone(),
        client_id: "great-wiki".into(),
        client_secret: "nicht-das-echte-geheimnis".into(),
        redirect_uri: "https://wiki.example.invalid/auth/callback".into(),
    }
}

fn app(store: &Arc<Store>, idp: Option<&Idp>) -> Router {
    let state = match idp {
        Some(idp) => gw_api::AppState::for_test_oidc(Arc::clone(store), config_for(idp))
            .expect("oidc client"),
        None => gw_api::AppState::for_test(Arc::clone(store), None),
    };
    gw_api::build_router(state)
}

// -------------------------------------------------------------------------------------
// A browser, near enough: it keeps cookies and follows nothing.
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

    /// Apply `Set-Cookie` the way a browser would, including removal: a cookie sent with
    /// `Max-Age=0` is deleted, which is how the tests observe the flow cookies being
    /// cleared rather than merely overwritten.
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

async fn send(app: &Router, method: &str, uri: &str, jar: &mut Jar) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if !jar.0.is_empty() {
        builder = builder.header("cookie", jar.header());
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    jar.apply(&response);
    response
}

fn location(response: &Response) -> String {
    response
        .headers()
        .get("location")
        .expect("a redirect must carry a Location")
        .to_str()
        .unwrap()
        .to_string()
}

fn set_cookie_for(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with(&format!("{name}=")))
        .unwrap_or_else(|| panic!("no Set-Cookie for `{name}`"))
}

async fn json_body(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

fn query_of(url: &str) -> HashMap<String, String> {
    url::Url::parse(url)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Drive the whole flow: `/auth/login`, then `/auth/callback` with whatever the provider
/// was scripted to return. Returns the callback's response and the browser's cookies.
async fn sign_in(app: &Router, idp: &Idp, jar: &mut Jar) -> Response {
    let start = send(app, "GET", "/auth/login", jar).await;
    assert_eq!(start.status(), StatusCode::FOUND, "login must redirect");

    // The stand-in cannot know the nonce this browser was issued, so the test tells it —
    // which is precisely what an attacker replaying somebody else's token cannot do.
    let nonce = jar
        .get("__Host-gw_oidc_nonce")
        .expect("login must issue a nonce")
        .to_string();
    idp.script(|s| s.nonce = Some(nonce));

    let state = jar.get("__Host-gw_oidc_state").unwrap().to_string();
    send(
        app,
        "GET",
        &format!("/auth/callback?code=abc&state={state}"),
        jar,
    )
    .await
}

// -------------------------------------------------------------------------------------
// The authorization request.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn login_redirects_to_the_provider_with_pkce_and_state() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    let response = send(&app, "GET", "/auth/login", &mut jar).await;
    assert_eq!(response.status(), StatusCode::FOUND);

    let target = location(&response);
    assert!(
        target.starts_with(&format!("{}/authorize", idp.issuer)),
        "{target}"
    );
    let query = query_of(&target);
    assert_eq!(query.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(
        query.get("client_id").map(String::as_str),
        Some("great-wiki")
    );
    assert_eq!(
        query.get("code_challenge_method").map(String::as_str),
        Some("S256"),
        "plain PKCE is no PKCE at all"
    );
    assert!(query.contains_key("code_challenge"));
    assert!(query.contains_key("state"));
    assert!(query.contains_key("nonce"));
    assert!(query
        .get("scope")
        .is_some_and(|s| s.split(' ').any(|part| part == "groups")));
}

#[tokio::test]
async fn the_flow_cookies_are_host_prefixed_httponly_secure_and_lax() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    let response = send(&app, "GET", "/auth/login", &mut jar).await;
    for name in [
        "__Host-gw_oidc_state",
        "__Host-gw_oidc_nonce",
        "__Host-gw_oidc_verifier",
    ] {
        let cookie = set_cookie_for(&response, name);
        let lower = cookie.to_ascii_lowercase();
        assert!(lower.contains("httponly"), "{cookie}");
        assert!(lower.contains("secure"), "{cookie}");
        assert!(lower.contains("samesite=lax"), "{cookie}");
        assert!(lower.contains("path=/"), "{cookie}");
        assert!(
            !lower.contains("domain="),
            "a `__Host-` cookie must carry no Domain: {cookie}"
        );
    }
}

#[tokio::test]
async fn login_without_a_configured_provider_is_unavailable_not_a_crash() {
    let store = seed().await;
    let app = app(&store, None);
    let response = send(&app, "GET", "/auth/login", &mut Jar::default()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// -------------------------------------------------------------------------------------
// The callback: state, nonce, signature, issuer.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_mismatched_state_is_refused() {
    // The CSRF defence for the whole flow. Without it, an attacker can start a login,
    // capture their own authorization code, and have somebody else's browser redeem it —
    // signing that person into the attacker's account without either of them noticing.
    //
    // EVERYTHING ELSE about this attempt is deliberately correct: the provider is scripted
    // with the nonce this browser was actually issued, so the id token verifies and a
    // session would be created. The `state` is the only thing wrong, which is what makes
    // this a test of the `state` check rather than of whatever happens to fail first.
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    send(&app, "GET", "/auth/login", &mut jar).await;
    let nonce = jar.get("__Host-gw_oidc_nonce").unwrap().to_string();
    idp.script(|s| s.nonce = Some(nonce));

    let response = send(
        &app,
        "GET",
        "/auth/callback?code=abc&state=nicht-der-erwartete",
        &mut jar,
    )
    .await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        set_cookie_names(&response)
            .iter()
            .all(|n| n != "__Host-gw_session"),
        "a refused callback must not issue a session"
    );
    assert!(
        store
            .principal_by_username("sergej")
            .await
            .unwrap()
            .is_none(),
        "a refused callback must not create a principal either"
    );
}

#[tokio::test]
async fn a_callback_with_no_cookies_at_all_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));

    let response = send(
        &app,
        "GET",
        "/auth/callback?code=abc&state=irgendwas",
        &mut Jar::default(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_callback_with_no_state_parameter_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    send(&app, "GET", "/auth/login", &mut jar).await;
    let response = send(&app, "GET", "/auth/callback?code=abc", &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_provider_error_response_is_refused_rather_than_reported_as_a_fault() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    send(&app, "GET", "/auth/login", &mut jar).await;
    let state = jar.get("__Host-gw_oidc_state").unwrap().to_string();
    let response = send(
        &app,
        "GET",
        &format!("/auth/callback?error=access_denied&state={state}"),
        &mut jar,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_id_token_carrying_another_browsers_nonce_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    send(&app, "GET", "/auth/login", &mut jar).await;
    idp.script(|s| s.nonce = Some("der-nonce-von-jemand-anderem".into()));
    let state = jar.get("__Host-gw_oidc_state").unwrap().to_string();

    let response = send(
        &app,
        "GET",
        &format!("/auth/callback?code=abc&state={state}"),
        &mut jar,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(jar.get("__Host-gw_session").is_none());
}

#[tokio::test]
async fn an_id_token_with_no_nonce_at_all_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    send(&app, "GET", "/auth/login", &mut jar).await;
    idp.script(|s| s.nonce = None);
    let state = jar.get("__Host-gw_oidc_state").unwrap().to_string();

    let response = send(
        &app,
        "GET",
        &format!("/auth/callback?code=abc&state={state}"),
        &mut jar,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_id_token_signed_by_a_key_the_provider_does_not_publish_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| s.sign_with_foreign_key = true);
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(jar.get("__Host-gw_session").is_none());
}

#[tokio::test]
async fn an_id_token_from_another_issuer_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| s.issuer_claim = Some("https://evil.example.invalid".into()));
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_id_token_issued_to_another_client_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| s.audience_claim = Some("ein-anderer-client".into()));
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_expired_id_token_is_refused() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| s.expires_in = -300);
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_id_token_with_no_username_is_refused_rather_than_signed_in_as_nobody() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| {
        s.preferred_username = None;
        s.groups = Some(vec![]);
    });
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// -------------------------------------------------------------------------------------
// PKCE, and what the token endpoint actually receives.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_verifier_the_provider_receives_is_the_preimage_of_the_challenge_it_was_sent() {
    // PKCE, proven rather than asserted: the challenge went out in the authorization
    // request, the verifier came back in the token request, and S256 relates them. A
    // stolen authorization code is useless without the verifier, which never left this
    // browser.
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    let start = send(&app, "GET", "/auth/login", &mut jar).await;
    let challenge = query_of(&location(&start))
        .get("code_challenge")
        .expect("no challenge")
        .clone();
    let nonce = jar.get("__Host-gw_oidc_nonce").unwrap().to_string();
    idp.script(|s| s.nonce = Some(nonce));
    let state = jar.get("__Host-gw_oidc_state").unwrap().to_string();
    send(
        &app,
        "GET",
        &format!("/auth/callback?code=abc&state={state}"),
        &mut jar,
    )
    .await;

    let sent = idp.last_token_request();
    let verifier = sent
        .get("code_verifier")
        .expect("no code_verifier was sent");
    let digest = <sha2::Sha256 as sha2::Digest>::digest(verifier.as_bytes());
    let recomputed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    assert_eq!(recomputed, challenge);

    assert_eq!(
        sent.get("grant_type").map(String::as_str),
        Some("authorization_code")
    );
    assert_eq!(sent.get("code").map(String::as_str), Some("abc"));
    assert!(
        idp.last_authorization_header()
            .is_some_and(|h| h.starts_with("Basic ")),
        "the client authenticates with client_secret_basic"
    );
    assert!(
        !sent.contains_key("client_secret"),
        "the secret belongs in the Authorization header, not in a logged form body"
    );
}

// -------------------------------------------------------------------------------------
// A successful sign-in.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_successful_sign_in_creates_a_principal_and_a_session() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(location(&response), "/");

    let (principal, _) = store
        .principal_by_username("sergej")
        .await
        .unwrap()
        .expect("the principal must exist after a sign-in");
    assert_eq!(principal.groups, vec!["admins"]);
    assert_eq!(store.session_count_for(&principal.id).await.unwrap(), 1);

    let me = json_body(send(&app, "GET", "/api/me", &mut jar).await).await;
    assert_eq!(me["authenticated"], json!(true));
    assert_eq!(me["username"], json!("sergej"));
    assert_eq!(me["groups"], json!(["admins"]));
    assert_eq!(me["baseline"], json!("admin"));
    assert_eq!(
        me["source"],
        json!("session"),
        "a real sign-in must be reported as a session, so the interface offers a sign-out"
    );

    // And the reach that group implies (D-M2-1), through the session rather than a shim.
    assert_eq!(
        send(&app, "GET", "/api/documents/geheim", &mut jar)
            .await
            .status(),
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_session_cookie_is_host_prefixed_httponly_secure_and_lax() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    let response = sign_in(&app, &idp, &mut jar).await;
    let cookie = set_cookie_for(&response, "__Host-gw_session");
    let lower = cookie.to_ascii_lowercase();
    assert!(lower.contains("httponly"), "{cookie}");
    assert!(lower.contains("secure"), "{cookie}");
    assert!(lower.contains("samesite=lax"), "{cookie}");
    assert!(lower.contains("path=/"), "{cookie}");
    assert!(!lower.contains("domain="), "{cookie}");
}

#[tokio::test]
async fn the_session_token_is_at_least_256_bits_and_is_not_what_is_stored() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    sign_in(&app, &idp, &mut jar).await;
    let token = jar.get("__Host-gw_session").unwrap().to_string();

    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&token)
        .expect("the token is base64url");
    assert!(
        raw.len() >= 32,
        "a session token must carry at least 256 bits of entropy, got {} bytes",
        raw.len()
    );

    // The database holds the digest. The token itself exists only in this cookie.
    assert!(
        store
            .principal_for_session(&hash_token(&token))
            .await
            .unwrap()
            .is_some(),
        "the digest must be what resolves"
    );
    assert!(
        store.principal_for_session(&token).await.unwrap().is_none(),
        "the plaintext token must not be a key into the session table"
    );
}

#[tokio::test]
async fn the_flow_cookies_are_cleared_once_they_have_been_spent() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    sign_in(&app, &idp, &mut jar).await;
    for name in [
        "__Host-gw_oidc_state",
        "__Host-gw_oidc_nonce",
        "__Host-gw_oidc_verifier",
    ] {
        assert!(jar.get(name).is_none(), "`{name}` outlived the flow");
    }
}

#[tokio::test]
async fn groups_are_replaced_on_every_sign_in_never_merged() {
    // Losing a group in Authelia must take effect at the next sign-in. Merging would make
    // removal impossible, which is the whole reason the claim is authoritative.
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));

    idp.script(|s| s.groups = Some(vec!["admins".into(), "users".into()]));
    sign_in(&app, &idp, &mut Jar::default()).await;

    idp.script(|s| s.groups = Some(vec!["users".into()]));
    let mut jar = Jar::default();
    sign_in(&app, &idp, &mut jar).await;

    let (principal, _) = store
        .principal_by_username("sergej")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(principal.groups, vec!["users"]);

    let me = json_body(send(&app, "GET", "/api/me", &mut jar).await).await;
    assert_eq!(me["groups"], json!(["users"]));
    assert_eq!(me["baseline"], json!("internal"));
    assert_eq!(
        send(&app, "GET", "/api/documents/geheim", &mut jar)
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "losing `admins` must lose the reach it conferred"
    );
}

#[tokio::test]
async fn groups_come_from_userinfo_when_the_id_token_omits_them() {
    // Authelia can be configured either way. An absent `groups` claim is not an empty
    // one, and treating it as empty would silently demote everybody.
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| {
        s.groups = None;
        s.userinfo_groups = Some(vec!["admins".into()]);
    });
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FOUND);

    let me = json_body(send(&app, "GET", "/api/me", &mut jar).await).await;
    assert_eq!(me["groups"], json!(["admins"]));
}

#[tokio::test]
async fn a_userinfo_response_for_a_different_subject_is_refused() {
    // Required by the specification, and it is the check that stops one person's profile
    // being grafted onto another person's token.
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    idp.script(|s| {
        s.groups = None;
        s.userinfo_sub = Some("jemand-anderes".into());
    });
    let response = sign_in(&app, &idp, &mut jar).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// -------------------------------------------------------------------------------------
// Sessions: expiry, forgery, sign-out, deactivation.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn no_session_is_anonymous_with_no_groups() {
    let store = seed().await;
    let app = app(&store, None);
    let me = json_body(send(&app, "GET", "/api/me", &mut Jar::default()).await).await;
    assert_eq!(me["authenticated"], json!(false));
    assert_eq!(me["groups"], json!([]));
    assert_eq!(me["username"], Value::Null);
    assert_eq!(me["baseline"], json!("public"));
    assert_eq!(me["source"], json!("anonymous"));
}

#[tokio::test]
async fn the_development_shim_is_reported_as_a_shim_not_as_a_session() {
    // Otherwise the header offers "sign out" against an identity that has no session, the
    // click clears nothing, and the next request arrives as the same person.
    let store = seed().await;
    store
        .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
        .await
        .unwrap();
    let app = gw_api::build_router(gw_api::AppState::for_test(
        Arc::clone(&store),
        Some(gw_api::Identity::dev("sergej", &["admins"])),
    ));

    let me = json_body(send(&app, "GET", "/api/me", &mut Jar::default()).await).await;
    assert_eq!(me["authenticated"], json!(true));
    assert_eq!(me["source"], json!("dev-shim"));
}

#[tokio::test]
async fn a_forged_session_cookie_confers_nothing() {
    // There IS a live session in this store, belonging to an `admins` member. The point
    // is that a *different* token does not reach it: the lookup has to be by the digest of
    // what was presented, not "is there a session at all".
    let store = seed().await;
    let principal = store
        .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
        .await
        .unwrap();
    store
        .create_session(&principal.id, &hash_token("das-echte-token"), 3600)
        .await
        .unwrap();

    let app = app(&store, None);
    let mut jar = Jar::default();
    jar.0.insert("__Host-gw_session".into(), "erfunden".into());

    let me = json_body(send(&app, "GET", "/api/me", &mut jar).await).await;
    assert_eq!(me["authenticated"], json!(false));
    assert_eq!(me["groups"], json!([]));
    assert_eq!(
        send(&app, "GET", "/api/documents/geheim", &mut jar)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_expired_session_is_rejected() {
    let store = seed().await;
    let principal = store
        .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
        .await
        .unwrap();
    let token = "abgelaufenes-token";
    store
        .create_session(&principal.id, &hash_token(token), -60)
        .await
        .unwrap();

    let app = app(&store, None);
    let mut jar = Jar::default();
    jar.0.insert("__Host-gw_session".into(), token.into());

    let me = json_body(send(&app, "GET", "/api/me", &mut jar).await).await;
    assert_eq!(me["authenticated"], json!(false));
    assert_eq!(
        send(&app, "GET", "/api/documents/geheim", &mut jar)
            .await
            .status(),
        StatusCode::FORBIDDEN,
        "an expired session must not keep the reach it once had"
    );
}

#[tokio::test]
async fn signing_out_deletes_the_server_side_session_not_just_the_cookie() {
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    sign_in(&app, &idp, &mut jar).await;
    let token = jar.get("__Host-gw_session").unwrap().to_string();
    let (principal, _) = store
        .principal_by_username("sergej")
        .await
        .unwrap()
        .unwrap();

    let response = send(&app, "POST", "/auth/logout", &mut jar).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert!(jar.get("__Host-gw_session").is_none(), "cookie not cleared");

    assert_eq!(
        store.session_count_for(&principal.id).await.unwrap(),
        0,
        "the row must be gone; a cookie that was copied is not recalled by clearing it"
    );

    // And the copy somebody took before signing out is now worthless.
    let mut stolen = Jar::default();
    stolen.0.insert("__Host-gw_session".into(), token);
    let me = json_body(send(&app, "GET", "/api/me", &mut stolen).await).await;
    assert_eq!(me["authenticated"], json!(false));
}

#[tokio::test]
async fn deactivating_a_principal_ends_its_session_on_the_next_request() {
    // D-M2-7, both halves: the session row is deleted with the flag, and the very next
    // request from that browser is anonymous.
    let idp = Idp::start().await;
    let store = seed().await;
    let app = app(&store, Some(&idp));
    let mut jar = Jar::default();

    sign_in(&app, &idp, &mut jar).await;
    let (principal, _) = store
        .principal_by_username("sergej")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        send(&app, "GET", "/api/documents/geheim", &mut jar)
            .await
            .status(),
        StatusCode::OK
    );

    store
        .set_principal_active(&principal.id, false)
        .await
        .unwrap();

    assert_eq!(store.session_count_for(&principal.id).await.unwrap(), 0);
    let me = json_body(send(&app, "GET", "/api/me", &mut jar).await).await;
    assert_eq!(me["authenticated"], json!(false));
    assert_eq!(
        send(&app, "GET", "/api/documents/geheim", &mut jar)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        send(&app, "GET", "/api/documents/oeffentlich", &mut jar)
            .await
            .status(),
        StatusCode::OK,
        "suspending an account must not make the public site vanish"
    );
}

#[tokio::test]
async fn signing_out_without_a_session_is_not_an_error() {
    let store = seed().await;
    let app = app(&store, None);
    let response = send(&app, "POST", "/auth/logout", &mut Jar::default()).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
}

// -------------------------------------------------------------------------------------

fn set_cookie_names(response: &Response) -> Vec<String> {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split('=').next())
        .map(str::to_string)
        .collect()
}
