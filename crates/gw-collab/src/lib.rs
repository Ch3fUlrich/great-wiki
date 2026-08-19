//! The collaborative editing core: one CRDT document per page, and the rooms that hold
//! them in memory while people are editing.
//!
//! This crate is deliberately free of HTTP and of the database. It is the only place in
//! the server that holds mutable state shared across connections, so its concurrency has
//! to be reasoned about on its own — with a socket handler in view it could not be. The
//! WebSocket endpoint (M3 task 3) owns the wire; the store owns persistence; what is here
//! is the document and the fan-out.
//!
//! # What the CRDT can and cannot carry
//!
//! [`CollabDoc`] is a [`yrs`] document — the Rust implementation of Yjs — holding the page
//! body in an XML fragment named [`CONTENT_FIELD`], shaped the way `y-prosemirror` shapes
//! one. `Block` and that fragment are not the same expressive power, and the differences
//! are stated here rather than discovered later:
//!
//! - **The fragment is the `doc` node.** Its children are the document's children, so a
//!   `doc` block's own `attrs` and `text` have nowhere to live and are dropped. Every
//!   other block's attrs survive with their JSON types intact.
//! - **Inline marks round-trip.** A browser bolding a word or making it a link writes Yjs
//!   formatting on the text; [`CollabDoc::from_block`] writes a leaf's `Block::marks` back as
//!   that same formatting, and [`CollabDoc::to_block`] reads it back as `Mark`s rather than
//!   dropping it or leaking it into the text as literal markup. A formatted run always comes
//!   back as its own leaf — `Block` has no way to mark only part of one — and a leaf's marks
//!   are always sorted into `gw_core::MARK_ORDER`, because Yjs holds them in an unordered
//!   attribute map and the markdown exporter trusts the array itself to already be ordered.
//! - **A node kind outside `BlockKind` cannot be named.** If the editor gains an image or a
//!   horizontal rule before `gw-core` gains the kind, the element lives in the CRDT but
//!   `to_block` cannot express it: its children are lifted into the parent so no text is
//!   lost, and the element itself disappears from the snapshot.
//!
//! The consequence is one sentence long and worth stating plainly: **the CRDT is the
//! document; a `Block` snapshot is a lossy view of it.** A snapshot is safe to index,
//! search and diff. It is not safe to write back over the CRDT.

pub mod doc;
pub mod room;

#[cfg(test)]
mod fixtures;

pub use doc::{CollabDoc, CONTENT_FIELD};
pub use room::{ConnectionId, Frame, Room, Rooms, Subscription};

/// What can go wrong in this crate.
///
/// Every variant is reachable from bytes a client sent, which is why they are errors and
/// not panics: a corrupt frame must cost one WebSocket connection, not the server.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CollabError {
    /// The bytes were not a Yjs v1 update.
    #[error("malformed CRDT update: {0}")]
    MalformedUpdate(yrs::encoding::read::Error),
    /// The bytes were not a Yjs v1 state vector.
    #[error("malformed state vector: {0}")]
    MalformedStateVector(yrs::encoding::read::Error),
    /// The update decoded but could not be integrated into the document.
    #[error("CRDT update could not be integrated: {0}")]
    Integration(#[from] yrs::error::UpdateError),
}

/// The crate's result type.
pub type Result<T> = std::result::Result<T, CollabError>;
