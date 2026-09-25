//! Renaming and moving over HTTP, and following an old address.
//!
//! `gw-store`'s `moves` tests pin the properties themselves — the subtree moves as one, the
//! preview is the move rolled back, grants travel, forwards end when an address is reused.
//! This file is about the wire: which verb is which, where 403, 404 and 409 land, and that
//! the widening gate is `path_admin` on the destination — so writing somewhere is not being
//! able to let people in there.
//!
//! The fixture, built so nothing passes vacuously:
//!
//! * `/a` and `/b`, restricted, with `/a/p` under `/a`.
//! * `schreiber` writes `/a` and `/b` — may move, may not widen.
//! * `leser` reads `/a`; `bernd` reads `/b`; `fremde` holds nothing.
//! * `chefin` writes `/a` and administers `/b` — may widen into `/b`.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_core::{Block, BlockKind, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
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

async fn page(store: &Store, parent: Option<&str>, slug: &str) {
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
                body: empty_body(),
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
    page(&store, None, "a").await;
    page(&store, Some("/a"), "p").await;
    page(&store, None, "b").await;

    for username in ["schreiber", "leser", "bernd", "fremde", "chefin"] {
        store
            .create_local_principal(username, username, None, "$argon2id$fake")
            .await
            .unwrap();
    }
    for (username, path, permission) in [
        ("schreiber", "/a", Permission::Write),
        ("schreiber", "/b", Permission::Write),
        ("leser", "/a", Permission::Read),
        ("bernd", "/b", Permission::Read),
        ("chefin", "/a", Permission::Write),
        ("chefin", "/b", Permission::Admin),
    ] {
        let (principal, _) = store
            .principal_by_username(username)
            .await
            .unwrap()
            .unwrap();
        store
            .add_grant(path, Subject::Principal(principal.id), permission)
            .await
            .unwrap();
    }
    Arc::new(store)
}

async fn app_as(store: &Arc<Store>, username: Option<&str>) -> axum::Router {
    let state = match username {
        None => gw_api::AppState::for_test(Arc::clone(store), None),
        Some(username) => {
            let (principal, _) = store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap();
            gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
        }
    };
    gw_api::build_router(state)
}

async fn send(
    store: &Arc<Store>,
    who: Option<&str>,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, String) {
    let mut builder = Request::builder().method(method).uri(uri);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let body = body.map_or_else(Body::empty, |b| Body::from(b.to_string()));
    let response = app_as(store, who)
        .await
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap()
}

fn to_b() -> serde_json::Value {
    serde_json::json!({ "parent": "/b", "title": "P" })
}

fn names(list: &serde_json::Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|c| c["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn the_preview_names_the_change_and_says_it_would_be_refused_and_moves_nothing() {
    let store = fixture().await;
    let (status, body) = send(
        &store,
        Some("schreiber"),
        "GET",
        "/api/move/a/p?parent=/b&title=P",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let plan = json(&body);
    assert_eq!(plan["from"], "/a/p");
    assert_eq!(plan["to"], "/b/p");
    assert_eq!(names(&plan["gains"]), ["bernd"]);
    assert_eq!(names(&plan["losses"]), ["leser"]);
    assert!(
        plan["refusal"].is_string(),
        "schreiber does not administer /b"
    );
    assert_eq!(plan["committed"], false);

    let (status, _) = send(&store, Some("schreiber"), "GET", "/api/documents/a/p", None).await;
    assert_eq!(status, StatusCode::OK, "the preview moved the page");
}

#[tokio::test]
async fn a_writer_who_would_widen_access_is_refused_and_nothing_moves() {
    let store = fixture().await;
    let (status, body) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/move/a/p",
        Some(to_b()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(json(&body)["error"].as_str().unwrap().contains("/b"));
    let (status, _) = send(&store, Some("schreiber"), "GET", "/api/documents/a/p", None).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn an_administrator_of_the_destination_moves_it_and_the_old_address_forwards() {
    let store = fixture().await;
    let (status, body) = send(
        &store,
        Some("chefin"),
        "POST",
        "/api/move/a/p",
        Some(to_b()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["committed"], true);

    let (status, _) = send(&store, Some("chefin"), "GET", "/api/documents/b/p", None).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = send(&store, Some("chefin"), "GET", "/api/documents/a/p", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = send(&store, Some("chefin"), "GET", "/api/forwards/a/p", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["path"], "/b/p");
}

/// ADR 0022 for the forward: somebody who may not read the page where it is now — including
/// somebody who could read it where it was — gets exactly what an address nothing ever left
/// answers.
#[tokio::test]
async fn a_forward_is_byte_identical_to_no_forward_for_somebody_who_may_not_read_the_page() {
    let store = fixture().await;
    let (status, _) = send(
        &store,
        Some("chefin"),
        "POST",
        "/api/move/a/p",
        Some(to_b()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for who in [None, Some("fremde"), Some("leser")] {
        let withheld = send(&store, who, "GET", "/api/forwards/a/p", None).await;
        let absent = send(&store, who, "GET", "/api/forwards/gibt-es-nicht", None).await;
        assert_eq!(withheld.0, StatusCode::NOT_FOUND, "{who:?}");
        assert_eq!(
            withheld, absent,
            "{who:?} can tell a withheld forward from none"
        );
    }
}

#[tokio::test]
async fn a_reader_is_told_what_was_refused_and_a_stranger_is_told_nothing() {
    let store = fixture().await;
    let (status, _) = send(&store, Some("leser"), "POST", "/api/move/a/p", Some(to_b())).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    for who in [None, Some("fremde")] {
        let (status, _) = send(&store, who, "POST", "/api/move/a/p", Some(to_b())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{who:?}");
    }
}

#[tokio::test]
async fn a_move_that_cannot_be_made_says_why() {
    let store = fixture().await;
    let (status, body) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/move/a",
        Some(serde_json::json!({ "parent": "/a/p", "title": "A" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        json(&body)["error"].as_str().unwrap().contains("itself"),
        "{body}"
    );

    // The top level needs somebody who administers the whole wiki; an empty parent is it.
    let (status, _) = send(
        &store,
        Some("schreiber"),
        "GET",
        "/api/move/a/p?parent=&title=P",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}
