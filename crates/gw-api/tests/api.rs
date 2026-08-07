use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::Permission;
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

fn body() -> Block {
    // Annotated because it is otherwise only constrained by `Serialize`, which is
    // ambiguous. Each document gets its own owned copy.
    serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap()
}

async fn insert(store: &Store, parent: Option<&str>, title: &str, visibility: Visibility) {
    store
        .insert_document(&NewDocument {
            parent_path: parent.map(str::to_string),
            doc_type: DocumentType::Page,
            title: title.into(),
            slug: None,
            language: "de".into(),
            visibility,
            body: body(),
            sort_key: 0,
        })
        .await
        .unwrap();
}

async fn seed() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    insert(&store, None, "Öffentlich", Visibility::Public).await;
    insert(&store, None, "Geheim", Visibility::Restricted).await;
    Arc::new(store)
}

/// The shape the access tests need: one restricted subtree the guest is granted, one it is
/// not, a public page, and an internal one. Plus the principals that reach them — a local
/// guest with no Authelia groups at all, and an `admins` member.
async fn seed_with_acl() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    insert(&store, None, "Handbuch", Visibility::Restricted).await;
    insert(
        &store,
        Some("/handbuch"),
        "Onboarding",
        Visibility::Restricted,
    )
    .await;
    insert(&store, None, "Geheim", Visibility::Restricted).await;
    insert(&store, None, "Intern", Visibility::Internal).await;
    insert(&store, None, "Öffentlich", Visibility::Public).await;

    let guest = store
        .create_local_principal("guest", "Gast", None, "$argon2id$fake")
        .await
        .unwrap();
    store.create_team("gaeste", "Gäste").await.unwrap();
    store.add_team_member("gaeste", &guest.id).await.unwrap();
    store
        .add_grant(
            "/handbuch",
            gw_auth::Subject::Team("gaeste".into()),
            Permission::Read,
        )
        .await
        .unwrap();

    store
        .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
        .await
        .unwrap();

    Arc::new(store)
}

fn app(store: Arc<Store>, dev: Option<gw_api::Identity>) -> axum::Router {
    gw_api::build_router(gw_api::AppState::for_test(store, dev))
}

