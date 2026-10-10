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

// --- schema CRUD: /api/datasets/schema/{path} ---------------------------------------------

use serde_json::json;

const SCHEMA: &str = "/api/datasets/schema/raum/tabelle";

async fn with_dataset() -> Arc<Store> {
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
    store
}

async fn add(store: &Arc<Store>, who: &str, key: &str, kind: &str) -> (StatusCode, String) {
    send(
        store,
        Some(who),
        "POST",
        SCHEMA,
        Some(json!({ "key": key, "label": key.to_uppercase(), "kind": kind })),
    )
    .await
}

fn keys_of(body: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(body).unwrap();
    v["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn a_writer_builds_and_reads_back_a_schema() {
    let store = with_dataset().await;
    let (s, b) = add(&store, "schreiber", "name", "text").await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let (s, b) = add(&store, "schreiber", "alter", "number").await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    // A duplicate key is a conflict; a hostile one a bad request.
    assert_eq!(
        add(&store, "schreiber", "name", "text").await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        add(&store, "schreiber", "a') OR 1=1 --", "text").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        add(&store, "schreiber", "f", "formula").await.0,
        StatusCode::BAD_REQUEST
    );
    // Rename the label; reorder; the reader sees the result.
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "PATCH",
        SCHEMA,
        Some(json!({ "key": "name", "label": "Vorname" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b.contains("Vorname"), "{b}");
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "PUT",
        SCHEMA,
        Some(json!({ "keys": ["alter", "name"] })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = send(&store, Some("leser"), "GET", SCHEMA, None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(keys_of(&b), ["alter", "name"]);
    assert!(b.contains("Vorname"), "{b}");
    let (s, _) = send(
        &store,
        Some("schreiber"),
        "DELETE",
        SCHEMA,
        Some(json!({ "key": "alter" })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, b) = send(&store, Some("schreiber"), "GET", SCHEMA, None).await;
    assert_eq!(keys_of(&b), ["name"]);
    let (s, _) = send(
        &store,
        Some("schreiber"),
        "DELETE",
        SCHEMA,
        Some(json!({ "key": "alter" })),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn key_and_kind_cannot_be_changed_through_the_rename() {
    let store = with_dataset().await;
    add(&store, "schreiber", "name", "text").await;
    for body in [
        json!({ "key": "name", "label": "X", "kind": "number" }),
        json!({ "key": "name", "label": "X", "new_key": "other" }),
    ] {
        let (s, _) = send(&store, Some("schreiber"), "PATCH", SCHEMA, Some(body)).await;
        assert!(s.is_client_error(), "{s}");
    }
    let (_, b) = send(&store, Some("schreiber"), "GET", SCHEMA, None).await;
    assert_eq!(keys_of(&b), ["name"]);
    assert!(b.contains("\"kind\":\"text\""), "{b}");
    assert!(!b.contains("\"label\":\"X\""), "{b}");
}

#[tokio::test]
async fn a_reader_gets_forbidden_and_a_stranger_gets_nothing_to_tell_apart() {
    let store = with_dataset().await;
    add(&store, "schreiber", "name", "text").await;
    // Reader: the normal forbidden, for every verb, and nothing changed.
    let (s, _) = add(&store, "leser", "x", "text").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    for (m, body) in [
        ("PATCH", json!({ "key": "name", "label": "Z" })),
        ("PUT", json!({ "keys": ["name"] })),
        ("DELETE", json!({ "key": "name" })),
    ] {
        let (s, _) = send(&store, Some("leser"), m, SCHEMA, Some(body)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m}");
    }
    let (_, b) = send(&store, Some("schreiber"), "GET", SCHEMA, None).await;
    assert_eq!(keys_of(&b), ["name"]);
    assert!(!b.contains("\"Z\""), "{b}");
    // Stranger (no grant) and a path that does not exist: same status, same bytes.
    let (s1, b1) = send(&store, Some("fremde"), "GET", SCHEMA, None).await;
    let (s2, b2) = send(
        &store,
        Some("fremde"),
        "GET",
        "/api/datasets/schema/raum/nichts",
        None,
    )
    .await;
    assert_eq!(s1, StatusCode::NOT_FOUND);
    assert_eq!((s1, &b1), (s2, &b2));
    let (s3, b3) = add(&store, "fremde", "x", "text").await;
    assert_eq!((s3, b3), (s1, b1));
    // Anonymous: nothing to read on a restricted dataset either.
    let (s, _) = send(&store, None, "GET", SCHEMA, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_hundred_and_first_field_is_a_bad_request() {
    let store = with_dataset().await;
    for i in 0..100 {
        let (s, b) = add(&store, "schreiber", &format!("f{i}"), "text").await;
        assert_eq!(s, StatusCode::CREATED, "{i}: {b}");
    }
    assert_eq!(
        add(&store, "schreiber", "extra", "text").await.0,
        StatusCode::BAD_REQUEST
    );
}

// --- row CRUD: /api/datasets/rows/{path} ----------------------------------------------------

const ROWS: &str = "/api/datasets/rows/raum/tabelle";

async fn with_columns() -> Arc<Store> {
    let store = with_dataset().await;
    add(&store, "schreiber", "name", "text").await;
    add(&store, "schreiber", "alter", "number").await;
    store
}

async fn new_row(store: &Arc<Store>, values: serde_json::Value) -> serde_json::Value {
    let (s, b) = send(
        store,
        Some("schreiber"),
        "POST",
        ROWS,
        Some(json!({ "values": values })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    serde_json::from_str(&b).unwrap()
}

#[tokio::test]
async fn a_writer_creates_reads_updates_and_deletes_a_row() {
    let store = with_columns().await;
    let row = new_row(&store, json!({ "name": "Ada", "alter": 36 })).await;
    assert_eq!(row["version"], 1);
    assert_eq!(row["values"]["name"], "Ada");
    let id = row["id"].as_str().unwrap().to_string();
    let one = format!("{ROWS}?id={id}");
    let (s, b) = send(&store, Some("leser"), "GET", &one, None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b.contains("Ada"), "{b}");
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "PATCH",
        ROWS,
        Some(json!({ "id": id, "version": 1, "values": { "name": "Eva" } })),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["version"], 2);
    assert_eq!(v["values"]["name"], "Eva");
    assert_eq!(v["values"]["alter"], 36);
    let (s, _) = send(
        &store,
        Some("schreiber"),
        "DELETE",
        ROWS,
        Some(json!({ "id": id })),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = send(&store, Some("schreiber"), "GET", &one, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_values_are_a_400_and_a_stale_version_a_409_with_the_current_row() {
    let store = with_columns().await;
    for bad in [json!({ "alter": "viel" }), json!({ "nope": 1 })] {
        let (s, _) = send(
            &store,
            Some("schreiber"),
            "POST",
            ROWS,
            Some(json!({ "values": bad })),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let row = new_row(&store, json!({ "name": "Ada" })).await;
    let id = row["id"].as_str().unwrap();
    let patch = |version: i64, name: &str| json!({ "id": id, "version": version, "values": { "name": name } });
    let (s, _) = send(
        &store,
        Some("schreiber"),
        "PATCH",
        ROWS,
        Some(patch(1, "Bo")),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "PATCH",
        ROWS,
        Some(patch(1, "Cy")),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["current"]["version"], 2, "{b}");
    assert_eq!(v["current"]["values"]["name"], "Bo", "{b}");
}

#[tokio::test]
async fn row_access_mirrors_the_schema_door() {
    let store = with_columns().await;
    let row = new_row(&store, json!({ "name": "Ada" })).await;
    let id = row["id"].as_str().unwrap();
    // A reader gets the ordinary 403; a visitor with no grant, the bare 404.
    let (s, _) = send(
        &store,
        Some("leser"),
        "POST",
        ROWS,
        Some(json!({ "values": {} })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = send(&store, None, "POST", ROWS, Some(json!({ "values": {} }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = send(
        &store,
        Some("leser"),
        "DELETE",
        ROWS,
        Some(json!({ "id": id })),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    // A stranger and a path that does not exist: same status, same bytes.
    let one = format!("{ROWS}?id={id}");
    let (s1, b1) = send(&store, Some("fremde"), "GET", &one, None).await;
    let (s2, b2) = send(
        &store,
        Some("fremde"),
        "GET",
        &format!("/api/datasets/rows/raum/nichts?id={id}"),
        None,
    )
    .await;
    assert_eq!(s1, StatusCode::NOT_FOUND);
    assert_eq!((s1, b1), (s2, b2));
}

// --- A7: one access seam. An unreadable dataset is indistinguishable from an absent one. ---

const GHOST: &str = "/api/datasets/rows/raum/nichts";

async fn with_chef(store: &Arc<Store>) {
    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();
}

/// Everything a caller can ask of a dataset's rows and schema, as (status, body) pairs.
async fn probes(
    store: &Arc<Store>,
    who: Option<&str>,
    base: &str,
    id: &str,
) -> Vec<(StatusCode, String)> {
    let schema = base.replace("/rows/", "/schema/");
    vec![
        send(store, who, "GET", base, None).await,
        send(store, who, "GET", &format!("{base}?limit=1"), None).await,
        send(store, who, "GET", &format!("{base}?id={id}"), None).await,
        send(store, who, "GET", &schema, None).await,
    ]
}

#[tokio::test]
async fn an_unreadable_dataset_answers_the_same_bytes_as_an_absent_one() {
    let store = with_columns().await;
    with_chef(&store).await;
    let row = new_row(&store, json!({ "name": "Ada" })).await;
    let id = row["id"].as_str().unwrap().to_string();

    // Guest and a user whose grants are on other pages only.
    for who in [None, Some("fremde")] {
        let real = probes(&store, who, ROWS, &id).await;
        let ghost = probes(&store, who, GHOST, &id).await;
        assert_eq!(real, ghost, "{who:?}");
        for (s, _) in &real {
            assert_eq!(*s, StatusCode::NOT_FOUND, "{who:?}");
        }
        assert!(
            real.iter()
                .all(|(_, b)| !b.contains("Ada") && !b.contains("total")),
            "{who:?}: {real:?}"
        );
    }

    // Admin reads everything (positive control: the checks above are not vacuous), and an
    // absent page is still a 404 for them.
    let (s, b) = send(&store, Some("chef"), "GET", ROWS, None).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert!(b.contains("Ada") && b.contains("\"total\":1"), "{b}");
    let (s, _) = send(&store, Some("chef"), "GET", GHOST, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Admin viewing as the stranger sees what the stranger sees: byte for byte.
    let (chef, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    let (fremde, _) = store
        .principal_by_username("fremde")
        .await
        .unwrap()
        .unwrap();
    let router = gw_api::build_router(gw_api::AppState::for_test_principal(
        Arc::clone(&store),
        &chef,
    ));
    let start = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/admin/view-as/{}", fremde.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cookie = start
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.contains("view_as="))
        .map(|v| v.split(';').next().unwrap().to_string())
        .expect("view-as issues a cookie");
    let as_view = |uri: String| {
        let router = router.clone();
        let cookie = cookie.clone();
        async move {
            let r = router
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("cookie", cookie)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let s = r.status();
            let b = axum::body::to_bytes(r.into_body(), usize::MAX)
                .await
                .unwrap();
            (s, String::from_utf8(b.to_vec()).unwrap())
        }
    };
    let viewed = as_view(ROWS.to_string()).await;
    let ghost = as_view(GHOST.to_string()).await;
    assert_eq!(viewed.0, StatusCode::NOT_FOUND, "{viewed:?}");
    assert_eq!(viewed, ghost);
}

#[tokio::test]
async fn the_count_is_this_datasets_own_and_follows_the_page() {
    let store = with_columns().await;
    // A second dataset with its own rows must not leak into the first one's count.
    let (s, b) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/datasets",
        Some(json!({ "parent": "/raum", "title": "Zweite" })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    for _ in 0..2 {
        new_row(&store, json!({})).await;
    }
    let (s, _) = send(
        &store,
        Some("schreiber"),
        "POST",
        "/api/datasets/rows/raum/zweite",
        Some(json!({ "values": {} })),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, b) = send(
        &store,
        Some("leser"),
        "GET",
        &format!("{ROWS}?limit=1"),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["total"], 2, "{b}");
    assert_eq!(v["rows"].as_array().unwrap().len(), 1, "{b}");
}
