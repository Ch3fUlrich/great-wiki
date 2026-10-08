//! **Search finds what the caller may read, and a search for what they may not read is
//! indistinguishable from a search for nothing.**
//!
//! `gw_store::search` pins the policy — every candidate the index yields goes through the
//! accessor, and the mutation script breaks each link of that chain on purpose. This file is
//! the wire on top of it, written in the style of `withheld.rs`: the property is about what a
//! caller can *observe*, so it is asserted on the status, the headers and the body bytes.
//!
//! The fixture, every part of it load-bearing:
//!
//! | Thing | Visibility | `leser` | `chefin` | `fremde`, anonymous |
//! |---|---|---|---|---|
//! | `/offen` "Offene Seite" | public | reads | reads | reads |
//! | `/geheim` "Geheimes Rezept" | restricted | read | admin | — |
//! | topic `Kündigung Mietvertrag` | on `/geheim` only | sees | sees | — |
//! | task "Geheime Aufgabe" | on `/geheim` | sees | sees | — |
//! | `/muell` "Gelöschte Notiz" | public, in the trash | — | — | — |
//!
//! `fremde` is signed in, active, and holds no grant anywhere — the invited relative.

use axum::body::Body;
use axum::http::header::HeaderMap;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{
    Author, MoveMode, MoveOutcome, MoveRequest, NewDocument, NewTask, Store, TaskHome, TaskOutcome,
    TaskStatus, TrashOutcome,
};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn body(text: &str) -> Block {
    serde_json::from_value(json!({
        "kind": "doc",
        "content": [{"kind": "paragraph", "content": [{"kind": "text", "text": text}]}]
    }))
    .unwrap()
}

async fn page(
    store: &Store,
    parent: Option<&str>,
    slug: &str,
    title: &str,
    text: &str,
    visibility: Visibility,
    topics: &[&str],
) -> String {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(str::to_string),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility,
                body: body(text),
                sort_key: 0,
                topics: topics.iter().map(|t| t.to_string()).collect(),
            },
            None,
        )
        .await
        .unwrap()
}

async fn grant(store: &Store, path: &str, who: &Principal, permission: Permission) {
    store
        .add_grant(path, Subject::Principal(who.id.clone()), permission)
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(
        &store,
        None,
        "offen",
        "Offene Seite",
        "Linsensuppe Rezept für alle",
        Visibility::Public,
        &["Küche"],
    )
    .await;
    let geheim = page(
        &store,
        None,
        "geheim",
        "Geheimes Rezept",
        "Passwort Tresorschluessel",
        Visibility::Restricted,
        &["Kündigung Mietvertrag"],
    )
    .await;
    page(
        &store,
        None,
        "muell",
        "Gelöschte Notiz",
        "Wegwerfwort",
        Visibility::Public,
        &[],
    )
    .await;

    let mut people = Vec::new();
    for username in ["leser", "chefin", "fremde"] {
        people.push(
            store
                .create_local_principal(username, username, None, "$argon2id$fake")
                .await
                .unwrap(),
        );
    }
    grant(&store, "/geheim", &people[0], Permission::Read).await;
    grant(&store, "/geheim", &people[1], Permission::Admin).await;
    grant(&store, "/muell", &people[1], Permission::Admin).await;

    let outcome = store
        .create_task(
            &people[1],
            &NewTask {
                home: TaskHome::Anchored {
                    doc_id: geheim,
                    block_id: None,
                },
                title: "Geheime Aufgabe".into(),
                status: TaskStatus::Laeuft,
                assignee: None,
                due_at: None,
                position: 0,
            },
        )
        .await
        .unwrap();
    assert!(matches!(outcome, TaskOutcome::Done(_)), "{outcome:?}");

    let outcome = store.trash_document(&people[1], "/muell").await.unwrap();
    assert!(matches!(outcome, TrashOutcome::Done(_)), "{outcome:?}");
    Arc::new(store)
}

async fn app_as(store: &Arc<Store>, username: Option<&str>) -> axum::Router {
    let state = match username {
        Some(username) => {
            let (principal, _) = store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
            gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
        }
        None => gw_api::AppState::for_test(Arc::clone(store), None),
    };
    gw_api::build_router(state)
}

/// Everything the caller can observe about an answer: status, headers and body bytes.
#[derive(Debug, PartialEq, Eq)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

impl Answer {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }
}

