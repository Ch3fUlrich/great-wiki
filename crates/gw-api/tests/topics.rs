//! Topics over HTTP: the index, one topic's page, and a page's own filing.
//!
//! `gw-store`'s own `topics` tests pin the filtering itself — a document the caller may not
//! read is omitted, and a topic they can see no document of does not exist for them at all.
//! This file is about the wire on top of that: the routes exist under their own prefix, the
//! status codes are the ones `/api/documents` and `/api/links` already use, and the JSON is
//! what an interface was promised — no internal id, and nothing that could count what the
//! filtering removed.
//!
//! The fixture, which is the same shape as `tests/links.rs`'s and for the same reason:
//! `/offen` is public and about `Medizin/Darm`; `/geheim` is restricted and about
//! `Medizin/Darm` as well as `Kündigung`. So the two callers see two different indexes, and
//! `Kündigung` — a topic whose *name* is the disclosure — must not exist for `leser` at all.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_core::{Block, BlockKind, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use serde_json::{json, Value};
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

async fn page(store: &Store, slug: &str, title: &str, visibility: Visibility, topics: &[&str]) {
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
                body: empty_body(),
                sort_key: 0,
                topics: topics.iter().map(|t| t.to_string()).collect(),
            },
            None,
        )
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(
        &store,
        "offen",
        "Offen",
        Visibility::Public,
        &["Medizin/Darm"],
    )
    .await;
    page(
        &store,
        "geheim",
        "Geheim",
        Visibility::Restricted,
        &["Medizin/Darm", "Kündigung Mietvertrag"],
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
        .add_grant("/geheim", Subject::Principal(chef.id), Permission::Write)
        .await
        .unwrap();

    Arc::new(store)
}

async fn app_as(store: &Arc<Store>, username: Option<&str>) -> axum::Router {
    match username {
        None => gw_api::build_router(gw_api::AppState::for_test(Arc::clone(store), None)),
        Some(username) => {
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
    }
}

async fn send(app: axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

async fn get(store: &Arc<Store>, who: Option<&str>, uri: &str) -> (StatusCode, Value) {
    let app = app_as(store, who).await;
    send(
        app,
        Request::builder().uri(uri).body(Body::empty()).unwrap(),
    )
    .await
}

async fn put(store: &Arc<Store>, who: Option<&str>, uri: &str, body: Value) -> (StatusCode, Value) {
    let app = app_as(store, who).await;
    send(
        app,
        Request::builder()
            .method("PUT")
            .uri(uri)
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
    )
    .await
}

fn paths(list: &Value) -> Vec<&str> {
    list.as_array()
        .expect("a list")
        .iter()
        .map(|item| item["path"].as_str().expect("a path"))
        .collect()
}

// --- the index -------------------------------------------------------------------------

#[tokio::test]
async fn the_index_offers_only_topics_the_caller_can_see_a_page_of() {
    let store = fixture().await;

    let (status, body) = get(&store, Some("leser"), "/api/topics").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(paths(&body["topics"]), ["/medizin", "/medizin/darm"]);

    // Anti-vacuity: the fixture really is hiding one, and `chef` sees it.
    let (_, mine) = get(&store, Some("chef"), "/api/topics").await;
    assert_eq!(
        paths(&mine["topics"]),
        ["/kuendigung-mietvertrag", "/medizin", "/medizin/darm"]
    );
}

#[tokio::test]
async fn the_index_names_a_topic_the_way_somebody_typed_it() {
    let store = fixture().await;
    let (_, body) = get(&store, Some("chef"), "/api/topics").await;
    let darm = &body["topics"].as_array().unwrap()[2];
    assert_eq!(darm["name"], json!("Darm"));
    assert_eq!(darm["display_path"], json!("Medizin/Darm"));
}

#[tokio::test]
async fn the_index_counts_only_what_it_would_show() {
    let store = fixture().await;

    let count = |body: &Value| body["topics"].as_array().unwrap()[0]["documents"].clone();
    let (_, theirs) = get(&store, Some("leser"), "/api/topics").await;
    assert_eq!(count(&theirs), json!(1), "{theirs}");

    let (_, mine) = get(&store, Some("chef"), "/api/topics").await;
    // `chef`'s first row is `Kündigung Mietvertrag`, which is theirs alone.
    assert_eq!(count(&mine), json!(1), "{mine}");
    let medizin = &mine["topics"].as_array().unwrap()[1];
    assert_eq!(medizin["documents"], json!(2), "{mine}");
}

#[tokio::test]
async fn the_index_carries_no_field_that_could_count_what_it_hid() {
    // Structural, on the keys, exactly as the board's own test is: a total, an `omitted`,
    // or an id for a topic that was filtered out each says that something is there. A field
    // that cannot exist cannot be wrong later.
    let store = fixture().await;
    let (_, body) = get(&store, Some("leser"), "/api/topics").await;

    let top: Vec<&String> = body.as_object().unwrap().keys().collect();
    assert_eq!(top, ["topics"], "{body}");
    for topic in body["topics"].as_array().unwrap() {
        let keys: Vec<&String> = topic.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["display_path", "documents", "name", "path"],
            "{topic}"
        );
    }
}

// --- one topic's page ------------------------------------------------------------------

#[tokio::test]
async fn a_topic_page_lists_the_documents_of_the_topics_inside_it() {
    let store = fixture().await;
    let (status, body) = get(&store, Some("leser"), "/api/topics/tagged/medizin").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["topic"]["display_path"], json!("Medizin"));
    assert_eq!(paths(&body["documents"]), ["/offen"]);
    assert_eq!(paths(&body["children"]), ["/medizin/darm"]);
}

