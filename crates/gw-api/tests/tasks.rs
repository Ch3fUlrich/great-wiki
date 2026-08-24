//! Tasks, boards and projects over HTTP.
//!
//! `gw-store`'s own `tasks` tests already pin the policy — who may see a card, who may move
//! one, who may be given one — and they are mutation-tested there. This file is about the
//! wire on top of that, and most of it is about **disclosure**.
//!
//! A board card is a page's own words on somebody else's screen (D-2). It says the page
//! exists, what it is called and what somebody wrote on it, which is the whole of what a
//! restricted title was hiding. The store filters per document (D-3), so the way this layer
//! loses the property is not by asking the wrong question but by *adding* to the answer: a
//! total, an id, a status code that differs. The tests below are written to fail if it does.
//!
//! The fixture, and every part of it is load-bearing:
//!
//! | Page | Visibility | `chef` | `leser` | `fremder` |
//! |---|---|---|---|---|
//! | `/projekt` (project home) | restricted | write | read | — |
//! | `/projekt/offen` | restricted | (inherited write) | (inherited read) | — |
//! | `/projekt/geheim` | restricted | write, granted AT that path | **nothing** | — |
//!
//! The grant defined at `/projekt/geheim` is what makes `leser`'s inherited grant stop
//! there: grants are resolved from the nearest path that carries any, so a path with its own
//! grants does not inherit. That is the shape D-3 calls normal rather than exceptional — one
//! project, two subtrees, different grants — and it is the only shape in which this
//! property can be tested at all.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use gw_auth::{Permission, Principal, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, NewTask, Store, Task, TaskHome, TaskOutcome, TaskStatus};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

// -------------------------------------------------------------------------------------
// The fixture.
// -------------------------------------------------------------------------------------

fn empty_body() -> Block {
    serde_json::from_str(r#"{"kind":"doc"}"#).unwrap()
}

/// A page holding one checklist line, so that publishing it mints a task the way a person
/// writing `- [ ] …` would (D-6).
fn checklist_body(text: &str) -> Block {
    serde_json::from_str(&format!(
        r#"{{"kind":"doc","content":[{{"kind":"taskList","content":[
             {{"kind":"taskItem","attrs":{{"checked":false}},"content":[
               {{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}]}}]}}"#
    ))
    .unwrap()
}

/// Restricted unless said otherwise, so a test that forgets a grant fails closed rather
/// than passing because everything was public.
async fn page(store: &Store, parent: Option<&str>, title: &str, body: Block) -> String {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(str::to_string),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: None,
                language: "de".into(),
                visibility: Visibility::Restricted,
                body,
                sort_key: 0,
            },
            None,
        )
        .await
        .unwrap()
}

/// A REAL account. `tasks.assignee` carries a foreign key to `principals`, and the
/// assignment rule loads the assignee's own groups to ask what THEY may read — which a
/// synthetic principal has no row to answer from.
async fn account(store: &Store, username: &str) -> Principal {
    store
        .create_local_principal(username, username, None, "irrelevanter-hash")
        .await
        .unwrap()
}

async fn grant(store: &Store, path: &str, who: &Principal, permission: Permission) {
    store
        .add_grant(path, Subject::Principal(who.id.clone()), permission)
        .await
        .unwrap();
}

fn done(outcome: TaskOutcome) -> Task {
    match outcome {
        TaskOutcome::Done(task) => *task,
        other => panic!("the fixture's own change was refused: {other:?}"),
    }
}

struct Fixture {
    store: Arc<Store>,
    project: String,
    /// The card anchored to `/projekt/geheim` — the one `leser` must never learn about.
    hidden_task: String,
    /// `/projekt/geheim`'s document id, for the tests that publish to it.
    geheim_doc: String,
}

