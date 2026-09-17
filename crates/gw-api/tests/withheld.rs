//! **A page the caller may not read is indistinguishable from a page that is not there.**
//!
//! This is the existence oracle the invitation walkthrough of 17 September 2026 found, and
//! this file is the fence around the fix. Every path-keyed endpoint in this API used to
//! answer **403** for a page the caller may not read and **404** for one that does not
//! exist, so a signed-in relative could enumerate, by guessing addresses, which pages exist
//! and are being kept from them. The addresses are guessable words; the titles are not, and
//! a title is one grant away from a path that is known to exist.
//!
//! The rule, in one sentence and in one place (`gw_api::routes::docs::withheld_or_absent`):
//! **a refusal may only tell itself apart from "absent" for somebody who may already read
//! the page.** Everything else is 404, byte for byte.
//!
//! ## Why "may read" and not "administers the path"
//!
//! The decision was written as "404 for both to anyone who is not an admin on the path; an
//! admin still sees 403". Administering a path *implies* being able to read what is at it —
//! `Permission::Admin` satisfies `Action::Read` in `can()`, and `Baseline::Admin` widens
//! every restricted read in `gw_store::acl::permits` — so an admin never meets a refusal on
//! a read at all: they are shown the page, which is a better diagnostic than a status code.
//! Gating the surviving 403 on "may read" therefore gives an admin everything the decision
//! asked for, and additionally keeps the refusal honest on the endpoints that need
//! **write**: somebody who may read a page but not change it already knows it is there, so
//! telling them what was refused discloses nothing.
//! [`an_admin_of_the_path_is_shown_the_page_itself`] and
//! [`a_reader_who_may_not_write_is_told_so_rather_than_lied_to`] are the two halves that
//! stop the sweep below from passing vacuously by refusing everything to everybody.
//!
//! ## The fixture
//!
//! * `/geheim` is **restricted** and is the page that must stay hidden.
//! * `/gibt-es-nicht` is the page that is not there. Nothing creates it.
//! * `fremde` is signed in, active, and holds no grant anywhere — the invited relative.
//! * `leser` holds **read** on `/geheim` — may read it, may change nothing.
//! * `chefin` holds **admin** on `/geheim` — the person a grant mistake is diagnosable for.

use axum::body::Body;
use axum::http::header::HeaderMap;
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

async fn page(store: &Store, slug: &str, title: &str, visibility: Visibility) {
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
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(&store, "geheim", "Nur intern", Visibility::Restricted).await;

    for (username, permission) in [
        ("leser", Some(Permission::Read)),
        ("chefin", Some(Permission::Admin)),
        // Signed in, active, and granted nothing: the invited relative.
        ("fremde", None),
    ] {
        let principal = store
            .create_local_principal(username, username, None, "$argon2id$fake")
            .await
            .unwrap();
        if let Some(permission) = permission {
            store
                .add_grant("/geheim", Subject::Principal(principal.id), permission)
                .await
                .unwrap();
        }
    }
    Arc::new(store)
}

/// The router as `username`, or as nobody at all when it is `None`.
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

/// One request, and **everything the caller can observe about the answer**: the status, the
/// headers, and the body bytes.
///
/// The headers are part of the value on purpose. A 404 and a 403 carrying the same JSON but
/// a different `content-length`, or a `content-type` that differs by a charset parameter,
/// are still two answers — and the whole property asserted here is that there is one.
#[derive(Debug, PartialEq, Eq)]
struct Answer {
    status: StatusCode,
    headers: HeaderMap,
    body: Vec<u8>,
}

/// One path-keyed request this API accepts. `{p}` in the address and in the body stands for
/// the page's captured path.
struct Probe {
    method: &'static str,
    uri: &'static str,
    body: Option<&'static str>,
}

const fn probe(method: &'static str, uri: &'static str, body: Option<&'static str>) -> Probe {
    Probe { method, uri, body }
}

