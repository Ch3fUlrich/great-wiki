//! The M2 exit criterion, written down as a test.
//!
//! The milestone plan states it in one sentence:
//!
//! > A guest account can be created, put in a team, granted read on one subtree, and
//! > **provably** cannot reach anything else.
//!
//! Every clause of that is covered somewhere by a narrower test. This file exists because
//! the criterion is about the COMPOSITION: an account, a team, and a team-scoped grant are
//! three mechanisms that each work in isolation, and "provably cannot reach anything else"
//! is a claim about what happens when they are combined by an administrator using nothing
//! but the HTTP API. So nothing here reaches into the store — every step is a request a
//! console would make, in the order a person would make them.
//!
//! The load-bearing assertion is the last one. It removes the team membership and shows
//! the reach disappears with it, which is what makes "put in a team" the reason the guest
//! can read `/raum` rather than a step that happened to be performed nearby. Without it
//! this test would pass just as well if team membership conferred nothing at all and the
//! guest were reading `/raum` for some entirely different reason.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{NewDocument, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

/// Long enough to clear the length floor, so the account is refused by nothing.
const PASSPHRASE: &str = "ein-vollkommen-brauchbarer-satz";

fn body() -> Block {
    serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap()
}

async fn insert(store: &Store, parent: Option<&str>, slug: &str, title: &str, vis: Visibility) {
    store
        .insert_document(&NewDocument {
            parent_path: parent.map(str::to_string),
            doc_type: DocumentType::Page,
            title: title.into(),
            slug: Some(slug.into()),
            language: "de".into(),
            visibility: vis,
            body: body(),
            sort_key: 0,
        })
        .await
        .unwrap();
}

/// Five documents, chosen so that each refusal below means something different.
///
/// - `/oeffentlich` — public, so an anonymous visitor reads it. Its presence in the guest's
///   tree is not evidence of the grant, which is why the assertions name it explicitly
///   rather than counting entries.
/// - `/intern` — internal: readable by any signed-in account. This is the one that catches
///   "having an account" being mistaken for "having been granted something".
/// - `/raum` and `/raum/unterseite` — the granted subtree. The child is there because a
///   grant that did not inherit downwards would still satisfy a test that only asked about
///   the parent.
/// - `/anderer-raum` — restricted, granted to nobody. The plain "anything else".
async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    insert(
        &store,
        None,
        "oeffentlich",
        "Öffentlich",
        Visibility::Public,
    )
    .await;
    insert(&store, None, "intern", "Intern", Visibility::Internal).await;
    insert(&store, None, "raum", "Raum", Visibility::Restricted).await;
    insert(
        &store,
        Some("/raum"),
        "unterseite",
        "Unterseite",
        Visibility::Restricted,
    )
    .await;
    insert(
        &store,
        None,
        "anderer-raum",
        "Anderer Raum",
        Visibility::Restricted,
    )
    .await;

    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();

    Arc::new(store)
}

/// A router whose requests arrive as the stored principal called `who`.
///
/// Looked up rather than invented, so no assertion here can rest on a group or an active
/// flag the database does not actually hold.
async fn router(store: &Arc<Store>, who: Option<&str>) -> axum::Router {
    let state = match who {
        Some(username) => {
            let (principal, _) = store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("`{username}` must exist"));
            gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
        }
        None => gw_api::AppState::for_test(Arc::clone(store), None),
    };
    gw_api::build_router(state)
}

