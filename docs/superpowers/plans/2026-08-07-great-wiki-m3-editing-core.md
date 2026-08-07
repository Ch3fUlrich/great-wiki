# great-wiki M3 — Editing Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two browsers edit one page simultaneously and both changes survive; every publish
is a revision you can diff and restore.

**Architecture:** A Y.js CRDT document is the live editing state, persisted as a binary
update log. Publishing snapshots the CRDT into an **immutable revision** holding the block
tree. The editor is TipTap over that same CRDT, so human edits, agent edits and concurrent
sessions are all transactions against one shared document — there is no second write path
to reconcile.

**Tech Stack:** Rust 1.97, `yrs` 0.21, Axum WebSockets, TipTap 2 with
`@tiptap/extension-collaboration`, `yjs`, `y-websocket`, `similar` for diffing.

## Global Constraints

Inherited from [the roadmap](2026-08-07-great-wiki-roadmap.md#global-constraints). The ones
that bite in M3:

- **No second write path.** Every content change goes through the CRDT. Nothing writes
  `documents.body` directly except the publish snapshot.
- **Permission checks come from M2's engine.** The WebSocket handshake authorises with
  `Action::Write` before a single update is accepted; `Action::Read` alone gives a
  read-only connection.
- **Every task ends green** on `just ci`.

## Why the CRDT rather than last-write-wins

Requirement: an AI agent editing while a person types must not clobber them. With
last-write-wins that is unsolvable without locking, and locking makes agent editing
useless. With a CRDT both are transactions against one document and merge deterministically
— which is also what retires ADR 0001's stated concern about a second write path.

## File Structure

```
crates/gw-store/migrations/0004_revisions.sql   revisions, crdt state, trash
crates/gw-collab/Cargo.toml                     the CRDT: awareness, updates, snapshots
crates/gw-collab/src/doc.rs                     yrs <-> Block conversion
crates/gw-collab/src/room.rs                    per-document room, broadcast, persistence
crates/gw-api/src/routes/collab.rs              WebSocket endpoint with authorisation
crates/gw-api/src/routes/revisions.rs           history, diff, restore
crates/gw-core/src/diff.rs                      prose, structure and design diffs
web/src/lib/editor/Editor.svelte                TipTap + collaboration
web/src/lib/editor/extensions.ts                the block schema, mirroring gw-core
web/src/routes/[...path]/history/+page.svelte   timeline and diff views
```

**Why `gw-collab` is its own crate:** it is the only place holding mutable in-memory state
shared across connections. Keeping it separate means its concurrency can be reasoned about
without the HTTP layer in view, and the conversion between `yrs` and `Block` — the piece
where fidelity is lost if it is wrong — is unit-testable with no network.

---

## Task 1: The revision model

**Files:**
- Create: `crates/gw-store/migrations/0004_revisions.sql`
- Create: `crates/gw-store/src/revisions.rs`
- Modify: `crates/gw-store/src/lib.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_core::Block`, `gw_auth::Principal`.
- Produces:
  - `struct Revision { id: String, document_id: String, parent_id: Option<String>, body: String, summary: Option<String>, author_id: String, author_name: String, created_at: String, byte_size: i64 }`
  - `async fn Store::publish_revision(&self, document_id, body: &Block, summary: Option<&str>, author: &Principal) -> Result<String>`
  - `async fn Store::revisions(&self, document_id) -> Result<Vec<Revision>>`
  - `async fn Store::revision(&self, id) -> Result<Option<Revision>>`
  - `async fn Store::restore_revision(&self, revision_id, author: &Principal) -> Result<String>`

- [ ] **Step 1: Write the migration**

`crates/gw-store/migrations/0004_revisions.sql`:
```sql
-- Revisions are APPEND-ONLY and never updated in place. That is what makes diff, restore
-- and blame trivial rather than features, and what makes "restore" safe: restoring
-- creates a new revision, so the history it restored from is still there.
CREATE TABLE revisions (
    id           TEXT PRIMARY KEY,
    document_id  TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    parent_id    TEXT REFERENCES revisions(id),
    body         TEXT NOT NULL,               -- Block tree as JSON
    summary      TEXT,
    author_id    TEXT NOT NULL,
    author_name  TEXT NOT NULL,               -- denormalised: history must survive a deleted account
    byte_size    INTEGER NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX revisions_document ON revisions (document_id, created_at DESC);

-- The live CRDT state, separate from revisions: it changes on every keystroke, whereas a
-- revision is a deliberate publish. Storing them together would make the revision table
-- churn and would blur "what was saved" with "what is being typed".
CREATE TABLE crdt_state (
    document_id  TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    state        BLOB NOT NULL,               -- yrs encoded state vector + document
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

ALTER TABLE documents ADD COLUMN current_revision_id TEXT REFERENCES revisions(id);
ALTER TABLE documents ADD COLUMN is_template INTEGER NOT NULL DEFAULT 0;
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/gw-store/src/lib.rs` tests module:
```rust
    use gw_auth::Principal;

    fn author() -> Principal {
        Principal::test("sergej", &["admins"], &[])
    }

    #[tokio::test]
    async fn publishing_creates_a_revision_and_advances_the_document() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Notiz", Visibility::Public)).await.unwrap();

        let rev = store
            .publish_revision(&id, &body("erste Fassung"), Some("initial"), &author())
            .await
            .unwrap();

        let doc = store.document_by_path("/notiz").await.unwrap().unwrap();
        assert!(doc.body.contains("erste Fassung"));
        assert_eq!(store.revisions(&id).await.unwrap().len(), 1);
        assert_eq!(store.revision(&rev).await.unwrap().unwrap().summary.as_deref(), Some("initial"));
    }

    #[tokio::test]
    async fn each_revision_links_to_its_parent() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Notiz", Visibility::Public)).await.unwrap();

        let first = store.publish_revision(&id, &body("eins"), None, &author()).await.unwrap();
        let second = store.publish_revision(&id, &body("zwei"), None, &author()).await.unwrap();

        let revs = store.revisions(&id).await.unwrap();
        assert_eq!(revs.len(), 2);
        assert_eq!(revs[0].id, second, "newest first");
        assert_eq!(revs[0].parent_id.as_deref(), Some(first.as_str()));
        assert!(revs[1].parent_id.is_none());
    }

    #[tokio::test]
    async fn restoring_creates_a_new_revision_rather_than_rewinding() {
        // History must never be destroyed by a restore, or "restore" becomes a way to
        // lose the thing you were unsure about.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Notiz", Visibility::Public)).await.unwrap();
        let first = store.publish_revision(&id, &body("eins"), None, &author()).await.unwrap();
        store.publish_revision(&id, &body("zwei"), None, &author()).await.unwrap();

        let restored = store.restore_revision(&first, &author()).await.unwrap();

        let revs = store.revisions(&id).await.unwrap();
        assert_eq!(revs.len(), 3, "restore appends; it does not remove");
        assert_eq!(revs[0].id, restored);
        let doc = store.document_by_path("/notiz").await.unwrap().unwrap();
        assert!(doc.body.contains("eins"));
    }

    #[tokio::test]
    async fn the_author_name_survives_the_account_being_removed() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Notiz", Visibility::Public)).await.unwrap();
        store.publish_revision(&id, &body("eins"), None, &author()).await.unwrap();

        sqlx::query("DELETE FROM principals").execute(&store.pool).await.unwrap();

        let revs = store.revisions(&id).await.unwrap();
        assert_eq!(revs[0].author_name, "sergej", "history must not lose attribution");
    }

    #[tokio::test]
    async fn byte_size_is_recorded_so_the_timeline_can_show_a_delta() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Notiz", Visibility::Public)).await.unwrap();
        store.publish_revision(&id, &body("kurz"), None, &author()).await.unwrap();
        store
            .publish_revision(&id, &body("deutlich laenger als vorher"), None, &author())
            .await
            .unwrap();

        let revs = store.revisions(&id).await.unwrap();
        assert!(revs[0].byte_size > revs[1].byte_size);
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gw-store revision`
Expected: FAIL — `no method named publish_revision`.

- [ ] **Step 4: Implement**

`crates/gw-store/src/revisions.rs`:
```rust
use crate::Store;
use anyhow::{anyhow, Result};
use gw_auth::Principal;
use gw_core::Block;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Revision {
    pub id: String,
    pub document_id: String,
    pub parent_id: Option<String>,
    pub body: String,
    pub summary: Option<String>,
    pub author_id: String,
    /// Denormalised deliberately: history must remain attributable after the account is
    /// deleted. A join would render every old revision as "unknown".
    pub author_name: String,
    pub byte_size: i64,
    pub created_at: String,
}

impl Store {
    /// Append a revision and point the document at it.
    ///
    /// The document body and the revision are written in one transaction, so the
    /// document can never point at a revision that does not exist, nor hold content with
    /// no revision behind it.
    pub async fn publish_revision(
        &self,
        document_id: &str,
        body: &Block,
        summary: Option<&str>,
        author: &Principal,
    ) -> Result<String> {
        let json = serde_json::to_string(body)?;
        let id = uuid::Uuid::now_v7().to_string();
        let size = json.len() as i64;

        let mut tx = self.pool.begin().await?;

        let parent: Option<(String,)> =
            sqlx::query_as("SELECT current_revision_id FROM documents WHERE id = ?1 AND current_revision_id IS NOT NULL")
                .bind(document_id)
                .fetch_optional(&mut *tx)
                .await?;

        sqlx::query(
            "INSERT INTO revisions \
             (id, document_id, parent_id, body, summary, author_id, author_name, byte_size) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(&id)
        .bind(document_id)
        .bind(parent.map(|(p,)| p))
        .bind(&json)
        .bind(summary)
        .bind(&author.id)
        .bind(&author.username)
        .bind(size)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            "UPDATE documents SET body = ?2, current_revision_id = ?3, updated_at = datetime('now') \
             WHERE id = ?1",
        )
        .bind(document_id)
        .bind(&json)
        .bind(&id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(id)
    }

    pub async fn revisions(&self, document_id: &str) -> Result<Vec<Revision>> {
        Ok(sqlx::query_as::<_, Revision>(
            "SELECT id, document_id, parent_id, body, summary, author_id, author_name, \
                    byte_size, created_at \
             FROM revisions WHERE document_id = ?1 ORDER BY created_at DESC, id DESC",
        )
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn revision(&self, id: &str) -> Result<Option<Revision>> {
        Ok(sqlx::query_as::<_, Revision>(
            "SELECT id, document_id, parent_id, body, summary, author_id, author_name, \
                    byte_size, created_at \
             FROM revisions WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Restore by publishing the old content as a NEW revision.
    ///
    /// Never by rewinding: a restore that destroys the revisions after it turns "let me
    /// look at the old version" into data loss.
    pub async fn restore_revision(&self, revision_id: &str, author: &Principal) -> Result<String> {
        let rev = self
            .revision(revision_id)
            .await?
            .ok_or_else(|| anyhow!("revision {revision_id} not found"))?;
        let body: Block = serde_json::from_str(&rev.body)?;
        let summary = format!("Restored revision {}", &rev.id[..8.min(rev.id.len())]);
        self.publish_revision(&rev.document_id, &body, Some(&summary), author)
            .await
    }
}
```

Add `pub mod revisions; pub use revisions::Revision;` to `lib.rs`.

- [ ] **Step 5: Run the tests, lint, changelog, commit**

Run: `cargo test -p gw-store`
Expected: all green, 18 passed.

Add under `### Added`:
```markdown
- Append-only revisions. Publishing writes the revision and advances the document in one
  transaction; restoring appends a new revision rather than rewinding, so history is never
  destroyed. Author names are denormalised so attribution survives account deletion.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-store CHANGELOG.md
git commit -m "feat(store): append-only revisions with non-destructive restore"
```

---

## Task 2: The CRDT document

**Files:**
- Create: `crates/gw-collab/Cargo.toml`
- Create: `crates/gw-collab/src/lib.rs`
- Create: `crates/gw-collab/src/doc.rs`
- Create: `crates/gw-collab/src/room.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_core::Block`.
- Produces:
  - `struct CollabDoc` wrapping `yrs::Doc` with the `"content"` XML fragment TipTap uses
  - `fn CollabDoc::from_block(block: &Block) -> CollabDoc`
  - `fn CollabDoc::to_block(&self) -> Block`
  - `fn CollabDoc::apply_update(&self, update: &[u8]) -> Result<()>`
  - `fn CollabDoc::encode_state(&self) -> Vec<u8>` / `fn CollabDoc::encode_diff(&self, state_vector: &[u8]) -> Vec<u8>`
  - `struct Rooms` with `async fn Rooms::join(&self, document_id: &str, initial: &Block) -> Arc<Room>`
  - `struct Room` with `broadcast`, `subscribe`, `snapshot`

- [ ] **Step 1: Write the failing tests**

`crates/gw-collab/src/doc.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::doc::CollabDoc;
    use gw_core::Block;

    fn sample() -> Block {
        serde_json::from_str(
            r#"{"kind":"doc","content":[
                 {"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Größe"}]},
                 {"kind":"paragraph","content":[{"kind":"text","text":"Ein Satz."}]}
               ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_block_tree_survives_a_round_trip_through_the_crdt() {
        // This is THE fidelity test. If it fails, editing loses content.
        let doc = CollabDoc::from_block(&sample());
        let back = doc.to_block();
        assert_eq!(back.plain_text(), sample().plain_text());
        assert_eq!(back.headings(), sample().headings());
    }

    #[test]
    fn two_documents_converge_after_exchanging_updates() {
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::new();

        // b learns everything a knows.
        b.apply_update(&a.encode_diff(&b.state_vector())).unwrap();
        assert_eq!(b.to_block().plain_text(), a.to_block().plain_text());
    }

    #[test]
    fn concurrent_edits_from_two_replicas_both_survive() {
        // The requirement this whole milestone exists for: an agent editing while a
        // person types must not clobber them.
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::new();
        b.apply_update(&a.encode_diff(&b.state_vector())).unwrap();

        a.append_paragraph("von A");
        b.append_paragraph("von B");

        // Exchange in both directions.
        let a_update = a.encode_diff(&b.state_vector());
        let b_update = b.encode_diff(&a.state_vector());
        b.apply_update(&a_update).unwrap();
        a.apply_update(&b_update).unwrap();

        for doc in [&a, &b] {
            let text = doc.to_block().plain_text();
            assert!(text.contains("von A"), "A's edit was lost");
            assert!(text.contains("von B"), "B's edit was lost");
        }
        assert_eq!(a.to_block().plain_text(), b.to_block().plain_text(), "must converge");
    }

    #[test]
    fn applying_the_same_update_twice_is_idempotent() {
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::new();
        let update = a.encode_diff(&b.state_vector());
        b.apply_update(&update).unwrap();
        b.apply_update(&update).unwrap();
        assert_eq!(b.to_block().plain_text(), a.to_block().plain_text());
    }

    #[test]
    fn a_malformed_update_is_an_error_not_a_panic() {
        // Updates arrive over a WebSocket from a client. A corrupt frame must not take
        // the server down.
        let doc = CollabDoc::new();
        assert!(doc.apply_update(&[0xff, 0xff, 0xff, 0xff]).is_err());
    }

    #[test]
    fn state_survives_encode_and_reload() {
        let a = CollabDoc::from_block(&sample());
        let encoded = a.encode_state();
        let b = CollabDoc::from_state(&encoded).unwrap();
        assert_eq!(b.to_block().plain_text(), a.to_block().plain_text());
    }
}
```

- [ ] **Step 2: Create the manifest**

`crates/gw-collab/Cargo.toml`:
```toml
[package]
name = "gw-collab"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
gw-core = { path = "../gw-core" }
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
# yrs is the Rust implementation of the same CRDT the browser runs, so server and client
# converge by construction rather than by an interop layer we would have to test.
yrs = "0.21"
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gw-collab`
Expected: FAIL — `could not find doc in the crate root`.

- [ ] **Step 4: Implement the CRDT document**

Implement `crates/gw-collab/src/doc.rs` with `CollabDoc` wrapping `yrs::Doc`, storing the
content in the `XmlFragment` named `"content"` — the name TipTap's collaboration extension
uses by default, so the browser and the server address the same structure.

`from_block` walks the `Block` tree and writes `XmlElement` nodes whose tag is the block
kind and whose attributes are the block's `attrs`, with `XmlText` for leaves. `to_block`
reverses it. Both are pure functions over a transaction, which is what makes the round-trip
test above meaningful.

`apply_update` decodes with `yrs::Update::decode_v1` and returns `Err` on a decode failure
rather than unwrapping — the input is attacker-reachable. `encode_diff` takes a remote state
vector and returns only what the remote lacks; `encode_state` returns the full state for
persistence. `append_paragraph` exists for tests and for the MCP agent path.

- [ ] **Step 5: Implement rooms**

`crates/gw-collab/src/room.rs`: a `Rooms` registry keyed by document id, holding
`Arc<Room>`. Each `Room` owns a `CollabDoc`, a `tokio::sync::broadcast` sender for updates
and an awareness map for presence. `join` loads persisted state on first use and reuses the
live room afterwards. Rooms with no subscribers are dropped after a grace period, persisting
state first.

Persistence is **debounced**: writing on every keystroke would make the database the
bottleneck, and the CRDT already tolerates losing the tail of an update stream because
clients re-send from their state vector on reconnect.

- [ ] **Step 6: Run the tests, lint, changelog, commit**

Run: `cargo test -p gw-collab`
Expected: `test result: ok. 6 passed`.

Add under `### Added`:
```markdown
- `gw-collab`: the collaborative editing core. A Y.js-compatible CRDT (`yrs`) holds live
  document state, so concurrent edits from people and agents merge rather than clobber.
  Block trees round-trip through the CRDT without loss, and malformed updates from a
  client are errors rather than panics.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-collab CHANGELOG.md
git commit -m "feat(collab): CRDT document with lossless block round-trip"
```

---

## Task 3: The collaboration endpoint

**Files:**
- Create: `crates/gw-api/src/routes/collab.rs`
- Modify: `crates/gw-api/src/routes/mod.rs`
- Modify: `crates/gw-api/Cargo.toml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: `GET /api/collab/{document_id}` (WebSocket upgrade), `POST /api/documents/{id}/publish`.

- [ ] **Step 1: Write the failing tests**

Append to `crates/gw-api/tests/api.rs`:
```rust
#[tokio::test]
async fn an_anonymous_websocket_upgrade_is_refused() {
    let store = seed_with_acl().await;
    assert_eq!(ws_upgrade(&store, None, "handbuch").await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn a_reader_gets_a_connection_but_updates_are_rejected() {
    // Read access must not become write access via the socket. This is the easiest
    // permission check in the system to forget, because the HTTP handler already passed.
    let store = seed_with_acl().await;
    let conn = ws_connect(&store, "reader", "handbuch").await.unwrap();
    assert!(conn.read_only, "a reader's connection must be marked read-only");
    assert!(conn.send_update(sample_update()).await.is_err());
}

#[tokio::test]
async fn a_writer_can_send_updates_and_they_are_broadcast() {
    let store = seed_with_acl().await;
    let a = ws_connect(&store, "editor", "handbuch").await.unwrap();
    let b = ws_connect(&store, "editor", "handbuch").await.unwrap();
    a.send_update(sample_update()).await.unwrap();
    assert!(b.next_update().await.is_some(), "the peer must receive the update");
}

#[tokio::test]
async fn publishing_requires_write_and_creates_a_revision() {
    let store = seed_with_acl().await;
    assert_eq!(publish_as(&store, "reader", "handbuch").await, StatusCode::FORBIDDEN);
    assert_eq!(publish_as(&store, "editor", "handbuch").await, StatusCode::OK);
}
```

- [ ] **Step 2: Add dependencies and run the tests**

Add to `crates/gw-api/Cargo.toml`: `axum = { version = "0.8", features = ["ws"] }`,
`gw-collab = { path = "../gw-collab" }`, `futures = "0.3"`.

Run: `cargo test -p gw-api --test api`
Expected: FAIL — the route does not exist.

- [ ] **Step 3: Implement**

`crates/gw-api/src/routes/collab.rs`: authorise **before** the upgrade — resolve the
principal, fetch the document, and call `can(&principal, Action::Write, ...)`. A caller with
only `Action::Read` gets an upgrade flagged read-only; a caller with neither is refused with
403 and never reaches the socket.

Inside the socket loop: on connect, send the full state; then relay client updates into the
room and room broadcasts back out. **A read-only connection drops inbound updates and closes
the socket with a policy-violation code** — silently ignoring them would let a client
believe its edits are saved.

`POST /api/documents/{id}/publish` takes the room's current CRDT state, converts it to a
`Block`, and calls `Store::publish_revision`, requiring `Action::Write`.

- [ ] **Step 4: Run the tests, lint, changelog, commit**

Run: `cargo test -p gw-api`
Expected: green.

Add under `### Added`:
```markdown
- Collaborative editing endpoint. Authorisation happens before the WebSocket upgrade; a
  read-only caller gets a connection that rejects updates and closes rather than silently
  discarding them, so a client can never believe an edit was saved when it was not.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-api CHANGELOG.md
git commit -m "feat(api): authorised collaborative editing over websocket"
```

---

## Task 4: The editor

**Files:**
- Create: `web/src/lib/editor/extensions.ts`
- Create: `web/src/lib/editor/Editor.svelte`
- Modify: `web/src/routes/[...path]/+page.svelte`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces: an `<Editor>` component that connects to the collaboration socket and edits in
  place on the rendered page.

- [ ] **Step 1: Install and define the schema**

```bash
cd web && npm install @tiptap/core @tiptap/starter-kit @tiptap/extension-collaboration \
  @tiptap/extension-collaboration-cursor yjs y-websocket
```

`web/src/lib/editor/extensions.ts` defines exactly the node types `gw-core::BlockKind`
knows — no more. A node the server cannot represent is a node whose content is lost on
publish, so the schema is the contract and the two must be changed together.

- [ ] **Step 2: Write the failing test**

`web/src/lib/editor/extensions.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { EDITOR_NODE_NAMES } from './extensions';

// The server's BlockKind enum, in camelCase as serde emits it.
const SERVER_KINDS = [
  'doc', 'paragraph', 'heading', 'bulletList', 'orderedList',
  'listItem', 'blockquote', 'codeBlock', 'text'
];

describe('editor schema', () => {
  it('matches the server block kinds exactly', () => {
    // A node the server cannot represent loses its content on publish; a kind the editor
    // lacks cannot be edited. Both are silent, so this test is the guard.
    expect([...EDITOR_NODE_NAMES].sort()).toEqual([...SERVER_KINDS].sort());
  });
});
```

- [ ] **Step 3: Run it, implement, run again**

Run: `cd web && npx vitest run src/lib/editor/extensions.test.ts` → FAIL, then implement
`EDITOR_NODE_NAMES` and the extension list, then re-run → PASS.

- [ ] **Step 4: Build the editor component**

`web/src/lib/editor/Editor.svelte`: creates a `Y.Doc`, connects a `WebsocketProvider` to
`/api/collab/{documentId}`, and mounts TipTap with `Collaboration` and
`CollaborationCursor`. Presence shows who else is editing. An explicit **Publish** button
posts to `/api/documents/{id}/publish` with an optional summary — autosave keeps the CRDT
current, but a revision is a deliberate act.

Read-only connections render the editor with `editable: false` and show why.

Accessibility: the editor region carries `role="textbox"`, `aria-multiline="true"` and an
accessible name; the toolbar is a `toolbar` role with arrow-key navigation; every control
has a visible label or `aria-label`.

- [ ] **Step 5: Wire it into the page**

The document page shows the read-only renderer by default and swaps to `<Editor>` when the
user has write access and clicks **Edit**, or immediately at `?edit=1`.

- [ ] **Step 6: Verify by hand — this is the milestone's whole point**

```bash
just dev
```
Open the same page in two browser windows. Type in both. Expected: both sets of changes
appear in both windows, and neither is lost. Publish from one; expected: a revision appears
in the history for both.

- [ ] **Step 7: Run the gate, changelog, commit**

Run: `just ci` → green.

Add under `### Added`:
```markdown
- In-place editing on the rendered page: TipTap over the shared CRDT with live presence
  and cursors. The editor's node schema is asserted equal to the server's block kinds by
  a test, because a mismatch loses content silently.
```

```bash
git add web CHANGELOG.md
git commit -m "feat(web): collaborative in-place editing with TipTap"
```

---

## Task 5: History, diffs and restore

**Files:**
- Create: `crates/gw-core/src/diff.rs`
- Create: `crates/gw-api/src/routes/revisions.rs`
- Create: `web/src/routes/[...path]/history/+page.server.ts`
- Create: `web/src/routes/[...path]/history/+page.svelte`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `fn diff_prose(a: &Block, b: &Block) -> Vec<ProseChange>` — word-level
  - `fn diff_structure(a: &Block, b: &Block) -> Vec<StructureChange>` — blocks added, removed, moved
  - `fn diff_design(a: &Block, b: &Block) -> Vec<DesignChange>` — attribute and layout changes
  - `GET /api/documents/{id}/revisions`, `GET /api/revisions/{a}/diff/{b}`, `POST /api/revisions/{id}/restore`

**Why three diff modes:** a prose diff hides exactly what a design change is. Changing a
heading's level, reordering blocks or altering a chart's configuration produces no word-level
difference at all, so a single diff mode would report "no changes" for edits that plainly
changed the page.

- [ ] **Step 1: Write the failing tests**

`crates/gw-core/src/diff.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::block::Block;
    use crate::diff::{diff_design, diff_prose, diff_structure, ChangeKind};

    fn doc(json: &str) -> Block {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn prose_diff_reports_word_level_changes() {
        let a = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der schnelle Fuchs"}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der langsame Fuchs"}]}]}"#);
        let changes = diff_prose(&a, &b);
        assert!(changes.iter().any(|c| c.kind == ChangeKind::Removed && c.text == "schnelle"));
        assert!(changes.iter().any(|c| c.kind == ChangeKind::Added && c.text == "langsame"));
    }

    #[test]
    fn prose_diff_is_empty_when_only_the_design_changed() {
        let a = doc(r#"{"kind":"doc","content":[{"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Titel"}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[{"kind":"heading","attrs":{"level":4},"content":[{"kind":"text","text":"Titel"}]}]}"#);
        assert!(diff_prose(&a, &b).is_empty(), "no words changed");
        // ...but the page plainly changed, which is why the design diff exists.
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1);
        assert_eq!(design[0].attribute, "level");
        assert_eq!(design[0].before.as_deref(), Some("2"));
        assert_eq!(design[0].after.as_deref(), Some("4"));
    }

    #[test]
    fn structure_diff_reports_an_added_block() {
        let a = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins"}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins"}]},{"kind":"paragraph","content":[{"kind":"text","text":"zwei"}]}]}"#);
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Added);
    }

    #[test]
    fn structure_diff_reports_a_move_rather_than_an_add_and_a_remove() {
        // Reordering two paragraphs is one change, not two. Reporting it as add+remove
        // makes a reorder look like a rewrite.
        let a = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"A"}]},{"kind":"paragraph","content":[{"kind":"text","text":"B"}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"B"}]},{"kind":"paragraph","content":[{"kind":"text","text":"A"}]}]}"#);
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Moved);
    }

    #[test]
    fn identical_documents_produce_no_changes_in_any_mode() {
        let a = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"gleich"}]}]}"#);
        assert!(diff_prose(&a, &a).is_empty());
        assert!(diff_structure(&a, &a).is_empty());
        assert!(diff_design(&a, &a).is_empty());
    }
}
```

- [ ] **Step 2: Add the dependency, run, implement, run**

Add `similar = "2"` to `gw-core`'s dependencies. Run the tests → FAIL. Implement
`diff_prose` over whitespace-split tokens with `similar::TextDiff`; `diff_structure` by
comparing block fingerprints (kind plus plain text) as sequences, classifying a
removed-and-re-added fingerprint as `Moved`; `diff_design` by walking both trees in
parallel and reporting attribute differences on matched blocks. Re-run → PASS.

- [ ] **Step 3: Build the history page**

The timeline lists revisions newest first with author, relative time, summary and size
delta. Selecting two revisions shows a three-tab diff — **Prose**, **Structure**,
**Design** — plus a **View source** tab rendering the export triple (`.md`, `.meta.yml`,
`.design.json`) for the selected revision, which is the "see the full file" requirement.
**Restore** confirms, then posts, then navigates to the document.

Diffs use text as well as colour to mark additions and removals, so they are legible
without colour vision.

- [ ] **Step 4: Run the gate, changelog, commit**

Run: `just ci` → green.

Add under `### Added`:
```markdown
- Revision history with three diff modes — prose, structure and design — because a
  word-level diff reports "no changes" for a reordered or restyled page. A structural move
  is reported as one change, not as an addition and a removal. Includes view-source for
  any revision and non-destructive restore.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates web CHANGELOG.md
git commit -m "feat(history): timeline with prose, structure and design diffs"
```

---

## Task 6: Templates, duplication, reordering and trash

**Files:**
- Create: `crates/gw-store/src/tree_ops.rs`
- Create: `crates/gw-api/src/routes/documents.rs`
- Create: `web/src/routes/trash/+page.svelte`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `async fn Store::duplicate_document(&self, id, new_parent: Option<&str>, author) -> Result<String>`
  - `async fn Store::move_document(&self, id, new_parent: Option<&str>, sort_key: i64) -> Result<()>` — rewrites descendant paths
  - `async fn Store::soft_delete(&self, id, author) -> Result<()>` / `restore_document` / `purge_deleted_before(cutoff)`
  - `POST /api/documents`, `POST /api/documents/{id}/duplicate`, `POST /api/documents/{id}/move`, `DELETE /api/documents/{id}`, `POST /api/documents/{id}/restore`, `GET /api/trash`

- [ ] **Step 1: Write the failing tests**

```rust
    #[tokio::test]
    async fn moving_a_branch_rewrites_every_descendant_path() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store.insert_document(&new_doc(None, "Alt", Visibility::Public)).await.unwrap();
        store.insert_document(&new_doc(None, "Neu", Visibility::Public)).await.unwrap();
        let child = store
            .insert_document(&new_doc(Some("/alt"), "Kind", Visibility::Public))
            .await
            .unwrap();
        store
            .insert_document(&new_doc(Some("/alt/kind"), "Enkel", Visibility::Public))
            .await
            .unwrap();

        store.move_document(&child, Some("/neu"), 0).await.unwrap();

        assert!(store.document_by_path("/neu/kind").await.unwrap().is_some());
        // A stale descendant path is a page that becomes unreachable — the failure mode
        // that makes "move" feel like "delete".
        assert!(store.document_by_path("/neu/kind/enkel").await.unwrap().is_some());
        assert!(store.document_by_path("/alt/kind").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn moving_into_a_descendant_of_itself_is_rejected() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let parent = store.insert_document(&new_doc(None, "Alt", Visibility::Public)).await.unwrap();
        store.insert_document(&new_doc(Some("/alt"), "Kind", Visibility::Public)).await.unwrap();
        // Would detach the branch from the tree entirely.
        assert!(store.move_document(&parent, Some("/alt/kind"), 0).await.is_err());
    }

    #[tokio::test]
    async fn duplicating_gives_a_fresh_slug_rather_than_colliding() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Notiz", Visibility::Public)).await.unwrap();
        store.duplicate_document(&id, None, &author()).await.unwrap();
        assert!(store.document_by_path("/notiz").await.unwrap().is_some());
        assert!(store.document_by_path("/notiz-kopie").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn soft_deleting_hides_a_branch_but_keeps_it_restorable() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Alt", Visibility::Public)).await.unwrap();
        store.insert_document(&new_doc(Some("/alt"), "Kind", Visibility::Public)).await.unwrap();

        store.soft_delete(&id, &author()).await.unwrap();
        assert!(store.document_by_path("/alt").await.unwrap().is_none());
        assert!(store.document_by_path("/alt/kind").await.unwrap().is_none(), "children go too");
        assert_eq!(store.trash().await.unwrap().len(), 2);

        store.restore_document(&id, &author()).await.unwrap();
        assert!(store.document_by_path("/alt/kind").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_duplicate_of_a_template_is_not_itself_a_template() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store.insert_document(&new_doc(None, "Vorlage", Visibility::Public)).await.unwrap();
        store.set_template(&id, true).await.unwrap();
        let copy = store.duplicate_document(&id, None, &author()).await.unwrap();
        assert!(!store.is_template(&copy).await.unwrap());
    }
```

- [ ] **Step 2: Run, implement, run**

Run: `cargo test -p gw-store tree_ops` → FAIL. Implement in `tree_ops.rs`: `move_document`
inside a transaction that rejects a move into its own subtree, updates the moved row, and
rewrites descendants with a prefix replace on `path`; `duplicate_document` appending
`-kopie` (incrementing on further collisions) and clearing the template flag; soft delete
and restore applying to the whole subtree. Re-run → PASS.

- [ ] **Step 3: Build the interface**

Tree navigation gains drag-to-reorder with a keyboard equivalent (move up/down/in/out via
buttons, since drag alone is not accessible). "New from template" lists templates. The trash
page lists deleted branches with when and by whom, restore, and permanent delete with an
explicit confirmation naming the number of pages affected.

- [ ] **Step 4: Run the gate, changelog, commit**

Run: `just ci` → green.

Add under `### Added`:
```markdown
- Tree operations: move a branch (rewriting every descendant path, and refusing a move
  into its own subtree), duplicate a page, templates and "new from template", and soft
  delete with a trash view and restore. Reordering has a keyboard equivalent, because
  drag-and-drop alone excludes keyboard users.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates web CHANGELOG.md
git commit -m "feat(documents): move, duplicate, templates and recoverable trash"
```

---

## Milestone exit criteria

- [ ] `just ci` passes.
- [ ] Two browsers editing the same page both keep their changes, with live cursors.
- [ ] A read-only user's editor is not editable, and a forged update over the socket is
      rejected with a close rather than silently ignored.
- [ ] Publishing creates a revision; the timeline shows author, time and size delta.
- [ ] All three diff modes work, and changing only a heading level shows an empty prose
      diff and a non-empty design diff.
- [ ] Restoring an old revision appends rather than removing later ones.
- [ ] Moving a branch leaves no unreachable descendant.
- [ ] Deleting a branch is recoverable from the trash.

## Self-Review

**Spec coverage.** Implements spec §1.1 criteria 1–3 (edit the rendered page, concurrent
edits both survive, agents merge rather than overwrite), §9 in full (timeline, three diff
modes, view source, non-destructive restore, compare arbitrary pairs), and the editing
portion of §10.

**Placeholders.** Tasks 1, 2 and 5 carry complete code for the parts where correctness is
subtle — the revision transaction, the CRDT round-trip, the diff semantics. Tasks 3, 4 and
6 specify implementations in prose because their behaviour is fully pinned by the tests
given in full, and the code is assembly over interfaces already defined. Each names the
exact failure it is preventing rather than leaving it to judgement.

**Type consistency.** `Block` is the single content type across `gw-core`, `gw-store`,
`gw-collab` and the API. `EDITOR_NODE_NAMES` is asserted equal to `BlockKind` by a test
rather than by convention. `Action::Write` gates the socket, publish, move, duplicate and
delete identically. `Revision` field names match between the store, the API JSON and the
timeline component.

**The invariant this milestone establishes:** after M3 there is exactly one write path for
content — the CRDT, snapshotted by publish. `documents.body` is written only by
`publish_revision`. Later milestones add block *types* and *views*, never a second way to
change a document.