async fn ask(store: &Arc<Store>, username: Option<&str>, probe: &Probe, path: &str) -> Answer {
    let app = app_as(store, username).await;
    let mut builder = Request::builder()
        .method(probe.method)
        .uri(probe.uri.replace("{p}", path));
    if probe.body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let body = probe
        .body
        .map_or_else(Body::empty, |body| Body::from(body.replace("{p}", path)));
    let response = app.oneshot(builder.body(body).unwrap()).await.unwrap();
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

/// Every path-keyed request in this API.
///
/// The list is deliberately long: the walkthrough named four endpoints, and this sweep is
/// what stops the fifth from being missed. Splitting it in two is not decoration — the
/// second half needs **write** on the page, so its refusal is one a reader legitimately
/// meets, and the two halves are therefore asserted about different people.
const READ_KEYED: &[Probe] = &[
    probe("GET", "/api/documents/{p}", None),
    probe("GET", "/api/links/backlinks/{p}", None),
    probe("GET", "/api/revisions/document/{p}", None),
    probe("GET", "/api/attachments/{p}", None),
    probe("GET", "/api/attachment/rezept.txt/{p}", None),
    probe("GET", "/api/tasks/document/{p}", None),
    probe("GET", "/api/topics/document/{p}", None),
    probe("GET", "/api/board?seite=/{p}", None),
    probe(
        "GET",
        "/api/revisions/00000000-0000-0000-0000-000000000000/source?path=/{p}",
        None,
    ),
];

const WRITE_KEYED: &[Probe] = &[
    probe("DELETE", "/api/documents/{p}", None),
    probe("PUT", "/api/topics/document/{p}", Some(r#"{"topics":[]}"#)),
    probe("POST", "/api/projects", Some(r#"{"home_path":"/{p}"}"#)),
    probe("POST", "/api/attachment/rezept.txt/{p}", Some("hallo")),
    probe("DELETE", "/api/attachment/rezept.txt/{p}", None),
    probe("POST", "/api/collab/{p}", Some("{}")),
];

/// Assert that `who` is told the same thing about a page that is withheld from them and a
/// page that is not there — on every endpoint, down to the bytes.
async fn assert_indistinguishable(store: &Arc<Store>, who: Option<&str>, withheld_path: &str) {
    for probe in READ_KEYED.iter().chain(WRITE_KEYED) {
        let withheld = ask(store, who, probe, withheld_path).await;
        let absent = ask(store, who, probe, "gibt-es-nicht").await;

        assert_eq!(
            withheld.status,
            StatusCode::NOT_FOUND,
            "{} {} answered {} for a page withheld from {who:?}",
            probe.method,
            probe.uri,
            withheld.status
        );
        assert_eq!(
            withheld, absent,
            "{} {} tells {who:?} a withheld page apart from an absent one",
            probe.method, probe.uri
        );
    }
}

// -------------------------------------------------------------------------------------
// The property: withheld and absent are one answer.
// -------------------------------------------------------------------------------------

/// **The byte-identity test.** For a signed-in caller with no grant, a page that is withheld
/// and a page that is not there produce the same status, the same headers and the same
/// bytes, on every path-keyed endpoint.
#[tokio::test]
async fn a_withheld_page_and_an_absent_one_are_byte_identical_to_somebody_without_a_grant() {
    let store = fixture().await;
    assert_indistinguishable(&store, Some("fremde"), "geheim").await;
}

/// The same for somebody who has not signed in at all — the other half of "anyone who is not
/// an admin on that path", and the cheapest oracle there is to probe.
#[tokio::test]
async fn an_anonymous_visitor_cannot_tell_a_withheld_page_from_an_absent_one() {
    let store = fixture().await;
    assert_indistinguishable(&store, None, "geheim").await;
}

/// A grant is about one page. Holding read on `/geheim` must not make `/anderswo` visible as
/// an existence — otherwise the oracle is merely moved one address along.
#[tokio::test]
async fn a_grant_on_one_page_does_not_open_the_oracle_on_another() {
    let store = fixture().await;
    page(&store, "anderswo", "Anderswo", Visibility::Restricted).await;
    assert_indistinguishable(&store, Some("leser"), "anderswo").await;
}

// -------------------------------------------------------------------------------------
// Anti-vacuity: the sweep above must not be passing because everything answers 404.
// -------------------------------------------------------------------------------------

/// The person who can fix a grant mistake is not lied to: an admin of the path is shown the
/// page and everything derived from it, rather than the 404 everybody else gets.
///
/// This is the half of the decision that keeps the diagnostic alive, and it is why the 404
/// above is a rule about disclosure rather than a page that has gone missing.
#[tokio::test]
async fn an_admin_of_the_path_is_shown_the_page_itself() {
    let store = fixture().await;
    for probe in READ_KEYED {
        // The two entries naming a filename or a revision id are 404 for everybody — no such
        // file, no such revision — so they say nothing either way about the rule.
        if probe.uri.contains("rezept.txt") || probe.uri.contains("/source") {
            continue;
        }
        let answer = ask(&store, Some("chefin"), probe, "geheim").await;
        assert_eq!(
            answer.status,
            StatusCode::OK,
            "{} {} refused the admin of the path: {}",
            probe.method,
            probe.uri,
            String::from_utf8_lossy(&answer.body)
        );
    }
}

/// A reader who may not write is told what was actually refused, because the page's presence
/// at that address is already theirs to know — they are reading it.
///
/// This is the 403 that survives. Without it the rule would collapse to "everything is 404",
/// which would make a refusal unactionable for the people who legitimately meet one.
#[tokio::test]
async fn a_reader_who_may_not_write_is_told_so_rather_than_lied_to() {
    let store = fixture().await;
    for probe in WRITE_KEYED {
        let answer = ask(&store, Some("leser"), probe, "geheim").await;
        assert_eq!(
            answer.status,
            StatusCode::FORBIDDEN,
            "{} {} did not say what was refused to a reader: {}",
            probe.method,
            probe.uri,
            String::from_utf8_lossy(&answer.body)
        );
        assert_eq!(
            answer.body, br#"{"error":"forbidden"}"#,
            "{} {} refused with something other than the settled reason",
            probe.method, probe.uri
        );
    }
}