async fn fixture() -> Fixture {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(&store, None, "Projekt", empty_body()).await;
    let offen = page(&store, Some("/projekt"), "Offen", empty_body()).await;
    let geheim = page(&store, Some("/projekt"), "Geheim", empty_body()).await;

    let chef = account(&store, "chef").await;
    let leser = account(&store, "leser").await;
    account(&store, "fremder").await;

    grant(&store, "/projekt", &chef, Permission::Write).await;
    grant(&store, "/projekt", &leser, Permission::Read).await;
    // Defined AT the secret page, which is what stops `leser`'s grant above reaching it.
    grant(&store, "/projekt/geheim", &chef, Permission::Write).await;

    let project = store
        .create_project(&chef, "/projekt", None)
        .await
        .unwrap()
        .expect("the fixture's project was refused");

    for (home, title) in [
        (
            TaskHome::Standalone {
                project_id: project.id.clone(),
            },
            "Lose Karte",
        ),
        (
            TaskHome::Anchored {
                doc_id: offen.clone(),
                block_id: None,
            },
            "Harmlos",
        ),
    ] {
        done(
            store
                .create_task(
                    &chef,
                    &NewTask {
                        home,
                        title: title.into(),
                        status: TaskStatus::Offen,
                        assignee: None,
                        due_at: None,
                        position: 0,
                    },
                )
                .await
                .unwrap(),
        );
    }

    let hidden = done(
        store
            .create_task(
                &chef,
                &NewTask {
                    home: TaskHome::Anchored {
                        doc_id: geheim.clone(),
                        block_id: None,
                    },
                    title: "Befund besprechen".into(),
                    status: TaskStatus::Offen,
                    assignee: None,
                    due_at: None,
                    position: 0,
                },
            )
            .await
            .unwrap(),
    );

    Fixture {
        store: Arc::new(store),
        project: project.id,
        hidden_task: hidden.id,
        geheim_doc: geheim,
    }
}

// -------------------------------------------------------------------------------------
// The wire.
// -------------------------------------------------------------------------------------

