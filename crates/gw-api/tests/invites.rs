//! Invites, and the account an invite creates (M2 Task 7).
//!
//! An invite is a **credential**: whoever holds the link can create an account and walk
//! into whatever the invite carries. So the tests here are almost all negative ones, and
//! each is written so that removing the defence it covers makes it fail rather than
//! quietly passing for a second reason.
//!
//! Three properties get more attention than the rest, because each has a documented
//! history of being got wrong somewhere in this codebase:
//!
//! - **Scope.** D-M2-2: an invite may grant only into spaces the inviter administers.
//!   The refusal is server-side, against the permission engine, and the positive case is
//!   asserted alongside every negative one — a test that only checks the refusal would
//!   pass against an endpoint that refused everybody.
//! - **Indistinguishability.** An unknown, expired, revoked or consumed token must
//!   produce the same bytes. Anything else turns the endpoint into a list of which tokens
//!   exist.
//! - **All or nothing.** A refused password, a second accept, a token that expired while
//!   the form was open: every one of them must leave no account, no grant, no membership
//!   and no session behind.
//!
//! The fixture is a database FILE rather than `sqlite::memory:`, so a second connection
//! can read what was actually written. `Store`'s pool is crate-private on purpose; that
//! second connection is the only way an integration test can assert on the stored bytes,
//! which is what "only a hash of the token is stored" has to mean to be worth anything.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use base64::Engine;
use gw_api::auth::session::hash_token;
use gw_api::AppState;
use gw_auth::breach::{BreachFuture, BreachRange};
use gw_auth::{Permission, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::{Column, Row, SqlitePool};
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

/// Long enough to clear the twelve-character floor, so a test about anything else is not
/// silently a test about length.
const PASSPHRASE: &str = "ein-vollkommen-brauchbarer-satz";

/// Below the floor, and — this is the fiddly part — a word that appears nowhere in the
/// page's own markup. The obvious "kurz" is a substring of the form's hint ("ein kurzes
/// Kunstwort"), so `!page.contains(...)` would have failed against a page that echoed
/// nothing at all: a test failing for a reason unrelated to what it claims to check.
const TOO_SHORT: &str = "winzig";

/// The cookie the sign-in page and the invite page both use for their double-submit token.
const CSRF_COOKIE: &str = "__Host-gw_login_csrf";
const SESSION_COOKIE: &str = "__Host-gw_session";

// -------------------------------------------------------------------------------------
// Fixture.
// -------------------------------------------------------------------------------------

struct Fixture {
    store: Arc<Store>,
    /// A SECOND connection to the same file, for reading what was really stored and for
    /// the one thing no endpoint offers: moving an invite's expiry into the past.
    db: SqlitePool,
    _dir: tempfile::TempDir,
}

fn body_block() -> Block {
    serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap()
}

fn page(parent: Option<&str>, title: &str, visibility: Visibility) -> NewDocument {
    NewDocument {
        parent_path: parent.map(str::to_string),
        doc_type: DocumentType::Page,
        title: title.to_string(),
        slug: None,
        language: "de".into(),
        visibility,
        body: body_block(),
        sort_key: 0,
    }
}

/// Four principals and five documents.
///
/// - `chef` — an `admins` member, so an instance admin by baseline (D-M2-1).
/// - `lektor` — `admin` on `/raum` and nothing else. A SPACE admin.
/// - `leser` — `read` on `/raum`. Being able to read a space is not being able to invite
///   into it, and that distinction is a test.
/// - team `redaktion`, holding `read` on `/anderer-raum`, which is the reach a
///   team-carrying invite hands over.
///
/// `/offen` carries `anyone: admin`. It is what makes the anonymous test mean something:
/// `can()` answers an `Anyone` grant *before* it looks at whether the caller is signed in,
/// so on that path the engine alone would hand the invite endpoint to a request that has
/// not said who it is.
async fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", dir.path().join("great-wiki.db").display());
    let store = Store::open(&url).await.unwrap();

    for doc in [
        page(None, "Öffentlich", Visibility::Public),
        page(None, "Intern", Visibility::Internal),
        page(None, "Raum", Visibility::Restricted),
        page(Some("/raum"), "Unterseite", Visibility::Restricted),
        page(None, "Anderer Raum", Visibility::Restricted),
        page(None, "Offen", Visibility::Restricted),
    ] {
        store
            .create_document(Author::Import, &doc, None)
            .await
            .unwrap();
    }

    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();
    let lektor = store
        .create_local_principal("lektor", "Lektor", None, "$argon2id$fake")
        .await
        .unwrap();
    let leser = store
        .create_local_principal("leser", "Leser", None, "$argon2id$fake")
        .await
        .unwrap();

    store
        .add_grant(
            "/raum",
            Subject::Principal(lektor.id.clone()),
            Permission::Admin,
        )
        .await
        .unwrap();
    store
        .add_grant(
            "/raum",
            Subject::Principal(leser.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();
    store
        .add_grant("/offen", Subject::Anyone, Permission::Admin)
        .await
        .unwrap();
    store.create_team("redaktion", "Redaktion").await.unwrap();
    store
        .add_grant(
            "/anderer-raum",
            Subject::Team("redaktion".into()),
            Permission::Read,
        )
        .await
        .unwrap();

    let db = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();

    Fixture {
        store: Arc::new(store),
        db,
        _dir: dir,
    }
}

/// A router whose requests arrive as the stored principal called `who`, or anonymously
/// when it is `None`.
async fn router(fx: &Fixture, who: Option<&str>) -> Router {
    let state = match who {
        Some(username) => {
            let (principal, _) = fx
                .store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
            AppState::for_test_principal(Arc::clone(&fx.store), &principal)
        }
        None => AppState::for_test(Arc::clone(&fx.store), None),
    };
    gw_api::build_router(state)
}

/// The application as somebody who has not signed in sees it — which is who follows an
/// invite link.
async fn browser(fx: &Fixture) -> Router {
    router(fx, None).await
}

// -------------------------------------------------------------------------------------
// The JSON API.
// -------------------------------------------------------------------------------------

async fn api(
    fx: &Fixture,
    who: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = router(fx, who).await.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn get(fx: &Fixture, who: Option<&str>, uri: &str) -> (StatusCode, Value) {
    api(fx, who, Method::GET, uri, None).await
}

async fn post(fx: &Fixture, who: Option<&str>, uri: &str, body: Value) -> (StatusCode, Value) {
    api(fx, who, Method::POST, uri, Some(body)).await
}

async fn del(fx: &Fixture, who: Option<&str>, uri: &str) -> (StatusCode, Value) {
    api(fx, who, Method::DELETE, uri, None).await
}

/// What a caller keeps after creating an invite: the id, and the one and only copy of the
/// token.
struct Invite {
    id: String,
    token: String,
}

fn invite_from(created: &Value) -> Invite {
    let url = created["delivery"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("the create response must carry the link once: {created}"));
    Invite {
        id: created["id"].as_str().expect("an invite has an id").into(),
        token: url
            .rsplit('/')
            .next()
            .expect("the link ends in the token")
            .to_string(),
    }
}

/// Create an invite and assert it was created. Used wherever the creation itself is a
/// precondition rather than the thing under test.
async fn invite(fx: &Fixture, who: &str, body: Value) -> Invite {
    let (status, created) = post(fx, Some(who), "/api/admin/invites", body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    invite_from(&created)
}

/// The commonest one: a read grant on `/raum`, made by the space admin.
async fn invite_to_raum(fx: &Fixture, username: &str) -> Invite {
    invite(
        fx,
        "lektor",
        json!({"username": username, "path": "/raum", "permission": "read"}),
    )
    .await
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

async fn visit(
    app: &Router,
    jar: &mut Jar,
    method: &str,
    uri: &str,
    form: Option<String>,
) -> Response {
    let mut builder = Request::builder().method(method).uri(uri);
    if !jar.0.is_empty() {
        builder = builder.header("cookie", jar.header());
    }
    let request = match form {
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
    String::from_utf8(
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn accept_form(display_name: &str, password: &str, csrf: &str) -> String {
    format!(
        "display_name={}&password={}&csrf={}",
        encode(display_name),
        encode(password),
        encode(csrf)
    )
}

/// Open the invite page so the browser holds a double-submit token, and return it.
async fn open_invite(app: &Router, jar: &mut Jar, token: &str) -> (StatusCode, String) {
    let response = visit(app, jar, "GET", &format!("/auth/invite/{token}"), None).await;
    let status = response.status();
    (status, text(response).await)
}

/// A token minted by the SIGN-IN page rather than by an invite page.
///
/// The four-way indistinguishability test needs a valid CSRF token for a POST against a
/// token that has none to give — and without one, all four refusals would be the same
/// *CSRF* refusal and the test would prove nothing about the invite states at all.
async fn csrf_from_the_sign_in_page(app: &Router, jar: &mut Jar) -> String {
    visit(app, jar, "GET", "/auth/login", None).await;
    jar.get(CSRF_COOKIE)
        .expect("the sign-in page issues a double-submit token")
        .to_string()
}

/// Follow an invite the way a person does: open the page, fill it in, submit.
async fn accept(app: &Router, jar: &mut Jar, token: &str, name: &str, password: &str) -> Response {
    open_invite(app, jar, token).await;
    let csrf = jar.get(CSRF_COOKIE).unwrap_or_default().to_string();
    visit(
        app,
        jar,
        "POST",
        &format!("/auth/invite/{token}/accept"),
        Some(accept_form(name, password, &csrf)),
    )
    .await
}

// -------------------------------------------------------------------------------------
// Corpora.
// -------------------------------------------------------------------------------------

/// A corpus that reports exactly one password as breached — and it is one this file
/// actually submits, at full length.
///
/// The suffix is derived from `sha1_hex`, which `gw-auth` pins against the published
/// SHA-1 of "password", so this is a real range response rather than a shape that happens
/// to satisfy the parser. Deriving it matters: a canned line for a password nobody submits
/// would let the test pass with the corpus never consulted at all.
struct BreachedCorpus {
    line: String,
}

impl BreachedCorpus {
    fn holding(password: &str) -> Self {
        Self {
            line: format!("{}:9545824\r\n", &gw_auth::breach::sha1_hex(password)[5..]),
        }
    }
}

impl BreachRange for BreachedCorpus {
    fn fetch<'a>(&'a self, _prefix: &'a str) -> BreachFuture<'a> {
        let line = self.line.clone();
        Box::pin(async move { Ok(line) })
    }
}

/// The application with a corpus that knows `password`. Anonymous, because it is the
/// invitee who is setting the password.
async fn browser_with_corpus(fx: &Fixture, password: &str) -> Router {
    let state = AppState {
        corpus: Arc::new(BreachedCorpus::holding(password)),
        ..AppState::for_test(Arc::clone(&fx.store), None)
    };
    gw_api::build_router(state)
}

// -------------------------------------------------------------------------------------
// Reading the tree.
// -------------------------------------------------------------------------------------

fn collect(nodes: &Value, out: &mut Vec<String>) {
    for node in nodes.as_array().cloned().unwrap_or_default() {
        out.push(node["path"].as_str().unwrap().to_string());
        collect(&node["children"], out);
    }
}

/// Every path the holder of `jar` sees in the navigation, sorted.
async fn visible_tree(app: &Router, jar: &mut Jar) -> Vec<String> {
    let response = visit(app, jar, "GET", "/api/tree", None).await;
    let tree: Value = serde_json::from_str(&text(response).await).unwrap();
    let mut paths = Vec::new();
    collect(&tree, &mut paths);
    paths.sort();
    paths
}

async fn status_of(app: &Router, jar: &mut Jar, uri: &str) -> StatusCode {
    visit(app, jar, "GET", uri, None).await.status()
}

// =====================================================================================
// Who may create an invite (D-M2-2).
// =====================================================================================

#[tokio::test]
async fn an_invite_into_a_space_the_inviter_does_not_administer_is_refused() {
    // The same request twice, differing only in the path. The first must SUCCEED: without
    // it this test would pass just as well against an endpoint that refused everybody.
    let fx = fixture().await;

    let (allowed, created) = post(
        &fx,
        Some("lektor"),
        "/api/admin/invites",
        json!({"username": "gast-raum", "path": "/raum", "permission": "read"}),
    )
    .await;
    assert_eq!(allowed, StatusCode::CREATED, "{created}");

    let (refused, body) = post(
        &fx,
        Some("lektor"),
        "/api/admin/invites",
        json!({"username": "gast-anderswo", "path": "/anderer-raum", "permission": "read"}),
    )
    .await;
    assert_eq!(
        refused,
        StatusCode::FORBIDDEN,
        "a space admin minted a credential into a space they do not administer: {body}"
    );

    // And nothing was written. A 403 that still stored the row would be a credential
    // waiting for somebody to find it.
    let (_, listed) = get(&fx, Some("chef"), "/api/admin/invites").await;
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["gast-raum"], "{listed}");
}

#[tokio::test]
async fn a_space_admin_cannot_put_the_invited_account_in_a_team() {
    // A team's reach is not bounded by any path: `redaktion` reads `/anderer-raum`, which
    // `lektor` does not administer. If a space admin could attach a team, D-M2-2's scope
    // rule would be one field away from being nothing at all.
    let fx = fixture().await;

    let (refused, body) = post(
        &fx,
        Some("lektor"),
        "/api/admin/invites",
        json!({"username": "gast", "path": "/raum", "permission": "read", "team": "redaktion"}),
    )
    .await;
    assert_eq!(refused, StatusCode::FORBIDDEN, "{body}");

    // The same request from an instance admin is allowed, so this is about who asked
    // rather than about the request being malformed.
    let (allowed, created) = post(
        &fx,
        Some("chef"),
        "/api/admin/invites",
        json!({"username": "gast", "path": "/raum", "permission": "read", "team": "redaktion"}),
    )
    .await;
    assert_eq!(allowed, StatusCode::CREATED, "{created}");
}

#[tokio::test]
async fn an_invite_that_grants_nothing_is_refused() {
    // D-M2-20 exists because the gap between "make an account" and "give it access" is
    // where somebody gets forgotten. An invite carrying neither reopens exactly that gap,
    // and what it mints in the meantime is a credential that reaches only what needs no
    // account at all.
    let fx = fixture().await;

    let (status, body) = post(
        &fx,
        Some("chef"),
        "/api/admin/invites",
        json!({"username": "niemand"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (_, listed) = get(&fx, Some("chef"), "/api/admin/invites").await;
    assert!(listed.as_array().unwrap().is_empty(), "{listed}");
}

#[tokio::test]
async fn an_anonymous_caller_cannot_create_an_invite_even_where_anyone_holds_admin() {
    // `/offen` carries `anyone: admin`, and `can()` answers an `Anyone` grant BEFORE it
    // checks whether the caller is signed in — that is what a public share link is. So on
    // that path the permission engine alone would hand the invite endpoint to a request
    // that has not said who it is. Without the grant in the fixture, no subject would
    // match anything and this test would pass with the authentication check deleted.
    let fx = fixture().await;

    let (status, body) = post(
        &fx,
        None,
        "/api/admin/invites",
        json!({"username": "eindringling", "path": "/offen", "permission": "admin"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    let (_, listed) = get(&fx, Some("chef"), "/api/admin/invites").await;
    assert!(listed.as_array().unwrap().is_empty(), "{listed}");
}

#[tokio::test]
async fn reading_a_space_does_not_confer_inviting_into_it() {
    let fx = fixture().await;
    let (status, body) = post(
        &fx,
        Some("leser"),
        "/api/admin/invites",
        json!({"username": "gast", "path": "/raum", "permission": "read"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}

#[tokio::test]
async fn a_space_admin_sees_their_own_invites_and_not_another_space_s() {
    // The listing is filtered in the retriever, like every other retrieval path here.
    let fx = fixture().await;
    invite_to_raum(&fx, "gast-raum").await;
    invite(
        &fx,
        "chef",
        json!({"username": "gast-anderswo", "path": "/anderer-raum", "permission": "read"}),
    )
    .await;

    let (status, listed) = get(&fx, Some("lektor"), "/api/admin/invites").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["gast-raum"], "{listed}");
}

#[tokio::test]
async fn an_anonymous_caller_cannot_list_invites_even_where_anyone_holds_admin() {
    // The invite is on `/offen`, which carries `anyone: admin`. So the retriever's own
    // permission check would let an anonymous caller have this row, and the only thing
    // standing in front of it is the endpoint establishing that there IS a caller.
    // Without the invite being on that path, no subject would match and this test would
    // pass with both checks deleted.
    let fx = fixture().await;
    invite(
        &fx,
        "chef",
        json!({"username": "gast", "path": "/offen", "permission": "read"}),
    )
    .await;

    let (status, body) = get(&fx, None, "/api/admin/invites").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "an anonymous caller read the invite list: {body}"
    );

    // And it really is listable by somebody who administers that path, so the refusal is
    // about who asked rather than about the row being invisible to everyone.
    let (status, listed) = get(&fx, Some("chef"), "/api/admin/invites").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
}

// =====================================================================================
// The link, and what is kept of it.
// =====================================================================================

#[tokio::test]
async fn the_link_is_returned_once_and_never_appears_again() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;

    let (_, listed) = get(&fx, Some("chef"), "/api/admin/invites").await;
    let raw = listed.to_string();
    assert!(
        !raw.contains(&created.token),
        "the listing handed the token back: {raw}"
    );
    assert!(
        !raw.contains(&hash_token(&created.token)),
        "the listing handed back the stored digest, which is as good as the token to \
         anybody who can write to the database: {raw}"
    );
    assert!(raw.contains(&created.id), "the id is not a secret: {raw}");
}

#[tokio::test]
async fn only_a_hash_of_the_token_is_ever_stored() {
    // Read straight out of the database file. A copy of this database must not be a bag of
    // live invitations.
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;

    let rows = sqlx::query("SELECT * FROM invites")
        .fetch_all(&fx.db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);

    let mut digest_seen = false;
    for row in &rows {
        for (index, column) in row.columns().iter().enumerate() {
            let value: Option<String> = row.try_get(index).unwrap_or(None);
            let Some(value) = value else { continue };
            assert!(
                !value.contains(&created.token),
                "column `{}` holds the token itself",
                column.name()
            );
            if value == hash_token(&created.token) {
                digest_seen = true;
            }
        }
    }
    assert!(
        digest_seen,
        "no column holds the token's SHA-256, so the link cannot be what the lookup \
         matches — either nothing is stored or something else is"
    );

    // And the audit log, which is read by people and kept indefinitely (D-M2-13).
    let entries = sqlx::query("SELECT action, target, detail FROM audit_log")
        .fetch_all(&fx.db)
        .await
        .unwrap();
    assert!(!entries.is_empty(), "creating an invite recorded nothing");
    for entry in entries {
        for index in 0..3 {
            let value: Option<String> = entry.try_get(index).unwrap_or(None);
            let Some(value) = value else { continue };
            assert!(
                !value.contains(&created.token) && !value.contains(&hash_token(&created.token)),
                "the audit log carries the invite token: {value}"
            );
        }
    }
}

#[tokio::test]
async fn a_token_carries_at_least_256_bits_and_two_never_match() {
    let fx = fixture().await;
    let first = invite_to_raum(&fx, "eins").await;
    let second = invite_to_raum(&fx, "zwei").await;

    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(&first.token)
        .expect("tokens are base64url");
    assert!(raw.len() >= 32, "got {} bytes", raw.len());
    assert_ne!(first.token, second.token);
}

// =====================================================================================
// The page somebody actually sees.
// =====================================================================================

#[tokio::test]
async fn the_invite_page_names_who_invited_them_and_what_they_will_get() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;

    let (status, page) = open_invite(&app, &mut Jar::default(), &created.token).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert!(page.contains("lang=\"de\""), "{page}");
    assert!(page.contains("Lektor"), "who invited them: {page}");
    assert!(page.contains("/raum"), "what they will get: {page}");
    assert!(page.contains("Lesezugriff"), "at which permission: {page}");
    assert!(page.contains("gast"), "under which username: {page}");
    assert!(page.contains("name=\"display_name\""), "{page}");
    assert!(page.contains("name=\"password\""), "{page}");
    assert!(
        page.contains(&format!("/auth/invite/{}/accept", created.token)),
        "{page}"
    );
    assert!(!page.contains("http://"), "the page loads nothing: {page}");
}

#[tokio::test]
async fn markup_in_an_inviter_s_name_cannot_escape_the_invite_page() {
    // The invite page interpolates names somebody else chose, which the sign-in page never
    // does. Escaping is load-bearing here rather than precautionary.
    let fx = fixture().await;
    fx.store
        .upsert_oidc_principal(
            "boese",
            "<script>alert(1)</script>",
            None,
            &["admins".into()],
        )
        .await
        .unwrap();

    let created = invite(
        &fx,
        "boese",
        json!({"username": "gast", "path": "/raum", "permission": "read"}),
    )
    .await;
    let app = browser(&fx).await;
    let (_, page) = open_invite(&app, &mut Jar::default(), &created.token).await;

    assert!(!page.contains("<script>alert(1)</script>"), "{page}");
    assert!(page.contains("&lt;script&gt;"), "{page}");
}

// =====================================================================================
// Single use, expiry, revocation — and that none of the four can be told apart.
// =====================================================================================

#[tokio::test]
async fn a_used_token_cannot_be_used_again() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;

    let first = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    let second = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Noch Ein Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(
        second.status(),
        StatusCode::NOT_FOUND,
        "a consumed invite was accepted a second time"
    );
    assert_eq!(
        fx.store.list_principals().await.unwrap().len(),
        4,
        "chef, lektor, leser and exactly one guest"
    );
}

#[tokio::test]
async fn two_simultaneous_accepts_produce_exactly_one_account() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;

    // Each browser opens the page first, so both hold a valid double-submit token and the
    // only thing left to refuse either of them is the invite itself.
    let mut left = Jar::default();
    let mut right = Jar::default();
    open_invite(&app, &mut left, &created.token).await;
    open_invite(&app, &mut right, &created.token).await;
    let left_csrf = left.get(CSRF_COOKIE).unwrap().to_string();
    let right_csrf = right.get(CSRF_COOKIE).unwrap().to_string();

    let uri = format!("/auth/invite/{}/accept", created.token);
    let (a, b) = tokio::join!(
        visit(
            &app,
            &mut left,
            "POST",
            &uri,
            Some(accept_form("Links", PASSPHRASE, &left_csrf))
        ),
        visit(
            &app,
            &mut right,
            "POST",
            &uri,
            Some(accept_form("Rechts", PASSPHRASE, &right_csrf))
        ),
    );

    let mut statuses = [a.status(), b.status()];
    statuses.sort_by_key(|s| s.as_u16());
    assert_eq!(
        statuses,
        [StatusCode::SEE_OTHER, StatusCode::NOT_FOUND],
        "one accept must win and the other must be refused as a spent invite — a 500 here \
         means the UNIQUE constraint refused it rather than the invite guard"
    );

    let (guest, _) = fx
        .store
        .principal_by_username("gast")
        .await
        .unwrap()
        .expect("the winner has an account");
    assert_eq!(fx.store.list_principals().await.unwrap().len(), 4);
    assert_eq!(
        fx.store.session_count_for(&guest.id).await.unwrap(),
        1,
        "the losing accept still issued a session"
    );
}

#[tokio::test]
async fn an_expired_token_is_refused() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;

    // D-M2-21 gives thirty days. Nothing offers a way to shorten that, and rightly, so the
    // fixture reaches past the API to move the expiry into the past.
    sqlx::query("UPDATE invites SET expires_at = '2020-01-01 00:00:00' WHERE id = ?1")
        .bind(&created.id)
        .execute(&fx.db)
        .await
        .unwrap();

    let app = browser(&fx).await;
    let (status, _) = open_invite(&app, &mut Jar::default(), &created.token).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let refused = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
    assert!(fx
        .store
        .principal_by_username("gast")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn a_revoked_token_is_refused() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;

    let (status, body) = del(
        &fx,
        Some("lektor"),
        &format!("/api/admin/invites/{}", created.id),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let app = browser(&fx).await;
    let refused = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(refused.status(), StatusCode::NOT_FOUND);
    assert!(fx
        .store
        .principal_by_username("gast")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn an_invite_into_a_space_the_caller_does_not_administer_cannot_be_revoked() {
    let fx = fixture().await;
    let created = invite(
        &fx,
        "chef",
        json!({"username": "gast", "path": "/anderer-raum", "permission": "read"}),
    )
    .await;

    // 404 rather than 403: to a space admin who may not see it, an invite in another
    // space is not there at all. Answering 403 would confirm the id.
    let (status, _) = del(
        &fx,
        Some("lektor"),
        &format!("/api/admin/invites/{}", created.id),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // And it still works, so the refusal really did refuse rather than revoke and lie.
    let app = browser(&fx).await;
    let accepted = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn unknown_expired_revoked_and_consumed_tokens_are_indistinguishable() {
    // Four states, one answer. Anything else — a different status, a different byte, a
    // cookie set in one case and not another — turns this endpoint into a way to ask which
    // tokens exist.
    let fx = fixture().await;
    let expired = invite_to_raum(&fx, "abgelaufen").await;
    let revoked = invite_to_raum(&fx, "zurueckgezogen").await;
    let consumed = invite_to_raum(&fx, "verbraucht").await;
    let unknown = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string();

    sqlx::query("UPDATE invites SET expires_at = '2020-01-01 00:00:00' WHERE id = ?1")
        .bind(&expired.id)
        .execute(&fx.db)
        .await
        .unwrap();
    del(
        &fx,
        Some("lektor"),
        &format!("/api/admin/invites/{}", revoked.id),
    )
    .await;

    let app = browser(&fx).await;
    let accepted = accept(
        &app,
        &mut Jar::default(),
        &consumed.token,
        "Verbraucht",
        PASSPHRASE,
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);

    let mut gets = Vec::new();
    let mut posts = Vec::new();
    for token in [&unknown, &expired.token, &revoked.token, &consumed.token] {
        // A fresh browser holding a VALID double-submit token, minted somewhere that is
        // not an invite page. Without it every POST would be refused for its CSRF and the
        // four answers would match without saying anything about the invites at all.
        let mut jar = Jar::default();
        let csrf = csrf_from_the_sign_in_page(&app, &mut jar).await;

        let response = visit(
            &app,
            &mut jar,
            "GET",
            &format!("/auth/invite/{token}"),
            None,
        )
        .await;
        gets.push((
            response.status(),
            response.headers().get_all("set-cookie").iter().count(),
            text(response).await,
        ));

        let response = visit(
            &app,
            &mut jar,
            "POST",
            &format!("/auth/invite/{token}/accept"),
            Some(accept_form("Jemand", PASSPHRASE, &csrf)),
        )
        .await;
        posts.push((
            response.status(),
            response.headers().get_all("set-cookie").iter().count(),
            text(response).await,
        ));
    }

    for (label, answers) in [("GET", &gets), ("POST", &posts)] {
        assert_eq!(answers[0].0, StatusCode::NOT_FOUND, "{label}");
        for (index, answer) in answers.iter().enumerate() {
            assert_eq!(
                answer, &answers[0],
                "{label}: state {index} answered differently from an unknown token"
            );
        }
    }

    // Nothing beyond the one legitimate acceptance was created.
    assert_eq!(fx.store.list_principals().await.unwrap().len(), 4);
}

// =====================================================================================
// The password the recipient chooses (D-M2-16).
// =====================================================================================

#[tokio::test]
async fn a_password_under_twelve_characters_is_refused_and_creates_nothing() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;

    let response = accept(&app, &mut Jar::default(), &created.token, "Gast", TOO_SHORT).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let page = text(response).await;
    assert!(page.contains("12 Zeichen"), "{page}");
    assert!(
        !page.contains(TOO_SHORT),
        "the page echoed the password: {page}"
    );

    assert!(fx
        .store
        .principal_by_username("gast")
        .await
        .unwrap()
        .is_none());
    // And the invite is untouched, so somebody who mistyped is not locked out of their own
    // invitation.
    let accepted = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn a_breached_password_is_refused_and_creates_nothing() {
    // Long enough that the length floor would let it through, so the only thing that can
    // refuse it is the corpus actually being consulted.
    const LEAKED: &str = "ein-passwort-aus-einem-datenleck";
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser_with_corpus(&fx, LEAKED).await;

    let response = accept(&app, &mut Jar::default(), &created.token, "Gast", LEAKED).await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a breached password was accepted, so the corpus is not being consulted"
    );
    let page = text(response).await;
    assert!(page.contains("Datenleck"), "{page}");
    assert!(
        !page.contains(LEAKED),
        "the page echoed the password: {page}"
    );

    assert!(fx
        .store
        .principal_by_username("gast")
        .await
        .unwrap()
        .is_none());
    // Another password the same corpus does not hold is accepted, so the refusal was about
    // this password rather than about the corpus being reachable at all.
    let accepted = accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::SEE_OTHER);
}

// =====================================================================================
// What the account can reach afterwards.
// =====================================================================================

#[tokio::test]
async fn accepting_signs_the_person_in_and_api_me_accepts_the_cookie() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;
    let mut jar = Jar::default();

    let response = accept(&app, &mut jar, &created.token, "Gast Gastmann", PASSPHRASE).await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        response.headers().get("location").unwrap(),
        "/",
        "a POST answered with a redirect must send the browser to a GET"
    );
    assert!(
        jar.get(SESSION_COOKIE).is_some(),
        "accepting did not sign them in"
    );

    let me: Value =
        serde_json::from_str(&text(visit(&app, &mut jar, "GET", "/api/me", None).await).await)
            .unwrap();
    assert_eq!(me["authenticated"], json!(true), "{me}");
    assert_eq!(me["username"], json!("gast"), "{me}");
    assert_eq!(me["display_name"], json!("Gast Gastmann"), "{me}");
    assert_eq!(
        me["baseline"],
        json!("public"),
        "an invited account has no Authelia groups, so it has no baseline beyond public"
    );
}

#[tokio::test]
async fn the_invited_account_reaches_the_invited_path_and_public_content_and_nothing_else() {
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;
    let mut jar = Jar::default();
    assert_eq!(
        accept(&app, &mut jar, &created.token, "Gast", PASSPHRASE)
            .await
            .status(),
        StatusCode::SEE_OTHER
    );

    // `/offen` is in the list because it carries an `anyone: admin` grant, which every
    // caller matches including an anonymous one — it is a public share link and says
    // nothing about the invite. `/intern` and `/anderer-raum` are the two that would mean
    // the invite handed over more than it promised.
    assert_eq!(
        visible_tree(&app, &mut jar).await,
        vec![
            "/oeffentlich".to_string(),
            "/offen".to_string(),
            "/raum".to_string(),
            "/raum/unterseite".to_string()
        ],
        "the granted subtree plus public pages, and nothing else"
    );

    // The tree is not the only way in. A direct URL has to agree with it.
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/raum").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/raum/unterseite").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/oeffentlich").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/intern").await,
        StatusCode::FORBIDDEN,
        "an account by itself must not confer the internal wiki"
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/anderer-raum").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/offen").await,
        StatusCode::OK,
        "`/offen` carries an `anyone` grant, so this is not evidence of the invite"
    );

    // And the invite conferred READ, not write: D-M2-8.
    let (status, _) = get(&fx, Some("gast"), "/api/admin/acl?path=/raum").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a read grant conferred administration of the space"
    );
}

#[tokio::test]
async fn a_team_carrying_invite_puts_them_in_the_team_and_they_reach_what_it_reaches() {
    let fx = fixture().await;
    let created = invite(
        &fx,
        "chef",
        json!({"username": "gast", "team": "redaktion"}),
    )
    .await;
    let app = browser(&fx).await;
    let mut jar = Jar::default();
    assert_eq!(
        accept(&app, &mut jar, &created.token, "Gast", PASSPHRASE)
            .await
            .status(),
        StatusCode::SEE_OTHER
    );

    let me: Value =
        serde_json::from_str(&text(visit(&app, &mut jar, "GET", "/api/me", None).await).await)
            .unwrap();
    assert_eq!(me["teams"], json!(["redaktion"]), "{me}");

    assert_eq!(
        visible_tree(&app, &mut jar).await,
        vec![
            "/anderer-raum".to_string(),
            "/oeffentlich".to_string(),
            "/offen".to_string()
        ],
        "what the team reaches, plus public pages and the `anyone` share link"
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/anderer-raum").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&app, &mut jar, "/api/documents/raum").await,
        StatusCode::FORBIDDEN,
        "the team's reach is not everybody's reach"
    );
}

// =====================================================================================
// The record.
// =====================================================================================

#[tokio::test]
async fn creating_and_accepting_are_recorded_where_the_space_admin_can_see_them() {
    // D-M2-4: a space admin needs to know who was granted access to their space, and an
    // invite is how that access is handed out without them ever meeting the person.
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;

    let (_, log) = get(&fx, Some("lektor"), "/api/admin/audit").await;
    let actions: Vec<&str> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"invite.create"), "{log}");

    let app = browser(&fx).await;
    accept(
        &app,
        &mut Jar::default(),
        &created.token,
        "Gast",
        PASSPHRASE,
    )
    .await;

    let (_, log) = get(&fx, Some("lektor"), "/api/admin/audit").await;
    let actions: Vec<&str> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"invite.accept"), "{log}");
    assert!(
        actions.contains(&"acl.grant"),
        "the grant an invite writes must read like any other grant: {log}"
    );
}

#[tokio::test]
async fn an_accept_that_fails_leaves_no_account_no_grant_and_no_record() {
    // The worst outcome an invite can have is a half-acceptance: an account with no way in,
    // or a grant naming somebody who does not exist. One transaction, or none of it.
    let fx = fixture().await;
    let created = invite_to_raum(&fx, "gast").await;
    let app = browser(&fx).await;

    accept(&app, &mut Jar::default(), &created.token, "Gast", TOO_SHORT).await;

    assert!(fx
        .store
        .principal_by_username("gast")
        .await
        .unwrap()
        .is_none());
    let grants = fx.store.grants_defined_at("/raum").await.unwrap();
    assert_eq!(
        grants.len(),
        2,
        "lektor's admin and leser's read, and no more"
    );

    let (_, log) = get(&fx, Some("chef"), "/api/admin/audit").await;
    let actions: Vec<&str> = log["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(
        !actions.contains(&"invite.accept"),
        "an acceptance that did not happen was recorded: {log}"
    );
}
