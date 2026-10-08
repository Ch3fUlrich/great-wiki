//! Comments over the wire. `gw-store`'s `comments` tests pin who may read and write; this
//! file pins what the HTTP layer adds or fails to hide: any reader may comment, withheld and
//! absent are one answer on both verbs, a bad parent is 400, there is no delete, no total.

use axum::body::Body;
use axum::http::header::HeaderMap;
use axum::http::{Method, Request, StatusCode};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use gw_auth::{Permission, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn page(store: &Store, slug: &str) {
    let body: Block = serde_json::from_str(r#"{"kind":"doc"}"#).unwrap();
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: slug.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: Visibility::Restricted,
                body,
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
    page(&store, "geheim").await;
    page(&store, "zweite").await;
    for (name, grants) in [
        ("leser", vec!["/geheim", "/zweite"]),
        ("anna", vec!["/geheim"]),
        ("fremde", vec![]),
    ] {
        let p = store
            .create_local_principal(name, name, None, "h")
            .await
            .unwrap();
        for path in grants {
            store
                .add_grant(path, Subject::Principal(p.id.clone()), Permission::Read)
                .await
                .unwrap();
        }
    }
    Arc::new(store)
}

struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
    fn same_as(&self, other: &Answer) -> bool {
        self.status == other.status && self.headers == other.headers && self.body == other.body
    }
}

async fn send(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> Answer {
    let state = match who {
        Some(username) => {
            let (principal, _) = store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap();
            gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
        }
        None => gw_api::AppState::for_test(Arc::clone(store), None),
    };
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(v) => {
            builder = builder.header("content-type", "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let response = gw_api::build_router(state)
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Answer {
        status,
        headers,
        body: bytes.to_vec(),
    }
}

async fn post(store: &Arc<Store>, who: Option<&str>, path: &str, body: Value) -> Answer {
    let uri = format!("/api/comments/document/{path}");
    send(store, who, Method::POST, &uri, Some(body)).await
}

async fn get(store: &Arc<Store>, who: Option<&str>, path: &str) -> Answer {
    let uri = format!("/api/comments/document/{path}");
    send(store, who, Method::GET, &uri, None).await
}

#[tokio::test]
async fn a_reader_with_a_read_only_grant_can_post_and_list() {
    let store = fixture().await;
    let a = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "Tippfehler"}),
    )
    .await;
    assert_eq!(a.status, StatusCode::CREATED);
    let c = a.json();
    assert_eq!(c["body"], "Tippfehler");
    assert_eq!(c["author_name"], "leser");
    assert!(c["anchor"].is_null());
    let id = c["id"].as_str().unwrap().to_string();
    let r = post(
        &store,
        Some("anna"),
        "geheim",
        json!({"body": "stimmt", "parent_id": id}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED);

    let l = get(&store, Some("anna"), "geheim").await;
    assert_eq!(l.status, StatusCode::OK);
    let list = l.json();
    let threads = list["threads"].as_array().unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0]["id"], id.as_str());
    assert_eq!(threads[0]["replies"][0]["body"], "stimmt");
    // No internal identifiers leave.
    let text = list.to_string();
    assert!(!text.contains("doc_id") && !text.contains("author_id"));
}

