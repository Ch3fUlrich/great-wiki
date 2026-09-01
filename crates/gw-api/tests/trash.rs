//! The Papierkorb over HTTP: deleting a page, listing what is in the trash, putting one
//! back, and the two-step purge.
//!
//! `gw-store`'s own `trash` tests pin the properties themselves — the subtree moves as one,
//! the listing filters per document, a restore puts back exactly what went down. This file is
//! about the wire on top of them: which verb is which operation, that 403 and 404 and 409
//! land where they should, and — the part no store test can assert — **that the purge gate is
//! the one the API already uses for administering a page's own path**, so being able to write
//! a page is not being able to destroy it.
//!
//! The fixture is built so nothing here can pass vacuously:
//!
//! * `/raum` (public) with `/raum/notiz` (public) under it, and `/geheim` (restricted).
//! * `schreiber` holds **write** on `/raum` — enough to delete, never enough to purge.
//! * `leser` holds **read** on `/raum` — enough to see it in the trash, never to delete it.
//! * `chefin` holds **admin** on `/raum` and on `/geheim`.
//!
//! Without the write grant every delete would be refused for the wrong reason, and the purge
//! mutations would pass with the gate deleted.

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

async fn page(store: &Store, parent: Option<&str>, slug: &str, title: &str, v: Visibility) {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(Into::into),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: v,
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
    page(&store, None, "raum", "Raum", Visibility::Public).await;
    page(&store, Some("/raum"), "notiz", "Notiz", Visibility::Public).await;
    page(&store, None, "geheim", "Geheim", Visibility::Restricted).await;

    for (username, path, permission) in [
        ("schreiber", "/raum", Permission::Write),
        ("leser", "/raum", Permission::Read),
        ("chefin", "/raum", Permission::Admin),
        ("chefin", "/geheim", Permission::Admin),
    ] {
        let principal = match store.principal_by_username(username).await.unwrap() {
            Some((principal, _)) => principal,
            None => store
                .create_local_principal(username, username, None, "$argon2id$fake")
                .await
                .unwrap(),
        };
        store
            .add_grant(path, Subject::Principal(principal.id), permission)
            .await
            .unwrap();
    }
    Arc::new(store)
}

