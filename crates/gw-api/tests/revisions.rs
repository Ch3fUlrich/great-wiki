//! Revision history over HTTP: the timeline, the three diffs, the source of one version,
//! and restoring one.
//!
//! `gw-store`'s own `revisions` tests already pin the storage properties — append-only,
//! attributed to the authenticated principal, restore-by-appending — and `gw-core`'s `diff`
//! tests pin what the three modes report. This file is about the wire on top of them, and
//! most of it is about **disclosure**.
//!
//! A revision list is not metadata about a page; it *is* the page, several times over. It
//! says the page exists, who edits it, when they were last at work, what they said they were
//! doing, and — through the diff and source endpoints — every word that was ever on it,
//! including the paragraph somebody deleted five minutes after publishing it by mistake. So
//! every endpoint here resolves the document through `Store::document_for`, the one
//! permission-checked accessor, and the tests below are written to fail if any of them ever
//! stops doing that.
//!
//! The fixture: `/oeffentlich` is public and has three revisions; `/geheim` is restricted
//! and has two. `chef` may write both. `leser` may READ `/geheim` and nothing more.
//! `fremder` holds no grant at all, so `/geheim` is invisible to them, and an anonymous
//! caller is a fourth case again.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

fn body(text: &str) -> Block {
    serde_json::from_str(&format!(
        r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}"#
    ))
    .unwrap()
}

async fn page(
    store: &Store,
    slug: &str,
    title: &str,
    visibility: Visibility,
    text: &str,
) -> String {
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
                body: body(text),
                sort_key: 0,
            },
            None,
        )
        .await
        .unwrap()
}

async fn principal(store: &Store, username: &str) -> Principal {
    store
        .create_local_principal(username, username, None, "$argon2id$fake")
        .await
        .unwrap();
    store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap()
        .0
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let open = page(
        &store,
        "oeffentlich",
        "Öffentlich",
        Visibility::Public,
        "Fassung eins",
    )
    .await;
    let secret = page(
        &store,
        "geheim",
        "Geheim",
        Visibility::Restricted,
        "Geheime Fassung eins",
    )
    .await;

    let chef = principal(&store, "chef").await;
    let leser = principal(&store, "leser").await;
    principal(&store, "fremder").await;

    for path in ["/oeffentlich", "/geheim"] {
        store
            .add_grant(path, Subject::Principal(chef.id.clone()), Permission::Write)
            .await
            .unwrap();
    }
    store
        .add_grant("/geheim", Subject::Principal(leser.id), Permission::Read)
        .await
        .unwrap();

    // Two more revisions on the public page, so there is a history to page through and two
    // revisions to diff. Both changes are a WORD, so the prose diff has something to say.
    store
        .publish_revision(&chef, &open, &body("Fassung zwei"), Some("zweiter Wurf"))
        .await
        .unwrap()
        .expect("chef may write /oeffentlich");
    store
        .publish_revision(&chef, &open, &body("Fassung drei"), None)
        .await
        .unwrap()
        .expect("chef may write /oeffentlich");
    store
        .publish_revision(&chef, &secret, &body("Geheime Fassung zwei"), None)
        .await
        .unwrap()
        .expect("chef may write /geheim");

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

async fn get_as(store: &Arc<Store>, username: &str, uri: &str) -> (StatusCode, String) {
    send(app_as(store, username).await, "GET", uri).await
}

async fn get_anonymous(store: &Arc<Store>, uri: &str) -> (StatusCode, String) {
    send(app(store, None), "GET", uri).await
}

async fn post_as(store: &Arc<Store>, username: &str, uri: &str) -> (StatusCode, String) {
    send(app_as(store, username).await, "POST", uri).await
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("not JSON ({e}): {body}"))
}

/// The revision ids of a page, newest first, as the API reports them.
async fn revision_ids(store: &Arc<Store>, username: &str, path: &str) -> Vec<String> {
    let (status, body) = get_as(store, username, &format!("/api/revisions/document{path}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    json(&body)["revisions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_string())
        .collect()
}

// -------------------------------------------------------------------------------------
// Who may see that there is a history at all.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_history_of_a_page_a_caller_cannot_read_is_refused() {
    let store = fixture().await;
    let (status, body) = get_as(&store, "fremder", "/api/revisions/document/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(
        !body.contains("Geheime"),
        "a refusal disclosed the very content it refused: {body}"
    );
}