#[tokio::test]
async fn withheld_and_absent_are_byte_identical_on_get_and_post() {
    let store = fixture().await;
    for who in [Some("fremde"), None] {
        let w = get(&store, who, "geheim").await;
        let a = get(&store, who, "gibt-es-nicht").await;
        assert_eq!(w.status, StatusCode::NOT_FOUND);
        assert!(w.same_as(&a), "GET differs for {who:?}");
    }
    // POST, signed in without a grant. One body is invalid on purpose: the page check must
    // come first.
    for body in [json!({"body": "x"}), json!({"body": ""})] {
        let w = post(&store, Some("fremde"), "geheim", body.clone()).await;
        let a = post(&store, Some("fremde"), "gibt-es-nicht", body).await;
        assert_eq!(w.status, StatusCode::NOT_FOUND);
        assert!(w.same_as(&a));
    }
    // Anonymous POST is 401 for every path alike, so it tells nothing about any page.
    let w = post(&store, None, "geheim", json!({"body": "x"})).await;
    let a = post(&store, None, "gibt-es-nicht", json!({"body": "x"})).await;
    assert_eq!(w.status, StatusCode::UNAUTHORIZED);
    assert!(w.same_as(&a));
    // And nothing was written by any of it.
    let l = get(&store, Some("leser"), "geheim").await.json();
    assert!(l["threads"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn a_parent_from_another_page_or_a_reply_is_400() {
    let store = fixture().await;
    let top = post(&store, Some("leser"), "zweite", json!({"body": "oben"}))
        .await
        .json();
    let other = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "x", "parent_id": top["id"]}),
    )
    .await;
    assert_eq!(other.status, StatusCode::BAD_REQUEST);
    let none = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "x", "parent_id": "nope"}),
    )
    .await;
    assert_eq!(none.status, StatusCode::BAD_REQUEST);
    let reply = post(
        &store,
        Some("leser"),
        "zweite",
        json!({"body": "r", "parent_id": top["id"]}),
    )
    .await
    .json();
    let nested = post(
        &store,
        Some("leser"),
        "zweite",
        json!({"body": "rr", "parent_id": reply["id"]}),
    )
    .await;
    assert_eq!(nested.status, StatusCode::BAD_REQUEST);
    // A caller who cannot read the page never learns about parents: 404, not 400.
    let hidden = post(
        &store,
        Some("anna"),
        "zweite",
        json!({"body": "x", "parent_id": top["id"]}),
    )
    .await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_mention_reaches_the_mentioned_reader_through_notifications() {
    let store = fixture().await;
    let made = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "schau @anna"}),
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED);
    let n = send(
        &store,
        Some("anna"),
        Method::GET,
        "/api/notifications",
        None,
    )
    .await
    .json();
    let items = n["notifications"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["kind"], "mention");
    assert_eq!(items[0]["page"]["path"], "/geheim");
    // fremde was not mentioned and holds no grant.
    let n = send(
        &store,
        Some("fremde"),
        Method::GET,
        "/api/notifications",
        None,
    )
    .await
    .json();
    assert!(n["notifications"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_anchored_comment_round_trips_as_base64() {
    let store = fixture().await;
    let doc = gw_collab::CollabDoc::new();
    doc.append_paragraph("Hello brave world");
    let (s, e, quote) = gw_collab::anchor::anchor_from_range(&doc, 6, 11).unwrap();
    let anchor = json!({"start": STANDARD.encode(&s), "end": STANDARD.encode(&e), "quote": quote});
    let a = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "hier", "anchor": anchor}),
    )
    .await;
    assert_eq!(a.status, StatusCode::CREATED);
    assert_eq!(a.json()["anchor"], anchor);
    assert_eq!(a.json()["orphaned"], false);
    let l = get(&store, Some("leser"), "geheim").await.json();
    assert_eq!(l["threads"][0]["anchor"], anchor);
}

#[tokio::test]
async fn a_bad_anchor_or_body_is_400() {
    let store = fixture().await;
    let good = STANDARD.encode([1u8]);
    let junk = STANDARD.encode([255u8, 1]);
    for anchor in [
        json!({"start": "%%%", "end": good, "quote": "q"}),
        json!({"start": junk, "end": junk, "quote": "q"}),
        json!({"start": good, "end": good, "quote": "x".repeat(201)}),
    ] {
        let a = post(
            &store,
            Some("leser"),
            "geheim",
            json!({"body": "x", "anchor": anchor}),
        )
        .await;
        assert_eq!(a.status, StatusCode::BAD_REQUEST, "{anchor}");
    }
    for body in ["", "   "] {
        let a = post(&store, Some("leser"), "geheim", json!({"body": body})).await;
        assert_eq!(a.status, StatusCode::BAD_REQUEST);
    }
    let long = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "x".repeat(8001)}),
    )
    .await;
    assert_eq!(long.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn comments_cannot_be_deleted_or_replaced() {
    let store = fixture().await;
    let c = post(&store, Some("leser"), "geheim", json!({"body": "bleibt"}))
        .await
        .json();
    let id = c["id"].as_str().unwrap();
    for uri in [
        "/api/comments/document/geheim".to_string(),
        format!("/api/comments/document/geheim/{id}"),
        format!("/api/comments/{id}"),
    ] {
        for method in [Method::DELETE, Method::PUT, Method::PATCH] {
            let a = send(
                &store,
                Some("leser"),
                method.clone(),
                &uri,
                Some(json!({"body": "neu"})),
            )
            .await;
            assert!(
                matches!(
                    a.status,
                    StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
                ),
                "{method} {uri} answered {}",
                a.status
            );
        }
    }
    let l = get(&store, Some("leser"), "geheim").await.json();
    assert_eq!(l["threads"][0]["body"], "bleibt");
    // And the store has no way to delete one either.
    let src = include_str!("../../gw-store/src/comments.rs");
    let production = src.split("#[cfg(test)]").next().unwrap().to_lowercase();
    assert!(
        !production.contains("delete from comments") && !production.contains("fn delete"),
        "the store grew a delete (ADR 0025)"
    );
}

#[tokio::test]
async fn the_list_carries_no_total_or_hidden_field() {
    let store = fixture().await;
    post(&store, Some("leser"), "geheim", json!({"body": "eins"})).await;
    let l = get(&store, Some("leser"), "geheim").await.json();
    let keys: Vec<&String> = l.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["threads"]);
}

