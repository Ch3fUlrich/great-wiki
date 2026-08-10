# 0006 — Yjs for collaborative editing, with the server as a relay

**Status:** Accepted (2026-08-10)

## Context

M3 requires two browsers editing one page simultaneously with both changes surviving. That
needs a CRDT, and the choice is hard to reverse: it determines the wire format, the stored
representation of every document body, and what the server is capable of doing with a
document at all. Picking it during M3 would mean picking it under pressure, so it is settled
now.

Three candidates were considered against one another rather than in isolation.

**The block model is already ProseMirror-shaped.** `gw_core::Block` carries kind, attrs,
content and text because M1 chose editor fidelity over a simpler tree. That decision has a
consequence here: whatever CRDT is chosen has to bind to ProseMirror, and the quality of
that binding matters more than the elegance of the CRDT underneath it.

## Decision

**Yjs**, with `y-prosemirror` as the binding. The authoritative document lives in
JavaScript; the Rust server persists and relays updates.

## Why not the Rust-native options

This is the uncomfortable part of the decision and it should be stated plainly rather than
buried.

**Loro** and **Automerge** are both Rust-native, which would let `gw-api` read and write
document content directly. That is genuinely valuable to this project specifically: import,
export, full-text indexing, the knowledge graph and the AI assistant all need the document
server-side, and every one of them is easier when the server understands the document rather
than storing an opaque blob.

They were not chosen because the ProseMirror binding is the load-bearing part, and Yjs's is
the one with years of production use behind it. A CRDT that is theoretically better and
practically loses a keystroke under concurrent editing is worse than one that does not, and
losing keystrokes is the failure this whole milestone exists to prevent.

## Consequences

**The server cannot read a document body without a JavaScript runtime.** This is the real
cost and it lands on M5 (text extraction), M7 (search indexing), M9 (the graph) and M12
(export). Three ways out exist, and the choice between them belongs to whichever milestone
hits it first, not to this ADR:

- keep a **derived plain representation** alongside the CRDT, written by the client on save
  — cheapest, but it can drift from the truth if a client ever fails to write it
- use **`yrs`**, the Rust port of Yjs, server-side for read-only operations
- run a small **Node sidecar** for the operations that need the document

The derived representation is the likely answer, because the existing `Block` tree already
*is* one — but it must then be treated as a cache of the CRDT, never as a second source of
truth, or ADR 0001 quietly stops being true.

**Offline editing stays out of scope** regardless (M16 does offline *reading* only). The
CRDT leaves it open; nothing in this decision closes it.

**Revisions are not the CRDT's history.** M3 stores explicit revisions for diff and restore.
A CRDT's internal history is an implementation detail with different retention and different
semantics, and treating it as the user-facing history would tie the two together.
