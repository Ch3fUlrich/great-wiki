//! The collaboration endpoint: the wire between a browser's CRDT and the room that holds
//! the live document.
//!
//! `gw-collab` owns the document and the fan-out and is deliberately free of HTTP and of
//! the database. This file is the other three quarters of the problem: who may open a
//! session, what a client is allowed to make the server allocate, when a session that
//! should no longer exist is ended, and how what was typed reaches the database.
//!
//! # The authorisation story
//!
//! **A session is keyed by PATH, never by a document id the client chose.** The client
//! asks for `/api/collab/handbuch/onboarding`; the store resolves that path *as this
//! principal* through [`gw_store::Store::document_for`] — the crate's one
//! permission-checked document accessor — and the room is keyed by the id that comes back.
//! An id supplied by the client would have to be authorised by resolving it to a path
//! anyway, and every extra step between "what was asked for" and "what was checked" is a
//! step where the two can differ.
//!
//! **The action is [`Action::Write`], and reading a page is not permission to join its
//! editing session.** A caller with only `read` is refused at the handshake with 403 and
//! never reaches the socket. This is stricter than the M3 plan, which specified a
//! read-only connection for such a caller; see the module's `DEVIATIONS` note below.
//!
//! **View-as (D-M2-17) cannot open one.** [`crate::view_as::enforce`] refuses every non-GET
//! request while an administrator is viewing as somebody else — and a WebSocket upgrade
//! *is* a GET, so the layer that closes this class of hole everywhere else does not close
//! it here. It is closed in [`authorise`] instead, which is the only place it can be.
//!
//! **Authorisation is re-checked during the session**, not only at the handshake. A grant
//! can be revoked, an account deactivated and a session ended while a socket is open, and
//! an editing session that outlives the permission behind it is a permission that was never
//! really revoked. See [`CollabPolicy::reauth_interval`].
//!
//! # DEVIATIONS from the M3 plan, and why
//!
//! - **No read-only connection.** The plan says `Action::Read` alone yields a connection
//!   that rejects updates. It is refused outright instead: a reader on the socket is a
//!   subscriber, and a subscriber keeps a room from ever being idle — so read access would
//!   confer the ability to pin a document's CRDT in memory for as long as the connection is
//!   held open. Editing is a write-shaped activity and the endpoint that serves it takes
//!   the write decision.
//! - **`POST /api/collab/{path}` rather than `POST /api/documents/{id}/publish`.** The
//!   latter cannot coexist safely with the existing `GET /api/documents/{*path}`: matchit
//!   prefers the more specific route, so a page whose slug is literally `publish` would
//!   answer 405 to a read of itself. Publishing therefore lives on the same resource as the
//!   session it publishes, which also keeps the document id out of the client's hands.
//!
//! # What reaches the database, and what a crash can lose
//!
//! `gw-collab` has no store dependency by design: [`gw_collab::Rooms::evict_idle`] hands
//! the evicted rooms back *so that the caller persists them*, and this is that caller. One
//! [`sweep`] does both halves — write out every room that has changed, then evict the ones
//! nobody is in — and [`spawn_janitor`] runs it every
//! [`CollabPolicy::snapshot_interval`].
//!
//! **The window of loss is one snapshot interval.** A crash loses whatever was typed since
//! the last sweep, which is at most 30 seconds of work in production.
//!
//! **What a sweep writes is the CRDT STATE, and never a revision.** The two are different
//! kinds of thing and migration 0008 gave them different tables for that reason. A revision
//! is a deliberate publish: it is append-only, it records an author, and it is what a
//! reader sees. Autosave is neither deliberate nor a publication, and filing it as a
//! revision meant an actively edited page collected a version every thirty seconds that
//! nobody asked for — a history full of rows that say nothing is a history nobody can read.
//! Publishing is what [`publish`] does, when a **person** asks for it.
//!
//! It also matters for what is *kept*. A `Block` — what a revision stores — has no field
//! for an inline mark, so [`gw_collab::Room::snapshot`] drops bold, italic and link
//! destinations on the floor; the encoded CRDT state keeps them. While a revision was the
//! only thing surviving a restart, a session that came back had already lost its
//! formatting. Nothing renders marks yet, which is exactly why this had to be fixed before
//! M4 adds them rather than after.
//!
//! The consequence, stated because it is a real change in behaviour: between two publishes,
//! what an **editor** opens (the CRDT state) and what a **reader** sees (`documents.body`,
//! which only a revision moves) are allowed to differ. That is what "publish" means.
//!
//! # Rebuilding a room, and the one way to get it wrong
//!
//! [`open_room`] builds a room from the stored CRDT state whenever there is one, and seeds
//! it from the page body **only** when there is not. That is not a preference. `from_block`
//! *creates* content: a room seeded from the body and then given its own stored state holds
//! every word of the page twice, under two client ids, and the CRDT will faithfully keep
//! both for ever. `gw-collab`'s
//! `seeding_twice_from_one_block_makes_two_documents_that_never_converge` is that failure
//! written down, and `an_evicted_room_comes_back_from_its_stored_state` is what stops this
//! file from causing it.
//!
//! # What is NOT bounded here, stated so it is not mistaken for covered
//!
//! [`CollabPolicy`] bounds the size of a message, the rate of messages, and the number of
//! connections to one page. It does not bound how large a document may *become*: a client
//! that may write can keep sending valid updates, and a CRDT has no way to un-apply one. No
//! write path in this system caps a body — `documents.body` is equally unbounded over
//! HTTP — so a cap belongs in `gw-store`, where it would apply to every path at once,
//! rather than here, where it would apply to one and read as though it applied to all.