/// A router whose requests arrive as the stored principal called `username`.
///
/// The principal is looked up rather than invented, so a test cannot accidentally assert
/// against groups or teams the database does not actually hold.
async fn app_as(store: &Arc<Store>, username: &str) -> axum::Router {
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{username}` must exist in the seeded store"));
    gw_api::build_router(gw_api::AppState::for_test_principal(
        Arc::clone(store),
        &principal,
    ))
}

async fn get(app: axum::Router, uri: &str) -> StatusCode {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

async fn get_as(store: &Arc<Store>, username: &str, uri: &str) -> StatusCode {
    get(app_as(store, username).await, uri).await
}

async fn get_body_as(store: &Arc<Store>, username: &str, uri: &str) -> String {
    let response = app_as(store, username)
        .await
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn deactivate(store: &Store, username: &str) {
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .expect("cannot deactivate a principal that does not exist");
    store
        .set_principal_active(&principal.id, false)
        .await
        .unwrap();
}

#[tokio::test]
async fn health_is_ok() {
    assert_eq!(
        get(app(seed().await, None), "/api/health").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn public_document_is_readable_anonymously() {
    assert_eq!(
        get(app(seed().await, None), "/api/documents/oeffentlich").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn restricted_document_is_forbidden_anonymously() {
    // 403 not 404: the proxy already knows the path exists, and a misleading 404 makes
    // debugging an authentication problem needlessly hard.
    assert_eq!(
        get(app(seed().await, None), "/api/documents/geheim").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn restricted_document_is_readable_by_the_admins_group() {
    // Not "because they are signed in" — because `admins` maps to the admin baseline
    // (D-M2-1). The development shim goes through the same engine a real sign-in does.
    let identity = gw_api::Identity::dev("sergej", &["admins"]);
    assert_eq!(
        get(app(seed().await, Some(identity)), "/api/documents/geheim").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn restricted_document_is_refused_to_an_authenticated_caller_with_no_groups() {
    // The rule M1's deleted visibility stub got wrong: holding an account was enough for
    // it, so creating one silently handed over everything not marked public. Under the
    // engine an account by itself confers nothing beyond public (D-M2-1).
    let identity = gw_api::Identity::dev("gast", &[]);
    assert_eq!(
        get(app(seed().await, Some(identity)), "/api/documents/geheim").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn absent_document_is_not_found() {
    assert_eq!(
        get(app(seed().await, None), "/api/documents/gibt-es-nicht").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn anonymous_tree_omits_restricted_documents() {
    let response = app(seed().await, None)
        .oneshot(
            Request::builder()
                .uri("/api/tree")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("Öffentlich"));
    // A restricted title leaking into the navigation is a disclosure even without the body.
    assert!(
        !text.contains("Geheim"),
        "restricted titles must not appear in the tree"
    );
}

// -------------------------------------------------------------------------------------
// M2 Task 4: every retrieval goes through the permission engine.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_granted_guest_reaches_only_the_granted_subtree() {
    let store = seed_with_acl().await;

    // Granted: readable.
    assert_eq!(
        get_as(&store, "guest", "/api/documents/handbuch").await,
        StatusCode::OK
    );
    // And down the subtree, because grants inherit.
    assert_eq!(
        get_as(&store, "guest", "/api/documents/handbuch/onboarding").await,
        StatusCode::OK
    );
    // Not granted: refused, even though the same principal is authenticated.
    assert_eq!(
        get_as(&store, "guest", "/api/documents/geheim").await,
        StatusCode::FORBIDDEN
    );
    // An account is not a membership: `internal` follows the Authelia group (D-M2-1), and
    // a local guest has none.
    assert_eq!(
        get_as(&store, "guest", "/api/documents/intern").await,
        StatusCode::FORBIDDEN
    );
    // Public survives everything.
    assert_eq!(
        get_as(&store, "guest", "/api/documents/oeffentlich").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn the_tree_endpoint_omits_branches_a_guest_cannot_read() {
    let store = seed_with_acl().await;
    let body = get_body_as(&store, "guest", "/api/tree").await;
    assert!(body.contains("Handbuch"), "{body}");
    assert!(
        body.contains("Onboarding"),
        "the granted subtree comes with it"
    );
    assert!(body.contains("Öffentlich"), "{body}");
    assert!(
        !body.contains("Geheim"),
        "an ungranted title must not appear in navigation: {body}"
    );
    assert!(
        !body.contains("Intern"),
        "an internal title must not appear either: {body}"
    );
}

#[tokio::test]
async fn a_deactivated_principal_is_refused_everything_but_public() {
    // D-M2-7: the principal is re-read on every request, so this takes effect on the next
    // click rather than at the next sign-in.
    let store = seed_with_acl().await;
    deactivate(&store, "guest").await;

    assert_eq!(
        get_as(&store, "guest", "/api/documents/handbuch").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get_as(&store, "guest", "/api/documents/oeffentlich").await,
        StatusCode::OK
    );
    let body = get_body_as(&store, "guest", "/api/tree").await;
    assert!(
        !body.contains("Handbuch"),
        "a revoked branch must leave the navigation too: {body}"
    );
}

#[tokio::test]
async fn an_absent_path_is_404_and_a_forbidden_one_is_403() {
    // Both are `None` from the store on purpose; this layer decides which to reveal.
    // Collapsing them either way loses something: 404 for everything hides configuration
    // mistakes, 403 for everything confirms the existence of any path somebody guesses.
    let store = seed_with_acl().await;
    assert_eq!(
        get_as(&store, "guest", "/api/documents/gibt-es-nicht").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get_as(&store, "guest", "/api/documents/geheim").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_admins_member_reaches_the_whole_tree() {
    // The other side of D-M2-1, so the guest tests above cannot pass merely because the
    // filter refuses everybody.
    let store = seed_with_acl().await;
    assert_eq!(
        get_as(&store, "sergej", "/api/documents/geheim").await,
        StatusCode::OK
    );
    let body = get_body_as(&store, "sergej", "/api/tree").await;
    for title in ["Handbuch", "Onboarding", "Geheim", "Intern", "Öffentlich"] {
        assert!(body.contains(title), "`{title}` is missing: {body}");
    }
}

#[tokio::test]
async fn a_session_cookie_confers_nothing_until_there_is_a_session_store() {
    // Task 6 adds the session store. Until then a presented cookie must resolve to nobody
    // rather than to whatever it claims — the branch has to fail closed while it is a stub,
    // not merely once it is finished.
    let store = seed_with_acl().await;
    let response = gw_api::build_router(gw_api::AppState::for_test(Arc::clone(&store), None))
        .oneshot(
            Request::builder()
                .uri("/api/documents/geheim")
                .header("cookie", "gw_session=sergej")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
