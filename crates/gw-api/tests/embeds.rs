//! What an embedded page discloses over the wire (D-27 … D-30, ADR 0020).
//!
//! An embed names a page and a section and stores nothing of either. What a reader is shown
//! inside the frame is fetched when the page is read and filtered against **that reader**, so
//! a frame drawn on a page the whole internet may read can be quoting a page almost nobody
//! may — and the only thing that may ever come back about a target is what
//! [`gw_store::Store::embeds_for`] answers for that caller.
//!
//! `gw-store`'s own tests pin the rule; this file pins the WIRE, and it asserts on the raw
//! response text rather than on a parsed field, for `tests/references.rs`' reason: a handler
//! that answered 200 with the restricted title somewhere else in the body would satisfy a
//! test that deserialised one key and looked at it.
//!
//! The fixture, and every half of it matters:
//!
//! - `/geheim` is restricted, is called **Blutbild Müller**, and its body says
//!   **sehr vertraulich**. Neither the name, nor the address, nor the words may reach
//!   somebody who may not read it.
//! - `/quelle` is public and holds a `##` section called **Dosierung**.
//! - `/host` is public and embeds both of them, one section and one whole page, under the
//!   author's own labels.
//! - `chef` holds `read` on `/geheim` and is the anti-vacuity half. Without him every
//!   assertion below would pass just as happily against a fixture that never contained an
//!   embed at all.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_core::{Block, BlockKind, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

const GEHEIMER_TITEL: &str = "Blutbild Müller";
const GEHEIME_WORTE: &str = "sehr vertraulich";
const EIGENES_LABEL: &str = "Laborbefund";
const SECTION: &str = "0199c0de-0000-7000-8000-00000000000a";

fn block(kind: BlockKind) -> Block {
    Block {
        kind,
        attrs: Default::default(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

fn paragraph(text: &str) -> Block {
    let mut leaf = block(BlockKind::Text);
    leaf.text = Some(text.into());
    let mut p = block(BlockKind::Paragraph);
    p.content.push(leaf);
    p
}

fn doc(children: Vec<Block>) -> Block {
    let mut d = block(BlockKind::Doc);
    d.content = children;
    d
}

fn embed(target: &str, heading: Option<&str>, label: &str) -> Block {
    let mut b = block(BlockKind::Transclusion);
    b.attrs
        .insert("doc".into(), serde_json::Value::from(target));
    if let Some(heading) = heading {
        b.attrs
            .insert("heading".into(), serde_json::Value::from(heading));
    }
    b.attrs
        .insert("label".into(), serde_json::Value::from(label));
    b
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
        doc(vec![paragraph(GEHEIME_WORTE)]),
    )
    .await;

    let mut heading = block(BlockKind::Heading);
    heading
        .attrs
        .insert("level".into(), serde_json::Value::from(2));
    heading
        .attrs
        .insert("id".into(), serde_json::Value::from(SECTION));
    let mut label = block(BlockKind::Text);
    label.text = Some("Dosierung".into());
    heading.content.push(label);
    let quelle = page(
        &store,
        "quelle",
        "Quelle",
        Visibility::Public,
        doc(vec![
            paragraph("Vorspann, der nicht zitiert wird."),
            heading,
            paragraph("5 mg."),
        ]),
    )
    .await;

    page(
        &store,
        "host",
        "Host",
        Visibility::Public,
        doc(vec![
            embed(&geheim, None, EIGENES_LABEL),
            embed(&quelle, Some(SECTION), "Dosierung"),
        ]),
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
async fn an_embed_of_a_page_the_reader_may_not_read_discloses_nothing_about_it() {
    let store = fixture().await;
    let (status, body) = get_anonymous(&store, "/api/documents/host").await;
    assert_eq!(status, StatusCode::OK);

    // The author's own label is theirs and is in a body this caller is already reading.
    assert!(
        body.contains(EIGENES_LABEL),
        "the author's own label was blanked, which hides nothing the block does not \
         contain:\n{body}"
    );
    assert!(
        !body.contains(GEHEIMER_TITEL),
        "a restricted page's title came back to a reader who may not read it:\n{body}"
    );
    assert!(
        !body.contains("/geheim"),
        "a restricted page's address came back to a reader who may not read it:\n{body}"
    );
    assert!(
        !body.contains(GEHEIME_WORTE),
        "a restricted page's WORDS came back inside somebody else's frame:\n{body}"
    );
}

#[tokio::test]
async fn an_embed_of_a_page_the_reader_may_read_carries_its_words_its_name_and_its_address() {
    // The anti-vacuity half. Without it every assertion above holds for a fixture with no
    // embed in it, which is exactly the shape `scripts/mutate.sh` exists to catch.
    let store = fixture().await;
    let (status, body) = get_as(&store, "chef", "/api/documents/host").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(GEHEIMER_TITEL) && body.contains(GEHEIME_WORTE),
        "the fixture never had a resolvable embed, so nothing above was proved:\n{body}"
    );
    assert!(body.contains("/geheim"), "{body}");
}

#[tokio::test]
async fn an_embed_is_resolved_against_the_reader_and_never_against_the_author() {
    let store = fixture().await;
    let (_, als_leser) = get_as(&store, "leser", "/api/documents/host").await;
    let (_, als_chef) = get_as(&store, "chef", "/api/documents/host").await;
    assert!(!als_leser.contains(GEHEIME_WORTE), "{als_leser}");
    assert!(als_chef.contains(GEHEIME_WORTE), "{als_chef}");
}

#[tokio::test]
async fn a_section_embed_carries_that_section_and_nothing_else_of_the_page() {
    let store = fixture().await;
    let (_, body) = get_anonymous(&store, "/api/documents/host").await;
    assert!(
        body.contains("5 mg."),
        "the section did not resolve:\n{body}"
    );
    assert!(
        !body.contains("Vorspann"),
        "the frame quoted more of the page than the author asked for:\n{body}"
    );
}

#[tokio::test]
async fn an_embed_of_nothing_at_all_is_answered_exactly_like_a_forbidden_one() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    page(
        &store,
        "host",
        "Host",
        Visibility::Public,
        doc(vec![embed(
            "0199c0de-0000-7000-8000-00000000dead",
            None,
            EIGENES_LABEL,
        )]),
    )
    .await;
    let store = Arc::new(store);

    let (status, body) = get_anonymous(&store, "/api/documents/host").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains(EIGENES_LABEL), "{body}");
    assert!(
        !body.contains("\"embeds\":{\""),
        "an id naming no document must resolve to nothing at all:\n{body}"
    );
}
