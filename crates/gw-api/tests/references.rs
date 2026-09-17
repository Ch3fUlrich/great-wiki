//! What a document reference discloses over the wire (D-5, D-21d, ADR 0019).
//!
//! A `doc` mark is an id the AUTHOR chose. It reaches `documents.body` over the
//! collaboration socket with nothing validating it, and the page carrying it may be readable
//! by the whole internet while the page it points at is not. So the reader's own permission
//! is what decides what it resolves to, and the only thing that may ever come back about a
//! target is what [`gw_store::Store::references_for`] answers for *that caller*.
//!
//! `gw-store`'s own tests pin the rule; this file pins the WIRE, and it asserts on the raw
//! response text rather than on a parsed field. A handler that answered 200 with the
//! restricted title somewhere else in the body — in a field nobody thought to check, or in
//! an error message — would satisfy a test that deserialised one key and looked at it.
//!
//! The fixture, and both halves matter:
//!
//! - `/geheim` is restricted and is called **Blutbild Müller**. Neither that name nor the
//!   path `/geheim` may appear in a response to somebody who may not read it.
//! - `/quelle` is public and its body holds one reference to `/geheim`'s id, under the
//!   author's own words, **siehe dort**. Those words are the author's and are shown to
//!   everybody: they are in a body the caller is already reading.
//! - `chef` holds `read` on `/geheim` and is the anti-vacuity half. Without him every
//!   assertion below would pass just as happily against a fixture that never contained a
//!   reference at all.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_core::{Block, BlockKind, DocumentType, Mark, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

const GEHEIMER_TITEL: &str = "Blutbild Müller";
const EIGENER_TEXT: &str = "siehe dort";

fn empty_body() -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

/// A one-paragraph document whose only content is a reference to `id`.
fn referring_body(id: &str) -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: Default::default(),
        content: vec![Block {
            kind: BlockKind::Paragraph,
            attrs: Default::default(),
            content: vec![Block {
                kind: BlockKind::Text,
                attrs: Default::default(),
                content: Vec::new(),
                text: Some(EIGENER_TEXT.into()),
                marks: vec![Mark::link_to_doc(id)],
            }],
            text: None,
            marks: Vec::new(),
        }],
        text: None,
        marks: Vec::new(),
    }
}

async fn page(
    store: &Store,
    slug: &str,
    title: &str,
    visibility: Visibility,
    body: Block,
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
                body,
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap()
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let geheim = page(
        &store,
        "geheim",
        GEHEIMER_TITEL,
        Visibility::Restricted,
        empty_body(),
    )
    .await;
    page(
        &store,
        "quelle",
        "Quelle",
        Visibility::Public,
        referring_body(&geheim),
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
        .add_grant("/geheim", Subject::Principal(chef.id), Permission::Read)
        .await
        .unwrap();

    Arc::new(store)
}

async fn get(app: axum::Router, uri: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn get_as(store: &Arc<Store>, username: &str, uri: &str) -> (StatusCode, String) {
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
    get(
        gw_api::build_router(gw_api::AppState::for_test_principal(
            Arc::clone(store),
            &principal,
        )),
        uri,
    )
    .await
}

async fn get_anonymous(store: &Arc<Store>, uri: &str) -> (StatusCode, String) {
    get(
        gw_api::build_router(gw_api::AppState::for_test(Arc::clone(store), None)),
        uri,
    )
    .await
}

#[tokio::test]
async fn a_reference_to_a_page_the_reader_may_not_read_discloses_nothing_about_it() {
    let store = fixture().await;
    let (status, body) = get_anonymous(&store, "/api/documents/quelle").await;
    assert_eq!(status, StatusCode::OK);

    // The author's own words are theirs and are in a body this caller is already reading.
    assert!(
        body.contains(EIGENER_TEXT),
        "the author's link text was blanked, which corrupts a sentence to hide something it \
         does not contain:\n{body}"
    );
    // The target's CURRENT name and CURRENT address are the target's, and a rename would
    // otherwise keep reporting the new one to a reader who may not see the page at all.
    assert!(
        !body.contains(GEHEIMER_TITEL),
        "a restricted page's title came back to a reader who may not read it:\n{body}"
    );
    assert!(
        !body.contains("/geheim"),
        "a restricted page's address came back to a reader who may not read it:\n{body}"
    );
}

#[tokio::test]
async fn a_reference_to_a_page_the_reader_may_read_resolves_to_its_current_title_and_path() {
    // The anti-vacuity half. Without it every assertion above holds for a fixture with no
    // reference in it, which is exactly the shape `scripts/mutate.sh` exists to catch.
    let store = fixture().await;
    let (status, body) = get_as(&store, "chef", "/api/documents/quelle").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(EIGENER_TEXT), "{body}");
    assert!(
        body.contains(GEHEIMER_TITEL),
        "the fixture never had a resolvable reference, so nothing above was proved:\n{body}"
    );
    assert!(body.contains("/geheim"), "{body}");
}

#[tokio::test]
async fn a_reference_is_resolved_against_the_reader_and_never_against_the_author() {
    // Stated separately because it is the thing an implementation gets wrong by being
    // reasonable: the ids came out of a body the caller may read, so a batch
    // `ids -> (path, title)` query "cannot leak anything new". It can. `chef` WROTE this
    // page and may read the target; `leser` may read the page and not the target.
    let store = fixture().await;
    let (_, als_leser) = get_as(&store, "leser", "/api/documents/quelle").await;
    let (_, als_chef) = get_as(&store, "chef", "/api/documents/quelle").await;
    assert!(!als_leser.contains(GEHEIMER_TITEL), "{als_leser}");
    assert!(als_chef.contains(GEHEIMER_TITEL), "{als_chef}");
}

#[tokio::test]
async fn a_reference_to_nothing_at_all_is_answered_exactly_like_a_forbidden_one() {
    // Four states, one answer: not for you, not there any more, never existed, past the
    // cap. Distinguishing them is itself the disclosure — "you may not see this" and "there
    // is nothing here" differ only in confirming that something exists.
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(
        &store,
        "quelle",
        "Quelle",
        Visibility::Public,
        referring_body("0199c0de-0000-7000-8000-00000000dead"),
    )
    .await;
    let store = Arc::new(store);

    let (status, body) = get_anonymous(&store, "/api/documents/quelle").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(EIGENER_TEXT), "{body}");
    assert!(
        !body.contains("\"references\":{\""),
        "an id naming no document must resolve to nothing at all:\n{body}"
    );
}