async fn send(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
    payload: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match payload {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };
    let response = router(store, who).await.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// Every path the tree endpoint offers `who`, flattened and sorted.
///
/// The tree is what the interface renders, so it is the honest answer to "what can this
/// person see" — but it is not the only way in, which is why every test below also asks
/// for the documents directly.
async fn visible_tree(store: &Arc<Store>, who: Option<&str>) -> Vec<String> {
    let (status, tree) = send(store, who, Method::GET, "/api/tree", None).await;
    assert_eq!(status, StatusCode::OK);

    fn walk(nodes: &[Value], into: &mut Vec<String>) {
        for node in nodes {
            into.push(node["path"].as_str().unwrap().to_string());
            if let Some(children) = node["children"].as_array() {
                walk(children, into);
            }
        }
    }

    let mut paths = Vec::new();
    walk(tree.as_array().expect("the tree is an array"), &mut paths);
    paths.sort();
    paths
}

async fn status_of(store: &Arc<Store>, who: Option<&str>, path: &str) -> StatusCode {
    send(
        store,
        who,
        Method::GET,
        &format!("/api/documents{path}"),
        None,
    )
    .await
    .0
}

#[tokio::test]
async fn a_guest_in_a_team_reads_the_granted_subtree_and_provably_nothing_else() {
    let store = fixture().await;

    // ---- What an administrator does, entirely through the API. --------------------
    let (status, created) = send(
        &store,
        Some("chef"),
        Method::POST,
        "/api/admin/principals",
        Some(json!({
            "username": "gast",
            "display_name": "Gast Konto",
            "password": PASSPHRASE,
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let gast_id = created["id"].as_str().expect("the new account has an id");

    let (status, _) = send(
        &store,
        Some("chef"),
        Method::POST,
        "/api/admin/teams",
        Some(json!({"slug": "gaeste", "name": "Gäste"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = send(
        &store,
        Some("chef"),
        Method::POST,
        "/api/admin/teams/gaeste/members",
        Some(json!({"principal_id": gast_id})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // The grant names the TEAM, never the account. That is what makes the membership the
    // thing under test rather than a detail alongside a personal grant.
    let (status, _) = send(
        &store,
        Some("chef"),
        Method::POST,
        "/api/admin/acl",
        Some(json!({
            "path": "/raum",
            "subject": {"kind": "team", "id": "gaeste"},
            "permission": "read",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // ---- What the guest can then reach. -------------------------------------------
    assert_eq!(
        visible_tree(&store, Some("gast")).await,
        vec![
            "/oeffentlich".to_string(),
            "/raum".to_string(),
            "/raum/unterseite".to_string(),
        ],
        "the granted subtree plus what is public, and nothing else"
    );

    // The tree is one door. A direct URL has to give the same answer, or the tree is a
    // decoration over an API that hands out more than it shows.
    assert_eq!(
        status_of(&store, Some("gast"), "/raum").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&store, Some("gast"), "/raum/unterseite").await,
        StatusCode::OK,
        "the grant did not inherit down the subtree"
    );
    assert_eq!(
        status_of(&store, Some("gast"), "/oeffentlich").await,
        StatusCode::OK
    );
    assert_eq!(
        status_of(&store, Some("gast"), "/intern").await,
        StatusCode::FORBIDDEN,
        "holding an account was mistaken for having been granted something"
    );
    assert_eq!(
        status_of(&store, Some("gast"), "/anderer-raum").await,
        StatusCode::FORBIDDEN,
        "a restricted space granted to nobody was readable"
    );

    // Read is read. The guest administers nothing, so the console refuses them (D-M2-8).
    let (status, _) = send(
        &store,
        Some("gast"),
        Method::GET,
        "/api/admin/acl?path=/raum",
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a read grant conferred administration of the space"
    );

    // ---- And the team is the REASON, not a coincidence. ---------------------------
    //
    // Everything above would read identically if team membership conferred nothing and the
    // guest reached `/raum` some other way. Removing the membership is the only assertion
    // that distinguishes the two, so it is the one this file exists for.
    let (status, _) = send(
        &store,
        Some("chef"),
        Method::DELETE,
        &format!("/api/admin/teams/gaeste/members/{gast_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        visible_tree(&store, Some("gast")).await,
        vec!["/oeffentlich".to_string()],
        "leaving the team did not take the reach with it"
    );
    assert_eq!(
        status_of(&store, Some("gast"), "/raum").await,
        StatusCode::FORBIDDEN,
        "the subtree was still readable after the membership that granted it was removed"
    );
}