fn app(store: &Arc<Store>) -> axum::Router {
    gw_api::build_router(gw_api::AppState::for_test(Arc::clone(store), None))
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

async fn send(app: axum::Router, method: &str, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn as_user(
    store: &Arc<Store>,
    username: &str,
    method: &str,
    uri: &str,
) -> (StatusCode, String) {
    send(app_as(store, username).await, method, uri).await
}

async fn anonymous(store: &Arc<Store>, method: &str, uri: &str) -> (StatusCode, String) {
    send(app(store), method, uri).await
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap()
}

// -------------------------------------------------------------------------------------
// Deleting: an edit, so it follows write.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn deleting_a_page_takes_it_off_the_wiki_and_says_what_moved() {
    let store = fixture().await;
    let (status, body) = as_user(&store, "schreiber", "DELETE", "/api/documents/raum/notiz").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let body = json(&body);
    assert_eq!(body["path"], "/raum/notiz");
    assert_eq!(body["pages"], 1);

    let (status, _) = as_user(&store, "schreiber", "GET", "/api/documents/raum/notiz").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_page_you_may_only_read_is_forbidden() {
    let store = fixture().await;
    let (status, _) = as_user(&store, "leser", "DELETE", "/api/documents/raum/notiz").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = as_user(&store, "leser", "GET", "/api/documents/raum/notiz").await;
    assert_eq!(status, StatusCode::OK, "the refused delete happened anyway");
}

#[tokio::test]
async fn deleting_a_page_that_is_not_there_is_not_found() {
    let store = fixture().await;
    let (status, _) = as_user(
        &store,
        "schreiber",
        "DELETE",
        "/api/documents/gibt-es-nicht",
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_anonymous_caller_deletes_nothing() {
    let store = fixture().await;
    let (status, _) = anonymous(&store, "DELETE", "/api/documents/raum/notiz").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

// -------------------------------------------------------------------------------------
// The listing: an aggregate view, so it discloses per document.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_trash_shows_a_restricted_page_only_to_somebody_who_could_read_it() {
    let store = fixture().await;
    let (status, _) = as_user(&store, "chefin", "DELETE", "/api/documents/geheim").await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = as_user(&store, "leser", "GET", "/api/trash").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json(&body)["entries"].as_array().unwrap().len(),
        0,
        "the trash listed a page whose ACL was hiding it"
    );

    let (status, body) = as_user(&store, "chefin", "GET", "/api/trash").await;
    assert_eq!(status, StatusCode::OK);
    let entries = json(&body);
    let entries = entries["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["path"], "/geheim");
    assert_eq!(entries[0]["deleted_by_name"], "chefin");
    assert_eq!(entries[0]["may_restore"], true);
}

#[tokio::test]
async fn an_entry_a_caller_may_read_but_not_write_offers_no_restore() {
    let store = fixture().await;
    as_user(&store, "schreiber", "DELETE", "/api/documents/raum/notiz").await;

    let (status, body) = as_user(&store, "leser", "GET", "/api/trash").await;
    assert_eq!(status, StatusCode::OK);
    let entries = json(&body);
    let entries = entries["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "a page they may read vanished entirely");
    assert_eq!(entries[0]["may_restore"], false);
}

#[tokio::test]
async fn the_trash_is_an_empty_list_for_somebody_entitled_to_nothing() {
    let store = fixture().await;
    as_user(&store, "chefin", "DELETE", "/api/documents/geheim").await;
    let (status, body) = anonymous(&store, "GET", "/api/trash").await;
    assert_eq!(status, StatusCode::OK, "an empty trash is not a refusal");
    assert_eq!(json(&body)["entries"].as_array().unwrap().len(), 0);
}

// -------------------------------------------------------------------------------------
// Restoring.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn restoring_puts_the_page_back_where_it_was() {
    let store = fixture().await;
    as_user(&store, "schreiber", "DELETE", "/api/documents/raum/notiz").await;

    let (status, body) =
        as_user(&store, "schreiber", "POST", "/api/trash/restore/raum/notiz").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["pages"], 1);

    let (status, _) = as_user(&store, "schreiber", "GET", "/api/documents/raum/notiz").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn restoring_a_page_you_may_only_read_is_forbidden() {
    let store = fixture().await;
    as_user(&store, "schreiber", "DELETE", "/api/documents/raum/notiz").await;
    let (status, _) = as_user(&store, "leser", "POST", "/api/trash/restore/raum/notiz").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn restoring_something_that_is_not_in_the_trash_is_not_found() {
    let store = fixture().await;
    let (status, _) = as_user(&store, "schreiber", "POST", "/api/trash/restore/raum/notiz").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn restoring_under_a_parent_still_in_the_trash_is_a_conflict_that_names_it() {
    let store = fixture().await;
    as_user(&store, "schreiber", "DELETE", "/api/documents/raum/notiz").await;
    as_user(&store, "schreiber", "DELETE", "/api/documents/raum").await;

    let (status, body) =
        as_user(&store, "schreiber", "POST", "/api/trash/restore/raum/notiz").await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let message = json(&body)["error"].as_str().unwrap().to_string();
    assert!(message.contains("/raum"), "{message}");
}

// -------------------------------------------------------------------------------------
// Purging: the second, deliberate act, and the gate that makes it one.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn purging_needs_admin_on_the_page_and_write_is_not_enough() {
    // The whole of D-14's "second, deliberate act". `schreiber` may delete this page and may
    // put it back; destroying it is a different question with a different answer.
    let store = fixture().await;
    as_user(&store, "schreiber", "DELETE", "/api/documents/raum/notiz").await;

    let (status, _) = as_user(&store, "schreiber", "POST", "/api/trash/purge/raum/notiz").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = as_user(&store, "schreiber", "GET", "/api/trash/purge/raum/notiz").await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "the preview names what is there, so it is gated exactly as the purge is"
    );

    let (status, body) = as_user(&store, "schreiber", "GET", "/api/trash").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        json(&body)["entries"].as_array().unwrap().len(),
        1,
        "the refused purge destroyed the entry anyway"
    );
}

#[tokio::test]
async fn a_preview_names_what_would_go_and_leaves_it_all_there() {
    let store = fixture().await;
    as_user(&store, "chefin", "DELETE", "/api/documents/raum").await;

    let (status, body) = as_user(&store, "chefin", "GET", "/api/trash/purge/raum").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report = json(&body);
    assert_eq!(report["committed"], false);
    let pages: Vec<&str> = report["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["path"].as_str().unwrap())
        .collect();
    assert_eq!(pages, vec!["/raum", "/raum/notiz"]);
    assert_eq!(report["revisions"], 2);

    let (_, body) = as_user(&store, "chefin", "GET", "/api/trash").await;
    assert_eq!(
        json(&body)["entries"].as_array().unwrap().len(),
        1,
        "a preview destroyed the thing it was describing"
    );
}

#[tokio::test]
async fn a_purge_reports_what_it_destroyed_and_empties_the_entry() {
    let store = fixture().await;
    as_user(&store, "chefin", "DELETE", "/api/documents/raum").await;

    let (status, body) = as_user(&store, "chefin", "POST", "/api/trash/purge/raum").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let report = json(&body);
    assert_eq!(report["committed"], true);
    assert_eq!(report["pages"].as_array().unwrap().len(), 2);
    assert_eq!(report["revisions"], 2);

    let (_, body) = as_user(&store, "chefin", "GET", "/api/trash").await;
    assert_eq!(json(&body)["entries"].as_array().unwrap().len(), 0);
    let (status, _) = as_user(&store, "chefin", "GET", "/api/documents/raum").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn purging_a_page_that_is_not_in_the_trash_is_not_found() {
    // Not "deleted for you". The trash is the only way in, so a live page has no purge.
    let store = fixture().await;
    let (status, _) = as_user(&store, "chefin", "POST", "/api/trash/purge/raum").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = as_user(&store, "chefin", "GET", "/api/documents/raum").await;
    assert_eq!(status, StatusCode::OK, "a live page was destroyed");
}

#[tokio::test]
async fn an_anonymous_caller_purges_nothing() {
    let store = fixture().await;
    as_user(&store, "chefin", "DELETE", "/api/documents/geheim").await;
    let (status, _) = anonymous(&store, "POST", "/api/trash/purge/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = anonymous(&store, "GET", "/api/trash/purge/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
