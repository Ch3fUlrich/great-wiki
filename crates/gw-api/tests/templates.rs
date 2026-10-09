//! Templates and creating a page, over HTTP. The store's tests pin the rules; this file is
//! the wire: verbs, status codes, and that an unreadable template looks like an absent one.
//!
//! Fixture: `/vorlagen` (read: `leser`, `schreiber`) with `/vorlagen/protokoll`; `/raum`
//! (write: `schreiber`); `fremde` holds nothing.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_core::{Block, BlockKind, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

fn body_of(text: Option<&str>) -> Block {
    let content = text
        .map(|t| {
            vec![Block {
                kind: BlockKind::Paragraph,
                attrs: Default::default(),
                content: Vec::new(),
                text: Some(t.into()),
                marks: Vec::new(),
            }]
        })
        .unwrap_or_default();
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content,
        text: None,
        marks: Vec::new(),
    }
}

async fn page(store: &Store, parent: Option<&str>, slug: &str, text: Option<&str>) {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(Into::into),
                doc_type: DocumentType::Page,
                title: slug.to_uppercase(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: Visibility::Restricted,
                body: body_of(text),
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(&store, None, "vorlagen", None).await;
    page(&store, Some("/vorlagen"), "protokoll", Some("{{titel}}")).await;
    page(&store, None, "raum", None).await;
    for u in ["schreiber", "leser", "fremde"] {
        store
            .create_local_principal(u, u, None, "$argon2id$fake")
            .await
            .unwrap();
    }
    for (u, path, perm) in [
        ("schreiber", "/vorlagen", Permission::Read),
        ("schreiber", "/raum", Permission::Write),
        ("leser", "/vorlagen", Permission::Read),
        ("leser", "/raum", Permission::Read),
    ] {
        let (p, _) = store.principal_by_username(u).await.unwrap().unwrap();
        store
            .add_grant(path, Subject::Principal(p.id), perm)
            .await
            .unwrap();
    }
    Arc::new(store)
}

async fn send(
    store: &Arc<Store>,
    who: Option<&str>,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, String) {
    let state = match who {
        None => gw_api::AppState::for_test(Arc::clone(store), None),
        Some(u) => {
            let (p, _) = store.principal_by_username(u).await.unwrap().unwrap();
            gw_api::AppState::for_test_principal(Arc::clone(store), &p)
        }
    };
    let mut b = Request::builder().method(method).uri(uri);
    if body.is_some() {
        b = b.header("content-type", "application/json");
    }
    let body = body.map_or_else(Body::empty, |v| Body::from(v.to_string()));
    let r = gw_api::build_router(state)
        .oneshot(b.body(body).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn new_page(template: Option<&str>) -> serde_json::Value {
    serde_json::json!({ "parent": "/raum", "title": "Montag", "template": template })
}

#[tokio::test]
async fn the_list_holds_only_templates_the_caller_may_read() {
    let store = fixture().await;
    let (s, b) = send(&store, Some("schreiber"), "GET", "/api/templates", None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.contains("/vorlagen/protokoll"), "{b}");
    let (_, b) = send(&store, Some("fremde"), "GET", "/api/templates", None).await;
    assert_eq!(b, "[]");
    let (_, b) = send(&store, None, "GET", "/api/templates", None).await;
    assert_eq!(b, "[]");
}

#[tokio::test]
async fn a_writer_creates_a_page_from_a_template_and_it_reads_filled() {
    let store = fixture().await;
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/pages",
        Some(new_page(Some("/vorlagen/protokoll"))),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    assert!(b.contains("/raum/montag"));
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "GET",
        "/api/documents/raum/montag",
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.contains("Montag"), "{b}");
    assert!(!b.contains("{{titel}}"), "{b}");
}

#[tokio::test]
async fn the_gates_answer_with_the_right_status() {
    let store = fixture().await;
    // Read-only parent: 409 with the reason.
    let (s, _) = send(
        &store,
        Some("leser"),
        "POST",
        "/api/pages",
        Some(new_page(None)),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    // Anonymous: 401.
    let (s, _) = send(&store, None, "POST", "/api/pages", Some(new_page(None))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // A template the caller cannot read and one that does not exist: identical bytes.
    let (s1, b1) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/pages",
        Some(new_page(Some("/vorlagen/gibt-es-nicht"))),
    )
    .await;
    assert_eq!(s1, StatusCode::CONFLICT);
    assert!(b1.contains("there is no template at"));
    let (s, _) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/pages",
        Some(serde_json::json!({"parent": "/raum", "title": "T", "template": "/raum"})),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::CONFLICT,
        "a non-template page is not a template"
    );
}