use crate::error::ApiError;
use crate::routes::AppState;
use axum::extract::ws::{close_code, CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use futures_util::stream::{SplitSink, StreamExt};
use futures_util::SinkExt;
use gw_auth::{Action, Principal};
use gw_collab::Room;
use gw_core::{Block, BlockKind};
use gw_store::StoredDocument;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast::error::RecvError;

// -------------------------------------------------------------------------------------
// Policy: every limit and every interval, in one place and as VALUES.
// -------------------------------------------------------------------------------------

/// What a session may cost, and how often the server re-asks the questions it asked at the
/// handshake.
///
/// A struct of values rather than a set of constants read at the point of use, for the
/// reason [`crate::view_as::Registry::with_ttl`] gives about the view-as deadline: a
/// production interval no test can outlive is an interval no test can *prove*. Deleting the
/// re-authorisation call, or the eviction, must break something — and it only can if a test
/// is able to reach the moment it matters.
#[derive(Debug, Clone, Copy)]
pub struct CollabPolicy {
    /// How often a live session re-asks "may this caller still edit this page?".
    ///
    /// Ten seconds, and it is a compromise between two costs that are both real. Checking
    /// on every inbound frame would put three SQLite queries in the path of every keystroke
    /// on a pool that holds ONE connection; checking only at the handshake would let a
    /// revoked grant stay effective until the browser tab is closed. Ten seconds is the
    /// same order as `view_as`'s "next click" and bounds how long a demotion can be
    /// outlived.
    ///
    /// It is enforced from both ends: a timer, so an idle listener is closed too, and a
    /// check before applying an update, so the first thing a demoted client tries to write
    /// is refused rather than merely reported late.
    pub reauth_interval: Duration,
    /// How often [`sweep`] runs. Also the window a crash can lose, which is why it is a
    /// short interval and not an hour.
    pub snapshot_interval: Duration,
    /// How long a room with nobody in it is kept before it is persisted and dropped.
    ///
    /// Not zero: a page reload takes a second or two, and rebuilding the room from the
    /// database in between would be pure work. Not long either — a room is a copy of a
    /// document held in memory.
    pub idle_grace: Duration,
    /// The largest single message a session will act on.
    ///
    /// One mebibyte. The largest thing a legitimate client sends is its whole document
    /// state during the initial sync, and the biggest page in this corpus is a 27×8 table
    /// that encodes to a few tens of kilobytes; a keystroke is tens of bytes. The transport
    /// is given twice this as its hard ceiling — see [`Self::transport_limit`] — so that a
    /// frame over the policy limit is refused with a close frame the client can act on
    /// rather than by the connection simply dying.
    pub max_message_bytes: usize,
    /// Messages per second, per connection.
    ///
    /// A fast typist generates perhaps fifteen updates a second and awareness heartbeats
    /// arrive every thirty; 240 is more than ten times the busiest honest client and still
    /// cheap to refuse.
    pub max_messages_per_second: u32,
    /// Bytes per second, per connection. A message limit alone does not bound volume, and a
    /// byte limit alone does not bound the per-message cost of many tiny frames.
    pub max_bytes_per_second: usize,
    /// How many connections one document's room accepts.
    ///
    /// Every applied update is copied to each subscriber, so this bounds the cost of a
    /// keystroke as well as the memory a room holds. Thirty-two people editing one page at
    /// once is not this wiki; a client opening thirty-two sockets to one page is.
    pub max_connections_per_room: usize,
}

impl Default for CollabPolicy {
    fn default() -> Self {
        Self {
            reauth_interval: Duration::from_secs(10),
            snapshot_interval: Duration::from_secs(30),
            idle_grace: Duration::from_secs(60),
            max_message_bytes: 1024 * 1024,
            max_messages_per_second: 240,
            max_bytes_per_second: 1024 * 1024,
            max_connections_per_room: 32,
        }
    }
}

impl CollabPolicy {
    /// The ceiling the WebSocket transport itself enforces.
    ///
    /// Deliberately above [`Self::max_message_bytes`]: at the transport limit tungstenite
    /// aborts the connection with no close frame, which is exactly the silent failure this
    /// endpoint must not have. Leaving headroom means the handler sees the oversized frame
    /// and refuses it with a code and a reason, while the transport still bounds what any
    /// frame can make the server allocate.
    fn transport_limit(&self) -> usize {
        self.max_message_bytes * 2
    }
}

// -------------------------------------------------------------------------------------
// The live state.
// -------------------------------------------------------------------------------------

/// What the server remembers about one open room between sweeps.
#[derive(Debug)]
struct Bookkeeping {
    /// The principal a snapshot would be attributed to: whoever last had an update accepted.
    ///
    /// `None` means nothing has been typed since the last sweep, so there is nothing to
    /// write. It is a principal *id* rather than a `Principal` because the account is
    /// re-read at sweep time — writing a page's content is `Action::Write` whether a person
    /// asked for it or a timer did, and an account can be suspended between the keystroke
    /// and the write.
    writer: Option<String>,
    /// The encoded CRDT state as it was last written to the database, so an unchanged room
    /// is not rewritten once per sweep. It is not hypothetical: a client's first sync reply
    /// is an update like any other, so opening an editor and touching nothing would
    /// otherwise mark the room as needing a write for as long as the tab is open.
    ///
    /// Bytes and not a `Block`, because bytes are what is compared against and what is
    /// stored. A comparison made on the lossy view would call two documents equal whenever
    /// the only difference between them was formatting — which is precisely the difference
    /// this table exists to keep.
    saved: Vec<u8>,
}

/// Every open room, and the bookkeeping the database needs that the rooms themselves
/// deliberately do not carry.
pub struct CollabState {
    rooms: gw_collab::Rooms,
    open: Mutex<HashMap<String, Bookkeeping>>,
    /// Held while a room is being BUILT, and for the whole of a [`sweep`].
    ///
    /// It exists for one interleaving, and that interleaving forks a document silently.
    /// A sweep evicts an idle room before it writes it out — deliberately, so the only
    /// remaining reference to what was typed is the one `evict_idle` handed back. In the
    /// window between those two steps the room is gone from [`gw_collab::Rooms`] and its
    /// state has not reached the database yet, so a connection arriving right then would
    /// find no live room, be told the page has no stored state, and seed a fresh room from
    /// the body — which the sweep then overwrites, or which later receives the stored state
    /// on top of its own copy of every word.
    ///
    /// A `tokio` mutex rather than a `std` one because both holders await across it: the
    /// sweep writes to the database, and building a room reads from it. It is taken once
    /// per connection and once per sweep interval, so contention is not a consideration;
    /// correctness of what a room is built from is.
    building: tokio::sync::Mutex<()>,
    policy: CollabPolicy,
}

impl Default for CollabState {
    fn default() -> Self {
        Self::with_policy(CollabPolicy::default())
    }
}

impl CollabState {
    pub fn with_policy(policy: CollabPolicy) -> Self {
        Self {
            rooms: gw_collab::Rooms::new(),
            open: Mutex::new(HashMap::new()),
            building: tokio::sync::Mutex::new(()),
            policy,
        }
    }

    pub fn policy(&self) -> &CollabPolicy {
        &self.policy
    }

    /// How many rooms are open. For tests and for a future health endpoint.
    pub fn open_rooms(&self) -> usize {
        self.rooms.len()
    }

    /// A poisoned lock is recovered from rather than propagated, for the same reason
    /// [`crate::view_as::Registry`] does it: a panic elsewhere must not make it impossible
    /// to save what people have typed.
    fn open(&self) -> std::sync::MutexGuard<'_, HashMap<String, Bookkeeping>> {
        self.open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Note that a room is open, recording what its content already is.
    ///
    /// `baseline` is computed by the caller from the room it just joined, so the comparison
    /// at snapshot time is between two values produced the same way. Only the first joiner
    /// sets it: a later one arrives at a room that may hold unsaved edits, and overwriting
    /// the baseline with what it looks like *now* would mark those edits as already saved.
    /// It is a closure for the same reason — encoding a document to find out that the
    /// answer is already known is work done for nothing on every join.
    fn opened(&self, document_id: &str, baseline: impl FnOnce() -> Vec<u8>) {
        self.open()
            .entry(document_id.to_string())
            .or_insert_with(|| Bookkeeping {
                writer: None,
                saved: baseline(),
            });
    }

    /// Record that `writer_id` had an update accepted into `document_id`.
    fn wrote(&self, document_id: &str, writer_id: &str) {
        if let Some(entry) = self.open().get_mut(document_id) {
            entry.writer = Some(writer_id.to_string());
        }
    }

    /// Every room with unsaved work, and who to attribute it to.
    fn unsaved(&self) -> Vec<(String, String)> {
        self.open()
            .iter()
            .filter_map(|(id, entry)| entry.writer.clone().map(|writer| (id.clone(), writer)))
            .collect()
    }

    /// Whether `state` differs from what was last written for this room.
    fn differs(&self, document_id: &str, state: &[u8]) -> bool {
        self.open()
            .get(document_id)
            .is_none_or(|entry| entry.saved != state)
    }

    /// Record that `state` is now in the database, and that there is nothing outstanding.
    fn saved(&self, document_id: &str, state: Vec<u8>) {
        if let Some(entry) = self.open().get_mut(document_id) {
            entry.writer = None;
            entry.saved = state;
        }
    }

    /// Forget a room. Called when it is evicted, so that a room reopened later compares
    /// against what the database actually holds rather than against a stale baseline.
    fn closed(&self, document_id: &str) {
        self.open().remove(document_id);
    }
}

impl std::fmt::Debug for CollabState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CollabState")
            .field("rooms", &self.rooms.len())
            .field("policy", &self.policy)
            .finish()
    }
}

