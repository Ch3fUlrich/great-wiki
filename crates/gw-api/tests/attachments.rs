//! Attachments over HTTP: what a page lists, what an upload accepts, and — the part every
//! other test here exists to support — **what a download hands over, and to whom**.
//!
//! `gw-store`'s own `attachments` and `blobs` tests pin the properties themselves: one blob
//! per distinct file, the permission asked per page, the name that could not be an address.
//! This file is about the wire on top of them, and about the two things no store test can
//! reach:
//!
//! * **D-16 on the wire.** The same bytes sit on two pages; one caller may read one of them.
//!   The refusal is asserted on the *raw response bytes*, because "403" is not the property
//!   — "the file did not come back" is.
//! * **The digest is nowhere.** Not in a body, not in a header, not in the address a reader
//!   is handed. If it were, the page check would be optional.
//!
//! The fixture is built so nothing here can pass vacuously:
//!
//! * `/raum` (public) with `/raum/notiz` (public) under it, and `/geheim` (restricted).
//! * `schreiber` holds **write** on `/raum` — enough to attach and detach there.
//! * `leser` holds **read** on `/raum` — enough to download from it, never to attach.
//! * `chefin` holds **admin** on `/raum` and on `/geheim`, so she can put the same file in
//!   both places and the disclosure test has something to disclose.
//!
//! Without `leser`'s read grant every refusal below would be a refusal for the wrong reason,
//! and the D-16 test would pass with the whole page check deleted.
//!
//! **Every test that touches the mount uses its own bytes**, via `png("<something unique>")`.
//! `AppState::for_test` hands the whole process one media directory, and the store is
//! content-addressed — so two tests uploading the identical file share one file on disk, and
//! a test that removes it removes the other one's too. That is the store behaving correctly;
//! distinct bytes per test is the discipline that goes with it.

use axum::body::Body;
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

async fn page(store: &Store, parent: Option<&str>, slug: &str, title: &str, v: Visibility) {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(Into::into),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: v,
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
    page(&store, None, "raum", "Raum", Visibility::Public).await;
    page(&store, Some("/raum"), "notiz", "Notiz", Visibility::Public).await;
    page(&store, None, "geheim", "Geheim", Visibility::Restricted).await;

    for (username, path, permission) in [
        ("schreiber", "/raum", Permission::Write),
        ("leser", "/raum", Permission::Read),
        ("chefin", "/raum", Permission::Admin),
        ("chefin", "/geheim", Permission::Admin),
    ] {
        let principal = match store.principal_by_username(username).await.unwrap() {
            Some((principal, _)) => principal,
            None => store
                .create_local_principal(username, username, None, "$argon2id$fake")
                .await
                .unwrap(),
        };
        store
            .add_grant(path, Subject::Principal(principal.id), permission)
            .await
            .unwrap();
    }
    Arc::new(store)
}

fn state(store: &Arc<Store>) -> gw_api::AppState {
    gw_api::AppState::for_test(Arc::clone(store), None)
}

async fn state_as(store: &Arc<Store>, username: &str) -> gw_api::AppState {
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
    gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
}

/// One request, and **the bytes that came back**, unparsed.
///
/// Everything here compares raw bytes rather than a decoded string or a parsed body: the
/// question a disclosure test asks is "did the file come out of this socket", and any
/// decoding step in between is a step that could quietly answer a different question.
async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    // `Content-Length`, because a real client sends one and the request-body limit checks it
    // BEFORE reading a byte. Without it `Request::builder` sends none, the limit degrades to
    // "error while streaming", and a test of the limit measures whichever extractor happened
    // to reject the request first instead.
    let response = app
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header("content-length", body.len().to_string())
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, headers, bytes.to_vec())
}

async fn as_user(
    store: &Arc<Store>,
    username: &str,
    method: &str,
    uri: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    let app = gw_api::build_router(state_as(store, username).await);
    send(app, method, uri, body).await
}

async fn anonymous(
    store: &Arc<Store>,
    method: &str,
    uri: &str,
) -> (StatusCode, Vec<(String, String)>, Vec<u8>) {
    send(gw_api::build_router(state(store)), method, uri, Vec::new()).await
}

