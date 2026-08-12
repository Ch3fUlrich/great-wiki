//! The collaboration endpoint: who may open an editing session, and what happens to one
//! that should no longer exist.
//!
//! Every test here drives a REAL socket against a real listener. That is not thoroughness
//! for its own sake: the properties under test are "the handshake is refused with a status"
//! and "the session is ended with a CLOSE", and neither is observable through
//! `oneshot`-ing the router in process. A test that asserted "no update was applied" would
//! pass just as well against a server that accepted the connection and quietly ignored
//! everything on it — which is the exact failure this endpoint must not have, because to
//! the person typing it looks like an editor that is saving their work.
//!
//! The fixture holds one page and four people, chosen so that no two of them see the same
//! thing:
//!
//! - `autorin` — granted `write` on `/handbuch`. The only one who may edit.
//! - `leserin` — granted `read` on `/handbuch`. May read the page and may not join the
//!   session.
//! - `fremde` — an account with no grant at all.
//! - `chef` — an `admins` member, so an instance admin by baseline (D-M2-1) — and still
//!   not a writer, because write is only ever an explicit grant (D-M2-8).

use axum::http::{header, HeaderValue, StatusCode};
use futures_util::{SinkExt, StreamExt};
use gw_api::routes::collab::{sweep, CollabPolicy, CollabState};
use gw_api::AppState;
use gw_auth::{Permission, Principal, Subject};
use gw_collab::CollabDoc;
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

// -------------------------------------------------------------------------------------
// Fixture.
// -------------------------------------------------------------------------------------

fn body(text: &str) -> Block {
    serde_json::from_str(&format!(
        r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}"#
    ))
    .unwrap()
}

async fn insert(store: &Store, parent: Option<&str>, slug: &str, title: &str, vis: Visibility) {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(str::to_string),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: vis,
                body: body("Größe und Maß"),
                sort_key: 0,
            },
            None,
        )
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    insert(&store, None, "handbuch", "Handbuch", Visibility::Restricted).await;
    insert(
        &store,
        Some("/handbuch"),
        "onboarding",
        "Onboarding",
        Visibility::Restricted,
    )
    .await;
    insert(
        &store,
        None,
        "oeffentlich",
        "Öffentlich",
        Visibility::Public,
    )
    .await;

    for (username, display) in [
        ("autorin", "Autorin"),
        ("leserin", "Leserin"),
        ("fremde", "Fremde"),
    ] {
        store
            .create_local_principal(username, display, None, "$argon2id$fake")
            .await
            .unwrap();
    }
    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();

    grant(&store, "autorin", Permission::Write).await;
    grant(&store, "leserin", Permission::Read).await;

    Arc::new(store)
}

async fn principal(store: &Store, username: &str) -> Principal {
    store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"))
        .0
}

async fn grant(store: &Store, username: &str, permission: Permission) {
    let id = principal(store, username).await.id;
    store
        .add_grant("/handbuch", Subject::Principal(id), permission)
        .await
        .unwrap();
}

async fn revoke(store: &Store, username: &str, permission: Permission) {
    let id = principal(store, username).await.id;
    assert!(
        store
            .remove_grant("/handbuch", &Subject::Principal(id), permission)
            .await
            .unwrap(),
        "the grant this test revokes was never there"
    );
}

/// A state whose requests arrive as `username`, through the development shim — so the
/// principal is re-read from the store on every resolution, exactly as a session cookie's
/// is. That re-read is what makes "revoke the grant mid-session" testable at all.
async fn state_as(store: &Arc<Store>, username: &str) -> AppState {
    let principal = principal(store, username).await;
    AppState::for_test_principal(Arc::clone(store), &principal)
}

fn anonymous_state(store: &Arc<Store>) -> AppState {
    AppState::for_test(Arc::clone(store), None)
}

/// Bind a real listener and serve `state` on it. The address, as `127.0.0.1:port`.
async fn serve(state: AppState) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = gw_api::build_router(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr.to_string()
}

