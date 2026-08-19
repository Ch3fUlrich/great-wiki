//! Backlinks over HTTP: which pages point at the one being read, filtered to what the
//! caller may see.
//!
//! `gw-store`'s own `links` tests already pin the filtering property itself — a candidate
//! the caller may not read is omitted, and the page being asked about answers nothing at
//! all if the caller may not read IT. This file is about the wire on top of that: the route
//! exists under its own prefix, an absent page is 404 and a forbidden one is 403 exactly as
//! `/api/documents` and `/api/collab` already are, and the JSON shape is what the frontend
//! was promised — `path` and `title`, and nothing that would leak an internal id.
//!
//! The fixture: `/ziel` is public and has no outgoing links. `/quelle` (public) and
//! `/geheim` (restricted) both link to it, so `/ziel`'s backlinks differ by who is asking.
//! `leser` holds no grant anywhere; `chef` is granted `read` on `/geheim`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_core::{Block, BlockKind, DocumentType, Mark, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::collections::BTreeSet;
use std::sync::Arc;
use tower::ServiceExt;

fn empty_body() -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

/// A one-paragraph document whose only content links to `href`.
fn linking_body(href: &str) -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content: vec![Block {
            kind: BlockKind::Paragraph,
            attrs: Default::default(),
            content: vec![Block {
                kind: BlockKind::Text,
                attrs: Default::default(),
                content: Vec::new(),
                text: Some("siehe dort".into()),
                marks: vec![Mark::link_to_url(href)],
            }],
            text: None,
            marks: Vec::new(),
        }],
        text: None,
        marks: Vec::new(),
    }
}

async fn page(store: &Store, slug: &str, title: &str, visibility: Visibility, body: Block) {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility,
                body,
                sort_key: 0,
            },
            None,
        )
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(&store, "ziel", "Ziel", Visibility::Public, empty_body()).await;
    page(
        &store,
        "quelle",
        "Quelle",
        Visibility::Public,
        linking_body("/ziel"),
    )
    .await;
    page(
        &store,
        "geheim",
        "Geheim",
        Visibility::Restricted,
        linking_body("/ziel"),
    )
    .await;

    for username in ["leser", "chef"] {
        store
            .create_local_principal(username, username, None, "$argon2id$fake")
            .await
            .unwrap();
    }
    let (chef, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    store
        .add_grant("/geheim", Subject::Principal(chef.id), Permission::Read)
        .await
        .unwrap();

    Arc::new(store)
}

fn app(store: &Arc<Store>, dev: Option<gw_api::Identity>) -> axum::Router {
    gw_api::build_router(gw_api::AppState::for_test(Arc::clone(store), dev))
}

async fn app_as(store: &Arc<Store>, username: &str) -> axum::Router {
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
    gw_api::build_router(gw_api::AppState::for_test_principal(
        Arc::clone(store),
        &principal,
    ))
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn get_anonymous(store: &Arc<Store>, uri: &str) -> (StatusCode, String) {
    get(app(store, None), uri).await
}

async fn get_as(store: &Arc<Store>, username: &str, uri: &str) -> (StatusCode, String) {
    get(app_as(store, username).await, uri).await
}

// -------------------------------------------------------------------------------------
// Who may ask.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn backlinks_are_refused_to_somebody_who_cannot_read_the_page() {
    let store = fixture().await;
    let (status, _) = get_anonymous(&store, "/api/links/backlinks/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_absent_page_answers_not_found() {
    let store = fixture().await;
    let (status, _) = get_anonymous(&store, "/api/links/backlinks/gibt-es-nicht").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// -------------------------------------------------------------------------------------
// What comes back, and to whom.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn backlinks_are_filtered_to_what_the_caller_may_read() {
    let store = fixture().await;

    // `leser` holds no grant on `/geheim`, so its link to `/ziel` must not appear — even
    // though `/ziel` itself is public and this request is otherwise allowed.
    let (status, body) = get_as(&store, "leser", "/api/links/backlinks/ziel").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("Quelle"),
        "the public backlink is missing: {body}"
    );
    assert!(
        !body.contains("Geheim"),
        "a backlink from a page this caller cannot read was disclosed: {body}"
    );

    // Anti-vacuity: `chef` holds a grant on `/geheim` and sees both, so the fixture really
    // has two backlinks to hide from `leser` rather than the filter merely finding none.
    let (status, body) = get_as(&store, "chef", "/api/links/backlinks/ziel").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Quelle"), "{body}");
    assert!(body.contains("Geheim"), "{body}");
}

#[tokio::test]
async fn a_page_with_no_backlinks_answers_with_an_empty_list() {
    // Nobody links to `/quelle`. The empty case is not an error and not a 404 — the page
    // itself is perfectly readable.
    let store = fixture().await;
    let (status, body) = get_anonymous(&store, "/api/links/backlinks/quelle").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, r#"{"backlinks":[]}"#);
}

#[tokio::test]
async fn the_response_carries_exactly_a_path_and_a_title() {
    // The wire contract is `{"backlinks": [{"path": …, "title": …}]}` — not
    // `gw_store::Backlink`'s own shape, which also carries the document id. An id is an
    // internal identifier with no reason to leave this crate over an endpoint the frontend
    // reads directly.
    let store = fixture().await;
    let (status, body) = get_as(&store, "chef", "/api/links/backlinks/ziel").await;
    assert_eq!(status, StatusCode::OK);

    let value: serde_json::Value = serde_json::from_str(&body).unwrap();
    let backlinks = value["backlinks"].as_array().expect("a `backlinks` array");
    assert_eq!(backlinks.len(), 2, "{body}");
    for entry in backlinks {
        let keys: BTreeSet<&str> = entry
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from(["path", "title"]),
            "unexpected shape for one backlink: {entry}"
        );
    }
}