#[tokio::test]
async fn a_topic_page_omits_a_document_the_caller_may_not_read() {
    let store = fixture().await;

    let (_, theirs) = get(&store, Some("leser"), "/api/topics/tagged/medizin/darm").await;
    assert_eq!(paths(&theirs["documents"]), ["/offen"]);
    let (_, mine) = get(&store, Some("chef"), "/api/topics/tagged/medizin/darm").await;
    assert_eq!(paths(&mine["documents"]), ["/geheim", "/offen"]);
}

#[tokio::test]
async fn a_topic_whose_pages_are_all_out_of_reach_is_not_found_rather_than_forbidden() {
    // 403 would say the topic exists, and its NAME is the thing being kept back — see
    // ADR 0011. The same 404 answers a topic nobody ever typed.
    let store = fixture().await;

    let (hidden, _) = get(
        &store,
        Some("leser"),
        "/api/topics/tagged/kuendigung-mietvertrag",
    )
    .await;
    assert_eq!(hidden, StatusCode::NOT_FOUND);
    let (invented, _) = get(&store, Some("leser"), "/api/topics/tagged/gibt-es-nicht").await;
    assert_eq!(invented, StatusCode::NOT_FOUND);

    // Anti-vacuity: it is there, and `chef` gets it.
    let (mine, _) = get(
        &store,
        Some("chef"),
        "/api/topics/tagged/kuendigung-mietvertrag",
    )
    .await;
    assert_eq!(mine, StatusCode::OK);
}

#[tokio::test]
async fn a_topic_may_be_asked_for_by_the_name_somebody_typed() {
    let store = fixture().await;
    for spelling in ["Medizin/Darm", "medizin/darm", "/medizin/darm"] {
        let (status, _) = get(
            &store,
            Some("leser"),
            &format!("/api/topics/tagged/{spelling}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "`{spelling}` names the same topic");
    }
}

// --- a page's own filing -----------------------------------------------------------------

#[tokio::test]
async fn a_pages_topics_are_readable_by_whoever_may_read_the_page() {
    let store = fixture().await;
    let (status, body) = get(&store, Some("leser"), "/api/topics/document/offen").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(paths(&body["topics"]), ["/medizin/darm"]);
}

#[tokio::test]
async fn what_a_page_is_about_is_refused_to_somebody_who_may_not_read_it() {
    let store = fixture().await;
    let (status, _) = get(&store, Some("leser"), "/api/topics/document/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = get(&store, Some("leser"), "/api/topics/document/gibt-es-nicht").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn re_filing_a_page_replaces_its_topics_and_needs_write_on_it() {
    let store = fixture().await;

    let (status, _) = put(
        &store,
        Some("leser"),
        "/api/topics/document/geheim",
        json!({"topics": ["Neu"]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, body) = put(
        &store,
        Some("chef"),
        "/api/topics/document/geheim",
        json!({"topics": ["Medizin/Leber"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(paths(&body["topics"]), ["/medizin/leber"]);

    // Replaced, not merged — and the topic nothing carries any more is gone with it.
    let (_, after) = get(&store, Some("chef"), "/api/topics").await;
    assert_eq!(
        paths(&after["topics"]),
        ["/medizin", "/medizin/darm", "/medizin/leber"]
    );
}

#[tokio::test]
async fn a_topic_that_cannot_be_keyed_is_a_bad_request_that_names_it() {
    let store = fixture().await;
    let (status, body) = put(
        &store,
        Some("chef"),
        "/api/topics/document/geheim",
        json!({"topics": ["🧬"]}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        body["error"].as_str().unwrap_or_default().contains('🧬'),
        "a refusal nobody can act on is not a refusal: {body}"
    );
}

#[tokio::test]
async fn an_absent_page_cannot_be_filed_and_says_so_without_confirming_anything() {
    let store = fixture().await;
    let (status, _) = put(
        &store,
        Some("chef"),
        "/api/topics/document/gibt-es-nicht",
        json!({"topics": ["Neu"]}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn filing_a_page_is_refused_to_an_anonymous_caller() {
    let store = fixture().await;
    let (status, _) = put(
        &store,
        None,
        "/api/topics/document/offen",
        json!({"topics": ["Neu"]}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_page_can_be_filed_under_nothing_at_all() {
    let store = fixture().await;
    let (status, body) = put(
        &store,
        Some("chef"),
        "/api/topics/document/geheim",
        json!({"topics": []}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["topics"].as_array().unwrap().len(), 0);
}