async fn router(store: &Arc<Store>, who: Option<&str>) -> axum::Router {
    let state = match who {
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

/// The raw bytes, because several tests below are about what the body does NOT contain and
/// a parsed `Value` has already thrown away the only evidence.
async fn raw(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, String) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
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
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn send(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let (status, text) = raw(store, who, method, uri, body).await;
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

async fn get(store: &Arc<Store>, who: Option<&str>, uri: &str) -> (StatusCode, Value) {
    send(store, who, Method::GET, uri, None).await
}

async fn post(
    store: &Arc<Store>,
    who: Option<&str>,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(store, who, Method::POST, uri, Some(body)).await
}

async fn patch(
    store: &Arc<Store>,
    who: Option<&str>,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(store, who, Method::PATCH, uri, Some(body)).await
}

async fn delete(store: &Arc<Store>, who: Option<&str>, uri: &str) -> (StatusCode, Value) {
    send(store, who, Method::DELETE, uri, None).await
}

/// Every card on a board, flattened out of its columns, in the order the columns give.
fn cards(board: &Value) -> Vec<Value> {
    board["columns"]
        .as_array()
        .unwrap_or_else(|| panic!("no columns in {board}"))
        .iter()
        .flat_map(|column| column["tasks"].as_array().unwrap().clone())
        .collect()
}

fn titles(board: &Value) -> Vec<String> {
    cards(board)
        .iter()
        .map(|task| task["title"].as_str().unwrap().to_string())
        .collect()
}

// -------------------------------------------------------------------------------------
// The routing trap. This repository has been bitten by it twice.
// -------------------------------------------------------------------------------------

/// matchit prefers a literal segment over a catch-all, so `/api/documents/{id}/tasks` would
/// shadow `GET /api/documents/{*path}` for any page whose slug is `tasks` — that page would
/// answer this module's 404 instead of its own content, and nothing would say why.
///
/// Keyed by path under their own prefix instead, exactly as `/api/revisions/document/{*path}`
/// and `/api/collab/{*path}` are, so the class of bug does not exist here.
#[tokio::test]
async fn a_page_named_after_one_of_these_routes_is_still_readable() {
    let f = fixture().await;
    for title in ["Tasks", "Board", "Projects", "Projekte"] {
        page(&f.store, Some("/projekt"), title, empty_body()).await;
    }

    for slug in ["tasks", "board", "projects", "projekte"] {
        let (status, body) = get(
            &f.store,
            Some("chef"),
            &format!("/api/documents/projekt/{slug}"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the page at /projekt/{slug} was shadowed by a task route: {body}"
        );
    }
}

// -------------------------------------------------------------------------------------
// The board, and the property the design's Security section names.
// -------------------------------------------------------------------------------------

/// **The one that matters.** `leser` may read the project home and `/projekt/offen`, and may
/// not read `/projekt/geheim` — one project spanning two subtrees with different grants,
/// which D-3 calls normal. A board that trusted the subtree, which is the natural thing to
/// write because a project IS a subtree, would hand over the secret page's card; and the
/// card's title is that page's own words (D-2), so the leak is prose and not merely a name.
///
/// The anti-vacuity half is at the bottom: `chef` sees all three cards, so the assertions
/// above are about filtering rather than about a fixture with nothing in it.
#[tokio::test]
async fn a_board_discloses_no_card_whose_page_the_caller_may_not_read() {
    let f = fixture().await;
    let uri = format!("/api/projects/{}/board", f.project);

    let (status, text) = raw(&f.store, Some("leser"), Method::GET, &uri, None).await;
    assert_eq!(status, StatusCode::OK, "{text}");
    let board: Value = serde_json::from_str(&text).unwrap();

    // Board order: all three cards are Offen at position 0, so the tie breaks on the id,
    // which for a uuid v7 is the order they were created in.
    assert_eq!(
        titles(&board),
        vec!["Lose Karte", "Harmlos"],
        "a card leaked a page the caller cannot read, or omitted one they can"
    );
    assert!(
        !text.contains("Befund besprechen"),
        "the hidden card's TITLE — the secret page's own words — is in the response: {text}"
    );
    assert!(
        !text.contains(&f.hidden_task),
        "the hidden card's ID is in the response, which says the card exists: {text}"
    );

    // Anti-vacuity. Without this the test would pass against a board that is empty for
    // everybody, which is the failure mode this whole file exists to rule out.
    let (status, all) = get(&f.store, Some("chef"), &uri).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        titles(&all),
        vec!["Lose Karte", "Harmlos", "Befund besprechen"],
        "the fixture never had a card to hide: {all}"
    );
}

/// The other half of the same property, and the one an aggregate query loses by *adding* to
/// its answer rather than by asking the wrong question. A total, a "3 cards" badge or an
/// `omitted` count would each say that something was filtered out, and how much — which is
/// exactly what the filtering exists to avoid saying.
///
/// Asserted structurally, on the keys, rather than by grepping for the number: a field that
/// cannot exist cannot be wrong later.
#[tokio::test]
async fn the_board_carries_no_field_that_could_count_what_it_hid() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("leser"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;

    let keys: Vec<&str> = board
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec!["columns", "project"],
        "the board grew a field beyond the project and its columns: {board}"
    );
    for column in board["columns"].as_array().unwrap() {
        let keys: Vec<&str> = column
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["status", "tasks"],
            "a column grew a field that could count what it hid: {column}"
        );
    }
}

/// A project the caller may not reach and a project that does not exist must be the same
/// answer, byte for byte. Anything else answers "does this project exist" to anybody who
/// asks, which is the disclosure the board filtering is for.
#[tokio::test]
async fn an_unreachable_project_answers_exactly_what_a_missing_one_answers() {
    let f = fixture().await;
    let real = format!("/api/projects/{}/board", f.project);
    let invented = "/api/projects/0192f000-0000-7000-8000-000000000000/board";

    let refused = raw(&f.store, Some("fremder"), Method::GET, &real, None).await;
    let missing = raw(&f.store, Some("fremder"), Method::GET, invented, None).await;

    assert_eq!(refused.0, StatusCode::NOT_FOUND);
    assert_eq!(
        refused, missing,
        "a project the caller may not read is distinguishable from one that is not there"
    );

    // And the same for an anonymous caller, who has no account to be refused as.
    let anonymous = raw(&f.store, None, Method::GET, &real, None).await;
    assert_eq!(anonymous, missing);
}

/// D-9: the same three columns on every board, built in. Present even when empty, because
/// "nothing is running" is a thing a board has to be able to say.
#[tokio::test]
async fn a_board_has_the_three_fixed_columns_even_when_they_are_empty() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;

    let columns: Vec<&str> = board["columns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["status"].as_str().unwrap())
        .collect();
    assert_eq!(columns, vec!["Offen", "Läuft", "Fertig"]);
    assert!(
        board["columns"][1]["tasks"].as_array().unwrap().is_empty(),
        "the Läuft column should be empty in this fixture: {board}"
    );
}

// -------------------------------------------------------------------------------------
// D-8: a detached card stays on its board, marked.
// -------------------------------------------------------------------------------------

/// Publish a page with a checklist line, then publish it without one. The record survives —
/// deleting it would silently discard a due date somebody set — and the board says so,
/// because a card that looks live but is no longer written anywhere is worse than no card.
#[tokio::test]
async fn a_detached_card_stays_on_the_board_and_the_response_says_so() {
    let f = fixture().await;
    let (chef, _) = f
        .store
        .principal_by_username("chef")
        .await
        .unwrap()
        .unwrap();
    let seite = page(&f.store, Some("/projekt"), "Ablauf", empty_body()).await;

    f.store
        .publish_revision(&chef, &seite, &checklist_body("Termin machen"), None)
        .await
        .unwrap()
        .expect("chef may write the project subtree");

    let uri = format!("/api/projects/{}/board", f.project);
    let (_, board) = get(&f.store, Some("chef"), &uri).await;
    let card = cards(&board)
        .into_iter()
        .find(|task| task["title"] == "Termin machen")
        .unwrap_or_else(|| panic!("the checklist line minted no card: {board}"));
    assert_eq!(card["detached"], json!(false));

    // The line goes away. The card does not.
    f.store
        .publish_revision(&chef, &seite, &empty_body(), None)
        .await
        .unwrap()
        .expect("chef may write the project subtree");

    let (_, board) = get(&f.store, Some("chef"), &uri).await;
    let card = cards(&board)
        .into_iter()
        .find(|task| task["title"] == "Termin machen")
        .unwrap_or_else(|| panic!("the card vanished with its line: {board}"));
    assert_eq!(
        card["detached"],
        json!(true),
        "the board hid the detached state instead of carrying it: {card}"
    );
}

// -------------------------------------------------------------------------------------
// D-2: the page owns the words, the record owns the state.
// -------------------------------------------------------------------------------------

/// Dragging a card must change only the record. An endpoint here that wrote a document would
/// file revisions nobody typed, need write permission on a page for a drag, and collide in
/// the CRDT when two people move cards that came from one document.
#[tokio::test]
async fn moving_a_card_changes_no_page_and_files_no_revision() {
    let f = fixture().await;
    let (chef, _) = f
        .store
        .principal_by_username("chef")
        .await
        .unwrap()
        .unwrap();
    let seite = page(&f.store, Some("/projekt"), "Ablauf", empty_body()).await;
    f.store
        .publish_revision(&chef, &seite, &checklist_body("Termin machen"), None)
        .await
        .unwrap()
        .unwrap();

    let before = f.store.revisions_for(&chef, &seite).await.unwrap();
    let (_, page_before) = get(&f.store, Some("chef"), "/api/documents/projekt/ablauf").await;

    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    let id = cards(&board)
        .into_iter()
        .find(|task| task["title"] == "Termin machen")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, moved) = patch(
        &f.store,
        Some("chef"),
        &format!("/api/tasks/{id}"),
        json!({"status": "Fertig", "position": 3}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{moved}");
    assert_eq!(moved["status"], json!("Fertig"));

    let after = f.store.revisions_for(&chef, &seite).await.unwrap();
    assert_eq!(
        after.len(),
        before.len(),
        "moving a card filed a revision nobody typed"
    );
    let (_, page_after) = get(&f.store, Some("chef"), "/api/documents/projekt/ablauf").await;
    assert_eq!(
        page_after, page_before,
        "moving a card rewrote the page it was written on"
    );
}

// -------------------------------------------------------------------------------------
// D-9: the status is a closed set of three, and a typo is a 400.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_status_outside_the_fixed_three_is_a_clean_bad_request() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    let id = cards(&board)[0]["id"].as_str().unwrap().to_string();

    for bogus in ["Erledigt", "offen", "LÄUFT", "Läuft ", "in Bearbeitung", ""] {
        let (status, body) = patch(
            &f.store,
            Some("chef"),
            &format!("/api/tasks/{id}"),
            json!({ "status": bogus }),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "the status {bogus:?} was not refused cleanly: {body}"
        );
        let message = body["error"].as_str().unwrap_or_default();
        for name in ["Offen", "Läuft", "Fertig"] {
            assert!(
                message.contains(name),
                "the refusal does not say what the three are: {message}"
            );
        }
    }

    // And on the way in, not only on the way through.
    let (status, _) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Neu", "status": "Erledigt"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// All three round-trip, including the one whose name is not ASCII. `Läuft` composed
/// (U+00E4) is what the schema's CHECK constraint holds and what SQLite compares byte for
/// byte, so a client that sends the decomposed form — which is what a Mac produces — spells
/// the status correctly and must not be refused for it.
#[tokio::test]
async fn every_status_the_board_shows_is_a_status_it_accepts_back() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    let id = cards(&board)[0]["id"].as_str().unwrap().to_string();

    for wanted in ["Offen", "Läuft", "Fertig", "La\u{308}uft"] {
        let (status, task) = patch(
            &f.store,
            Some("chef"),
            &format!("/api/tasks/{id}"),
            json!({ "status": wanted }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{task}");
        let expected = if wanted == "La\u{308}uft" {
            "Läuft"
        } else {
            wanted
        };
        assert_eq!(task["status"], json!(expected));
    }
}

// -------------------------------------------------------------------------------------
// Creating, changing and deleting a card.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_standalone_card_lands_on_the_board_of_a_project_you_may_write() {
    let f = fixture().await;
    let (status, created) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Rezept holen", "due_at": "2026-09-01"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["status"], json!("Offen"), "a new card starts open");
    assert_eq!(created["anchored"], json!(false));
    assert_eq!(created["detached"], json!(false));
    assert_eq!(created["due_at"], json!("2026-09-01"));

    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    assert!(
        titles(&board).contains(&"Rezept holen".to_string()),
        "the card was created and is not on the board: {board}"
    );
}

/// Two refusals that must not be one. Somebody who may read the board is told to ask for
/// access; somebody who cannot see it at all is told nothing, because a 403 there would say
/// the project exists.
#[tokio::test]
async fn creating_a_card_is_forbidden_to_a_reader_and_invisible_to_a_stranger() {
    let f = fixture().await;
    let body = json!({"project_id": f.project, "title": "Nicht erlaubt"});

    let (status, _) = post(&f.store, Some("leser"), "/api/tasks", body.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = post(&f.store, Some("fremder"), "/api/tasks", body.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": "0192f000-0000-7000-8000-000000000000", "title": "Nirgendwo"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // And nothing was written by any of them.
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    assert_eq!(
        cards(&board).len(),
        3,
        "a refused create wrote a card: {board}"
    );
}

#[tokio::test]
async fn a_card_without_words_is_refused() {
    let f = fixture().await;
    for title in ["", "   "] {
        let (status, body) = post(
            &f.store,
            Some("chef"),
            "/api/tasks",
            json!({"project_id": f.project, "title": title}),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
}

/// The three states a PATCH field can be in, which `Option` alone cannot express on the
/// wire: absent leaves it alone, `null` clears it, a value sets it. Without the distinction
/// a board could never clear a due date, and every PATCH would silently unassign.
#[tokio::test]
async fn an_omitted_field_is_left_alone_and_an_explicit_null_clears_it() {
    let f = fixture().await;
    let (leser, _) = f
        .store
        .principal_by_username("leser")
        .await
        .unwrap()
        .unwrap();
    let (_, created) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Karte", "due_at": "2026-09-01",
               "assignee": leser.id}),
    )
    .await;
    let uri = format!("/api/tasks/{}", created["id"].as_str().unwrap());

    let (status, task) = patch(&f.store, Some("chef"), &uri, json!({"status": "Läuft"})).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(
        task["assignee"],
        json!(leser.id),
        "an omitted field was cleared"
    );
    assert_eq!(
        task["due_at"],
        json!("2026-09-01"),
        "an omitted field was cleared"
    );

    let (status, task) = patch(&f.store, Some("chef"), &uri, json!({"due_at": null})).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(
        task["due_at"],
        Value::Null,
        "an explicit null did not clear the field"
    );
    assert_eq!(task["assignee"], json!(leser.id));
}

#[tokio::test]
async fn a_patch_that_names_nothing_is_refused() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    let id = cards(&board)[0]["id"].as_str().unwrap().to_string();

    let (status, body) = patch(
        &f.store,
        Some("chef"),
        &format!("/api/tasks/{id}"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn changing_a_card_is_forbidden_to_a_reader_and_invisible_to_a_stranger() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;
    let id = cards(&board)
        .into_iter()
        .find(|task| task["title"] == "Lose Karte")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let uri = format!("/api/tasks/{id}");
    let change = json!({"status": "Fertig"});

    let (status, _) = patch(&f.store, Some("leser"), &uri, change.clone()).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = patch(&f.store, Some("fremder"), &uri, change.clone()).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The card `leser` may not READ answers exactly as a card that is not there — a 403
    // would confirm it exists, which is the whole of what the board filtering hides.
    let hidden = raw(
        &f.store,
        Some("leser"),
        Method::PATCH,
        &format!("/api/tasks/{}", f.hidden_task),
        Some(change.clone()),
    )
    .await;
    let invented = raw(
        &f.store,
        Some("leser"),
        Method::PATCH,
        "/api/tasks/0192f000-0000-7000-8000-000000000000",
        Some(change),
    )
    .await;
    assert_eq!(hidden.0, StatusCode::NOT_FOUND);
    assert_eq!(hidden, invented);
}

#[tokio::test]
async fn deleting_a_card_takes_it_off_the_board_and_needs_write() {
    let f = fixture().await;
    let uri = format!("/api/projects/{}/board", f.project);
    let (_, board) = get(&f.store, Some("chef"), &uri).await;
    let id = cards(&board)
        .into_iter()
        .find(|task| task["title"] == "Lose Karte")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let (status, _) = delete(&f.store, Some("leser"), &format!("/api/tasks/{id}")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = delete(&f.store, Some("fremder"), &format!("/api/tasks/{id}")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, board) = get(&f.store, Some("chef"), &uri).await;
    assert_eq!(cards(&board).len(), 3, "a refused delete removed a card");

    let (status, body) = delete(&f.store, Some("chef"), &format!("/api/tasks/{id}")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["changed"], json!(true));
    let (_, board) = get(&f.store, Some("chef"), &uri).await;
    assert!(
        !titles(&board).contains(&"Lose Karte".to_string()),
        "the card is still on the board: {board}"
    );
}

// -------------------------------------------------------------------------------------
// D-10, clause 3: you may only assign somebody who may read the governing page.
// -------------------------------------------------------------------------------------

/// Assigning somebody to a task on a page they cannot open hands them an obligation they
/// cannot see, and the card's title tells them what a page they may not read is called.
/// Refused — and not as a permission problem, because the caller may do this and would be
/// allowed to a moment after granting that person read. So 409, naming the way out, exactly
/// as the last-administrator interlock does.
#[tokio::test]
async fn assigning_somebody_who_cannot_read_the_page_is_a_conflict_that_names_the_way_out() {
    let f = fixture().await;
    let (fremder, _) = f
        .store
        .principal_by_username("fremder")
        .await
        .unwrap()
        .unwrap();
    let (leser, _) = f
        .store
        .principal_by_username("leser")
        .await
        .unwrap()
        .unwrap();

    let (status, body) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Karte", "assignee": fremder.id}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let message = body["error"].as_str().unwrap_or_default();
    assert!(
        message.contains("read"),
        "the refusal does not say what to do about it: {message}"
    );
    assert!(
        !message.contains(&fremder.id) && !message.contains("fremder"),
        "the refusal names the person, which is more than it was asked: {message}"
    );

    // Anti-vacuity: the same request with somebody who MAY read the page is accepted, so
    // the refusal above is about the assignee's access and not about the field being
    // rejected out of hand.
    let (status, created) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Karte", "assignee": leser.id}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["assignee"], json!(leser.id));

    // And an id that is not an account at all is the same answer, fail-closed.
    let (status, _) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Karte", "assignee": "niemand"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

/// Clause 4. A stale assignee must be clearable after that person loses their read, or the
/// assignment outlives the access it was granted under and the only fix is deleting the
/// card — discarding the due date, which is the outcome D-8 exists to prevent.
#[tokio::test]
async fn unassigning_is_allowed_after_the_assignee_has_lost_their_read() {
    let f = fixture().await;
    let (leser, _) = f
        .store
        .principal_by_username("leser")
        .await
        .unwrap()
        .unwrap();
    let (_, created) = post(
        &f.store,
        Some("chef"),
        "/api/tasks",
        json!({"project_id": f.project, "title": "Karte", "assignee": leser.id}),
    )
    .await;
    let uri = format!("/api/tasks/{}", created["id"].as_str().unwrap());

    f.store
        .remove_grant(
            "/projekt",
            &Subject::Principal(leser.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();

    // Re-asserting the name would now be refused …
    let (status, _) = patch(&f.store, Some("chef"), &uri, json!({"assignee": leser.id})).await;
    assert_eq!(status, StatusCode::CONFLICT);

    // … and clearing it is still allowed.
    let (status, task) = patch(&f.store, Some("chef"), &uri, json!({"assignee": null})).await;
    assert_eq!(status, StatusCode::OK, "{task}");
    assert_eq!(task["assignee"], Value::Null);
}

// -------------------------------------------------------------------------------------
// A page's own tasks — the other half of D-2, so a page can render its checkboxes from
// the records rather than from the words.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_pages_own_cards_follow_that_pages_read() {
    let f = fixture().await;

    let (status, body) = get(&f.store, Some("chef"), "/api/tasks/document/projekt/geheim").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["tasks"][0]["title"],
        json!("Befund besprechen"),
        "the page's own card is missing: {body}"
    );

    let (status, _) = get(
        &f.store,
        Some("leser"),
        "/api/tasks/document/projekt/geheim",
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = get(&f.store, Some("chef"), "/api/tasks/document/gibt-es-nicht").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // The anchored card knows it came from a page; the loose one knows it did not.
    let (_, body) = get(&f.store, Some("chef"), "/api/tasks/document/projekt/offen").await;
    assert_eq!(body["tasks"][0]["anchored"], json!(true));
}

// -------------------------------------------------------------------------------------
// Projects.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_project_is_made_on_a_page_you_may_write() {
    let f = fixture().await;

    // Keyed by a PATH, so the split is `/api/documents`' own: 404 for a page that is not
    // there, 403 for one the caller may not write — including one they may not read at all.
    // Collapsing the second into 404 would be a third answer to a question this wiki has
    // already settled, and would reveal less than a plain `GET /api/documents` on the same
    // path already does.
    // `/projekt` is already a project's home, and `leser` may read it — so a handler that
    // looked for the conflict before it authorised the caller would answer 409 here and tell
    // a reader something a 403 does not.
    for path in ["/projekt/geheim", "/projekt/offen", "/projekt"] {
        let (status, body) = post(
            &f.store,
            Some("leser"),
            "/api/projects",
            json!({ "home_path": path }),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}: {body}");
    }

    let (status, _) = post(
        &f.store,
        Some("chef"),
        "/api/projects",
        json!({"home_path": "/gibt-es-nicht"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, project) = post(
        &f.store,
        Some("chef"),
        "/api/projects",
        json!({"home_path": "/projekt/offen"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{project}");
    assert_eq!(project["home_path"], json!("/projekt/offen"));
    assert_eq!(project["home_title"], json!("Offen"));
    assert!(
        project.as_object().unwrap().get("home_doc").is_none(),
        "the project names its home page by an internal id: {project}"
    );
}

#[tokio::test]
async fn a_second_project_on_the_same_page_is_a_conflict_that_names_the_way_out() {
    let f = fixture().await;
    let (status, body) = post(
        &f.store,
        Some("chef"),
        "/api/projects",
        json!({"home_path": "/projekt"}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    let message = body["error"].as_str().unwrap_or_default();
    assert!(
        message.contains("already"),
        "the refusal does not say why: {message}"
    );
}

#[tokio::test]
async fn the_project_list_omits_one_whose_home_page_the_caller_may_not_read() {
    let f = fixture().await;
    let (_, geheim) = post(
        &f.store,
        Some("chef"),
        "/api/projects",
        json!({"home_path": "/projekt/geheim"}),
    )
    .await;
    let hidden = geheim["id"].as_str().unwrap().to_string();

    let (status, text) = raw(&f.store, Some("leser"), Method::GET, "/api/projects", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !text.contains(&hidden) && !text.contains("Geheim"),
        "a project whose home page the caller cannot read is in the listing: {text}"
    );
    let listing: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(listing["projects"].as_array().unwrap().len(), 1);

    // Anti-vacuity: it really is there for somebody who may read that page.
    let (_, all) = get(&f.store, Some("chef"), "/api/projects").await;
    assert_eq!(all["projects"].as_array().unwrap().len(), 2, "{all}");

    // And a stranger's listing is empty rather than an error that says there is something.
    let (status, none) = get(&f.store, Some("fremder"), "/api/projects").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(none["projects"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn retagging_and_deleting_a_project_need_write_on_its_home_page() {
    let f = fixture().await;
    let uri = format!("/api/projects/{}", f.project);

    let (status, _) = patch(&f.store, Some("leser"), &uri, json!({"tag_id": "thema"})).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = delete(&f.store, Some("leser"), &uri).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = delete(&f.store, Some("fremder"), &uri).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, body) = patch(&f.store, Some("chef"), &uri, json!({"tag_id": "thema"})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["tag_id"], json!("thema"));
    let (status, body) = patch(&f.store, Some("chef"), &uri, json!({"tag_id": null})).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["tag_id"], Value::Null);
    let (status, body) = patch(&f.store, Some("chef"), &uri, json!({})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    let (status, body) = delete(&f.store, Some("chef"), &uri).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let (status, _) = get(&f.store, Some("chef"), &format!("{uri}/board")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "the board outlived its project"
    );
}

/// The store keeps `doc_id`, `block_id` and `project_id` on a task and this crate does not
/// put them on the wire — the same division `links.rs` makes for a graph edge, and for the
/// same reason: an internal identifier has no business leaving this crate, and a path is
/// what an interface has to link to anyway.
#[tokio::test]
async fn a_card_carries_no_internal_identifier() {
    let f = fixture().await;
    let (_, board) = get(
        &f.store,
        Some("chef"),
        &format!("/api/projects/{}/board", f.project),
    )
    .await;

    for card in cards(&board) {
        let keys: Vec<&str> = card
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec![
                "anchored",
                "assignee",
                "created_at",
                "detached",
                "due_at",
                "id",
                "position",
                "status",
                "title",
                "updated_at",
            ],
            "the card's fields are not the ones the wire promises: {card}"
        );
    }
    let (_, text) = raw(
        &f.store,
        Some("chef"),
        Method::GET,
        &format!("/api/projects/{}/board", f.project),
        None,
    )
    .await;
    assert!(
        !text.contains(&f.geheim_doc),
        "a document id reached the wire: {text}"
    );
}