#[tokio::test]
async fn an_anonymous_caller_gets_no_history_for_a_restricted_page() {
    let store = fixture().await;
    let (status, _) = get_anonymous(&store, "/api/revisions/document/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_history_of_a_page_that_is_not_there_is_not_found() {
    let store = fixture().await;
    let (status, _) = get_anonymous(&store, "/api/revisions/document/gibt-es-nicht").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn reading_a_page_is_enough_to_read_its_history() {
    let store = fixture().await;
    let (status, body) = get_as(&store, "leser", "/api/revisions/document/geheim").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["revisions"].as_array().unwrap().len(), 2);
}

// -------------------------------------------------------------------------------------
// What the timeline says.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_timeline_is_newest_first_and_names_its_authors() {
    let store = fixture().await;
    let (status, body) = get_anonymous(&store, "/api/revisions/document/oeffentlich").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let revisions = json(&body)["revisions"].as_array().unwrap().clone();
    assert_eq!(revisions.len(), 3);

    // Newest first: the last thing published is the first thing listed.
    assert_eq!(revisions[1]["summary"], "zweiter Wurf");
    assert_eq!(revisions[1]["author_name"], "chef");
    assert_eq!(revisions[1]["author_is_account"], true);

    // The first revision came from the import, which is nobody's edit and must not be
    // rendered as somebody's.
    assert_eq!(revisions[2]["author_is_account"], false);
    assert!(revisions[2]["parent_id"].is_null());
}

#[tokio::test]
async fn the_timeline_carries_no_bodies_and_no_internal_ids() {
    // 34 revisions of a page is 34 bodies, and the timeline shows none of them. The author
    // id is left out for the reason `BacklinkView` leaves out the document id: it is an
    // internal identifier with no reason to be on the wire.
    let store = fixture().await;
    let (_, body) = get_anonymous(&store, "/api/revisions/document/oeffentlich").await;
    let revisions = json(&body)["revisions"].as_array().unwrap().clone();
    for revision in revisions {
        assert!(
            revision.get("body").is_none(),
            "a body reached the timeline"
        );
        assert!(revision.get("author_id").is_none());
        assert!(revision.get("document_id").is_none());
        assert!(revision["byte_size"].as_i64().unwrap() > 0);
    }
}