// -------------------------------------------------------------------------------------
// Routes.
// -------------------------------------------------------------------------------------

pub fn routes() -> Router<AppState> {
    // One resource, two verbs: GET joins the session, POST publishes what is in it. The
    // POST is refused while somebody is being viewed as, by the layer in `build_router`
    // that refuses every non-GET; the GET is refused by `authorise`, because a layer that
    // only sees the method cannot tell an upgrade from a page view.
    Router::new().route("/api/collab/{*path}", get(join).post(publish))
}

/// Paths are stored with a leading slash; the route captures without one.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// The one authorisation decision this endpoint makes, on the handshake and again on every
/// re-check. There is deliberately no second spelling of it.
///
/// The order is the security argument:
///
/// 1. **Not while viewing as somebody else.** An editing session run as a substituted
///    principal would write into the live document as them, and the next snapshot would
///    file it in the history under their name. Checked first, so no store read for a
///    document even happens in that mode.
/// 2. **Existence, then permission.** The same split `/api/documents` makes: an absent path
///    is 404 and a forbidden one is 403, because collapsing them either hides configuration
///    mistakes or confirms the existence of every path somebody guesses.
/// 3. **[`Action::Write`], through the store.** `document_for` consults `can()`; nothing
///    here decides anything about grants, baselines or visibility.
async fn authorise(
    state: &AppState,
    jar: &CookieJar,
    path: &str,
) -> Result<(Principal, StoredDocument), ApiError> {
    if let Some(active) = crate::view_as::active(state, jar).await {
        tracing::warn!(
            path,
            viewer = %active.viewer.username,
            target = %active.target.username,
            "refused: an editing session while viewing as somebody else"
        );
        return Err(ApiError::Forbidden);
    }

    let principal = state.principal(jar).await;

    if !state
        .store
        .document_exists(path)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }

    let document = state
        .store
        .document_for(&principal, path, Action::Write)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    Ok((principal, document))
}