/// Open an editing session at `path`, or report the status the handshake was refused with.
async fn connect(addr: &str, path: &str, cookie: Option<&str>) -> Result<Socket, StatusCode> {
    let mut request = format!("ws://{addr}/api/collab/{path}")
        .into_client_request()
        .unwrap();
    if let Some(value) = cookie {
        request
            .headers_mut()
            .insert(header::COOKIE, HeaderValue::from_str(value).unwrap());
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((socket, _)) => Ok(socket),
        Err(WsError::Http(response)) => Err(response.status()),
        Err(other) => panic!("the handshake failed for an unexpected reason: {other}"),
    }
}

/// The status an upgrade at `path` is refused with, as `username` — or a panic naming the
/// fact that it was NOT refused, which is the failure that matters here.
async fn refused(store: &Arc<Store>, username: Option<&str>, path: &str) -> StatusCode {
    let state = match username {
        Some(username) => state_as(store, username).await,
        None => anonymous_state(store),
    };
    let addr = serve(state).await;
    match connect(&addr, path, None).await {
        Ok(_) => panic!("the upgrade was ACCEPTED for {username:?} at /{path}"),
        Err(status) => status,
    }
}

// -------------------------------------------------------------------------------------
// Who may open a session at all.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn an_anonymous_upgrade_is_refused() {
    let store = fixture().await;
    assert_eq!(
        refused(&store, None, "handbuch").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_account_with_no_grant_is_refused() {
    let store = fixture().await;
    assert_eq!(
        refused(&store, Some("fremde"), "handbuch").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_reader_is_refused_because_reading_a_page_is_not_editing_it() {
    // D-M2-8. `leserin` may read this page — `/api/documents/handbuch` answers 200 for
    // her — and that must not admit her to the editing session, where every keystroke
    // anyone else makes is broadcast and where an update would be applied.
    let store = fixture().await;
    assert_eq!(
        refused(&store, Some("leserin"), "handbuch").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn an_instance_admin_with_no_grant_is_refused() {
    // The one that would pass if the socket authorised on "can reach the page" rather than
    // on `Action::Write`: an `admins` member reaches every page in the tree by baseline,
    // and still holds no write grant anywhere (D-M2-8).
    let store = fixture().await;
    assert_eq!(
        refused(&store, Some("chef"), "handbuch").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_public_page_is_not_publicly_editable() {
    // Public means readable, never editable. A socket that read visibility instead of the
    // grant would hand the whole internet an editing session on this page.
    let store = fixture().await;
    assert_eq!(
        refused(&store, None, "oeffentlich").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        refused(&store, Some("autorin"), "oeffentlich").await,
        StatusCode::FORBIDDEN,
        "a write grant on /handbuch must not reach a page outside it"
    );
}

#[tokio::test]
async fn a_page_that_does_not_exist_is_404_and_a_forbidden_one_is_403() {
    // The same split `/api/documents` makes, for the same reason: collapsing both to 404
    // hides configuration mistakes, collapsing both to 403 confirms the existence of every
    // path somebody guesses.
    let store = fixture().await;
    assert_eq!(
        refused(&store, Some("autorin"), "gibt-es-nicht").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        refused(&store, Some("leserin"), "handbuch").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn a_writer_may_open_a_session() {
    // The other side of every refusal above: without this they could all pass because the
    // endpoint refuses everybody.
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    assert!(
        connect(&addr, "handbuch", None).await.is_ok(),
        "the writer was refused her own editing session"
    );
}

#[tokio::test]
async fn a_grant_on_a_parent_reaches_the_page_below_it() {
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    assert!(connect(&addr, "handbuch/onboarding", None).await.is_ok());
}

// -------------------------------------------------------------------------------------
// View-as (D-M2-17): a WebSocket upgrade is a GET, and `view_as::enforce` passes GETs.
// -------------------------------------------------------------------------------------

/// Put `viewer` into view-as mode on `target` and hand back the cookie the browser holds.
///
/// Activated over HTTP against the SAME state the socket is served from, because the
/// registry lives in `AppState`: a second state would issue a cookie naming a record this
/// server has never heard of, and the test would prove nothing.
async fn view_as_cookie(state: &AppState, store: &Arc<Store>, target: &str) -> String {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let id = principal(store, target).await.id;
    let response = gw_api::build_router(state.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/admin/view-as/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "the fixture could not enter view-as mode"
    );
    let raw = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .find(|v| v.starts_with(gw_api::view_as::VIEW_AS_COOKIE))
        .expect("no view-as cookie was issued");
    raw.split(';').next().unwrap().to_string()
}

#[tokio::test]
async fn an_administrator_viewing_as_a_writer_cannot_open_that_writer_s_session() {
    // `view_as::enforce` refuses every non-GET, and a WebSocket upgrade IS a GET — so the
    // layer that closes this class of hole everywhere else does not close it here. Without
    // an explicit refusal in the handler, an administrator could open `autorin`'s editing
    // session, type into the live document as her, and have it published under her name by
    // the next snapshot.
    let store = fixture().await;
    let state = state_as(&store, "chef").await;
    let addr = serve(state.clone()).await;
    let cookie = view_as_cookie(&state, &store, "autorin").await;

    match connect(&addr, "handbuch", Some(&cookie)).await {
        Ok(_) => panic!("an editing session was opened while viewing as somebody else"),
        Err(status) => assert_eq!(status, StatusCode::FORBIDDEN),
    }
}

#[tokio::test]
async fn the_same_writer_without_the_mode_is_admitted() {
    // The control for the test above. If `autorin` could not connect either, that test
    // would be passing on the wrong refusal.
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    assert!(connect(&addr, "handbuch", None).await.is_ok());
}

// -------------------------------------------------------------------------------------
// The wire: the y-protocol, encoded here by hand so the test does not share an
// implementation with the server it is testing.
// -------------------------------------------------------------------------------------

const MSG_SYNC: u8 = 0;
const MSG_AWARENESS: u8 = 1;
const SYNC_STEP1: u8 = 0;
const SYNC_STEP2: u8 = 1;
const SYNC_UPDATE: u8 = 2;

/// lib0's unsigned varint: seven bits per byte, little-endian, high bit continues.
fn varuint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while n > 0x7f {
        out.push((n as u8 & 0x7f) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
    out
}

fn read_varuint(bytes: &[u8], at: &mut usize) -> u64 {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = bytes[*at];
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return value;
        }
        shift += 7;
    }
}

fn message(kind: u8, sub: Option<u8>, payload: &[u8]) -> Message {
    let mut out = vec![kind];
    if let Some(sub) = sub {
        out.push(sub);
    }
    out.extend(varuint(payload.len() as u64));
    out.extend_from_slice(payload);
    Message::Binary(out.into())
}

/// The `(message type, sync type, payload)` of a binary frame from the server.
fn parse(message: &Message) -> (u8, Option<u8>, Vec<u8>) {
    let Message::Binary(bytes) = message else {
        panic!("expected a binary frame, got {message:?}");
    };
    let mut at = 0usize;
    let kind = read_varuint(bytes, &mut at) as u8;
    let sub = if kind == MSG_SYNC {
        Some(read_varuint(bytes, &mut at) as u8)
    } else {
        None
    };
    let len = read_varuint(bytes, &mut at) as usize;
    (kind, sub, bytes[at..at + len].to_vec())
}

/// How long any single wait in this file may take before it is a failure.
///
/// A DEADLINE for the whole wait and not a gap between frames, which matters because the
/// server heartbeats: a per-message timeout is reset by every ping, so a test waiting for
/// something that will never arrive would wait for ever instead of failing. That is not a
/// hypothetical — it cost an afternoon of debugging a build that was merely stale, because
/// the symptom was a hang rather than a red test.
const PATIENCE: Duration = Duration::from_secs(5);

/// The next binary frame, or a panic if the socket closes or goes quiet first.
async fn next_binary(socket: &mut Socket) -> (u8, Option<u8>, Vec<u8>) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let message = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("the server sent no binary frame before the deadline")
            .expect("the socket closed")
            .expect("the socket failed");
        match message {
            Message::Binary(_) => return parse(&message),
            Message::Ping(_) | Message::Pong(_) => continue,
            other => panic!("expected a binary frame, got {other:?}"),
        }
    }
}

/// Complete the sync handshake and return a replica holding what the server has.
///
/// Exactly what a browser does: send our (empty) state vector, apply the diff that comes
/// back. The server's own step 1 is answered too, because ignoring it would leave the
/// server waiting for content we do not have and change the timing of everything after.
async fn sync(socket: &mut Socket) -> CollabDoc {
    let replica = CollabDoc::new();
    socket
        .send(message(MSG_SYNC, Some(SYNC_STEP1), &replica.state_vector()))
        .await
        .unwrap();

    let mut applied = false;
    while !applied {
        let (kind, sub, payload) = next_binary(socket).await;
        assert_eq!(kind, MSG_SYNC, "unexpected message during the handshake");
        match sub {
            Some(SYNC_STEP1) => {
                let diff = replica.encode_diff(&payload).unwrap();
                socket
                    .send(message(MSG_SYNC, Some(SYNC_STEP2), &diff))
                    .await
                    .unwrap();
            }
            Some(SYNC_STEP2) => {
                replica.apply_update(&payload).unwrap();
                applied = true;
            }
            // Somebody else's work, arriving while this client is still syncing. A browser
            // applies it and carries on; so does this.
            Some(SYNC_UPDATE) => replica.apply_update(&payload).unwrap(),
            other => panic!("unexpected sync message {other:?}"),
        }
    }
    replica
}

/// An update that appends `text`, as a client that has synced would produce it.
fn edit(replica: &CollabDoc, text: &str) -> Vec<u8> {
    let before = replica.state_vector();
    replica.append_paragraph(text);
    replica.encode_diff(&before).unwrap()
}

/// The close frame the server ended the session with, skipping anything still in flight.
async fn close_code(socket: &mut Socket) -> CloseCode {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let next = tokio::time::timeout_at(deadline, socket.next())
            .await
            .expect("the session was not closed before the deadline");
        match next {
            Some(Ok(Message::Close(Some(frame)))) => return frame.code,
            Some(Ok(Message::Close(None))) => panic!("closed with no code and no reason"),
            Some(Ok(_)) => continue,
            Some(Err(error)) => panic!("the socket failed instead of closing: {error}"),
            None => panic!("the stream ended without a close frame"),
        }
    }
}

/// Send `update` as an unsolicited sync update — the frame a browser sends per keystroke.
async fn send_update(socket: &mut Socket, update: &[u8]) {
    socket
        .send(message(MSG_SYNC, Some(SYNC_UPDATE), update))
        .await
        .unwrap();
}

/// Wait until everything sent so far has been processed, and report how many updates
/// arrived while waiting.
///
/// Frames are handled in order, so a reply to a request sent *after* an update proves the
/// update was dealt with. Without it, every assertion about what the server did with an
/// update would be a race dressed up as a test.
///
/// The count is not decoration: without it this helper would swallow the sender's own echo
/// on its way to the reply it is waiting for, and the test that exists to notice an echo
/// would notice nothing. That is not hypothetical — it is what the first version did, and
/// deleting the echo check in the server left it green.
async fn settle(socket: &mut Socket, replica: &CollabDoc) -> usize {
    socket
        .send(message(MSG_SYNC, Some(SYNC_STEP1), &replica.state_vector()))
        .await
        .unwrap();
    let mut updates = 0;
    loop {
        let (kind, sub, payload) = next_binary(socket).await;
        match (kind, sub) {
            (MSG_SYNC, Some(SYNC_STEP2)) => {
                replica.apply_update(&payload).unwrap();
                return updates;
            }
            (MSG_SYNC, Some(SYNC_UPDATE)) => {
                replica.apply_update(&payload).unwrap();
                updates += 1;
            }
            other => panic!("unexpected frame while settling: {other:?}"),
        }
    }
}

// -------------------------------------------------------------------------------------
// Reaching the store from a test.
// -------------------------------------------------------------------------------------

async fn document_id(store: &Store, username: &str, path: &str) -> String {
    let principal = principal(store, username).await;
    store
        .document_for(&principal, path, gw_auth::Action::Read)
        .await
        .unwrap()
        .expect("the fixture's page must be readable by this account")
        .id
}

/// Every revision body of `/handbuch`, newest first, as read by somebody who may read it.
async fn history(store: &Store) -> Vec<String> {
    let leserin = principal(store, "leserin").await;
    let id = document_id(store, "leserin", "/handbuch").await;
    store
        .revisions_for(&leserin, &id)
        .await
        .unwrap()
        .into_iter()
        .map(|revision| revision.body)
        .collect()
}

/// A state whose rooms run under `policy`, so a test can reach limits and deadlines that
/// production deliberately puts out of reach.
fn under(state: AppState, policy: CollabPolicy) -> AppState {
    AppState {
        collab: Arc::new(CollabState::with_policy(policy)),
        ..state
    }
}

/// Intervals a test can outlive. Everything not named here stays at the production value,
/// so a test cannot accidentally assert against a limit nobody ships.
fn brisk() -> CollabPolicy {
    CollabPolicy {
        reauth_interval: Duration::from_millis(50),
        idle_grace: Duration::ZERO,
        ..CollabPolicy::default()
    }
}

// -------------------------------------------------------------------------------------
// What a session does when it works.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_session_is_handed_the_page_as_it_stands() {
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();

    let replica = sync(&mut ws).await;
    assert!(
        replica.to_block().plain_text().contains("Größe und Maß"),
        "the session began on an empty document instead of the stored page"
    );
}

#[tokio::test]
async fn two_editors_of_one_page_see_each_other_s_work() {
    // The requirement the milestone exists for, at the level this file is responsible for:
    // one room per page, and what one person types reaches the other.
    let store = fixture().await;
    let state = state_as(&store, "autorin").await;
    let addr = serve(state.clone()).await;

    let mut a = connect(&addr, "handbuch", None).await.unwrap();
    let mut b = connect(&addr, "handbuch", None).await.unwrap();
    let von_a = sync(&mut a).await;
    let von_b = sync(&mut b).await;

    send_update(&mut a, &edit(&von_a, "von A")).await;

    let (kind, sub, payload) = next_binary(&mut b).await;
    assert_eq!((kind, sub), (MSG_SYNC, Some(SYNC_UPDATE)));
    von_b.apply_update(&payload).unwrap();
    assert!(
        von_b.to_block().plain_text().contains("von A"),
        "the peer received a frame that did not carry the edit"
    );
    assert_eq!(state.collab.open_rooms(), 1, "one page, one room");
}

#[tokio::test]
async fn a_sender_is_not_sent_its_own_update_back() {
    // A client that received its own updates would apply them again. Yjs is idempotent so
    // nothing would corrupt, but every keystroke would cost a round trip for nothing.
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    send_update(&mut ws, &edit(&replica, "von A")).await;

    assert_eq!(
        settle(&mut ws, &replica).await,
        0,
        "the sender received its own update back"
    );
}

// -------------------------------------------------------------------------------------
// What a session does when it should not continue.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_forged_update_closes_the_session_rather_than_being_ignored() {
    // The milestone's exit criterion. Ignoring it would leave a client believing its edits
    // were being saved; `gw-collab` returns an error rather than panicking for exactly this
    // moment, and this is the moment.
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    send_update(&mut ws, &[0xff, 0xff, 0xff, 0xff]).await;

    assert_eq!(close_code(&mut ws).await, CloseCode::Unsupported);
    // And nothing of it reached the document.
    assert_eq!(
        replica.to_block().plain_text(),
        "Größe und Maß",
        "a rejected update changed the replica"
    );
}

#[tokio::test]
async fn a_malformed_state_vector_closes_the_session() {
    // `encode_diff` takes a state vector straight off the socket and is exactly as
    // reachable as an update.
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    ws.send(message(MSG_SYNC, Some(SYNC_STEP1), &[0xff, 0xff, 0xff]))
        .await
        .unwrap();
    assert_eq!(close_code(&mut ws).await, CloseCode::Unsupported);
}

#[tokio::test]
async fn a_truncated_frame_closes_the_session() {
    let store = fixture().await;
    let addr = serve(state_as(&store, "autorin").await).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    // "here are two hundred bytes", followed by two.
    ws.send(Message::Binary(
        vec![MSG_SYNC, SYNC_UPDATE, 200, 1, 2].into(),
    ))
    .await
    .unwrap();
    assert_eq!(close_code(&mut ws).await, CloseCode::Unsupported);
}

#[tokio::test]
async fn a_writer_whose_grant_is_revoked_is_closed_mid_session() {
    // D-M2-7 says a revocation takes effect on the next request. A socket makes no
    // requests, so without a re-check it would take effect when the tab is closed — which
    // is to say, not at all.
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    revoke(&store, "autorin", Permission::Write).await;

    // Nothing is sent from here on: this is the arm that closes a session which is merely
    // listening. Leaving it open would keep somebody subscribed to every keystroke on a
    // page they may no longer even read.
    assert_eq!(close_code(&mut ws).await, CloseCode::Policy);
}

#[tokio::test]
async fn a_demoted_writer_s_next_update_is_refused_with_a_close() {
    // The same property from the writing end: what a demoted client tries to save is
    // refused, and it is told so.
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state.clone()).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    revoke(&store, "autorin", Permission::Write).await;
    grant(&store, "autorin", Permission::Read).await;
    tokio::time::sleep(Duration::from_millis(80)).await;
    send_update(&mut ws, &edit(&replica, "nach dem Entzug")).await;

    assert_eq!(close_code(&mut ws).await, CloseCode::Policy);

    // And what she sent after the revocation is not in the database, whatever the sweep
    // does with the room afterwards.
    sweep(&state).await;
    assert!(
        !history(&store).await.iter().any(|b| b.contains("Entzug")),
        "an update sent after the grant was revoked reached the history"
    );
}

#[tokio::test]
async fn a_deactivated_account_is_closed_mid_session() {
    // The account, not the grant. `principal_by_username` is re-read on every resolution,
    // so a deactivation lands on the next check exactly as it lands on the next request.
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    let id = principal(&store, "autorin").await.id;
    store.set_principal_active(&id, false).await.unwrap();

    assert_eq!(close_code(&mut ws).await, CloseCode::Policy);
}

#[tokio::test]
async fn an_oversized_frame_closes_the_session() {
    let store = fixture().await;
    let policy = CollabPolicy {
        max_message_bytes: 4096,
        ..brisk()
    };
    let addr = serve(under(state_as(&store, "autorin").await, policy)).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    ws.send(Message::Binary(vec![0u8; 8192].into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut ws).await, CloseCode::Size);
}

#[tokio::test]
async fn a_session_is_pinged_so_that_a_peer_which_vanished_is_eventually_noticed() {
    // A subscriber is what makes a room ineligible for eviction, so a browser that goes
    // away without closing its socket would hold a document in memory for the life of the
    // process. The heartbeat is what turns that into a write that eventually fails.
    let store = fixture().await;
    let addr = serve(under(state_as(&store, "autorin").await, brisk())).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    let deadline = tokio::time::Instant::now() + PATIENCE;
    loop {
        let message = tokio::time::timeout_at(deadline, ws.next())
            .await
            .expect("no heartbeat before the deadline")
            .expect("the socket closed")
            .expect("the socket failed");
        if matches!(message, Message::Ping(_)) {
            return;
        }
    }
}

#[tokio::test]
async fn a_page_accepts_only_as_many_editors_as_its_policy_allows() {
    // One authenticated writer opening sockets in a loop is the cheapest way to make this
    // server allocate: a task, a buffer and a place in the fan-out for each one.
    let store = fixture().await;
    let policy = CollabPolicy {
        max_connections_per_room: 2,
        ..brisk()
    };
    let state = under(state_as(&store, "autorin").await, policy);
    let addr = serve(state.clone()).await;

    let _first = connect(&addr, "handbuch", None).await.unwrap();
    let _second = connect(&addr, "handbuch", None).await.unwrap();
    // The room needs a moment to register the second subscriber: the upgrade completes
    // before the task behind it runs.
    for _ in 0..100 {
        if connect(&addr, "handbuch", None).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the room accepted a third editor with a cap of two");
}

#[tokio::test]
async fn a_flood_of_frames_closes_the_session() {
    // A public endpoint must bound what one connection can make the server do. Sent as
    // presence frames so the refusal cannot be mistaken for one of the update checks.
    let store = fixture().await;
    let policy = CollabPolicy {
        max_messages_per_second: 8,
        ..brisk()
    };
    let addr = serve(under(state_as(&store, "autorin").await, policy)).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    for _ in 0..64 {
        ws.send(message(MSG_AWARENESS, None, b"hier"))
            .await
            .unwrap();
    }
    assert_eq!(close_code(&mut ws).await, CloseCode::Policy);
}

#[tokio::test]
async fn a_flood_of_pings_closes_the_session_too() {
    // The transport answers a ping whether or not the handler looks at it, so a message
    // type exempt from the budget is a message type a client may send without limit and be
    // answered every time. Counted like everything else.
    let store = fixture().await;
    let policy = CollabPolicy {
        max_messages_per_second: 8,
        ..brisk()
    };
    let addr = serve(under(state_as(&store, "autorin").await, policy)).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;

    for _ in 0..64 {
        ws.send(Message::Ping(Default::default())).await.unwrap();
    }
    assert_eq!(close_code(&mut ws).await, CloseCode::Policy);
}

// -------------------------------------------------------------------------------------
// Publishing.
// -------------------------------------------------------------------------------------

async fn publish_as(
    store: &Arc<Store>,
    state: &AppState,
    addr: &str,
    username: &str,
    summary: &str,
) -> StatusCode {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    // Through a router built on the SAME collaboration state the socket is served from —
    // publishing takes what is in the live room, and a second state has no room in it.
    let _ = addr;
    let publisher = AppState {
        collab: Arc::clone(&state.collab),
        ..state_as(store, username).await
    };
    gw_api::build_router(publisher)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/collab/handbuch")
                .header("content-type", "application/json")
                .body(Body::from(format!(r#"{{"summary":"{summary}"}}"#)))
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn publishing_records_what_was_typed_and_needs_write_to_do_it() {
    let store = fixture().await;
    let state = state_as(&store, "autorin").await;
    let addr = serve(state.clone()).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    send_update(&mut ws, &edit(&replica, "von A")).await;
    let _ = settle(&mut ws, &replica).await;

    assert_eq!(
        publish_as(&store, &state, &addr, "leserin", "von der Leserin").await,
        StatusCode::FORBIDDEN,
        "a reader published a revision"
    );
    assert!(
        !history(&store).await.iter().any(|b| b.contains("von A")),
        "the refused publish wrote a revision anyway"
    );

    assert_eq!(
        publish_as(&store, &state, &addr, "autorin", "erste Fassung").await,
        StatusCode::OK
    );
    assert!(
        history(&store).await[0].contains("von A"),
        "the published revision does not hold what was typed"
    );
}

#[tokio::test]
async fn publishing_a_page_nobody_is_editing_is_refused_rather_than_writing_nothing() {
    // 409 and not a silent 200: publishing the stored body back over itself would add a
    // revision saying that nothing happened.
    let store = fixture().await;
    let state = state_as(&store, "autorin").await;
    let before = history(&store).await.len();

    assert_eq!(
        publish_as(&store, &state, "", "autorin", "ins Leere").await,
        StatusCode::CONFLICT
    );
    assert_eq!(history(&store).await.len(), before);
}

#[tokio::test]
async fn publishing_while_viewing_as_somebody_else_is_refused() {
    // The other half of the view-as story, and this one IS covered by the router layer —
    // a POST is a non-GET. Asserted here anyway, because "the layer covers it" is a
    // property of another file that this endpoint depends on.
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    let store = fixture().await;
    let state = state_as(&store, "chef").await;
    let cookie = view_as_cookie(&state, &store, "autorin").await;

    let response = gw_api::build_router(state)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/collab/handbuch")
                .header(header::COOKIE, cookie)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// -------------------------------------------------------------------------------------
// Persistence: what reaches the database without anybody pressing a button.
// -------------------------------------------------------------------------------------

/// Sweep until the rooms are gone, or fail loudly. The server needs a moment to notice a
/// closed socket, which is a race in the operating system and not in the code under test.
async fn sweep_until_empty(state: &AppState) {
    for _ in 0..100 {
        sweep(state).await;
        if state.collab.open_rooms() == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the rooms were never evicted");
}

#[tokio::test]
async fn what_was_typed_reaches_the_database_without_anybody_publishing() {
    // The crash window. Nobody pressed publish and nobody closed the tab tidily; the
    // sweep is what stands between an editing session and losing it.
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state.clone()).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    send_update(&mut ws, &edit(&replica, "noch nicht veröffentlicht")).await;
    let _ = settle(&mut ws, &replica).await;

    sweep(&state).await;

    let history = history(&store).await;
    assert!(
        history[0].contains("noch nicht veröffentlicht"),
        "an unpublished session was not written out: {:?}",
        history[0]
    );
}

#[tokio::test]
async fn a_room_nobody_is_in_is_written_out_and_then_forgotten() {
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state.clone()).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    send_update(&mut ws, &edit(&replica, "und dann ging sie")).await;
    let _ = settle(&mut ws, &replica).await;
    ws.close(None).await.unwrap();

    sweep_until_empty(&state).await;
    assert!(history(&store).await[0].contains("und dann ging sie"));
}

#[tokio::test]
async fn a_session_that_typed_nothing_leaves_no_revision_behind() {
    // A client's first sync reply is an update like any other, so without a comparison
    // against what the database already holds, opening an editor and closing it again
    // would publish a revision identical to the one before it — once per sweep, for ever.
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state.clone()).await;
    let before = history(&store).await.len();

    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    sync(&mut ws).await;
    ws.close(None).await.unwrap();

    sweep_until_empty(&state).await;
    assert_eq!(
        history(&store).await.len(),
        before,
        "a session in which nothing was typed added a revision"
    );
}

#[tokio::test]
async fn a_room_that_is_still_being_edited_is_written_out_but_not_evicted() {
    // The two halves of a sweep are independent: work is saved on a schedule, and only a
    // room nobody is in is dropped. Evicting one somebody is still editing would take their
    // document out from under them.
    let store = fixture().await;
    let state = under(state_as(&store, "autorin").await, brisk());
    let addr = serve(state.clone()).await;
    let mut ws = connect(&addr, "handbuch", None).await.unwrap();
    let replica = sync(&mut ws).await;

    send_update(&mut ws, &edit(&replica, "mitten im Satz")).await;
    let _ = settle(&mut ws, &replica).await;
    sweep(&state).await;

    assert!(history(&store).await[0].contains("mitten im Satz"));
    assert_eq!(
        state.collab.open_rooms(),
        1,
        "the room somebody is editing in was evicted"
    );

    // And the session is still usable afterwards.
    send_update(&mut ws, &edit(&replica, "und weiter")).await;
    let _ = settle(&mut ws, &replica).await;
    sweep(&state).await;
    assert!(history(&store).await[0].contains("und weiter"));
}
