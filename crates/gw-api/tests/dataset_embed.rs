//! `GET /api/datasets/embed/{dataset_doc}/{view}` — the read a `dataset-view` block makes
//! over HTTP (ADR 0029 §8, ADR 0022).
//!
//! The permission tested here is the DATASET's. The embedding page is not part of the
//! request at all, and a caller who may read it must learn nothing about a dataset they
//! cannot read — not a title, not a path, not even that the ids point at anything. So the
//! central assertions compare raw bytes: `absent`, `a plain page`, `an unreadable dataset`
//! and `a page in the trash` must serialise to one and the same response.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, BlockKind, DocumentType, FieldKind, Visibility};
use gw_store::datasets::{NewField, SchemaOutcome};
use gw_store::{Author, CreateRequest, NewDocument, Store};
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

fn admin() -> Principal {
    Principal::test("chef", &["admins"], &[])
}

/// One GET, answered as bytes: status, body and the whole header list, because ADR 0022 is a
/// statement about what goes on the wire and not about what a struct in memory holds.
async fn get(
    store: &Arc<Store>,
    who: Option<&Principal>,
    uri: &str,
) -> (StatusCode, String, Vec<(String, String)>) {
    let state = match who {
        None => gw_api::AppState::for_test(Arc::clone(store), None),
        Some(p) => gw_api::AppState::for_test_principal(Arc::clone(store), p),
    };
    let r = gw_api::build_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status();
    let headers: Vec<(String, String)> = r
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let bytes = axum::body::to_bytes(r.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap(), headers)
}

struct Fixture {
    store: Arc<Store>,
    /// Owns everything below.
    anna: Principal,
    /// Reads `/raum` — the embedding page, the plain page, the dataset in it — and nothing
    /// under `/geheim`.
    bea: Principal,
    /// Reads nothing.
    cleo: Principal,
    tabelle: String,
    view: String,
    ziel: String,
    ziel_view: String,
    plain: String,
    trashed: String,
}

async fn raw_page(store: &Store, parent: Option<&str>, slug: &str, body: Block) -> String {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(Into::into),
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
        .unwrap()
}

fn empty_body() -> Block {
    serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap()
}

async fn dataset(store: &Store, anna: &Principal, parent: &str, title: &str) -> (String, String) {
    let req = CreateRequest {
        parent: Some(parent.into()),
        title: title.into(),
        slug: None,
        template: None,
    };
    match store
        .create_dataset_for(anna, &req, false, "d")
        .await
        .unwrap()
    {
        gw_store::CreateOutcome::Created { path, id } => (path, id),
        other => panic!("{other:?}"),
    }
}

async fn field(store: &Store, anna: &Principal, path: &str, f: NewField) {
    let out = store.add_dataset_field(anna, path, &f).await.unwrap();
    assert!(matches!(out, SchemaOutcome::Done(_)), "{}: {out:?}", f.key);
}

async fn saved_view(
    store: &Store,
    anna: &Principal,
    path: &str,
    config: serde_json::Value,
) -> String {
    match store
        .create_dataset_view(anna, path, "Alle", "table", &config)
        .await
        .unwrap()
    {
        SchemaOutcome::Done(v) => v.id,
        other => panic!("{other:?}"),
    }
}

/// A real account, because `AppState::for_test_principal` goes through the development
/// shim and the shim re-reads the principal from the store — an identity that has no row
/// is no identity.
async fn account(store: &Store, name: &str) -> Principal {
    store
        .create_local_principal(name, name, None, "$argon2id$fake")
        .await
        .unwrap();
    store.principal_by_username(name).await.unwrap().unwrap().0
}