/// Join the editing session for the page at `path`.
///
/// Everything is decided before the upgrade: a refused caller receives an HTTP status and
/// never reaches a socket. That is the difference between "you may not edit this" and a
/// connection that looks like an editor and discards what is typed into it.
async fn join(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Response, ApiError> {
    let path = full_path(&captured);
    let (principal, document) = authorise(&state, &jar, &path).await?;

    let room = open_room(&state, &principal, &document).await?;

    // Checked after joining rather than before, which is safe precisely because the cap can
    // only be reached by a room that already existed: a room with N subscribers had them
    // before this request arrived.
    if room.subscriber_count() >= state.collab.policy.max_connections_per_room {
        tracing::warn!(
            document_id = %document.id,
            subscribers = room.subscriber_count(),
            "refused: this document already has as many editors as it accepts"
        );
        return Err(ApiError::Unavailable);
    }

    // Taken HERE rather than inside the socket task, because a room with no subscribers is
    // a room that may be evicted — and between this handler and the first line of that task
    // there is an upgrade, a scheduler and however long the client takes. Subscribing now
    // means the room this connection joined is the room it will still be in. Dropped with
    // the closure if the upgrade never completes.
    let subscription = room.subscribe();

    let policy = state.collab.policy;
    let writer_id = principal.id.clone();
    Ok(ws
        // The transport ceiling. The handler refuses at the policy limit with a close
        // frame; this is what stops a frame that never reaches the handler from allocating
        // without bound.
        .max_message_size(policy.transport_limit())
        .max_frame_size(policy.transport_limit())
        .on_upgrade(move |socket| async move {
            run(socket, state, jar, path, room, subscription, writer_id).await;
        }))
}

/// A `doc` block with nothing in it.
///
/// The seed for a room that is about to be handed its stored CRDT state — and therefore
/// must be handed nothing else. See [`open_room`].
fn empty_document() -> Block {
    Block {
        kind: BlockKind::Doc,
        attrs: serde_json::Map::new(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

/// The room for `document`: the live one if there is one, otherwise one built from what the
/// database holds.
///
/// Three cases, and the middle one is the whole point of this function:
///
/// 1. **A live room.** It outranks everything stored — it holds edits the database has not
///    seen — and [`gw_collab::Rooms::join`] would ignore whatever was passed to it anyway.
/// 2. **Stored CRDT state.** The room is built EMPTY and given that state. Never
///    `from_block` as well: `from_block` creates content, so a room seeded from the page
///    body and then given its own stored state holds every word twice, under two client
///    ids, and a CRDT keeps both — for ever, because the duplicates are not a conflict to
///    resolve but two legitimate insertions. That is what
///    `seeding_twice_from_one_block_makes_two_documents_that_never_converge` demonstrates
///    in `gw-collab`, and `an_evicted_room_comes_back_from_its_stored_state` is what stops
///    it happening here.
/// 3. **Nothing stored.** A page nobody has ever edited, and seeding from the body is both
///    the only thing available and correct — exactly once, which is what
///    [`gw_collab::Rooms::join`] guarantees under its own lock.
///
/// The [`CollabState::building`] lock covers the decision AND the construction, because the
/// two must not be separated by a sweep: see the field's own note.
async fn open_room(
    state: &AppState,
    principal: &Principal,
    document: &StoredDocument,
) -> Result<Arc<Room>, ApiError> {
    let _building = state.collab.building.lock().await;

    let room = match state.collab.rooms.get(&document.id) {
        Some(live) => live,
        None => match state
            .store
            .crdt_state_for(principal, &document.id)
            .await
            .map_err(ApiError::Internal)?
        {
            Some(stored) => {
                let room = state
                    .collab
                    .rooms
                    .join(&document.id, &empty_document())
                    .await;
                if let Err(error) = room.doc().apply_update(&stored) {
                    // Refusing is the only safe answer. The room this connection would
                    // otherwise be handed is EMPTY, and an editor that opens on an empty
                    // page and then saves is how a document is destroyed rather than merely
                    // unavailable. It is closed first, because the line above created it
                    // and the next connection must not find it.
                    state.collab.rooms.close(&document.id);
                    tracing::error!(
                        %error,
                        document_id = %document.id,
                        "the stored CRDT state of this page could not be loaded"
                    );
                    return Err(ApiError::Internal(anyhow::Error::new(error)));
                }
                room
            }
            None => {
                let initial: Block = serde_json::from_str(&document.body)
                    .map_err(|error| ApiError::Internal(anyhow::Error::new(error)))?;
                state.collab.rooms.join(&document.id, &initial).await
            }
        },
    };

    // The baseline for "has anything changed since the database last saw this" is taken
    // from the room rather than from the bytes that built it, so both sides of that
    // comparison are produced the same way. It is only recorded if this is the first
    // joiner: a later one arrives at a room that may hold unsaved edits, and a baseline
    // taken then would mark those edits as already saved.
    state
        .collab
        .opened(&document.id, || room.doc().encode_state());
    Ok(room)
}

// -------------------------------------------------------------------------------------
// The session.
// -------------------------------------------------------------------------------------

/// A fixed one-second window of what a connection has spent.
///
/// Fixed rather than sliding on purpose: it is two integers and a timestamp, it cannot be
/// gamed into permitting more than twice the limit across any window, and a rate limiter
/// that needs its own data structure is a rate limiter with its own bugs.
struct Budget {
    window: Instant,
    messages: u32,
    bytes: usize,
}

impl Budget {
    fn new() -> Self {
        Self {
            window: Instant::now(),
            messages: 0,
            bytes: 0,
        }
    }

    fn admit(&mut self, len: usize, policy: &CollabPolicy) -> bool {
        if self.window.elapsed() >= Duration::from_secs(1) {
            *self = Self::new();
        }
        self.messages += 1;
        self.bytes += len;
        self.messages <= policy.max_messages_per_second && self.bytes <= policy.max_bytes_per_second
    }
}

type Sink = SplitSink<WebSocket, Message>;

/// End the session, saying why.
///
/// A close frame and not a silent drop, and for policy refusals a `permissionDenied`
/// message first: `y-websocket` understands that message and stops reconnecting, so a
/// client that may not edit is told once rather than reconnecting in a loop.
async fn close(sink: &mut Sink, code: u16, reason: &str, denied: bool) {
    if denied {
        let _ = sink
            .send(Message::Binary(wire::permission_denied(reason).into()))
            .await;
    }
    let _ = sink
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: Utf8Bytes::from(reason),
        })))
        .await;
    let _ = sink.close().await;
}

