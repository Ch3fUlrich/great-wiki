//! Creating a dataset over HTTP: verbs, status codes, and that an unreadable parent looks
//! like an absent one.
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
    page(&store, None, "privat", None).await;
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

fn make(parent: &str) -> serde_json::Value {
    serde_json::json!({ "parent": parent, "title": "Tabelle" })
}

#[tokio::test]
async fn a_writer_creates_an_empty_dataset_page() {
    let store = fixture().await;
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/datasets",
        Some(make("/raum")),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    assert!(b.contains("/raum/tabelle"), "{b}");
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "GET",
        "/api/documents/raum/tabelle",
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(b.contains("dataset"), "{b}");
}

#[tokio::test]
async fn the_gates_answer_with_the_right_status() {
    let store = fixture().await;
    let (s, _) = send(
        &store,
        Some("leser"),
        "POST",
        "/api/datasets",
        Some(make("/raum")),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = send(&store, None, "POST", "/api/datasets", Some(make("/raum"))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // Unreadable parent and absent parent: same status, same shape of words.
    let (s1, b1) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/datasets",
        Some(make("/privat")),
    )
    .await;
    let (s2, b2) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/datasets",
        Some(make("/nichts")),
    )
    .await;
    assert_eq!(s1, StatusCode::CONFLICT);
    assert_eq!(s1, s2);
    assert!(b1.contains("there is no page at /privat"), "{b1}");
    assert_eq!(b1.replace("/privat", "/x"), b2.replace("/nichts", "/x"));
}