async fn get_uri(store: &Arc<Store>, username: Option<&str>, uri: &str) -> Answer {
    let response = app_as(store, username)
        .await
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
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

async fn search(store: &Arc<Store>, username: Option<&str>, q: &str) -> Answer {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", q)
        .finish();
    get_uri(store, username, &format!("/api/search?{encoded}")).await
}

fn empty() -> Value {
    json!({"pages": [], "topics": [], "tasks": []})
}

/// Callers who may read nothing of `/geheim`.
const OUTSIDERS: [Option<&str>; 2] = [Some("fremde"), None];

/// Every way to reach the secret page: its title, its body, its topic, its task.
const SECRET_QUERIES: [&str; 6] = [
    "Geheimes",
    "Passwort",
    "Tresorschluessel",
    "Kündigung",
    "mietvertrag",
    "Geheime Aufgabe",
];

#[tokio::test]
async fn a_withheld_page_its_topic_and_its_task_are_absent_for_somebody_with_no_grant() {
    let store = fixture().await;
    for who in OUTSIDERS {
        for q in SECRET_QUERIES {
            let answer = search(&store, who, q).await;
            assert_eq!(answer.status, StatusCode::OK, "{who:?} {q}");
            assert_eq!(
                answer.json(),
                empty(),
                "{who:?} searching {q:?} was shown something"
            );
        }
        // The same words, riding along with a word that does match something readable: the
        // readable page is the only thing that may come back.
        let answer = search(&store, who, "Rezept").await;
        let json = answer.json();
        let pages = json["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 1, "{who:?}: {json}");
        assert_eq!(pages[0]["path"], "/offen");
        assert!(!String::from_utf8_lossy(&answer.body).contains("Geheimes"));
    }
}

#[tokio::test]
async fn a_query_matching_only_a_withheld_page_is_answered_exactly_as_one_matching_nothing() {
    let store = fixture().await;
    let long = "x".repeat(300);
    for who in OUTSIDERS {
        let nothing = search(&store, who, "zzzqxj").await;
        assert_eq!(nothing.status, StatusCode::OK);
        assert_eq!(nothing.json(), empty());
        for q in SECRET_QUERIES {
            assert_eq!(search(&store, who, q).await, nothing, "{who:?} {q:?}");
        }
        // Unsearchable queries are the same answer too — not a 400 that says "I parsed that".
        for q in ["", "   ", "***", "\"", "NEAR(", &long] {
            assert_eq!(search(&store, who, q).await, nothing, "{who:?} {q:?}");
        }
        assert_eq!(get_uri(&store, who, "/api/search").await, nothing);
        assert_eq!(get_uri(&store, who, "/api/search?q=%FF%FE").await, nothing);
        assert_eq!(get_uri(&store, who, "/api/search?q=%").await, nothing);
    }
}

#[tokio::test]
async fn somebody_who_may_read_the_page_is_shown_it_and_its_topic_and_its_task() {
    let store = fixture().await;
    // Anti-vacuity: without this, the absence tests pass for an endpoint that finds nothing.
    for who in ["leser", "chefin"] {
        let json = search(&store, Some(who), "Passwort").await.json();
        let pages = json["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 1, "{who}: {json}");
        assert_eq!(pages[0]["title"], "Geheimes Rezept");
        assert_eq!(pages[0]["path"], "/geheim");

        let json = search(&store, Some(who), "Kündigung").await.json();
        assert_eq!(json["topics"].as_array().unwrap().len(), 1, "{who}: {json}");
        assert_eq!(json["topics"][0]["name"], "Kündigung Mietvertrag");
        assert_eq!(json["topics"][0]["documents"], 1);

        let json = search(&store, Some(who), "Geheime Aufgabe").await.json();
        assert_eq!(json["tasks"].as_array().unwrap().len(), 1, "{who}: {json}");
        assert_eq!(json["tasks"][0]["title"], "Geheime Aufgabe");
        assert_eq!(json["tasks"][0]["status"], "Läuft");
        assert_eq!(json["tasks"][0]["page_path"], "/geheim");
    }
}

#[tokio::test]
async fn a_reader_finds_the_open_page_with_its_match_marked() {
    let store = fixture().await;
    for who in [Some("fremde"), Some("leser"), None] {
        let json = search(&store, who, "linsensuppe").await.json();
        let pages = json["pages"].as_array().unwrap();
        assert_eq!(pages.len(), 1, "{who:?}: {json}");
        assert_eq!(pages[0]["title"], "Offene Seite");
        assert_eq!(pages[0]["path"], "/offen");
        let snippet = pages[0]["snippet"].as_array().unwrap();
        assert!(
            snippet
                .iter()
                .any(|s| s["hit"] == true && s["text"] == "Linsensuppe"),
            "{snippet:?}"
        );
        let text: String = snippet
            .iter()
            .map(|s| s["text"].as_str().unwrap())
            .collect();
        assert_eq!(text, "Linsensuppe Rezept für alle");
    }
    // A topic on an open page is found by its folded name.
    let json = search(&store, Some("fremde"), "kuche").await.json();
    assert_eq!(json["topics"][0]["display_path"], "Küche", "{json}");
}

/// Every key at every depth of a JSON value.
fn keys(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                out.push(key.clone());
                keys(value, out);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| keys(v, out)),
        _ => {}
    }
}

#[tokio::test]
async fn the_response_has_no_key_that_could_count_what_it_hid() {
    let store = fixture().await;
    let mut all = Vec::new();
    // The richest answers there are: every group populated, and the empty one.
    for (who, q) in [
        (Some("chefin"), "Rezept"),
        (Some("chefin"), "Passwort"),
        (Some("chefin"), "Kündigung"),
        (Some("chefin"), "Aufgabe"),
        (Some("fremde"), "Rezept"),
        (Some("fremde"), "zzzqxj"),
    ] {
        keys(&search(&store, who, q).await.json(), &mut all);
    }
    all.sort();
    all.dedup();
    assert_eq!(
        all,
        vec![
            "display_path",
            "documents",
            "hit",
            "id",
            "name",
            "page_path",
            "pages",
            "path",
            "snippet",
            "status",
            "tasks",
            "text",
            "title",
            "topics"
        ]
    );
    // `documents` is the length of the list this caller would be handed under that topic,
    // and is the only number; it must equal what they can read, never what exists.
    let json = search(&store, Some("fremde"), "kuche").await.json();
    assert_eq!(json["topics"][0]["documents"], 1);
}

#[tokio::test]
async fn a_page_in_the_trash_is_not_found_by_anybody() {
    let store = fixture().await;
    for who in [Some("fremde"), Some("chefin"), None] {
        let answer = search(&store, who, "Wegwerfwort").await;
        assert_eq!(answer.json(), empty(), "{who:?}");
        let answer = search(&store, who, "Gelöschte").await;
        assert_eq!(answer.json(), empty(), "{who:?}");
    }
}

#[tokio::test]
async fn a_moved_page_is_found_at_its_new_address() {
    let store = fixture().await;
    page(&store, None, "alt", "Alt", "x", Visibility::Public, &[]).await;
    page(&store, None, "neu", "Neu", "x", Visibility::Public, &[]).await;
    page(
        &store,
        Some("/alt"),
        "ding",
        "Wanderding",
        "Alleinstellungswort",
        Visibility::Public,
        &[],
    )
    .await;
    let (chefin, _) = store
        .principal_by_username("chefin")
        .await
        .unwrap()
        .unwrap();
    grant(&store, "/alt", &chefin, Permission::Admin).await;
    grant(&store, "/neu", &chefin, Permission::Admin).await;

    let before = search(&store, Some("fremde"), "Alleinstellungswort")
        .await
        .json();
    assert_eq!(before["pages"][0]["path"], "/alt/ding");

    let outcome = store
        .move_document(
            &chefin,
            "/alt/ding",
            &MoveRequest {
                parent: Some("/neu".into()),
                title: "Wanderding".into(),
                slug: Some("ding".into()),
            },
            true,
            MoveMode::Commit,
        )
        .await
        .unwrap();
    assert!(
        matches!(outcome, MoveOutcome::Planned(ref plan) if plan.committed),
        "{outcome:?}"
    );

    let after = search(&store, Some("fremde"), "Alleinstellungswort")
        .await
        .json();
    assert_eq!(after["pages"].as_array().unwrap().len(), 1, "{after}");
    assert_eq!(after["pages"][0]["path"], "/neu/ding");
}

#[tokio::test]
async fn hostile_query_strings_are_answered_200() {
    let store = fixture().await;
    let nasty = [
        "\"",
        "\"\"",
        "'",
        "*",
        "NEAR(",
        "NEAR(a b)",
        "AND",
        "OR",
        "NOT",
        "a AND",
        "title:Geheimes",
        "body_text:Passwort",
        "Rezept*",
        "^Rezept",
        "(",
        ")",
        "Rezept OR Geheimes",
        "' OR 1=1 --",
        "\u{0}",
        "%",
        "ü",
        "𝔘𝔫𝔦",
    ];
    for who in [Some("fremde"), Some("chefin"), None] {
        for q in nasty {
            let answer = search(&store, who, q).await;
            assert_eq!(answer.status, StatusCode::OK, "{who:?} {q:?}");
            answer.json();
        }
        for uri in [
            "/api/search?q=%22",
            "/api/search?q=NEAR%28",
            "/api/search?q=a&q=b",
            "/api/search?q",
            "/api/search?q=",
            "/api/search?=",
            "/api/search?%ZZ",
        ] {
            assert_eq!(
                get_uri(&store, who, uri).await.status,
                StatusCode::OK,
                "{uri}"
            );
        }
    }
    // Hostile text neither widens nor narrows: the operators are words.
    let json = search(&store, Some("fremde"), "Rezept OR Geheimes")
        .await
        .json();
    assert_eq!(json, empty());
    let json = search(&store, Some("fremde"), "title:Geheimes")
        .await
        .json();
    assert_eq!(json, empty());
}