/// `/raum` (write: anna; read: anna, bea) holds the dataset `Tabelle` — `name`, `alter`,
/// `status` and a `mit` relation to `/geheim/ziel` that only anna may read — with one saved
/// view naming all three; a plain page; a page in the trash; and `einbettung`, whose body
/// holds a `dataset-view` block for `/raum/tabelle` **and** one for `/geheim/ziel`.
async fn fixture() -> Fixture {
    let store = Arc::new(Store::open("sqlite::memory:").await.unwrap());
    for slug in ["raum", "geheim"] {
        raw_page(&store, None, slug, empty_body()).await;
    }
    let anna = account(&store, "anna").await;
    let bea = account(&store, "bea").await;
    let cleo = account(&store, "cleo").await;
    for (path, who, perm) in [
        ("/raum", &anna, Permission::Write),
        ("/geheim", &anna, Permission::Write),
        ("/raum", &bea, Permission::Read),
    ] {
        store
            .add_grant(path, Subject::Principal(who.id.clone()), perm)
            .await
            .unwrap();
    }
    let (tabelle_path, tabelle) = dataset(&store, &anna, "/raum", "Tabelle").await;
    let (ziel_path, ziel) = dataset(&store, &anna, "/geheim", "Ziel").await;
    for (key, kind) in [
        ("name", FieldKind::Text),
        ("alter", FieldKind::Number),
        ("status", FieldKind::Select),
    ] {
        field(
            &store,
            &anna,
            &tabelle_path,
            NewField {
                key: key.into(),
                label: key.into(),
                kind,
                config: if key == "status" {
                    json!({"options": [{"id": "offen"}, {"id": "fertig"}]})
                } else {
                    json!({})
                },
            },
        )
        .await;
    }
    field(
        &store,
        &anna,
        &tabelle_path,
        NewField {
            key: "mit".into(),
            label: "Mit".into(),
            kind: FieldKind::Relation,
            config: json!({ "target": ziel_path, "multiple": true }),
        },
    )
    .await;
    field(
        &store,
        &anna,
        &ziel_path,
        NewField {
            key: "zweck".into(),
            label: "Zweck".into(),
            kind: FieldKind::Text,
            config: json!({}),
        },
    )
    .await;
    let view = saved_view(
        &store,
        &anna,
        &tabelle_path,
        json!({"fields": ["name", "mit", "alter"], "sort": {"key": "alter"}}),
    )
    .await;
    let ziel_view = saved_view(&store, &anna, &ziel_path, json!({"fields": ["zweck"]})).await;

    let plain = raw_page(&store, Some("/raum"), "seite", empty_body()).await;
    let trashed = raw_page(&store, Some("/raum"), "mull", empty_body()).await;
    store.trash_document(&anna, "/raum/mull").await.unwrap();

    let body: Block = serde_json::from_value(json!({"kind":"doc","content":[
        {"kind": "dataset-view",
         "attrs": {"dataset_doc": tabelle, "view": view, "label": "Laborwerte"}},
        {"kind": "dataset-view",
         "attrs": {"dataset_doc": ziel, "view": ziel_view, "label": "Geheim"}}
    ]}))
    .unwrap();
    assert_eq!(body.content[0].kind, BlockKind::DatasetView);
    assert_eq!(body.content[1].kind, BlockKind::DatasetView);
    raw_page(&store, Some("/raum"), "einbettung", body).await;

    Fixture {
        store,
        anna,
        bea,
        cleo,
        tabelle,
        view,
        ziel,
        ziel_view,
        plain,
        trashed,
    }
}

fn url(doc: &str, view: &str) -> String {
    format!("/api/datasets/embed/{doc}/{view}")
}

const REFUSED: &str = r#"{"state":"unavailable"}"#;

/// The point of ADR 0022 at the HTTP layer: four different reasons, one answer, compared as
/// raw bytes for three different callers — including the anonymous one.
#[tokio::test]
async fn every_refusal_is_the_same_response_byte_for_byte() {
    let f = fixture().await;
    let absent = "0199c0de-0000-7000-8000-000000000099";
    let unknown_view = "0199c0de-0000-7000-8000-000000000077";
    // Refusals for everybody: none of these says anything to anybody.
    let shared = [
        ("an absent id", url(absent, &f.view)),
        ("a plain page", url(&f.plain, &f.view)),
        ("a page in the trash", url(&f.trashed, &f.view)),
        ("a malformed dataset id", url("kein-uuid", &f.view)),
        ("a malformed view id", url(&f.tabelle, "nicht-ein-uuid")),
        (
            "an absent view of an absent page",
            url(absent, unknown_view),
        ),
    ];
    for (who, extra) in [
        (
            Some(&f.cleo),
            ("an unreadable dataset", url(&f.tabelle, &f.view)),
        ),
        // bea reads `/raum` and not `/geheim`: the dataset she may not read is a refusal
        // for her, in the same bytes as a page that does not exist.
        (
            Some(&f.bea),
            ("an unreadable dataset", url(&f.ziel, &f.ziel_view)),
        ),
        (None, ("a readable dataset", url(&f.tabelle, &f.view))),
    ] {
        let mut answers = Vec::new();
        for (what, uri) in shared.iter().chain(std::iter::once(&extra)) {
            let r = get(&f.store, who, uri).await;
            assert_eq!(
                r.1,
                REFUSED,
                "{what} as {:?} said something else: {:?}",
                who.map(|p| p.username.as_str()),
                r
            );
            answers.push((what, r));
        }
        let first = &answers[0].1;
        for (what, answer) in &answers {
            assert_eq!(answer.0, first.0, "{what}: a different status");
            assert_eq!(&answer.1, &first.1, "{what}: a different body");
            assert_eq!(&answer.2, &first.2, "{what}: different headers");
        }
    }
}

