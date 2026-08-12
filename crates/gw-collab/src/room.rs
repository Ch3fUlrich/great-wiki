//! Per-document rooms: the live CRDT, and the fan-out to everyone editing it.
//!
//! This is the mutable state the rest of the server shares, so the rules it obeys are
//! short enough to hold in one's head:
//!
//! - **One room per document, ever.** [`Rooms::join`] resolves the id and creates the room
//!   under the same lock, so two connections arriving at once cannot end up editing two
//!   different copies of one page.
//! - **The live room outranks the stored body.** A second joiner brings whatever the
//!   database last saw; seeding again would roll back everything typed since.
//! - **Apply, then relay.** A frame that this replica rejected is never forwarded, so a
//!   corrupt update costs one connection rather than every editor in the room.
//! - **Nothing here writes to the database.** This crate has no store dependency by
//!   design, so [`Rooms::evict_idle`] hands the rooms back and lets the caller persist
//!   them.

use crate::{CollabDoc, Result};
use gw_core::Block;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;

/// How many frames a slow connection may fall behind before it is told it lagged.
///
/// Lagging is not data loss here: a client that misses frames re-syncs from its state
/// vector, which is the same handshake it does on connect. Buffering without limit to
/// avoid that would let one stalled browser hold a document's updates in memory forever.
pub const UPDATE_BUFFER: usize = 256;

/// Identifies one connection within a room, so a frame can be attributed and the sender
/// can skip its own echo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(u64);

impl ConnectionId {
    /// Only for tests and for callers that need a placeholder id for a server-originated
    /// frame — a real one comes from [`Room::subscribe`].
    pub const SERVER: ConnectionId = ConnectionId(0);
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Bytes to forward to everyone in the room, and who sent them.
///
/// The payload is opaque on purpose. A CRDT update is applied before it gets here; an
/// awareness or presence frame is never applied at all. Neither needs this type to know
/// which it is, and the room stays out of the wire protocol's business.
#[derive(Clone, Debug)]
pub struct Frame {
    pub from: ConnectionId,
    pub bytes: Arc<[u8]>,
}

impl Frame {
    /// Whether this frame came from `id` — the check a connection does before writing a
    /// frame back to the socket it arrived on.
    pub fn is_from(&self, id: ConnectionId) -> bool {
        self.from == id
    }
}

/// A connection's place in a room: its id, and the frames meant for it.
pub struct Subscription {
    pub id: ConnectionId,
    pub updates: broadcast::Receiver<Frame>,
}

/// One document, live.
pub struct Room {
    document_id: String,
    doc: CollabDoc,
    updates: broadcast::Sender<Frame>,
    next_connection: AtomicU64,
    /// When this room last did anything. Only consulted once it has no subscribers left,
    /// which is why a plain mutex is enough: it is never on a hot path.
    last_active: Mutex<Instant>,
}

impl Room {
    fn new(document_id: &str, initial: &Block) -> Self {
        let (updates, _) = broadcast::channel(UPDATE_BUFFER);
        Self {
            document_id: document_id.to_string(),
            doc: CollabDoc::from_block(initial),
            updates,
            // 0 is `ConnectionId::SERVER`, so real connections start at 1.
            next_connection: AtomicU64::new(1),
            last_active: Mutex::new(Instant::now()),
        }
    }

    pub fn document_id(&self) -> &str {
        &self.document_id
    }

    /// The CRDT itself, for the sync handshake: state vectors, diffs and full state.
    pub fn doc(&self) -> &CollabDoc {
        &self.doc
    }

    /// Take a place in the room, with a fresh id and a receiver for everyone else's work.
    pub fn subscribe(&self) -> Subscription {
        self.touch();
        Subscription {
            id: ConnectionId(self.next_connection.fetch_add(1, Ordering::Relaxed)),
            updates: self.updates.subscribe(),
        }
    }

    /// How many connections are currently listening.
    pub fn subscriber_count(&self) -> usize {
        self.updates.receiver_count()
    }

    /// Integrate an update and forward it to the rest of the room.
    ///
    /// The order is load-bearing: a malformed update returns before anything is sent, so
    /// what reaches the other editors is only ever what this replica accepted.
    pub fn apply_update(&self, from: ConnectionId, update: &[u8]) -> Result<()> {
        self.doc.apply_update(update)?;
        self.broadcast(from, update);
        Ok(())
    }