async fn act(store: &Arc<Store>, who: Option<&str>, id: &str, verb: &str) -> Answer {
    let uri = format!("/api/comments/{id}/{verb}");
    send(store, who, Method::POST, &uri, None).await
}

#[tokio::test]
async fn a_read_only_reader_resolves_and_reopens_and_the_thread_stays_listed() {
    let store = fixture().await;
    let top = post(&store, Some("anna"), "geheim", json!({"body": "frage"}))
        .await
        .json();
    let id = top["id"].as_str().unwrap().to_string();
    let reply = post(
        &store,
        Some("leser"),
        "geheim",
        json!({"body": "antwort", "parent_id": id}),
    )
    .await
    .json();
    // Resolving by a reply id resolves the thread root.
    let rid = reply["id"].as_str().unwrap();
    let r = act(&store, Some("leser"), rid, "resolve").await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json()["id"], id.as_str());
    assert_eq!(r.json()["resolved"], true);
    let l = get(&store, Some("anna"), "geheim").await.json();
    assert_eq!(l["threads"][0]["resolved"], true);
    assert_eq!(l["threads"][0]["replies"][0]["body"], "antwort");
    let o = act(&store, Some("anna"), &id, "reopen").await;
    assert_eq!(o.status, StatusCode::OK);
    assert_eq!(o.json()["resolved"], false);
    assert!(o.json()["resolved_at"].is_null());
}

#[tokio::test]
async fn resolving_a_withheld_comment_is_the_404_of_an_absent_one_and_changes_nothing() {
    let store = fixture().await;
    let top = post(&store, Some("anna"), "geheim", json!({"body": "x"}))
        .await
        .json();
    let id = top["id"].as_str().unwrap();
    let nil = "00000000-0000-0000-0000-000000000000";
    for verb in ["resolve", "reopen"] {
        let withheld = act(&store, Some("fremde"), id, verb).await;
        let absent = act(&store, Some("fremde"), nil, verb).await;
        assert_eq!(withheld.status, StatusCode::NOT_FOUND);
        assert!(
            withheld.same_as(&absent),
            "{verb} told withheld from absent"
        );
        assert_eq!(
            act(&store, None, id, verb).await.status,
            StatusCode::UNAUTHORIZED
        );
    }
    let l = get(&store, Some("anna"), "geheim").await.json();
    assert_eq!(l["threads"][0]["resolved"], false);
}

#[tokio::test]
async fn a_removed_grant_ends_the_right_to_resolve() {
    let store = fixture().await;
    let top = post(&store, Some("anna"), "geheim", json!({"body": "x"}))
        .await
        .json();
    let id = top["id"].as_str().unwrap();
    let (anna, _) = store.principal_by_username("anna").await.unwrap().unwrap();
    store
        .remove_grant("/geheim", &Subject::Principal(anna.id), Permission::Read)
        .await
        .unwrap();
    assert_eq!(
        act(&store, Some("anna"), id, "resolve").await.status,
        StatusCode::NOT_FOUND
    );
    let l = get(&store, Some("leser"), "geheim").await.json();
    assert_eq!(l["threads"][0]["resolved"], false);
}