/// One connection, from the upgrade to the close.
async fn run(
    socket: WebSocket,
    state: AppState,
    jar: CookieJar,
    path: String,
    room: Arc<Room>,
    subscription: gw_collab::Subscription,
    mut writer_id: String,
) {
    let policy = state.collab.policy;
    let connection = subscription.id;
    let mut inbox = subscription.updates;
    let (mut sink, mut stream) = socket.split();

    // Step 1 on connect: this replica's state vector, so the client can send what we lack.
    // The client answers with step 2, and asks its own step 1, which is answered below.
    if sink
        .send(Message::Binary(
            wire::sync_step1(&room.doc().state_vector()).into(),
        ))
        .await
        .is_err()
    {
        return;
    }

    let mut budget = Budget::new();
    let mut checked = Instant::now();
    let mut ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + policy.reauth_interval,
        policy.reauth_interval,
    );

    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(message)) = incoming else { break };

                // Every frame is counted, including the ones this loop then ignores. A ping
                // is answered by the transport whether or not this handler looks at it, so
                // exempting pings from the budget would leave one message type that a client
                // may send without limit and be answered every time.
                let size = match &message {
                    Message::Binary(bytes) => bytes.len(),
                    Message::Text(text) => text.len(),
                    Message::Ping(bytes) | Message::Pong(bytes) => bytes.len(),
                    Message::Close(_) => 0,
                };
                if size > policy.max_message_bytes {
                    tracing::warn!(bytes = size, "refused: an oversized frame");
                    close(&mut sink, close_code::SIZE, "frame too large", false).await;
                    break;
                }
                if !budget.admit(size, &policy) {
                    tracing::warn!(%path, "refused: a connection exceeded its rate budget");
                    close(&mut sink, close_code::POLICY, "too many updates", false).await;
                    break;
                }

                let bytes = match message {
                    Message::Binary(bytes) => bytes,
                    // Text is not part of this protocol. Refused rather than ignored: a
                    // client sending it has misunderstood something, and finding that out
                    // now is cheaper than finding it out when nothing saves.
                    Message::Text(_) => {
                        close(&mut sink, close_code::UNSUPPORTED,
                              "this session speaks binary y-protocol frames only", false).await;
                        break;
                    }
                    Message::Close(_) => break,
                    // Ping and Pong are answered by the transport.
                    Message::Ping(_) | Message::Pong(_) => continue,
                };

                let Some(decoded) = wire::decode(&bytes) else {
                    // Malformed framing, as against a well-formed message this server has
                    // no handler for. The bytes came off a socket, so this is a close and
                    // never a panic.
                    close(&mut sink, close_code::UNSUPPORTED, "malformed frame", false).await;
                    break;
                };

                match decoded {
                    wire::Incoming::StateVector(vector) => {
                        let Ok(diff) = room.doc().encode_diff(vector) else {
                            close(&mut sink, close_code::UNSUPPORTED,
                                  "malformed state vector", false).await;
                            break;
                        };
                        if sink.send(Message::Binary(wire::sync_step2(&diff).into()))
                            .await.is_err() { break }
                    }
                    wire::Incoming::Update(update) => {
                        // The write path, and the only one. Re-authorised first when the
                        // interval has passed, so that the first thing a demoted client
                        // tries to write is what discovers the demotion.
                        //
                        // No test distinguishes this from the timer above, and that is
                        // stated rather than hidden: both are driven by one interval, so
                        // deleting this line leaves the whole suite green (verified). It is
                        // here because `select!` chooses at random between ready branches,
                        // so the timer alone bounds the staleness of an authorisation only
                        // in expectation — a client sending continuously could have a
                        // handful of updates applied after its permission lapsed. With this
                        // line the bound is exact for the case that matters, which is the
                        // one where content changes.
                        if checked.elapsed() >= policy.reauth_interval {
                            match reauthorise(&state, &jar, &path).await {
                                Some(id) => { writer_id = id; checked = Instant::now(); }
                                None => {
                                    close(&mut sink, close_code::POLICY,
                                          "you may no longer edit this page", true).await;
                                    break;
                                }
                            }
                        }
                        // Apply, THEN relay: a frame this replica rejected must never reach
                        // the other editors. `Room::apply_update` enforces that ordering for
                        // callers whose frames are bare updates; the frames here are encoded
                        // protocol messages, so that a receiving connection can write one to
                        // its socket without knowing what it is.
                        if room.doc().apply_update(update).is_err() {
                            tracing::warn!(%path, "refused: an update that could not be applied");
                            close(&mut sink, close_code::UNSUPPORTED,
                                  "this update could not be applied", false).await;
                            break;
                        }
                        room.broadcast(connection, &wire::sync_update(update));
                        state.collab.wrote(room.document_id(), &writer_id);
                    }
                    // Presence. Relayed verbatim and never applied to the document — it is
                    // not content, and the room does not interpret it either.
                    wire::Incoming::Relay => room.broadcast(connection, &bytes),
                    // A well-formed message of a type this server has no use for. Ignored
                    // rather than refused: it cannot become content, and closing on it
                    // would make any future addition to the client protocol an outage.
                    wire::Incoming::Ignored(kind) => {
                        tracing::debug!(kind, "ignoring an unhandled protocol message");
                    }
                }
            }

            frame = inbox.recv() => {
                match frame {
                    Ok(frame) if frame.is_from(connection) => {}
                    Ok(frame) => {
                        if sink.send(Message::Binary(frame.bytes.to_vec().into()))
                            .await.is_err() { break }
                    }
                    // This connection fell behind. Not data loss — the whole state is sent
                    // and Yjs applies it idempotently — but it must be repaired here,
                    // because the frames it missed are gone.
                    Err(RecvError::Lagged(missed)) => {
                        tracing::warn!(missed, %path, "a connection lagged; resending the state");
                        if sink.send(Message::Binary(
                                wire::sync_step2(&room.doc().encode_state()).into()))
                            .await.is_err() { break }
                    }
                    // The room was evicted from under us. Ending the session is right: the
                    // client reconnects and is given a room built from the database.
                    Err(RecvError::Closed) => break,
                }
            }

            _ = ticker.tick() => {
                // The other end of the re-check: a connection that sends nothing is closed
                // too. Without this, revoking somebody's access would leave them subscribed
                // to every keystroke of a page they may no longer read.
                match reauthorise(&state, &jar, &path).await {
                    Some(id) => { writer_id = id; checked = Instant::now(); }
                    None => {
                        close(&mut sink, close_code::POLICY,
                              "you may no longer edit this page", true).await;
                        break;
                    }
                }
                // And a heartbeat, because a room with a subscriber in it is never idle and
                // therefore never evicted. A browser that vanishes without closing its
                // socket — a laptop lid, a lost network — would otherwise hold a document in
                // memory for as long as the process lives. Every conforming peer answers a
                // ping automatically, and a peer that is gone makes this write fail once the
                // kernel gives up retransmitting, which ends the session and frees the room.
                if sink.send(Message::Ping(Default::default())).await.is_err() { break }
            }
        }
    }

    tracing::debug!(%path, %connection, "collaboration session ended");
}