    /// Forward bytes without interpreting them — awareness and presence.
    pub fn broadcast(&self, from: ConnectionId, bytes: &[u8]) {
        self.touch();
        // An error here means nobody is listening, which is normal for the last
        // connection's own final update.
        let _ = self.updates.send(Frame {
            from,
            bytes: Arc::from(bytes),
        });
    }

    /// The document as a block tree — for publishing a revision, indexing or diffing.
    ///
    /// A lossy view of the CRDT, not a replacement for it; see the crate documentation.
    pub fn snapshot(&self) -> Block {
        self.doc.to_block()
    }

    fn touch(&self) {
        *self
            .last_active
            .lock()
            .expect("a room's clock is never poisoned") = Instant::now();
    }

    /// How long this room has had nothing to do, or `None` while anyone is still in it.
    fn idle_for(&self) -> Option<Duration> {
        if self.subscriber_count() > 0 {
            return None;
        }
        Some(
            self.last_active
                .lock()
                .expect("a room's clock is never poisoned")
                .elapsed(),
        )
    }
}

impl std::fmt::Debug for Room {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Room")
            .field("document_id", &self.document_id)
            .field("subscribers", &self.subscriber_count())
            .finish_non_exhaustive()
    }
}

/// Every live room, keyed by document id.
#[derive(Default)]
pub struct Rooms {
    rooms: Mutex<HashMap<String, Arc<Room>>>,
}

impl Rooms {
    pub fn new() -> Self {
        Self::default()
    }

    /// The room for a document, creating it from `initial` if there is not one already.
    ///
    /// `initial` is only used when the room does not exist. Everything happens under one
    /// lock: a lookup that released the lock before inserting would let two connections
    /// arriving together each create a room, and every edit made in the loser would be
    /// invisible to the other editor and then lost.
    ///
    /// Building a room means converting a block tree, which is not free, and it happens
    /// with the registry locked. That is the deliberate trade — the alternative is a
    /// per-document lock table, which is more machinery guarding a path that is taken once
    /// per document per server lifetime.
    pub async fn join(&self, document_id: &str, initial: &Block) -> Arc<Room> {
        let mut rooms = self
            .rooms
            .lock()
            .expect("the room registry is never poisoned");
        let room = rooms
            .entry(document_id.to_string())
            .or_insert_with(|| {
                tracing::debug!(document_id, "opening a collaboration room");
                Arc::new(Room::new(document_id, initial))
            })
            .clone();
        room.touch();
        room
    }

    /// The room for a document if one is open, without opening one.
    pub fn get(&self, document_id: &str) -> Option<Arc<Room>> {
        self.rooms
            .lock()
            .expect("the room registry is never poisoned")
            .get(document_id)
            .cloned()
    }