fn json(body: &[u8]) -> serde_json::Value {
    serde_json::from_slice(body).unwrap_or_else(|_| {
        panic!(
            "expected JSON, got `{}`",
            String::from_utf8_lossy(&body[..body.len().min(200)])
        )
    })
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> &'a str {
    headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
        .unwrap_or_else(|| panic!("no `{name}` header in {headers:?}"))
}

/// A PNG by its magic number, with `marker` inside it so a body can be searched for it.
fn png(marker: &str) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    bytes.extend_from_slice(marker.as_bytes());
    bytes
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// -------------------------------------------------------------------------------------
// The list, and the address it hands out.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_file_that_was_attached_appears_in_the_page_s_list() {
    let store = fixture().await;
    let (status, _, body) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/befund.png/raum",
        png("bild"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );
    let created = json(&body);
    assert_eq!(created["filename"], "befund.png");
    assert_eq!(created["media_type"], "image/png");
    assert_eq!(created["byte_size"], png("bild").len());
    assert_eq!(created["uploaded_by_name"], "schreiber");

    let (status, _, body) =
        as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let listed = json(&body);
    assert_eq!(listed["attachments"][0]["filename"], "befund.png");
    assert_eq!(
        listed["may_write"], false,
        "a reader is not offered a control they cannot use"
    );

    let (_, _, body) = as_user(
        &store,
        "schreiber",
        "GET",
        "/api/attachments/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(json(&body)["may_write"], true);
}

#[tokio::test]
async fn the_address_of_a_file_carries_the_page_and_never_the_hash() {
    // D-16 as a reader sees it: the thing they are handed to fetch a file names the page.
    let store = fixture().await;
    let (_, _, body) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/Befund%202024.png/raum/notiz",
        png("bild"),
    )
    .await;
    let href = json(&body)["href"].as_str().unwrap().to_string();
    assert_eq!(href, "/api/attachment/Befund%202024.png/raum/notiz");

    // And it works, which is what makes the shape a contract rather than a suggestion.
    let (status, _, bytes) = as_user(&store, "leser", "GET", &href, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, png("bild"));
}

#[tokio::test]
async fn a_list_needs_read_on_the_page_and_tells_absent_from_forbidden() {
    let store = fixture().await;
    let (status, _, _) = anonymous(&store, "GET", "/api/attachments/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = anonymous(&store, "GET", "/api/attachments/gibt-es-nicht").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    assert_eq!(status, StatusCode::OK, "and a reader really can read it");
}

// -------------------------------------------------------------------------------------
// D-16: a download is authorised against the page it was reached through.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_download_is_authorised_against_the_page_and_not_against_the_bytes() {
    // The whole decision, on the wire. `chefin` puts the SAME file on a page `leser` may
    // read and on one they may not. There is one copy of the bytes on the mount, and the
    // answer differs by which page the request came through.
    //
    // Asserted on the raw response body, because the property is not "403" — it is that the
    // file did not come out of the socket. A handler that answered 403 with the bytes in the
    // body would satisfy a status-code test.
    let store = fixture().await;
    let secret = png("DIESE-BYTES-SIND-VERTRAULICH");
    for path in ["raum", "geheim"] {
        let (status, _, body) = as_user(
            &store,
            "chefin",
            "POST",
            &format!("/api/attachment/gleich.png/{path}"),
            secret.clone(),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&body)
        );
    }

    // Through the page they may read: the exact bytes. This half is what stops the test
    // below passing because downloads are broken for everybody.
    let (status, headers, body) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/gleich.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, secret, "the file itself, byte for byte");
    assert_eq!(header(&headers, "content-type"), "image/png");

    // Through the page they may not: nothing.
    let (status, _, body) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/gleich.png/geheim",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(
        !contains(&body, b"DIESE-BYTES-SIND-VERTRAULICH"),
        "the response body carried the file: {}",
        String::from_utf8_lossy(&body)
    );
    assert!(
        !contains(&body, &secret),
        "nor any part of it: {}",
        String::from_utf8_lossy(&body)
    );

    // And an anonymous caller gets the same refusal, so the boundary is not "signed in".
    let (status, _, body) = anonymous(&store, "GET", "/api/attachment/gleich.png/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!contains(&body, b"DIESE-BYTES-SIND-VERTRAULICH"));

    // Anti-vacuity for the other direction: the restricted page's own reader gets the file
    // through it, so `/geheim` is a working address and the refusal above was about `leser`.
    let (status, _, body) = as_user(
        &store,
        "chefin",
        "GET",
        "/api/attachment/gleich.png/geheim",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, secret);
}