// -------------------------------------------------------------------------------------
// The diff, and who may ask for one.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_diff_reports_all_three_modes() {
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/oeffentlich").await;
    let (status, body) = get_as(
        &store,
        "chef",
        &format!("/api/revisions/{}/diff/{}", ids[2], ids[0]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let diff = json(&body);
    assert_eq!(diff["from"]["id"], ids[2].as_str());
    assert_eq!(diff["to"]["id"], ids[0].as_str());
    assert!(diff["prose"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["kind"] == "removed" && c["text"] == "eins"));
    assert!(diff["prose"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["kind"] == "added" && c["text"] == "drei"));
    // The block is the same paragraph with different words in it: one change, not two.
    let structure = diff["structure"].as_array().unwrap();
    assert_eq!(structure.len(), 1);
    assert_eq!(structure[0]["kind"], "changed");
    assert!(diff["design"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_diff_of_a_page_the_caller_cannot_read_is_not_found() {
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/geheim").await;
    let (status, body) = get_as(
        &store,
        "fremder",
        &format!("/api/revisions/{}/diff/{}", ids[1], ids[0]),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert!(
        !body.contains("Geheime"),
        "the refusal carried the content: {body}"
    );
}

#[tokio::test]
async fn a_diff_across_two_documents_is_refused() {
    // Nothing useful, and it would let one page's history be read against another's.
    let store = fixture().await;
    let open = revision_ids(&store, "chef", "/oeffentlich").await;
    let secret = revision_ids(&store, "chef", "/geheim").await;
    let (status, _) = get_as(
        &store,
        "chef",
        &format!("/api/revisions/{}/diff/{}", open[0], secret[0]),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn an_unknown_revision_is_not_found_rather_than_forbidden() {
    // A revision id is a uuid nobody guesses, so there is no existence to protect and no
    // reason for two answers. Everything unreachable answers the same thing.
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/oeffentlich").await;
    let (status, _) = get_as(
        &store,
        "chef",
        &format!("/api/revisions/{}/diff/{}", ids[0], "gibt-es-nicht"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// -------------------------------------------------------------------------------------
// Seeing one whole version.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_source_of_a_revision_is_the_export_triple() {
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/oeffentlich").await;
    let (status, body) = get_as(
        &store,
        "chef",
        &format!("/api/revisions/{}/source?path=/oeffentlich", ids[2]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let source = json(&body);
    assert!(source["markdown"]
        .as_str()
        .unwrap()
        .contains("Fassung eins"));
    assert!(source["meta"]
        .as_str()
        .unwrap()
        .contains("title: Öffentlich"));
    assert!(source["design"].as_str().unwrap().contains("\"paragraph\""));
    assert!(source["problem"].is_null());
    // The OLD body, not the current one: this is what makes it "see that version".
    assert!(!source["markdown"].as_str().unwrap().contains("drei"));
}

#[tokio::test]
async fn the_source_of_a_revision_of_a_page_the_caller_cannot_read_is_refused() {
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/geheim").await;
    let (status, body) = get_as(
        &store,
        "fremder",
        &format!("/api/revisions/{}/source?path=/geheim", ids[0]),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(!body.contains("Geheime"));
}

#[tokio::test]
async fn a_revision_and_a_path_that_do_not_belong_together_are_not_found() {
    // Both halves are readable to `chef` and they are still refused: the metadata of one
    // page must never be stapled to the body of another.
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/oeffentlich").await;
    let (status, body) = get_as(
        &store,
        "chef",
        &format!("/api/revisions/{}/source?path=/geheim", ids[0]),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

#[tokio::test]
async fn the_source_of_a_revision_names_the_page_it_belongs_to_or_it_is_refused() {
    // The path is not decoration and it is not optional: it is what the permission decision
    // is taken on. An id on its own buys nothing here.
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/oeffentlich").await;
    let (status, _) = get_as(&store, "chef", &format!("/api/revisions/{}/source", ids[0])).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// -------------------------------------------------------------------------------------
// Restoring.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn restoring_appends_a_revision_and_never_rewinds_history() {
    let store = fixture().await;
    let before = revision_ids(&store, "chef", "/oeffentlich").await;
    let oldest = before.last().unwrap().clone();

    let (status, body) = post_as(&store, "chef", &format!("/api/revisions/{oldest}/restore")).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let after = revision_ids(&store, "chef", "/oeffentlich").await;
    assert_eq!(after.len(), before.len() + 1, "a restore must APPEND");
    for id in &before {
        assert!(
            after.contains(id),
            "restoring deleted history: {id} is gone"
        );
    }
    // And the page now says what the restored revision said.
    let (_, page) = get_as(&store, "chef", "/api/documents/oeffentlich").await;
    assert!(json(&page)["body"]
        .as_str()
        .unwrap()
        .contains("Fassung eins"));
    // The new revision is reported, so the caller can navigate straight to it.
    assert_eq!(json(&body)["revision"]["id"], after[0].as_str());
}

#[tokio::test]
async fn restoring_needs_write_and_reading_the_history_is_not_enough() {
    let store = fixture().await;
    let ids = revision_ids(&store, "leser", "/geheim").await;
    let (status, _) = post_as(
        &store,
        "leser",
        &format!("/api/revisions/{}/restore", ids[1]),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // And nothing was written.
    assert_eq!(revision_ids(&store, "leser", "/geheim").await.len(), 2);
}

#[tokio::test]
async fn an_anonymous_caller_cannot_restore_a_public_page() {
    // A revision records WHO, and an anonymous request names nobody. `/oeffentlich` is
    // public, so this is refused by the write decision rather than by visibility.
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/oeffentlich").await;
    let (status, _) = send(
        app(&store, None),
        "POST",
        &format!("/api/revisions/{}/restore", ids[2]),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(revision_ids(&store, "chef", "/oeffentlich").await.len(), 3);
}

#[tokio::test]
async fn restoring_a_revision_of_an_invisible_page_is_not_found() {
    let store = fixture().await;
    let ids = revision_ids(&store, "chef", "/geheim").await;
    let (status, _) = post_as(
        &store,
        "fremder",
        &format!("/api/revisions/{}/restore", ids[1]),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(revision_ids(&store, "chef", "/geheim").await.len(), 2);
}