    pub fn len(&self) -> usize {
        self.rooms
            .lock()
            .expect("the room registry is never poisoned")
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Forget a room, handing it back so its state can be persisted first.
    pub fn close(&self, document_id: &str) -> Option<Arc<Room>> {
        self.rooms
            .lock()
            .expect("the room registry is never poisoned")
            .remove(document_id)
    }

    /// Remove every room that has had no subscribers for at least `grace`.
    ///
    /// The returned rooms are the caller's to persist. Nothing else holds them, so
    /// dropping the returned vector without saving loses whatever was typed since the last
    /// write — which is exactly why they are returned rather than quietly freed here.
    pub fn evict_idle(&self, grace: Duration) -> Vec<Arc<Room>> {
        let mut rooms = self
            .rooms
            .lock()
            .expect("the room registry is never poisoned");
        let mut evicted = Vec::new();
        rooms.retain(|document_id, room| match room.idle_for() {
            Some(idle) if idle >= grace => {
                tracing::debug!(document_id, ?idle, "evicting an idle collaboration room");
                evicted.push(room.clone());
                false
            }
            _ => true,
        });
        evicted
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionId, Rooms};
    use crate::fixtures::{self, json_of};
    use crate::CollabDoc;
    use std::sync::Arc;
    use std::time::Duration;

    const CONN: ConnectionId = ConnectionId::SERVER;

    fn an_update(text: &str) -> Vec<u8> {
        let doc = CollabDoc::new();
        doc.append_paragraph(text);
        doc.encode_state()
    }

    #[tokio::test]
    async fn joining_the_same_document_twice_returns_the_same_room() {
        let rooms = Rooms::new();
        let a = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let b = rooms.join("doc-1", &fixtures::simple_doc()).await;
        assert!(
            Arc::ptr_eq(&a, &b),
            "two rooms for one document means two editors diverge"
        );
        assert_eq!(rooms.len(), 1);
    }

    #[tokio::test]
    async fn two_documents_get_two_rooms() {
        let rooms = Rooms::new();
        let a = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let b = rooms.join("doc-2", &fixtures::simple_doc()).await;
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(rooms.len(), 2);
        assert_eq!(a.document_id(), "doc-1");
        assert!(rooms.get("doc-2").is_some());
        assert!(rooms.get("doc-3").is_none());
    }

    #[tokio::test]
    async fn joining_a_live_room_does_not_reset_it_to_the_stored_body() {
        // The second joiner brings whatever the database last saw. The live room has
        // unsaved edits in it; seeding again would silently roll them back.
        let rooms = Rooms::new();
        let first = rooms.join("doc-1", &fixtures::simple_doc()).await;
        first.doc().append_paragraph("noch nicht gespeichert");

        let second = rooms.join("doc-1", &fixtures::simple_doc()).await;
        assert!(second
            .snapshot()
            .plain_text()
            .contains("noch nicht gespeichert"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn joining_from_many_threads_at_once_creates_exactly_one_room() {
        // Not the happy path: thirty-two tasks race on the same document id, released
        // together by a barrier. If `join` ever checks and then inserts without holding
        // the lock across both, some of these get a room of their own — and every edit
        // made in it is invisible to everyone else.
        const TASKS: usize = 32;
        let rooms = Arc::new(Rooms::new());
        let barrier = Arc::new(tokio::sync::Barrier::new(TASKS));

        let mut handles = Vec::new();
        for i in 0..TASKS {
            let rooms = rooms.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                // A big enough initial tree that constructing a room is not instant,
                // which is what makes the window between a check and an insert reachable.
                let room = rooms.join("hot", &fixtures::big_table(27, 8)).await;
                room.doc().append_paragraph(&format!("von {i}"));
                room
            }));
        }

        let mut joined = Vec::new();
        for handle in handles {
            joined.push(handle.await.expect("task panicked"));
        }

        let first = joined[0].clone();
        for room in &joined {
            assert!(
                Arc::ptr_eq(&first, room),
                "a second room was created for one document"
            );
        }
        assert_eq!(rooms.len(), 1);

        // The behavioural half: pointer equality alone would still pass if the losers'
        // edits had gone into rooms that were then thrown away.
        let text = first.snapshot().plain_text();
        for i in 0..TASKS {
            assert!(text.contains(&format!("von {i}")), "lost the edit from {i}");
        }
    }

    #[tokio::test]
    async fn an_update_reaches_every_subscriber_and_names_the_sender() {
        let rooms = Rooms::new();
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let author = room.subscribe();
        let mut watcher = room.subscribe();
        assert_ne!(author.id, watcher.id, "connections must be distinguishable");
        assert_eq!(room.subscriber_count(), 2);

        room.apply_update(author.id, &an_update("von A")).unwrap();

        let frame = watcher.updates.try_recv().expect("no frame was broadcast");
        assert_eq!(frame.from, author.id, "the sender must be identifiable");
        assert!(frame.is_from(author.id));
        assert!(!frame.is_from(watcher.id));
        assert!(room.snapshot().plain_text().contains("von A"));
    }

    #[tokio::test]
    async fn a_malformed_update_is_rejected_and_never_relayed() {
        // Relaying first and applying afterwards would push a corrupt frame to every other
        // editor in the room before discovering it was corrupt.
        let rooms = Rooms::new();
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let mut watcher = room.subscribe();

        assert!(room.apply_update(CONN, &[0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(
            watcher.updates.try_recv().is_err(),
            "a rejected update was relayed to the other editors"
        );
    }

    #[tokio::test]
    async fn a_relayed_frame_is_not_applied_to_the_document() {
        // Awareness and presence frames pass through the room without being interpreted.
        let rooms = Rooms::new();
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let mut watcher = room.subscribe();
        let before = json_of(&room.snapshot());

        room.broadcast(CONN, b"nicht mein Problem");

        assert_eq!(
            watcher.updates.try_recv().unwrap().bytes.as_ref(),
            b"nicht mein Problem"
        );
        assert_eq!(json_of(&room.snapshot()), before);
    }

    #[tokio::test]
    async fn a_room_with_a_subscriber_is_never_evicted() {
        let rooms = Rooms::new();
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let _subscriber = room.subscribe();

        assert!(rooms.evict_idle(Duration::ZERO).is_empty());
        assert_eq!(rooms.len(), 1);
    }

    #[tokio::test]
    async fn a_room_with_no_subscribers_is_evicted_once_the_grace_period_has_passed() {
        let rooms = Rooms::new();
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let subscriber = room.subscribe();
        drop(subscriber);

        assert!(
            rooms.evict_idle(Duration::from_secs(3600)).is_empty(),
            "evicted before its grace period was up"
        );
        assert_eq!(rooms.len(), 1);

        let evicted = rooms.evict_idle(Duration::ZERO);
        assert_eq!(evicted.len(), 1);
        assert_eq!(evicted[0].document_id(), "doc-1");
        assert!(rooms.is_empty());
    }

    #[tokio::test]
    async fn an_evicted_room_is_handed_back_so_its_state_can_be_persisted() {
        // This crate cannot write to the database — that is the caller's job — so eviction
        // returns the rooms rather than dropping them on the floor.
        let rooms = Rooms::new();
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        room.doc().append_paragraph("noch nicht gespeichert");
        drop(room);

        let evicted = rooms.evict_idle(Duration::ZERO);
        assert_eq!(evicted.len(), 1);
        assert!(evicted[0]
            .snapshot()
            .plain_text()
            .contains("noch nicht gespeichert"));

        // And a later join starts from what the caller passes in, not from the ghost.
        let fresh = rooms.join("doc-1", &fixtures::simple_doc()).await;
        assert!(!fresh
            .snapshot()
            .plain_text()
            .contains("noch nicht gespeichert"));
    }

    #[tokio::test]
    async fn closing_a_room_hands_it_back_too() {
        let rooms = Rooms::new();
        rooms.join("doc-1", &fixtures::simple_doc()).await;
        assert_eq!(
            rooms.close("doc-1").map(|r| r.document_id().to_string()),
            Some("doc-1".into())
        );
        assert!(rooms.is_empty());
        assert!(rooms.close("doc-1").is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn updates_applied_concurrently_in_one_room_all_land() {
        const TASKS: usize = 16;
        let rooms = Arc::new(Rooms::new());
        let room = rooms.join("doc-1", &fixtures::simple_doc()).await;
        let barrier = Arc::new(tokio::sync::Barrier::new(TASKS));

        let mut handles = Vec::new();
        for i in 0..TASKS {
            let room = room.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                let update = an_update(&format!("von {i}"));
                barrier.wait().await;
                room.apply_update(ConnectionId(i as u64), &update).unwrap();
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }

        let text = room.snapshot().plain_text();
        for i in 0..TASKS {
            assert!(
                text.contains(&format!("von {i}")),
                "lost the update from {i}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn two_rooms_fed_the_same_updates_in_opposite_orders_converge() {
        // Both rooms open empty and are handed the same seven updates — the page's body
        // among them — in opposite orders. Seeding the two from one `Block` instead would
        // give each its own copy of the body under its own client id, and they would then
        // legitimately differ; see `seeding_twice_from_one_block_makes_two_documents`.
        let rooms = Rooms::new();
        let left = rooms.join("left", &fixtures::empty_doc()).await;
        let right = rooms.join("right", &fixtures::empty_doc()).await;

        let mut updates = vec![CollabDoc::from_block(&fixtures::simple_doc()).encode_state()];
        updates.extend((0..6).map(|i| an_update(&format!("von {i}"))));

        for update in &updates {
            left.apply_update(CONN, update).unwrap();
        }
        for update in updates.iter().rev() {
            right.apply_update(CONN, update).unwrap();
        }

        assert_eq!(
            json_of(&left.snapshot()),
            json_of(&right.snapshot()),
            "the order updates arrived in changed the document"
        );
        let text = left.snapshot().plain_text();
        assert!(text.contains("Größe und Maß"), "lost the body: {text}");
        for i in 0..6 {
            assert!(
                text.contains(&format!("von {i}")),
                "lost update {i}: {text}"
            );
        }
    }
}
