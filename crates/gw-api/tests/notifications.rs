//! The notification inbox over the wire. `gw-store`'s `events` tests pin who is told what;
//! this file pins what the HTTP layer adds or fails to hide: identical 404s, no total, 401
//! for nobody.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::events::{EventKind, NewEvent};
use gw_store::{Author, NewDocument, Store};
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

struct Fixture {
    store: Arc<Store>,
    doc: String,
    actor: Principal,
    reader: Principal,
}

async fn emit(store: &Store, to: &Principal, by: &Principal, doc: &str, subject: &str) {
    store
        .emit_event(&NewEvent {
            kind: EventKind::Mention,
            recipient: to.id.clone(),
            actor: Some(by.id.clone()),
            doc_id: Some(doc.to_string()),
            path: None,
            subject: Some(subject.into()),
            dedupe_key: None,
        })
        .await
        .unwrap();
}

async fn fixture() -> Fixture {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let body: Block = serde_json::from_str(r#"{"kind":"doc"}"#).unwrap();
    let doc = store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: "Geheim".into(),
                slug: None,
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
    let path = "/geheim".to_string();
    let actor = store
        .create_local_principal("autor", "Autorin", None, "h")
        .await
        .unwrap();
    let reader = store
        .create_local_principal("leser", "Leser", None, "h")
        .await
        .unwrap();
    store
        .create_local_principal("fremder", "Fremder", None, "h")
        .await
        .unwrap();
    store
        .add_grant(
            &path,
            Subject::Principal(reader.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();
    emit(&store, &reader, &actor, &doc, "c1").await;
    Fixture {
        store: Arc::new(store),
        doc,
        actor,
        reader,
    }
}

async fn raw(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
) -> (StatusCode, String) {
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
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let response = gw_api::build_router(state).oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn json(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
) -> (StatusCode, Value) {
    let (s, t) = raw(store, who, method, uri).await;
    (s, serde_json::from_str(&t).unwrap_or(Value::Null))
}

#[tokio::test]
async fn list_count_and_mark_read_work_for_the_recipient() {
    let f = fixture().await;
    let (s, list) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications?limit=10",
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let items = list["notifications"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let n = &items[0];
    assert_eq!(n["kind"], "mention");
    assert_eq!(n["read"], false);
    assert_eq!(n["actor_name"], "Autorin");
    assert_eq!(n["page"]["title"], "Geheim");
    assert!(n["page"]["path"].as_str().unwrap().starts_with('/'));
    assert!(n["created_at"].is_string());
    let text = list.to_string();
    assert!(!text.contains(&f.doc) && !text.contains(&f.actor.id) && !text.contains(&f.reader.id));

    let (_, c) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications/unread-count",
    )
    .await;
    assert_eq!(c["count"], 1);

    let id = n["id"].as_str().unwrap();
    let (s, _) = raw(
        &f.store,
        Some("leser"),
        Method::POST,
        &format!("/api/notifications/{id}/read"),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, c) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications/unread-count",
    )
    .await;
    assert_eq!(c["count"], 0);

    emit(&f.store, &f.reader, &f.actor, &f.doc, "c2").await;
    let (_, c) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications/unread-count",
    )
    .await;
    assert_eq!(c["count"], 1);
    let (s, _) = raw(
        &f.store,
        Some("leser"),
        Method::POST,
        "/api/notifications/read-all",
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, c) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications/unread-count",
    )
    .await;
    assert_eq!(c["count"], 0);
}

#[tokio::test]
async fn a_reader_whose_grant_was_removed_sees_nothing() {
    let f = fixture().await;
    let path = "/geheim".to_string();
    f.store
        .remove_grant(
            &path,
            &Subject::Principal(f.reader.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();
    let (_, list) = json(&f.store, Some("leser"), Method::GET, "/api/notifications").await;
    assert_eq!(list["notifications"].as_array().unwrap().len(), 0);
    let (_, c) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications/unread-count",
    )
    .await;
    assert_eq!(c["count"], 0);
}

#[tokio::test]
async fn somebody_elses_event_and_a_nonexistent_one_answer_byte_identically() {
    let f = fixture().await;
    let (_, list) = json(&f.store, Some("leser"), Method::GET, "/api/notifications").await;
    let id = list["notifications"][0]["id"].as_str().unwrap().to_string();

    let theirs = raw(
        &f.store,
        Some("fremder"),
        Method::POST,
        &format!("/api/notifications/{id}/read"),
    )
    .await;
    let absent = raw(
        &f.store,
        Some("fremder"),
        Method::POST,
        "/api/notifications/no-such-id/read",
    )
    .await;
    assert_eq!(theirs.0, StatusCode::NOT_FOUND);
    assert_eq!(theirs, absent);

    // And the owner is told nothing different once the page is withheld from them.
    let path = "/geheim".to_string();
    f.store
        .remove_grant(
            &path,
            &Subject::Principal(f.reader.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();
    let withheld = raw(
        &f.store,
        Some("leser"),
        Method::POST,
        &format!("/api/notifications/{id}/read"),
    )
    .await;
    assert_eq!(withheld, absent);
}

#[tokio::test]
async fn anonymous_is_401_on_every_route() {
    let f = fixture().await;
    for (m, u) in [
        (Method::GET, "/api/notifications"),
        (Method::GET, "/api/notifications/unread-count"),
        (Method::POST, "/api/notifications/read-all"),
        (Method::POST, "/api/notifications/x/read"),
    ] {
        let (s, _) = raw(&f.store, None, m, u).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{u}");
    }
}

#[tokio::test]
async fn a_list_response_carries_no_total_or_hidden_field() {
    let f = fixture().await;
    let (_, list) = json(&f.store, Some("leser"), Method::GET, "/api/notifications").await;
    let keys: Vec<&String> = list.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["notifications"]);
    let (_, c) = json(
        &f.store,
        Some("leser"),
        Method::GET,
        "/api/notifications/unread-count",
    )
    .await;
    let keys: Vec<&String> = c.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["count"]);
}