/// May this session still write this page? The principal's id if so.
///
/// The whole answer is re-derived: the principal is resolved from the request's cookies
/// again — which re-reads the session and the account from the store (D-M2-7) — and
/// [`authorise`] is asked the same question it was asked at the handshake. A deactivated
/// account, a revoked grant, an ended session, a deleted page and an administrator who has
/// meanwhile entered view-as mode all come out of this as `None`.
async fn reauthorise(state: &AppState, jar: &CookieJar, path: &str) -> Option<String> {
    match authorise(state, jar, path).await {
        Ok((principal, _)) => Some(principal.id),
        Err(error) => {
            tracing::info!(%path, %error, "ending a session that is no longer authorised");
            None
        }
    }
}

// -------------------------------------------------------------------------------------
// Publishing.
// -------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
pub struct PublishRequest {
    /// Why this edit happened, in the author's words. Optional, exactly as the revision
    /// column is: demanding one produces "update" on every row.
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Published {
    pub revision_id: String,
}

/// Publish what is in the live session as a revision.
///
/// **The only thing in this file that writes a revision.** [`sweep`] used to as well, once
/// per interval, which is what filled a page's history with versions nobody asked for.
/// Publishing is a person saying "this is the version I mean"; a timer cannot say that.
///
/// Authorised twice, and neither is redundant: [`authorise`] decides whether this caller
/// may have the document at all (and refuses the view-as mode a POST would otherwise reach
/// only by way of the router layer), and [`gw_store::Store::publish_revision`] decides
/// again inside the write itself, where it also refuses an author who is not a signed-in,
/// active account. The second is the one that is load-bearing; the first is what turns
/// "not permitted" into a status code instead of a silent no-op.
async fn publish(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
    request: Option<Json<PublishRequest>>,
) -> Result<Json<Published>, ApiError> {
    let path = full_path(&captured);
    let (principal, document) = authorise(&state, &jar, &path).await?;

    // A publish takes what is in the room. With no room open there is nothing being edited,
    // and publishing the stored body back over itself would add a revision that says
    // nothing happened. 409 rather than 404: the page is there, the session is not.
    let Some(room) = state.collab.rooms.get(&document.id) else {
        return Err(ApiError::Conflict(
            "nothing is being edited on this page right now".into(),
        ));
    };

    let summary = request
        .and_then(|Json(body)| body.summary)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let snapshot = room.snapshot();
    let revision_id = state
        .store
        .publish_revision(&principal, &document.id, &snapshot, summary.as_deref())
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    // The room is NOT marked as saved. Publishing wrote a `Block` snapshot, and a `Block`
    // is a lossy view: the marks and any node kind `gw-core` cannot name are still only in
    // the CRDT. Telling the sweep there is nothing outstanding would mean a page that was
    // published and then closed came back without them.

    Ok(Json(Published { revision_id }))
}

// -------------------------------------------------------------------------------------
// Persistence.
// -------------------------------------------------------------------------------------

/// One pass: write out every room that has changed, then drop the ones nobody is in.
///
/// Public and `async` rather than hidden inside the janitor's loop, because a test that has
/// to sleep for a production interval to observe eviction is a test nobody runs. Eviction
/// happens FIRST so that a room that is both idle and unsaved is saved from the copy that
/// was handed back — after `evict_idle` returns it, `Rooms::get` no longer knows about it,
/// and the only reference to what was typed is the one in that vector.
///
/// That ordering is why the whole pass is under [`CollabState::building`]: between the
/// eviction and the write there is a room that exists nowhere a joining connection can find
/// it and whose state is not yet in the database, and a connection arriving in that window
/// would build a second copy of the page. The lock makes the window unreachable rather than
/// merely narrow.
pub async fn sweep(state: &AppState) {
    let _building = state.collab.building.lock().await;

    let evicted = state
        .collab
        .rooms
        .evict_idle(state.collab.policy.idle_grace);

    for (document_id, writer_id) in state.collab.unsaved() {
        let room = evicted
            .iter()
            .find(|room| room.document_id() == document_id)
            .cloned()
            .or_else(|| state.collab.rooms.get(&document_id));
        match room {
            Some(room) => persist(state, &room, &writer_id).await,
            // The room went away without being evicted here. Nothing can be written for it
            // and nothing is holding its content; say so rather than dropping it quietly.
            None => tracing::error!(
                document_id,
                "a room with unsaved edits vanished before it could be written"
            ),
        }
    }

    for room in &evicted {
        state.collab.closed(room.document_id());
    }
}

/// Write one room's CRDT state, attributed to whoever last typed in it.
///
/// The state and not a snapshot: this is the document, where [`gw_collab::Room::snapshot`]
/// is a view of it that cannot hold an inline mark. Nothing here touches `documents.body`
/// or the history — [`publish`] is what moves those, when somebody asks.
///
/// **It is still authorised, and that is a decision rather than an oversight.** A room whose
/// last writer has since been deactivated or had their grant revoked is NOT written.
/// `gw-store` has exactly one answer to "who may change this page's content", and a
/// persistence path with a weaker answer of its own would mean the question had two — the
/// second being the one nobody updates when the rules change. The counter-argument is real
/// and is recorded here rather than argued away: those keystrokes are already in the room,
/// already broadcast to everyone else editing it, and the room may hold other people's work
/// as well, since `writer` is merely whoever typed last. Refusing therefore costs more than
/// the one person's edits. Making that precise needs `gw-collab` to say who is *in* a room,
/// so that a still-authorised editor could stand behind the write; it exposes no such thing
/// today. Until then this is loud rather than silent: it is the one path here that loses
/// work, so it is logged at error level with the document named.
async fn persist(state: &AppState, room: &Room, writer_id: &str) {
    let document_id = room.document_id();
    let encoded = room.doc().encode_state();

    if !state.collab.differs(document_id, &encoded) {
        // Nothing changed — a client's empty first sync reply is an update like any other,
        // so without this an open editor would rewrite the same row once per sweep.
        state.collab.saved(document_id, encoded);
        return;
    }

    let author = match state.store.principal_by_id(writer_id).await {
        Ok(Some((principal, _))) => principal,
        Ok(None) => {
            tracing::error!(document_id, writer_id, "cannot save: the author is gone");
            return;
        }
        Err(error) => {
            tracing::error!(%error, document_id, "cannot save: the author could not be read");
            return;
        }
    };

    match state
        .store
        .save_crdt_state(&author, document_id, &encoded)
        .await
    {
        Ok(true) => {
            let bytes = encoded.len();
            state.collab.saved(document_id, encoded);
            tracing::debug!(
                document_id,
                bytes,
                "stored a collaboration room's CRDT state"
            );
        }
        Ok(false) => tracing::error!(
            document_id,
            author = %author.username,
            "cannot save: the last editor may no longer write this page"
        ),
        Err(error) => tracing::error!(%error, document_id, "cannot save a collaboration room"),
    }
}

/// Run [`sweep`] for as long as the process lives.
///
/// Started from [`AppState::serving`], which is the only constructor a running server has —
/// so a test gets rooms with no janitor behind them and drives [`sweep`] itself, and a
/// server always has one. Nothing else needs to remember to start it.
pub fn spawn_janitor(state: AppState) {
    let interval = state.collab.policy.snapshot_interval;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        // A sweep that runs long must not then run twice in quick succession trying to
        // catch up; it has nothing to catch up on.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;
            sweep(&state).await;
        }
    });
}