#[tokio::test]
async fn no_response_anywhere_carries_the_content_address() {
    // If a digest ever reaches a reader, the page check becomes optional: they can look for
    // the same bytes behind a page they may read, or ask for a `/blob/<sha>` route that
    // somebody will eventually add because the hash is obviously already public.
    let store = fixture().await;
    let bytes = png("bild");
    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&bytes))
    };

    let (_, _, created) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/befund.png/raum",
        bytes.clone(),
    )
    .await;

    // Anti-vacuity, before anything is asserted about the responses: this really is the
    // digest the store recorded for these bytes, and it really is 64 characters — so "it did
    // not appear" is a fact about the responses and not about a needle that was never there.
    let (principal, _) = store
        .principal_by_username("schreiber")
        .await
        .unwrap()
        .unwrap();
    let source = store
        .attachment_for(&principal, "/raum", "befund.png")
        .await
        .unwrap()
        .expect("it was just attached");
    assert_eq!(source.sha256, digest);
    assert_eq!(digest.len(), 64);

    let (_, _, listed) = as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    let (_, download_headers, _) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/befund.png/raum",
        Vec::new(),
    )
    .await;
    let (_, _, detached) = as_user(
        &store,
        "schreiber",
        "DELETE",
        "/api/attachment/befund.png/raum",
        Vec::new(),
    )
    .await;

    for (what, body) in [
        ("the upload's answer", created),
        ("the list", listed),
        ("the detach's answer", detached),
        (
            "the download's headers",
            format!("{download_headers:?}").into_bytes(),
        ),
    ] {
        assert!(
            !contains(&body, digest.as_bytes()),
            "{what} carried the content address: {}",
            String::from_utf8_lossy(&body)
        );
    }
}

// -------------------------------------------------------------------------------------
// The upload: what it accepts, what it refuses, and how big it may be.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn attaching_needs_write_on_the_page() {
    let store = fixture().await;
    let (status, _, _) = as_user(
        &store,
        "leser",
        "POST",
        "/api/attachment/x.png/raum",
        png("bild"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _, _) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/x.png/gibt-es-nicht",
        png("bild"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, _) = anonymous(&store, "GET", "/api/attachments/raum").await;
    assert_eq!(status, StatusCode::OK, "and nothing was attached anywhere");
}