/// A reader of the embedding page who may not read the dataset the second block points at
/// gets the identical bytes, with the block's own ids taken from the page's body.
#[tokio::test]
async fn reading_the_embedding_page_buys_nothing() {
    let f = fixture().await;
    let (s, page, _) = get(&f.store, Some(&f.bea), "/api/documents/raum/einbettung").await;
    assert_eq!(s, StatusCode::OK, "{page}");
    // A page's `body` is the stored block tree as text, so it is read as the web reads it.
    let text = serde_json::from_str::<serde_json::Value>(&page).unwrap()["body"]
        .as_str()
        .unwrap()
        .to_string();
    let body: Block = serde_json::from_str(&text).unwrap();
    let attrs = &body.content[1].attrs;
    let (doc, view) = (
        attrs["dataset_doc"].as_str().unwrap(),
        attrs["view"].as_str().unwrap(),
    );
    assert_eq!(
        doc, f.ziel,
        "the page does hand bea the secret dataset's id"
    );
    let (s, b, h) = get(&f.store, Some(&f.bea), &url(doc, view)).await;
    assert_eq!((s.as_u16(), b.as_str()), (200, REFUSED));
    assert!(
        !b.contains("Ziel") && !b.contains("Geheim") && !b.contains("/geheim"),
        "{b}"
    );
    assert!(!b.contains(&f.ziel), "{b}");
    assert!(
        !format!("{h:?}").contains(&f.ziel),
        "the refusal is in the bytes of the response, and a header is one of them: {h:?}"
    );
}

/// The successful answer, and that `view_missing` is only ever said to a reader of the
/// dataset — a stranger of it is refused even the difference between the two answers.
#[tokio::test]
async fn a_reader_of_the_dataset_gets_the_view_and_nobody_else_does() {
    let f = fixture().await;
    let (s, b, _) = get(&f.store, Some(&f.anna), &url(&f.tabelle, &f.view)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["state"], "ok");
    assert_eq!(v["path"], "/raum/tabelle");
    assert_eq!(v["title"], "Tabelle");
    assert_eq!(v["view"]["id"], f.view.as_str());
    assert_eq!(v["view"]["name"], "Alle");
    assert_eq!(v["view"]["kind"], "table");
    // The answer is config and nothing else: no rows, no count anywhere in it.
    assert!(!b.contains("rows") && !b.contains("count"), "{b}");

    let unknown = "0199c0de-0000-7000-8000-000000000077";
    let (s, b, _) = get(&f.store, Some(&f.anna), &url(&f.tabelle, unknown)).await;
    assert_eq!(
        (s.as_u16(), b.as_str()),
        (200, r#"{"state":"view_missing"}"#)
    );
    // bea reads `/raum/tabelle` too, so she is told the same; cleo is not told anything.
    let (_, for_bea, _) = get(&f.store, Some(&f.bea), &url(&f.tabelle, unknown)).await;
    assert_eq!(for_bea, r#"{"state":"view_missing"}"#, "{for_bea}");
    let (s, for_cleo, _) = get(&f.store, Some(&f.cleo), &url(&f.tabelle, unknown)).await;
    assert_eq!((s.as_u16(), for_cleo.as_str()), (200, REFUSED));
    assert_ne!(
        b, for_cleo,
        "view_missing leaked past the dataset's read gate"
    );
    // A view of another dataset, asked of this one: the same, and never the other view.
    let (_, foreign, _) = get(&f.store, Some(&f.anna), &url(&f.tabelle, &f.ziel_view)).await;
    assert_eq!(foreign, r#"{"state":"view_missing"}"#, "{foreign}");
}

/// The hidden-field strip is the same seam the view list uses — asserted over HTTP.
#[tokio::test]
async fn a_field_hidden_from_the_caller_is_stripped_from_the_answered_view() {
    let f = fixture().await;
    let (s, b, _) = get(&f.store, Some(&f.bea), &url(&f.tabelle, &f.view)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["state"], "ok", "{b}");
    let keys: Vec<&str> = v["view"]["config"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert_eq!(keys, ["name", "alter"], "{b}");
    // The owner still sees all three.
    let (_, own, _) = get(&f.store, Some(&f.anna), &url(&f.tabelle, &f.view)).await;
    let own: serde_json::Value = serde_json::from_str(&own).unwrap();
    assert_eq!(
        own["view"]["config"]["fields"].as_array().unwrap().len(),
        3,
        "{own}"
    );
}

/// An anonymous guest is refused in the same bytes, and the route answers them at all — a
/// 401 or a 404 would be a different response, which is what ADR 0022 forbids.
#[tokio::test]
async fn an_anonymous_guest_gets_the_uniform_refusal() {
    let f = fixture().await;
    let (s, b, _) = get(&f.store, None, &url(&f.tabelle, &f.view)).await;
    assert_eq!(s, StatusCode::OK, "the endpoint must answer a guest: {b}");
    assert_eq!(b, REFUSED, "{b}");
}

/// The admin baseline reads everything, and `ok` says so — the positive control that the
/// refusals above are about a dataset's ACL and not about a broken route.
#[tokio::test]
async fn an_admin_reading_everything_gets_ok() {
    let f = fixture().await;
    let (s, b, _) = get(&f.store, Some(&admin()), &url(&f.tabelle, &f.view)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let v: serde_json::Value = serde_json::from_str(&b).unwrap();
    assert_eq!(v["state"], "ok", "{b}");
    assert_eq!(v["path"], "/raum/tabelle", "{b}");
}