// -------------------------------------------------------------------------------------
// The wire format.
// -------------------------------------------------------------------------------------

/// The `y-protocol` framing, which is what `y-websocket` in the browser speaks.
///
/// It is implemented here rather than taken from a crate because it is forty lines and the
/// alternative is a dependency on a protocol helper that would have to be kept in step with
/// `yrs` anyway. Every reader is total: the bytes arrive from a socket, so a truncated
/// varint or a length that runs off the end of the buffer is `None` and never a panic.
mod wire {
    /// Message types, as `y-protocols` numbers them.
    const SYNC: u64 = 0;
    const AWARENESS: u64 = 1;
    const QUERY_AWARENESS: u64 = 3;
    /// Sub-types of a sync message.
    const STEP1: u64 = 0;
    const STEP2: u64 = 1;
    const UPDATE: u64 = 2;
    /// The one `messageAuth` sub-type a server sends.
    const AUTH: u64 = 2;
    const PERMISSION_DENIED: u64 = 0;

    /// What a client sent, once it has been recognised.
    pub enum Incoming<'a> {
        /// The peer's state vector: it wants everything this replica has that it lacks.
        StateVector(&'a [u8]),
        /// Content. The only variant that requires `Action::Write`.
        Update(&'a [u8]),
        /// Presence, forwarded to the room untouched and never applied to the document.
        Relay,
        /// Well formed, and nothing this server acts on.
        Ignored(u64),
    }

    /// lib0's unsigned varint: seven bits a byte, little-endian, high bit continues.
    fn write_varuint(out: &mut Vec<u8>, mut value: u64) {
        while value > 0x7f {
            out.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        out.push(value as u8);
    }

    /// The varint at `at`, or `None` for a truncated or absurdly long one. The length cap
    /// matters: without it a run of continuation bytes is an unbounded loop over an
    /// attacker-supplied buffer.
    fn read_varuint(bytes: &[u8], at: &mut usize) -> Option<u64> {
        let mut value: u64 = 0;
        let mut shift = 0;
        loop {
            let byte = *bytes.get(*at)?;
            *at += 1;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Some(value);
            }
            shift += 7;
            if shift > 63 {
                return None;
            }
        }
    }

    /// A length-prefixed slice, borrowed rather than copied.
    fn read_slice<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
        let len = usize::try_from(read_varuint(bytes, at)?).ok()?;
        let end = at.checked_add(len)?;
        let slice = bytes.get(*at..end)?;
        *at = end;
        Some(slice)
    }

    fn message(parts: &[u64], payload: Option<&[u8]>) -> Vec<u8> {
        let mut out = Vec::with_capacity(payload.map_or(4, |p| p.len() + 8));
        for part in parts {
            write_varuint(&mut out, *part);
        }
        if let Some(payload) = payload {
            write_varuint(&mut out, payload.len() as u64);
            out.extend_from_slice(payload);
        }
        out
    }

    pub fn sync_step1(state_vector: &[u8]) -> Vec<u8> {
        message(&[SYNC, STEP1], Some(state_vector))
    }

    pub fn sync_step2(update: &[u8]) -> Vec<u8> {
        message(&[SYNC, STEP2], Some(update))
    }

    pub fn sync_update(update: &[u8]) -> Vec<u8> {
        message(&[SYNC, UPDATE], Some(update))
    }

    /// `messageAuth` / `permissionDenied`, which `y-websocket` handles by disconnecting and
    /// NOT reconnecting — so a refused client is told once instead of looping.
    pub fn permission_denied(reason: &str) -> Vec<u8> {
        let mut out = message(&[AUTH, PERMISSION_DENIED], None);
        write_varuint(&mut out, reason.len() as u64);
        out.extend_from_slice(reason.as_bytes());
        out
    }