#[tokio::test]
async fn a_file_the_wiki_does_not_serve_is_refused_by_its_bytes() {
    let store = fixture().await;
    // Named `.png`, and it is an HTML document. The name is not consulted, so this is 415
    // rather than a stored file the browser would later be handed as `image/png`.
    let (status, _, body) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/harmlos.png/raum",
        b"<!doctype html><script>alert(1)</script>".to_vec(),
    )
    .await;
    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert!(
        String::from_utf8_lossy(&body).contains("renaming it does not help"),
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (_, _, body) = as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    assert_eq!(json(&body)["attachments"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn an_empty_upload_is_refused_rather_than_listed() {
    let store = fixture().await;
    let (status, _, _) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/leer.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_name_already_on_the_page_is_a_conflict_and_replaces_nothing() {
    let store = fixture().await;
    for _ in 0..1 {
        let (status, _, _) = as_user(
            &store,
            "schreiber",
            "POST",
            "/api/attachment/befund.png/raum",
            png("erste"),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _, body) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/befund.png/raum",
        png("zweite"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        String::from_utf8_lossy(&body).contains("befund.png"),
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (_, _, bytes) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/befund.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(bytes, png("erste"), "the first file is still the one there");
}

#[tokio::test]
async fn a_name_that_tries_to_leave_its_segment_is_refused() {
    let store = fixture().await;
    // `%2F` decodes to `/` after the route has matched, so the filename really does arrive
    // holding a separator. It is refused as a NAME; the file on disk is named by its digest
    // and this string never touches a path either way.
    for encoded in ["..%2F..%2Fetc%2Fpasswd", "..", "%2E%2E", "x%00y.png"] {
        let (status, _, _) = as_user(
            &store,
            "schreiber",
            "POST",
            &format!("/api/attachment/{encoded}/raum"),
            png("bild"),
        )
        .await;
        assert_eq!(status, StatusCode::CONFLICT, "`{encoded}` must be refused");
    }
    let (_, _, body) = as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    assert_eq!(json(&body)["attachments"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn an_attachment_may_be_bigger_than_every_other_request_body() {
    // D-17 allows 250 MB per file, and the ordinary limit is 2 MB. The two live in one
    // router, so the thing that can silently break is the attachment route ending up under
    // the small one — which is exactly what happened while the limit was a layer in
    // `main.rs`, outside anything this crate could except.
    let store = fixture().await;
    let mut big = png("gross");
    big.resize(3 * 1024 * 1024, 0x42);

    let (status, _, body) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/gross.png/raum",
        big.clone(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    // Anti-vacuity: the same body on an ordinary route is refused, so 3 MB really is past
    // the limit everything else runs under.
    let (status, _, _) = as_user(&store, "chefin", "POST", "/api/admin/visibility", big).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

// -------------------------------------------------------------------------------------
// The mount, when it is not there.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_file_whose_bytes_have_gone_answers_503_and_not_404() {
    // `/mnt/cloud` answers `Stale file handle` inside a container while the host is fine, and
    // it recovers. 404 would say the attachment does not exist — sending whoever is looking
    // into it at the database, which is the one place the problem is not.
    let store = fixture().await;
    let bytes = png("diese-bytes-verschwinden");
    for path in ["raum", "geheim"] {
        let (status, _, _) = as_user(
            &store,
            "chefin",
            "POST",
            &format!("/api/attachment/weg.png/{path}"),
            bytes.clone(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (principal, _) = store
        .principal_by_username("chefin")
        .await
        .unwrap()
        .unwrap();
    let source = store
        .attachment_for(&principal, "/raum", "weg.png")
        .await
        .unwrap()
        .expect("it was just attached");
    let blobs = state(&store).blobs;
    std::fs::remove_file(blobs.path_for(&source.sha256).unwrap()).unwrap();

    let (status, _, _) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/weg.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // The row is still there, so the list is unchanged and the file can be put back by
    // uploading it again — which is the actual fix, and would not be one if the row had been
    // treated as the thing that was wrong.
    let (_, _, body) = as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    assert_eq!(json(&body)["attachments"].as_array().unwrap().len(), 1);

    // The page is resolved BEFORE the mount is touched, so a caller who may not read the page
    // learns nothing about the mount's health: 403 here, 503 for the same file, on the same
    // page, for somebody who may read it.
    let (status, _, _) = anonymous(&store, "GET", "/api/attachment/weg.png/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, _) = as_user(
        &store,
        "chefin",
        "GET",
        "/api/attachment/weg.png/geheim",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    // And re-uploading really does repair it.
    let (status, _, _) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/wieder.png/raum",
        bytes.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, body) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/weg.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
}

// -------------------------------------------------------------------------------------
// What a download tells the browser.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_download_says_what_the_bytes_are_and_forbids_the_browser_from_guessing() {
    let store = fixture().await;
    let zip = {
        let mut bytes = vec![b'P', b'K', 0x03, 0x04];
        bytes.extend_from_slice(b"nicht wirklich ein Archiv");
        bytes
    };
    // Named `.png`, and it is a ZIP. What comes back is what the bytes are.
    as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/gefaked.png/raum",
        zip.clone(),
    )
    .await;

    let (status, headers, body) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/gefaked.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, zip);
    assert_eq!(
        header(&headers, "content-type"),
        "application/zip",
        "the declared type is never echoed back"
    );
    assert!(
        header(&headers, "content-disposition").starts_with("attachment;"),
        "a type that is not a picture is saved, not rendered: {headers:?}"
    );
    assert_eq!(header(&headers, "x-content-type-options"), "nosniff");
    assert_eq!(
        header(&headers, "content-security-policy"),
        "default-src 'none'; sandbox"
    );
    assert_eq!(header(&headers, "cache-control"), "private, no-store");
    assert_eq!(header(&headers, "content-length"), zip.len().to_string());

    // A picture is offered inline, because D-15 wants it beside the paragraph explaining it.
    as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/echt.png/raum",
        png("bild"),
    )
    .await;
    let (_, headers, _) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/echt.png/raum",
        Vec::new(),
    )
    .await;
    assert!(
        header(&headers, "content-disposition").starts_with("inline;"),
        "{headers:?}"
    );
}

// -------------------------------------------------------------------------------------
// Detaching.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn detaching_takes_the_file_off_the_list_and_needs_write() {
    let store = fixture().await;
    as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/befund.png/raum",
        png("bild"),
    )
    .await;

    let (status, _, _) = as_user(
        &store,
        "leser",
        "DELETE",
        "/api/attachment/befund.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Write, and a name that is not there: a different mistake with a different fix.
    let (status, _, _) = as_user(
        &store,
        "schreiber",
        "DELETE",
        "/api/attachment/andere.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _, body) = as_user(
        &store,
        "schreiber",
        "DELETE",
        "/api/attachment/befund.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json(&body)["filename"], "befund.png");

    let (_, _, body) = as_user(&store, "leser", "GET", "/api/attachments/raum", Vec::new()).await;
    assert_eq!(json(&body)["attachments"].as_array().unwrap().len(), 0);

    // The file itself is gone from the page, so the address stops working.
    let (status, _, _) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/befund.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_page_in_the_trash_carries_no_attachments_over_http() {
    let store = fixture().await;
    as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/befund.png/raum/notiz",
        png("bild"),
    )
    .await;
    let (status, _, _) = as_user(
        &store,
        "schreiber",
        "DELETE",
        "/api/documents/raum/notiz",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    for uri in [
        "/api/attachments/raum/notiz",
        "/api/attachment/befund.png/raum/notiz",
    ] {
        let (status, _, _) = as_user(&store, "leser", "GET", uri, Vec::new()).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
}

// -------------------------------------------------------------------------------------
// What an upload discloses about what is already stored.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn attaching_bytes_that_are_already_stored_answers_exactly_as_a_novel_one() {
    // The dedup oracle, on the wire. `chefin` puts a file on a page `schreiber` cannot read.
    // When `schreiber` attaches the very same bytes to their own page, the answer has to be
    // indistinguishable from attaching bytes nobody had — otherwise possessing a file is a
    // test for whether somebody else put it on a page you cannot see, which is a disclosure
    // about a PAGE obtained without ever naming one.
    let store = fixture().await;
    let shared = png("dieses-dokument-liegt-schon-da");
    // The same length as `shared`, so `byte_size` — a fact about the bytes the uploader
    // already holds, not about the corpus — cannot be what makes the two answers differ.
    let novel = png("dieses-dokument-liegt-nicht-da");
    let (status, _, _) = as_user(
        &store,
        "chefin",
        "POST",
        "/api/attachment/geheim.png/geheim",
        shared.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (already_status, _, already) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/a.png/raum",
        shared,
    )
    .await;
    let (novel_status, _, first_time) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/b.png/raum",
        novel,
    )
    .await;
    assert_eq!(already_status, StatusCode::CREATED);
    assert_eq!(already_status, novel_status, "not even a different status");

    // Everything both answers carry, with only the three fields that are about THIS
    // attachment rather than about the bytes taken out. They must be the same object — and
    // there is nowhere for a "this was already here" field to be added without this failing.
    let strip = |body: &[u8]| {
        let mut value = json(body);
        for key in ["filename", "href", "uploaded_at"] {
            value.as_object_mut().unwrap().remove(key);
        }
        value
    };
    assert_eq!(strip(&already), strip(&first_time));

    // Anti-vacuity: the first one really was a duplicate and the second really was not, so
    // there was something to give away.
    let (principal, _) = store
        .principal_by_username("chefin")
        .await
        .unwrap()
        .unwrap();
    let hidden = store
        .attachment_for(&principal, "/geheim", "geheim.png")
        .await
        .unwrap()
        .unwrap();
    let duplicate = store
        .attachment_for(&principal, "/raum", "a.png")
        .await
        .unwrap()
        .unwrap();
    let fresh = store
        .attachment_for(&principal, "/raum", "b.png")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        hidden.sha256, duplicate.sha256,
        "the second upload really was the same file as the hidden one"
    );
    assert_ne!(hidden.sha256, fresh.sha256, "and the third really was not");
}

// -------------------------------------------------------------------------------------
// What a purge says about the files it reaches.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_purge_reports_the_list_it_took_and_the_bytes_it_left() {
    // D-14 makes a purge say what it destroys "by name and by count, and the count includes
    // the things that cascade". Attachments cascade; the bytes do not go (ADR 0013). An
    // administrator confirming a purge is therefore told both numbers, because "3 Anhänge"
    // alone would read as "and the files are gone".
    let store = fixture().await;
    for (page, name, marker) in [
        ("raum/notiz", "nur-hier.png", "purge-eigen"),
        ("raum/notiz", "geteilt.png", "purge-geteilt"),
        ("geheim", "geteilt.png", "purge-geteilt"),
    ] {
        let (status, _, _) = as_user(
            &store,
            "chefin",
            "POST",
            &format!("/api/attachment/{name}/{page}"),
            png(marker),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }

    let (status, _, _) = as_user(
        &store,
        "chefin",
        "DELETE",
        "/api/documents/raum/notiz",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _, body) = as_user(
        &store,
        "chefin",
        "GET",
        "/api/trash/purge/raum/notiz",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let preview = json(&body);
    assert_eq!(preview["committed"], false);
    assert_eq!(preview["attachments"], 2);
    assert_eq!(
        preview["blobs_orphaned"], 1,
        "only the file the surviving page does not also carry"
    );

    let (status, _, body) = as_user(
        &store,
        "chefin",
        "POST",
        "/api/trash/purge/raum/notiz",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let done = json(&body);
    assert_eq!(done["committed"], true);
    assert_eq!(done["attachments"], preview["attachments"]);
    assert_eq!(done["blobs_orphaned"], preview["blobs_orphaned"]);

    // The file that was on both pages is still reachable through the one that survived: a
    // purge over there is not a change to this page.
    let (status, _, bytes) = as_user(
        &store,
        "chefin",
        "GET",
        "/api/attachment/geteilt.png/geheim",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(bytes, png("purge-geteilt"));
}

#[tokio::test]
async fn a_file_the_mount_has_truncated_is_refused_rather_than_served_short() {
    // `Content-Length` is sent from the size the database recorded, so a file the mount has
    // cut short would otherwise become a response that simply stops — which looks exactly
    // like a dropped connection and would never be reported by anybody.
    let store = fixture().await;
    let bytes = png("diese-bytes-werden-abgeschnitten");
    let (status, _, _) = as_user(
        &store,
        "schreiber",
        "POST",
        "/api/attachment/halb.png/raum",
        bytes.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (principal, _) = store
        .principal_by_username("schreiber")
        .await
        .unwrap()
        .unwrap();
    let source = store
        .attachment_for(&principal, "/raum", "halb.png")
        .await
        .unwrap()
        .unwrap();
    let path = state(&store).blobs.path_for(&source.sha256).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() - 3]).unwrap();

    let (status, _, body) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/halb.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        !contains(&body, &bytes[..bytes.len() - 3]),
        "and none of the short file came back"
    );

    // Anti-vacuity: the whole file, on the same address, is served.
    std::fs::write(&path, &bytes).unwrap();
    let (status, _, body) = as_user(
        &store,
        "leser",
        "GET",
        "/api/attachment/halb.png/raum",
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, bytes);
}