    pub fn decode(bytes: &[u8]) -> Option<Incoming<'_>> {
        let mut at = 0;
        let kind = read_varuint(bytes, &mut at)?;
        match kind {
            SYNC => match read_varuint(bytes, &mut at)? {
                STEP1 => Some(Incoming::StateVector(read_slice(bytes, &mut at)?)),
                // Step 2 answers this replica's own step 1, and an update is unsolicited.
                // Both carry content and both are authorised the same way.
                STEP2 | UPDATE => Some(Incoming::Update(read_slice(bytes, &mut at)?)),
                other => Some(Incoming::Ignored(other)),
            },
            // Read for length only: a truncated presence frame is refused like any other
            // malformed frame rather than being relayed to every editor in the room.
            AWARENESS => {
                read_slice(bytes, &mut at)?;
                Some(Incoming::Relay)
            }
            // "Who else is here?" — relayed so the peers answer it. This server keeps no
            // awareness state of its own to answer from.
            QUERY_AWARENESS => Some(Incoming::Relay),
            other => Some(Incoming::Ignored(other)),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_varint_round_trips_at_every_width() {
            for value in [0u64, 1, 127, 128, 255, 300, 16_383, 16_384, u64::MAX] {
                let mut bytes = Vec::new();
                write_varuint(&mut bytes, value);
                let mut at = 0;
                assert_eq!(read_varuint(&bytes, &mut at), Some(value), "{value}");
                assert_eq!(at, bytes.len(), "{value} left bytes unread");
            }
        }

        #[test]
        fn a_truncated_varint_is_none_rather_than_a_panic() {
            assert_eq!(read_varuint(&[], &mut 0), None);
            assert_eq!(read_varuint(&[0x80], &mut 0), None);
            // A run of continuation bytes must terminate rather than shift for ever.
            assert_eq!(read_varuint(&[0x80; 32], &mut 0), None);
        }

        #[test]
        fn a_length_that_runs_off_the_end_is_none() {
            // The shape of a deliberately hostile frame: "here are 4096 bytes", and then
            // three. Indexing on that length is a panic, which is a dropped connection at
            // best and a poisoned lock at worst.
            let mut bytes = Vec::new();
            write_varuint(&mut bytes, 4096);
            bytes.extend_from_slice(&[1, 2, 3]);
            assert!(read_slice(&bytes, &mut 0).is_none());
            assert!(decode(&[SYNC as u8, STEP1 as u8, 200, 1, 2]).is_none());
        }

        #[test]
        fn the_messages_this_server_sends_decode_as_what_they_are() {
            assert!(matches!(
                decode(&sync_step1(b"vector")),
                Some(Incoming::StateVector(b"vector"))
            ));
            assert!(matches!(
                decode(&sync_step2(b"update")),
                Some(Incoming::Update(b"update"))
            ));
            assert!(matches!(
                decode(&sync_update(b"update")),
                Some(Incoming::Update(b"update"))
            ));
        }

        #[test]
        fn a_payload_longer_than_a_single_varint_keeps_its_length() {
            // 127 is where the length prefix stops being one byte. A framing that assumed
            // one would truncate every real document.
            for len in [126usize, 127, 128, 300, 20_000] {
                let payload = vec![7u8; len];
                match decode(&sync_update(&payload)) {
                    Some(Incoming::Update(back)) => assert_eq!(back.len(), len),
                    other => panic!("{len} bytes did not survive: {}", other.is_none()),
                }
            }
        }

        #[test]
        fn presence_is_relayed_and_never_read_as_content() {
            assert!(matches!(
                decode(&message(&[AWARENESS], Some(b"wer ist da"))),
                Some(Incoming::Relay)
            ));
            assert!(matches!(
                decode(&message(&[QUERY_AWARENESS], None)),
                Some(Incoming::Relay)
            ));
            // A truncated presence frame is malformed, not relayable.
            assert!(decode(&[AWARENESS as u8, 200, 1]).is_none());
        }

        #[test]
        fn an_unknown_message_type_is_ignored_rather_than_misread() {
            assert!(matches!(decode(&[99]), Some(Incoming::Ignored(99))));
            assert!(matches!(
                decode(&[SYNC as u8, 9]),
                Some(Incoming::Ignored(9))
            ));
            assert!(decode(&[]).is_none());
        }

        #[test]
        fn permission_denied_carries_its_reason() {
            let bytes = permission_denied("nicht erlaubt");
            assert_eq!(bytes[0], AUTH as u8);
            assert_eq!(bytes[1], PERMISSION_DENIED as u8);
            assert_eq!(bytes[2] as usize, "nicht erlaubt".len());
            assert!(bytes.ends_with("nicht erlaubt".as_bytes()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{full_path, Budget, CollabPolicy};
    use std::time::Duration;

    #[test]
    fn a_captured_path_is_given_back_the_leading_slash_exactly_once() {
        assert_eq!(full_path("handbuch"), "/handbuch");
        assert_eq!(full_path("/handbuch"), "/handbuch");
        assert_eq!(full_path("handbuch/onboarding"), "/handbuch/onboarding");
        assert_eq!(full_path(""), "/");
    }

    #[test]
    fn the_budget_refuses_the_message_that_crosses_the_limit_and_not_the_one_before() {
        let policy = CollabPolicy {
            max_messages_per_second: 3,
            ..CollabPolicy::default()
        };
        let mut budget = Budget::new();
        assert!(budget.admit(1, &policy));
        assert!(budget.admit(1, &policy));
        assert!(budget.admit(1, &policy));
        assert!(!budget.admit(1, &policy), "the fourth message was admitted");
    }

    #[test]
    fn the_budget_counts_bytes_as_well_as_messages() {
        // A limit on messages alone would let one connection push a megabyte a frame; a
        // limit on bytes alone would let it push a million empty ones.
        let policy = CollabPolicy {
            max_bytes_per_second: 100,
            ..CollabPolicy::default()
        };
        let mut budget = Budget::new();
        assert!(budget.admit(60, &policy));
        assert!(!budget.admit(60, &policy));
    }

    #[test]
    fn the_budget_starts_again_in_the_next_window() {
        let policy = CollabPolicy {
            max_messages_per_second: 1,
            ..CollabPolicy::default()
        };
        let mut budget = Budget::new();
        assert!(budget.admit(1, &policy));
        assert!(!budget.admit(1, &policy));
        budget.window -= Duration::from_secs(2);
        assert!(budget.admit(1, &policy), "the window never reopened");
    }

    #[test]
    fn the_transport_ceiling_is_above_the_policy_limit() {
        // If they were equal the transport would abort the connection before the handler
        // could refuse the frame with a code and a reason, and the client would see a
        // socket that simply died.
        let policy = CollabPolicy::default();
        assert!(policy.transport_limit() > policy.max_message_bytes);
    }
}
