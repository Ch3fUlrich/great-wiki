# great-wiki M1 — Vertical Slice on a Real URL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Log in at `https://kb.ohje.ooguy.com` through Authelia OIDC and read a page from a
navigable document tree.

**Architecture:** The thinnest possible path through every layer — block model, SQLite store,
Axum API, SvelteKit reader, Caddy edge, OIDC login — so that the integration risks
(especially authentication, which cannot be developed without a real HTTPS hostname) are
retired first rather than last.

**Tech Stack:** Rust 1.97, Axum 0.8, sqlx 0.8 (SQLite), `openidconnect` 4, SvelteKit 2 /
Svelte 5, Caddy on OPNsense, Authelia 4.38.

## Global Constraints

Inherited from [the roadmap](2026-08-07-great-wiki-roadmap.md#global-constraints). The ones
that bite in M1:

- **Fail closed.** Missing visibility → private. Unset proxy secret in production → refuse
  to start. Unknown permission → deny.
- **Bind `0.0.0.0` in production** — Caddy runs on OPNsense (192.168.178.76), a different
  host, so a loopback bind is unreachable from it. **Never port 8090** (`omnigraph-viewer`).
- **Relative path defaults.** `./data/great-wiki.db`, not `/data/...`. Container paths come
  from compose in M18.
- **Every task ends green** on `just ci`.

## Decisions locked by this plan

**sqlx runtime queries, not the `query!` macro.** The compile-time macros require a
committed `.sqlx/` directory kept in sync by `cargo sqlx prepare`, which the predecessor plan
had to invoke in seven separate places and which breaks Docker builds with an error that
never mentions sqlx. Runtime `query_as` with `FromRow` removes that whole class of problem.
Injection safety is preserved by binding every parameter without exception; schema
correctness is proven by integration tests that run the real migrations.

**Materialised path for the tree.** `document.path` holds `/<parent-slug>/<slug>`. Subtree
queries and breadcrumbs become one indexed prefix match instead of recursion, which matters
because permission inheritance (M2) walks the same path.

## File Structure

```
crates/gw-core/src/block.rs        Block, BlockKind — the document content model
crates/gw-core/src/document.rs     DocumentType, Visibility, DocumentMeta
crates/gw-store/Cargo.toml
crates/gw-store/src/lib.rs         re-exports; the Store handle
crates/gw-store/src/migrations/    0001_init.sql
crates/gw-store/src/documents.rs   tree queries, get by path, insert
crates/gw-api/Cargo.toml
crates/gw-api/src/main.rs          binary entrypoint, CLI (serve | check)
crates/gw-api/src/config.rs        environment parsing and the fail-closed startup checks
crates/gw-api/src/identity.rs      Identity, the dev shim, OIDC claims
crates/gw-api/src/error.rs         ApiError -> HTTP mapping
crates/gw-api/src/routes/mod.rs    router assembly
crates/gw-api/src/routes/tree.rs   GET /api/tree
crates/gw-api/src/routes/docs.rs   GET /api/documents/{*path}
crates/gw-api/src/auth/oidc.rs     discovery, login redirect, callback, session
web/src/routes/+layout.svelte      chrome, theme, skip link
web/src/routes/+page.server.ts     home: the tree
web/src/routes/[...path]/+page.server.ts   document loader
web/src/routes/[...path]/+page.svelte      document renderer
web/src/lib/blocks/render.ts       block tree -> renderable structure
web/src/lib/api.ts                 typed fetch wrappers
```

**Why `identity.rs` is separate from `auth/oidc.rs`:** `Identity` is what every handler
consumes and every test constructs; OIDC is one way of producing one. Keeping them apart is
what lets M2 add local accounts without touching a single handler.

---

## Task 1: The block model

**Files:**
- Create: `crates/gw-core/src/block.rs`
- Create: `crates/gw-core/src/document.rs`
- Modify: `crates/gw-core/src/lib.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_core::slugify` from M0.
- Produces:
  - `enum BlockKind { Doc, Paragraph, Heading, BulletList, OrderedList, ListItem, Blockquote, CodeBlock, Text }`
  - `struct Block { kind: BlockKind, attrs: serde_json::Map<String, serde_json::Value>, content: Vec<Block>, text: Option<String> }`
  - `fn Block::plain_text(&self) -> String`
  - `fn Block::headings(&self) -> Vec<Heading>` where `struct Heading { level: u8, text: String, id: String }`
  - `enum DocumentType { Page, Research, Project, Dataset }` with `as_str` and `FromStr`
  - `enum Visibility { Public, Internal, Restricted }` with `Default = Restricted`

**Why this shape:** it is ProseMirror's node shape. Storing anything else would mean a
translation layer between what the editor produces and what the database holds, and that
layer is exactly where fidelity is lost.

- [ ] **Step 1: Write the failing tests**

`crates/gw-core/src/block.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::block::{Block, BlockKind};

    fn sample() -> Block {
        serde_json::from_str(
            r#"{
              "kind": "doc",
              "content": [
                {"kind": "heading", "attrs": {"level": 2},
                 "content": [{"kind": "text", "text": "Größe und Maß"}]},
                {"kind": "paragraph",
                 "content": [{"kind": "text", "text": "Ein Satz."},
                             {"kind": "text", "text": " Noch einer."}]}
              ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn deserialises_a_prosemirror_shaped_tree() {
        let doc = sample();
        assert_eq!(doc.kind, BlockKind::Doc);
        assert_eq!(doc.content.len(), 2);
        assert_eq!(doc.content[0].kind, BlockKind::Heading);
    }

    #[test]
    fn plain_text_concatenates_leaves_in_order() {
        assert_eq!(sample().plain_text(), "Größe und Maß Ein Satz. Noch einer.");
    }

    #[test]
    fn headings_carry_a_transliterated_anchor_id() {
        let headings = sample().headings();
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].level, 2);
        assert_eq!(headings[0].text, "Größe und Maß");
        // The anchor must be ASCII or the URL fragment needs percent-encoding.
        assert_eq!(headings[0].id, "groesse-und-mass");
    }

    #[test]
    fn heading_level_defaults_to_one_when_absent() {
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"heading","content":[{"kind":"text","text":"T"}]}]}"#,
        )
        .unwrap();
        assert_eq!(doc.headings()[0].level, 1);
    }

    #[test]
    fn round_trips_through_json_unchanged() {
        let doc = sample();
        let again: Block = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(again.plain_text(), doc.plain_text());
        assert_eq!(again.headings().len(), doc.headings().len());
    }
}
```

`crates/gw-core/src/document.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::document::{DocumentType, Visibility};
    use std::str::FromStr;

    #[test]
    fn document_type_round_trips_through_str() {
        for t in [
            DocumentType::Page,
            DocumentType::Research,
            DocumentType::Project,
            DocumentType::Dataset,
        ] {
            assert_eq!(DocumentType::from_str(t.as_str()).unwrap(), t);
        }
    }

    #[test]
    fn unknown_document_type_is_an_error() {
        assert!(DocumentType::from_str("wiki").is_err());
    }

    #[test]
    fn visibility_defaults_to_restricted() {
        // Fail closed: a document with no stated visibility must never be world-readable.
        assert_eq!(Visibility::default(), Visibility::Restricted);
    }

    #[test]
    fn visibility_parses_the_three_levels() {
        assert_eq!(Visibility::from_str("public").unwrap(), Visibility::Public);
        assert_eq!(Visibility::from_str("internal").unwrap(), Visibility::Internal);
        assert_eq!(Visibility::from_str("restricted").unwrap(), Visibility::Restricted);
        assert!(Visibility::from_str("world").is_err());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gw-core`
Expected: FAIL — `unresolved import crate::block` and `crate::document`.

- [ ] **Step 3: Implement the block model**

Prepend to `crates/gw-core/src/block.rs`:
```rust
use crate::slugify;
use serde::{Deserialize, Serialize};

/// The node kinds M1 understands. The registry in M4 adds more; this enum is
/// `#[non_exhaustive]` so adding one is not a breaking change for downstream matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum BlockKind {
    Doc,
    Paragraph,
    Heading,
    BulletList,
    OrderedList,
    ListItem,
    Blockquote,
    CodeBlock,
    Text,
}

/// A node in the document tree, shaped exactly like a ProseMirror node.
///
/// Matching the editor's own representation means there is no translation layer between
/// what is edited and what is stored — and therefore nowhere for fidelity to be lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub attrs: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub id: String,
}

impl Block {
    /// Concatenate every text leaf in document order.
    ///
    /// This is what feeds the search index and the embedding chunker, so it must be
    /// stable: two documents that read identically must produce identical text.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        if let Some(t) = &self.text {
            out.push_str(t);
        }
        for child in &self.content {
            child.collect_text(out);
        }
    }

    /// Extract the heading outline, with an ASCII anchor id for each.
    ///
    /// The id goes through `slugify`, so a German heading yields a fragment that needs no
    /// percent-encoding and survives being copied out of the address bar.
    pub fn headings(&self) -> Vec<Heading> {
        let mut out = Vec::new();
        self.collect_headings(&mut out);
        out
    }

    fn collect_headings(&self, out: &mut Vec<Heading>) {
        if self.kind == BlockKind::Heading {
            let level = self
                .attrs
                .get("level")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .clamp(1, 6) as u8;
            let text = self.plain_text();
            out.push(Heading {
                level,
                id: slugify(&text),
                text,
            });
            return; // headings do not nest
        }
        for child in &self.content {
            child.collect_headings(out);
        }
    }
}
```

Prepend to `crates/gw-core/src/document.rs`:
```rust
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown value `{0}`")]
pub struct ParseError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentType {
    Page,
    Research,
    Project,
    Dataset,
}

impl DocumentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentType::Page => "page",
            DocumentType::Research => "research",
            DocumentType::Project => "project",
            DocumentType::Dataset => "dataset",
        }
    }
}

impl FromStr for DocumentType {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "page" => Ok(DocumentType::Page),
            "research" => Ok(DocumentType::Research),
            "project" => Ok(DocumentType::Project),
            "dataset" => Ok(DocumentType::Dataset),
            other => Err(ParseError(other.to_string())),
        }
    }
}

/// Who may read a document, before per-document ACLs are consulted.
///
/// `Restricted` is the Default deliberately. A document that arrives without a stated
/// visibility — from an importer, a migration, a bug — must never be world-readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Internal,
    Restricted,
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Restricted
    }
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Internal => "internal",
            Visibility::Restricted => "restricted",
        }
    }
}

impl FromStr for Visibility {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "public" => Ok(Visibility::Public),
            "internal" => Ok(Visibility::Internal),
            "restricted" => Ok(Visibility::Restricted),
            other => Err(ParseError(other.to_string())),
        }
    }
}
```

Add to `crates/gw-core/src/lib.rs`, above the existing `pub mod slug;`:
```rust
pub mod block;
pub mod document;

pub use block::{Block, BlockKind, Heading};
pub use document::{DocumentType, Visibility};
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gw-core`
Expected: `test result: ok. 14 passed` (5 from M0 plus 9 here).

- [ ] **Step 5: Lint, changelog and commit**

Add under `### Added` in `CHANGELOG.md`:
```markdown
- Document content model: ProseMirror-shaped `Block` tree with plain-text extraction and
  a heading outline whose anchor ids are transliterated to ASCII. `Visibility` defaults to
  `Restricted` so a document with no stated visibility is never world-readable.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-core CHANGELOG.md
git commit -m "feat(core): prosemirror-shaped block model and fail-closed visibility"
```

---

## Task 2: The store — schema, migrations and the document tree

**Files:**
- Create: `crates/gw-store/Cargo.toml`
- Create: `crates/gw-store/src/lib.rs`
- Create: `crates/gw-store/migrations/0001_init.sql`
- Create: `crates/gw-store/src/documents.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_core::{Block, DocumentType, Visibility}`.
- Produces:
  - `struct Store { pool: sqlx::SqlitePool }`, `async fn Store::open(url: &str) -> anyhow::Result<Store>` (runs migrations)
  - `struct StoredDocument { id: String, path: String, doc_type: String, slug: String, title: String, language: String, visibility: String, body: String, parent_path: Option<String>, sort_key: i64 }`
  - `struct TreeNode { path: String, slug: String, title: String, doc_type: String, visibility: String, children: Vec<TreeNode> }`
  - `async fn Store::insert_document(&self, doc: &NewDocument) -> anyhow::Result<String>`
  - `async fn Store::document_by_path(&self, path: &str) -> anyhow::Result<Option<StoredDocument>>`
  - `async fn Store::tree(&self) -> anyhow::Result<Vec<TreeNode>>`
  - `struct NewDocument { parent_path: Option<String>, doc_type: DocumentType, title: String, slug: Option<String>, language: String, visibility: Visibility, body: Block, sort_key: i64 }`

- [ ] **Step 1: Write the migration**

`crates/gw-store/migrations/0001_init.sql`:
```sql
-- Documents are the SOURCE OF TRUTH, not a derived cache. Deleting this database loses
-- content. Backup and the git export (M17, M12) are how it is protected.

CREATE TABLE documents (
    id            TEXT PRIMARY KEY,               -- uuid v7: sortable by creation time
    parent_path   TEXT,                            -- NULL for a root document
    -- Materialised path, e.g. '/handbook/onboarding'. Subtree queries and permission
    -- inheritance are both a prefix match on this, which an index serves directly.
    path          TEXT NOT NULL UNIQUE,
    slug          TEXT NOT NULL,
    doc_type      TEXT NOT NULL,
    title         TEXT NOT NULL,
    language      TEXT NOT NULL DEFAULT 'de',
    -- Fail closed: anything that does not say otherwise is restricted.
    visibility    TEXT NOT NULL DEFAULT 'restricted'
                  CHECK (visibility IN ('public', 'internal', 'restricted')),
    body          TEXT NOT NULL,                   -- Block tree as JSON
    sort_key      INTEGER NOT NULL DEFAULT 0,      -- sibling order, for drag-to-reorder
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at    TEXT                             -- soft delete; M3 adds the trash UI
);

CREATE INDEX documents_parent   ON documents (parent_path, sort_key);
CREATE INDEX documents_prefix   ON documents (path);
CREATE INDEX documents_visible  ON documents (visibility) WHERE deleted_at IS NULL;
```

- [ ] **Step 2: Write the failing tests**

`crates/gw-store/src/lib.rs`:
```rust
pub mod documents;

pub use documents::{NewDocument, StoredDocument, TreeNode};

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub struct Store {
    pub pool: SqlitePool,
}

impl Store {
    /// Open (creating if needed) and migrate.
    ///
    /// `max_connections(1)` is deliberate and load-bearing for tests: an in-memory SQLite
    /// database is private per connection, so a larger pool silently gives some queries an
    /// empty database. Production is a single-writer workload anyway.
    pub async fn open(url: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_core::{Block, DocumentType, Visibility};

    fn body(text: &str) -> Block {
        serde_json::from_str(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}"#
        ))
        .unwrap()
    }

    fn new_doc(parent: Option<&str>, title: &str, vis: Visibility) -> NewDocument {
        NewDocument {
            parent_path: parent.map(str::to_string),
            doc_type: DocumentType::Page,
            title: title.to_string(),
            slug: None,
            language: "de".into(),
            visibility: vis,
            body: body("hallo"),
            sort_key: 0,
        }
    }

    #[tokio::test]
    async fn insert_then_fetch_by_path() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .insert_document(&new_doc(None, "Größe und Maß", Visibility::Public))
            .await
            .unwrap();

        let got = store
            .document_by_path("/groesse-und-mass")
            .await
            .unwrap()
            .expect("document should exist");
        assert_eq!(got.title, "Größe und Maß");
        assert_eq!(got.visibility, "public");
        assert!(got.body.contains("hallo"));
    }

    #[tokio::test]
    async fn path_is_derived_from_parent_and_slug() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store.insert_document(&new_doc(None, "Handbuch", Visibility::Public)).await.unwrap();
        store
            .insert_document(&new_doc(Some("/handbuch"), "Erste Schritte", Visibility::Public))
            .await
            .unwrap();

        let child = store.document_by_path("/handbuch/erste-schritte").await.unwrap();
        assert!(child.is_some(), "child path must nest under its parent");
    }

    #[tokio::test]
    async fn duplicate_path_is_rejected_rather_than_overwriting() {
        // Silent overwrite on slug collision is data loss. It must be an error.
        let store = Store::open("sqlite::memory:").await.unwrap();
        store.insert_document(&new_doc(None, "Notes", Visibility::Public)).await.unwrap();
        let second = store.insert_document(&new_doc(None, "Notes", Visibility::Public)).await;
        assert!(second.is_err(), "a colliding path must fail loudly");
    }

    #[tokio::test]
    async fn missing_document_is_none_not_an_error() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        assert!(store.document_by_path("/nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn tree_nests_children_under_parents_in_sort_order() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store.insert_document(&new_doc(None, "Handbuch", Visibility::Public)).await.unwrap();

        let mut b = new_doc(Some("/handbuch"), "Beta", Visibility::Public);
        b.sort_key = 2;
        let mut a = new_doc(Some("/handbuch"), "Alpha", Visibility::Public);
        a.sort_key = 1;
        store.insert_document(&b).await.unwrap();
        store.insert_document(&a).await.unwrap();

        let tree = store.tree().await.unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].path, "/handbuch");
        let kids: Vec<&str> = tree[0].children.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(kids, vec!["alpha", "beta"], "children must respect sort_key");
    }

    #[tokio::test]
    async fn soft_deleted_documents_are_excluded() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store
            .insert_document(&new_doc(None, "Temporär", Visibility::Public))
            .await
            .unwrap();
        sqlx::query("UPDATE documents SET deleted_at = datetime('now') WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();

        assert!(store.document_by_path("/temporaer").await.unwrap().is_none());
        assert!(store.tree().await.unwrap().is_empty());
    }
}
```

- [ ] **Step 3: Create the manifest**

`crates/gw-store/Cargo.toml`:
```toml
[package]
name = "gw-store"
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
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite", "migrate"] }
tokio = { workspace = true }
tracing = { workspace = true }
uuid = { version = "1", features = ["v7"] }

[dev-dependencies]
tokio = { workspace = true }
```

Note the absence of the `macros` feature: this crate uses runtime `query_as`, not
`sqlx::query!`, so there is no `.sqlx` offline metadata to generate or keep in sync.

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p gw-store`
Expected: FAIL — `could not find documents in the crate root`.

- [ ] **Step 5: Implement the document queries**

`crates/gw-store/src/documents.rs`:
```rust
use crate::Store;
use anyhow::Result;
use gw_core::{slugify, Block, DocumentType, Visibility};
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone)]
pub struct NewDocument {
    pub parent_path: Option<String>,
    pub doc_type: DocumentType,
    pub title: String,
    /// When `None`, derived from `title`. An explicit slug wins so a long title can have
    /// a short URL.
    pub slug: Option<String>,
    pub language: String,
    pub visibility: Visibility,
    pub body: Block,
    pub sort_key: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct StoredDocument {
    pub id: String,
    pub path: String,
    pub parent_path: Option<String>,
    pub slug: String,
    pub doc_type: String,
    pub title: String,
    pub language: String,
    pub visibility: String,
    /// The Block tree as JSON. Deserialised by the caller so the store stays agnostic
    /// about the content model's version.
    pub body: String,
    pub sort_key: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TreeNode {
    pub path: String,
    pub slug: String,
    pub title: String,
    pub doc_type: String,
    pub visibility: String,
    pub children: Vec<TreeNode>,
}

impl Store {
    pub async fn insert_document(&self, doc: &NewDocument) -> Result<String> {
        let slug = doc
            .slug
            .as_deref()
            .map(slugify)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| slugify(&doc.title));
        anyhow::ensure!(!slug.is_empty(), "title `{}` produced an empty slug", doc.title);

        let parent = doc.parent_path.as_deref().unwrap_or("");
        let path = format!("{parent}/{slug}");
        let id = uuid::Uuid::now_v7().to_string();
        let body = serde_json::to_string(&doc.body)?;

        // The UNIQUE constraint on `path` is what turns a slug collision into an error
        // instead of a silent overwrite. Do NOT add ON CONFLICT here.
        sqlx::query(
            r#"
            INSERT INTO documents
              (id, parent_path, path, slug, doc_type, title, language, visibility, body, sort_key)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&id)
        .bind(doc.parent_path.as_deref())
        .bind(&path)
        .bind(&slug)
        .bind(doc.doc_type.as_str())
        .bind(&doc.title)
        .bind(&doc.language)
        .bind(doc.visibility.as_str())
        .bind(&body)
        .bind(doc.sort_key)
        .execute(&self.pool)
        .await?;

        Ok(id)
    }

    pub async fn document_by_path(&self, path: &str) -> Result<Option<StoredDocument>> {
        let row = sqlx::query_as::<_, StoredDocument>(
            r#"
            SELECT id, path, parent_path, slug, doc_type, title, language, visibility, body, sort_key
            FROM documents
            WHERE path = ?1 AND deleted_at IS NULL
            "#,
        )
        .bind(path)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row)
    }

    /// The whole tree, nested.
    ///
    /// Fetched flat in one query and assembled in memory rather than issuing a query per
    /// level: a wiki's tree is small enough that one round trip beats N, and the ordering
    /// is then unambiguous.
    ///
    /// NOTE: this returns every document. Permission filtering is added in M2 by taking a
    /// principal and joining the ACL — it is NOT a caller's responsibility to filter, and
    /// M2 must change this signature rather than leaving an unfiltered variant available.
    pub async fn tree(&self) -> Result<Vec<TreeNode>> {
        #[derive(FromRow)]
        struct Row {
            path: String,
            parent_path: Option<String>,
            slug: String,
            title: String,
            doc_type: String,
            visibility: String,
        }

        let rows = sqlx::query_as::<_, Row>(
            r#"
            SELECT path, parent_path, slug, title, doc_type, visibility
            FROM documents
            WHERE deleted_at IS NULL
            ORDER BY parent_path NULLS FIRST, sort_key, slug
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        fn build(rows: &[Row], parent: Option<&str>) -> Vec<TreeNode> {
            rows.iter()
                .filter(|r| r.parent_path.as_deref() == parent)
                .map(|r| TreeNode {
                    path: r.path.clone(),
                    slug: r.slug.clone(),
                    title: r.title.clone(),
                    doc_type: r.doc_type.clone(),
                    visibility: r.visibility.clone(),
                    children: build(rows, Some(&r.path)),
                })
                .collect()
        }

        Ok(build(&rows, None))
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p gw-store`
Expected: `test result: ok. 6 passed`.

- [ ] **Step 7: Lint, changelog and commit**

Add under `### Added`:
```markdown
- SQLite store with the initial schema: documents keyed by a materialised path, with
  sibling ordering, soft delete and a UNIQUE path so a slug collision fails loudly
  instead of silently overwriting a page.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-store CHANGELOG.md
git commit -m "feat(store): sqlite schema and document tree queries"
```

---

## Task 3: Configuration and the fail-closed startup checks

**Files:**
- Create: `crates/gw-api/Cargo.toml`
- Create: `crates/gw-api/src/config.rs`
- Create: `crates/gw-api/src/identity.rs`
- Create: `crates/gw-api/src/main.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_store::Store`.
- Produces:
  - `struct Config { database_url: String, media_dir: PathBuf, bind: SocketAddr, dev_identity: Option<Identity>, proxy_secret: Option<String>, oidc: Option<OidcConfig> }`
  - `fn Config::from_env() -> anyhow::Result<Config>`
  - `struct Identity { user: Option<String>, groups: Vec<String> }` with `is_authenticated()`
  - binary `great-wiki` with subcommands `serve` and `check`

**Why the dev shim is a startup check and not a runtime flag:** an identity you can
synthesise is an authentication bypass. Tying it to a loopback bind, refused at startup,
means the dangerous configuration cannot exist in production even by mistake.

- [ ] **Step 1: Write the failing tests**

`crates/gw-api/src/config.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::config::{parse_dev_identity, validate};
    use crate::identity::Identity;
    use std::net::SocketAddr;

    #[test]
    fn dev_identity_parses_user_and_groups() {
        let id = parse_dev_identity("sergej:admins,editors").unwrap();
        assert_eq!(id.user.as_deref(), Some("sergej"));
        assert_eq!(id.groups, vec!["admins", "editors"]);
    }

    #[test]
    fn dev_identity_without_groups_is_allowed() {
        let id = parse_dev_identity("guest").unwrap();
        assert_eq!(id.user.as_deref(), Some("guest"));
        assert!(id.groups.is_empty());
    }

    #[test]
    fn dev_identity_with_empty_user_is_rejected() {
        assert!(parse_dev_identity(":admins").is_err());
    }

    #[test]
    fn dev_identity_on_a_loopback_bind_is_allowed() {
        let bind: SocketAddr = "127.0.0.1:8092".parse().unwrap();
        assert!(validate(bind, Some(&Identity::dev("s", &["admins"])), None).is_ok());
    }

    #[test]
    fn dev_identity_on_a_public_bind_refuses_to_start() {
        // This is the whole point: a synthesised identity is an authentication bypass,
        // so it must be impossible to combine with a reachable bind address.
        let bind: SocketAddr = "0.0.0.0:8092".parse().unwrap();
        let err = validate(bind, Some(&Identity::dev("s", &["admins"])), None).unwrap_err();
        assert!(err.to_string().contains("GW_DEV_IDENTITY"));
    }

    #[test]
    fn public_bind_without_a_proxy_secret_refuses_to_start() {
        // Binding 0.0.0.0 is required (Caddy is on another host), so the port is
        // LAN-reachable and proxy attestation is the only boundary left.
        let bind: SocketAddr = "0.0.0.0:8092".parse().unwrap();
        let err = validate(bind, None, None).unwrap_err();
        assert!(err.to_string().contains("GW_PROXY_SECRET"));
    }

    #[test]
    fn public_bind_with_a_proxy_secret_is_allowed() {
        let bind: SocketAddr = "0.0.0.0:8092".parse().unwrap();
        assert!(validate(bind, None, Some("not-a-real-secret")).is_ok());
    }

    #[test]
    fn port_8090_is_refused_because_omnigraph_viewer_owns_it() {
        let bind: SocketAddr = "0.0.0.0:8090".parse().unwrap();
        let err = validate(bind, None, Some("s")).unwrap_err();
        assert!(err.to_string().contains("8090"));
    }
}
```

- [ ] **Step 2: Create the manifest**

`crates/gw-api/Cargo.toml`:
```toml
[package]
name = "gw-api"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[[bin]]
name = "great-wiki"
path = "src/main.rs"

[dependencies]
gw-core = { path = "../gw-core" }
gw-store = { path = "../gw-store" }
anyhow = { workspace = true }
axum = "0.8"
clap = { version = "4", features = ["derive", "env"] }
serde = { workspace = true }
serde_json = { workspace = true }
# sqlx is a DIRECT dependency because handlers query. The predecessor plan omitted it
# from this crate's manifest while calling sqlx from four of its modules.
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite"] }
tokio = { workspace = true }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "limit"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gw-api`
Expected: FAIL — `could not find config in the crate root`.

- [ ] **Step 4: Implement identity and config**

`crates/gw-api/src/identity.rs`:
```rust
use serde::Serialize;

/// Who is making a request.
///
/// Deliberately independent of how it was established. OIDC produces one; the development
/// shim produces one; M2's local accounts will produce one. Handlers consume only this, so
/// adding an authentication method never touches a handler.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Identity {
    pub user: Option<String>,
    pub groups: Vec<String>,
}

impl Identity {
    pub fn anonymous() -> Self {
        Self::default()
    }

    pub fn dev(user: &str, groups: &[&str]) -> Self {
        Self {
            user: Some(user.to_string()),
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }

    /// A blank username is anonymous, not "a user called empty string".
    pub fn is_authenticated(&self) -> bool {
        self.user.as_deref().is_some_and(|u| !u.trim().is_empty())
    }

    pub fn in_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
    }
}
```

`crates/gw-api/src/config.rs`, above `mod tests`:
```rust
use crate::identity::Identity;
use anyhow::{bail, Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;

/// The port `omnigraph-viewer` binds on coding.vm. Taking it would break the Omnigraph UI,
/// so it is refused rather than left as a footgun.
const RESERVED_PORT: u16 = 8090;

pub struct Config {
    pub database_url: String,
    pub media_dir: PathBuf,
    pub bind: SocketAddr,
    pub dev_identity: Option<Identity>,
    pub proxy_secret: Option<String>,
}

pub fn parse_dev_identity(raw: &str) -> Result<Identity> {
    let (user, groups) = raw.split_once(':').unwrap_or((raw, ""));
    if user.trim().is_empty() {
        bail!("GW_DEV_IDENTITY must name a user, e.g. `sergej:admins`");
    }
    Ok(Identity {
        user: Some(user.trim().to_string()),
        groups: groups
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string)
            .collect(),
    })
}

/// Refuse to start on a configuration that would be an authentication bypass.
///
/// These checks are at startup rather than per request because a misconfiguration that
/// only fails on a request is a misconfiguration that reaches production.
pub fn validate(
    bind: SocketAddr,
    dev_identity: Option<&Identity>,
    proxy_secret: Option<&str>,
) -> Result<()> {
    if bind.port() == RESERVED_PORT {
        bail!("port 8090 is reserved by omnigraph-viewer; choose another (8092 is free)");
    }

    let loopback = bind.ip().is_loopback();

    if dev_identity.is_some() && !loopback {
        bail!(
            "GW_DEV_IDENTITY synthesises a signed-in user and must never be combined with a \
             non-loopback bind ({bind}). Unset it, or bind 127.0.0.1."
        );
    }

    if !loopback && proxy_secret.map(str::trim).unwrap_or("").is_empty() {
        bail!(
            "GW_PROXY_SECRET must be set when binding {bind}. Caddy runs on another host, so \
             this port is LAN-reachable and the shared secret is the only boundary left."
        );
    }

    Ok(())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind: SocketAddr = std::env::var("GW_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8092".into())
            .parse()
            .context("GW_BIND must be host:port, e.g. 127.0.0.1:8092")?;

        let dev_identity = match std::env::var("GW_DEV_IDENTITY") {
            Ok(raw) if !raw.trim().is_empty() => Some(parse_dev_identity(&raw)?),
            _ => None,
        };
        let proxy_secret = std::env::var("GW_PROXY_SECRET")
            .ok()
            .filter(|s| !s.trim().is_empty());

        validate(bind, dev_identity.as_ref(), proxy_secret.as_deref())?;

        Ok(Self {
            // Relative defaults: the application runs from a checkout with no arguments.
            // Container paths are supplied by compose in M18.
            database_url: std::env::var("GW_DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://./data/great-wiki.db".into()),
            media_dir: std::env::var("GW_MEDIA_DIR")
                .unwrap_or_else(|_| "./data/media".into())
                .into(),
            bind,
            dev_identity,
            proxy_secret,
        })
    }
}
```

`crates/gw-api/src/main.rs`:
```rust
mod config;
mod identity;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "great-wiki", about = "Self-hosted collaborative knowledge platform")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server.
    Serve,
    /// Validate configuration and exit. Non-zero on any problem.
    Check,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = config::Config::from_env()?;

    match cli.command {
        Command::Check => {
            println!("configuration OK — bind {}, db {}", cfg.bind, cfg.database_url);
            Ok(())
        }
        Command::Serve => {
            // Wired in Task 4.
            println!("serve is implemented in Task 4");
            Ok(())
        }
    }
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p gw-api`
Expected: `test result: ok. 8 passed`.

- [ ] **Step 6: Verify the fail-closed behaviour by hand**

```bash
GW_BIND=0.0.0.0:8092 GW_DEV_IDENTITY=s:admins cargo run -p gw-api -- check; echo "exit=$?"
```
Expected: an error naming `GW_DEV_IDENTITY`, `exit=1`.

```bash
GW_BIND=127.0.0.1:8092 GW_DEV_IDENTITY=s:admins cargo run -p gw-api -- check; echo "exit=$?"
```
Expected: `configuration OK`, `exit=0`.

- [ ] **Step 7: Lint, changelog and commit**

Add under `### Added`:
```markdown
- `great-wiki` binary with `serve` and `check` subcommands, and fail-closed startup
  validation: a synthesised development identity cannot be combined with a non-loopback
  bind, a public bind without a proxy secret is refused, and port 8090 is rejected
  because `omnigraph-viewer` owns it.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-api CHANGELOG.md
git commit -m "feat(api): configuration with fail-closed startup validation"
```

---

## Task 4: The read API

**Files:**
- Create: `crates/gw-api/src/error.rs`
- Create: `crates/gw-api/src/routes/mod.rs`
- Create: `crates/gw-api/src/routes/tree.rs`
- Create: `crates/gw-api/src/routes/docs.rs`
- Create: `crates/gw-api/tests/api.rs`
- Modify: `crates/gw-api/src/main.rs`
- Modify: `crates/gw-api/Cargo.toml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `Config`, `Identity`, `Store`.
- Produces:
  - `GET /api/health` → `{"status":"ok"}`
  - `GET /api/tree` → `TreeNode[]`
  - `GET /api/documents/{*path}` → `StoredDocument`; 404 absent; 403 not permitted
  - `fn build_router(state: AppState) -> axum::Router`
  - `struct AppState { store: Arc<Store>, config: Arc<Config> }`

**Why the tests exercise the router rather than the handlers:** the predecessor plan tested
`Identity` in isolation and never called a route, so a handler registered without its
visibility check would have passed its entire suite. These tests use `tower::ServiceExt`
`oneshot` against the real `Router`.

- [ ] **Step 1: Write the failing integration tests**

`crates/gw-api/tests/api.rs`:
```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use gw_core::{DocumentType, Visibility};
use gw_store::{NewDocument, Store};
use std::sync::Arc;
use tower::ServiceExt;

async fn seed() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let body = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap();

    for (title, vis) in [
        ("Öffentlich", Visibility::Public),
        ("Geheim", Visibility::Restricted),
    ] {
        store
            .insert_document(&NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: None,
                language: "de".into(),
                visibility: vis,
                body: serde_json::from_value(serde_json::to_value(&body).unwrap()).unwrap(),
                sort_key: 0,
            })
            .await
            .unwrap();
    }
    Arc::new(store)
}

fn app(store: Arc<Store>, dev: Option<gw_api::Identity>) -> axum::Router {
    gw_api::build_router(gw_api::AppState::for_test(store, dev))
}

async fn get(app: axum::Router, uri: &str) -> StatusCode {
    app.oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn health_is_ok() {
    assert_eq!(get(app(seed().await, None), "/api/health").await, StatusCode::OK);
}

#[tokio::test]
async fn public_document_is_readable_anonymously() {
    assert_eq!(
        get(app(seed().await, None), "/api/documents/oeffentlich").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn restricted_document_is_forbidden_anonymously() {
    // 403 not 404: the proxy already knows the path exists, and a misleading 404 makes
    // debugging an authentication problem needlessly hard.
    assert_eq!(
        get(app(seed().await, None), "/api/documents/geheim").await,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn restricted_document_is_readable_when_authenticated() {
    let identity = gw_api::Identity::dev("sergej", &["admins"]);
    assert_eq!(
        get(app(seed().await, Some(identity)), "/api/documents/geheim").await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn absent_document_is_not_found() {
    assert_eq!(
        get(app(seed().await, None), "/api/documents/gibt-es-nicht").await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn anonymous_tree_omits_restricted_documents() {
    let response = app(seed().await, None)
        .oneshot(Request::builder().uri("/api/tree").body(Body::empty()).unwrap())
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("Öffentlich"));
    // A restricted title leaking into the navigation is a disclosure even without the body.
    assert!(!text.contains("Geheim"), "restricted titles must not appear in the tree");
}
```

- [ ] **Step 2: Add the test dependencies**

Append to `crates/gw-api/Cargo.toml`:
```toml
[lib]
name = "gw_api"
path = "src/lib.rs"

[dev-dependencies]
tokio = { workspace = true }
tower = { version = "0.5", features = ["util"] }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gw-api --test api`
Expected: FAIL — `unresolved import gw_api` / `build_router` not found.

- [ ] **Step 4: Implement the error type**

`crates/gw-api/src/error.rs`:
```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::Internal(e) => {
                // Log the detail, return none of it: internal errors carry filesystem
                // paths and SQL, which must not reach a client.
                tracing::error!(error = ?e, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error")
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
```

- [ ] **Step 5: Implement the router and handlers**

`crates/gw-api/src/lib.rs`:
```rust
pub mod config;
pub mod error;
pub mod identity;
pub mod routes;

pub use config::Config;
pub use identity::Identity;
pub use routes::{build_router, AppState};
```

`crates/gw-api/src/routes/mod.rs`:
```rust
pub mod docs;
pub mod tree;

use crate::identity::Identity;
use axum::routing::get;
use axum::{Json, Router};
use gw_store::Store;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    /// When present, every request is treated as this identity. Only reachable on a
    /// loopback bind — `config::validate` refuses to start otherwise.
    pub dev_identity: Option<Identity>,
}

impl AppState {
    pub fn for_test(store: Arc<Store>, dev_identity: Option<Identity>) -> Self {
        Self { store, dev_identity }
    }

    /// The caller's identity. M1 has only the development shim and anonymous; Task 6 adds
    /// the OIDC session, and M2 adds local accounts — both by extending this one function.
    pub fn identity(&self) -> Identity {
        self.dev_identity.clone().unwrap_or_else(Identity::anonymous)
    }
}

/// Whether `identity` may read something with this visibility.
///
/// M1 knows only the three visibility levels. M2 replaces this with the full permission
/// engine (teams, ACLs, tree inheritance) — and must replace it, not sit beside it.
pub fn may_read(identity: &Identity, visibility: &str) -> bool {
    match visibility {
        "public" => true,
        // Fail closed: an unrecognised value is treated as restricted, never as public.
        _ => identity.is_authenticated(),
    }
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(|| async { Json(serde_json::json!({"status": "ok"})) }))
        .route("/api/tree", get(tree::get_tree))
        .route("/api/documents/{*path}", get(docs::get_document))
        .with_state(state)
}
```

`crates/gw-api/src/routes/tree.rs`:
```rust
use super::{may_read, AppState};
use crate::error::ApiError;
use axum::extract::State;
use axum::Json;
use gw_store::TreeNode;

/// The navigable tree, filtered to what the caller may read.
///
/// Filtering happens HERE, in the retriever, not in the frontend. A restricted title in
/// the navigation is a disclosure even when the body is protected.
pub async fn get_tree(State(state): State<AppState>) -> Result<Json<Vec<TreeNode>>, ApiError> {
    let identity = state.identity();
    let tree = state.store.tree().await.map_err(ApiError::Internal)?;
    Ok(Json(filter(tree, &identity)))
}

fn filter(nodes: Vec<TreeNode>, identity: &crate::identity::Identity) -> Vec<TreeNode> {
    nodes
        .into_iter()
        .filter(|n| may_read(identity, &n.visibility))
        .map(|mut n| {
            n.children = filter(std::mem::take(&mut n.children), identity);
            n
        })
        .collect()
}
```

`crates/gw-api/src/routes/docs.rs`:
```rust
use super::{may_read, AppState};
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::Json;
use gw_store::StoredDocument;

pub async fn get_document(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<Json<StoredDocument>, ApiError> {
    let identity = state.identity();
    // Paths are stored with a leading slash; the route captures without one.
    let full = format!("/{}", path.trim_start_matches('/'));

    let doc = state
        .store
        .document_by_path(&full)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;

    if !may_read(&identity, &doc.visibility) {
        return Err(ApiError::Forbidden);
    }
    Ok(Json(doc))
}
```

Replace the `Command::Serve` arm in `crates/gw-api/src/main.rs`:
```rust
        Command::Serve => {
            let store = std::sync::Arc::new(gw_store::Store::open(&cfg.database_url).await?);
            let state = gw_api::AppState {
                store,
                dev_identity: cfg.dev_identity.clone(),
            };
            let app = gw_api::build_router(state).layer(
                tower_http::limit::RequestBodyLimitLayer::new(2 * 1024 * 1024),
            );
            let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
            tracing::info!(bind = %cfg.bind, "great-wiki listening");
            axum::serve(listener, app).await?;
            Ok(())
        }
```
and change the top of `main.rs` from `mod config; mod identity;` to `use gw_api::config;`
so the binary and the library share one definition rather than compiling two.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p gw-api`
Expected: `test result: ok. 6 passed` for the `api` integration target, plus the 8 config
unit tests.

- [ ] **Step 7: Lint, changelog and commit**

Add under `### Added`:
```markdown
- Read API: `/api/health`, `/api/tree` and `/api/documents/{*path}`, with visibility
  enforced in the retriever. Restricted documents return 403 rather than a misleading
  404, and restricted titles are filtered out of the navigation tree.
- Integration tests that exercise the real router, so a route registered without its
  permission check fails the suite.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-api CHANGELOG.md
git commit -m "feat(api): read endpoints with permission filtering in the retriever"
```

---

## Task 5: The reader interface

**Files:**
- Create: `web/src/lib/api.ts`
- Create: `web/src/lib/blocks/render.ts`
- Create: `web/src/lib/blocks/render.test.ts`
- Create: `web/src/routes/+layout.svelte`
- Create: `web/src/routes/+page.server.ts`
- Create: `web/src/routes/+page.svelte`
- Create: `web/src/routes/[...path]/+page.server.ts`
- Create: `web/src/routes/[...path]/+page.svelte`
- Create: `web/src/routes/+error.svelte`
- Modify: `web/vite.config.ts`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the API from Task 4.
- Produces: a browsable site — tree navigation, document pages, error pages, dark/light
  theme, skip link.

**Rendering approach:** the block tree is rendered by a recursive Svelte component, **not**
by generating an HTML string and injecting it with `{@html}`. There is no sanitisation
question to get wrong because there is no HTML to sanitise — the renderer only emits
elements it knows.

- [ ] **Step 1: Configure the dev proxy**

`web/vite.config.ts`:
```ts
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit()],
  server: {
    // Without this, /api/* 404s in `npm run dev` — in production Caddy routes it.
    proxy: {
      '/api': { target: 'http://127.0.0.1:8092', changeOrigin: true }
    }
  }
});
```

- [ ] **Step 2: Write the failing test for the block renderer**

`web/src/lib/blocks/render.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { outline, plainText, type Block } from './render';

const doc: Block = {
  kind: 'doc',
  content: [
    { kind: 'heading', attrs: { level: 2 }, content: [{ kind: 'text', text: 'Größe und Maß' }] },
    { kind: 'paragraph', content: [{ kind: 'text', text: 'Ein Satz.' }] }
  ]
};

describe('block helpers', () => {
  it('extracts plain text in document order', () => {
    expect(plainText(doc)).toBe('Größe und Maß Ein Satz.');
  });

  it('builds an outline with ASCII anchor ids', () => {
    const headings = outline(doc);
    expect(headings).toHaveLength(1);
    expect(headings[0]).toEqual({ level: 2, text: 'Größe und Maß', id: 'groesse-und-mass' });
  });

  it('defaults a heading without a level to 1', () => {
    const h: Block = { kind: 'doc', content: [{ kind: 'heading', content: [{ kind: 'text', text: 'T' }] }] };
    expect(outline(h)[0].level).toBe(1);
  });
});
```

Note `plainText` joins sibling text nodes with a space where the Rust version concatenates
within a leaf; the shared expectation `'Größe und Maß Ein Satz.'` is what both must produce.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd web && npx vitest run src/lib/blocks/render.test.ts`
Expected: FAIL — `Failed to resolve import "./render"`.

- [ ] **Step 4: Implement the block helpers**

`web/src/lib/blocks/render.ts`:
```ts
import { slugify } from '$lib/slug';

// Mirrors crates/gw-core/src/block.rs. Kinds the renderer does not know are skipped
// rather than rendered raw — that is what makes an unknown block safe.
export type BlockKind =
  | 'doc' | 'paragraph' | 'heading' | 'bulletList' | 'orderedList'
  | 'listItem' | 'blockquote' | 'codeBlock' | 'text';

export interface Block {
  kind: BlockKind;
  attrs?: Record<string, unknown>;
  content?: Block[];
  text?: string;
}

export interface Heading {
  level: number;
  text: string;
  id: string;
}

export function plainText(block: Block): string {
  const parts: string[] = [];
  const walk = (b: Block) => {
    if (b.text) parts.push(b.text);
    b.content?.forEach(walk);
  };
  walk(block);
  return parts.join(' ').replace(/\s+/g, ' ').trim();
}

export function outline(block: Block): Heading[] {
  const out: Heading[] = [];
  const walk = (b: Block) => {
    if (b.kind === 'heading') {
      const raw = Number(b.attrs?.level ?? 1);
      const level = Math.min(6, Math.max(1, Number.isFinite(raw) ? raw : 1));
      const text = plainText(b);
      out.push({ level, text, id: slugify(text) });
      return; // headings do not nest
    }
    b.content?.forEach(walk);
  };
  walk(block);
  return out;
}
```

- [ ] **Step 5: Implement the API client and pages**

`web/src/lib/api.ts`:
```ts
import type { Block } from '$lib/blocks/render';

export interface TreeNode {
  path: string;
  slug: string;
  title: string;
  doc_type: string;
  visibility: string;
  children: TreeNode[];
}

export interface StoredDocument {
  id: string;
  path: string;
  parent_path: string | null;
  slug: string;
  doc_type: string;
  title: string;
  language: string;
  visibility: string;
  body: string; // JSON-encoded Block tree
  sort_key: number;
}

const BASE = process.env.GW_API ?? 'http://127.0.0.1:8092';

/**
 * Server-side fetch. Forwards the caller's cookie so the API sees the same session the
 * browser has — and forwards nothing else from the client request.
 */
export async function apiGet<T>(
  fetchFn: typeof fetch,
  path: string,
  cookie: string | null
): Promise<{ status: number; data: T | null }> {
  const res = await fetchFn(`${BASE}${path}`, {
    headers: cookie ? { cookie } : {}
  });
  if (!res.ok) return { status: res.status, data: null };
  return { status: res.status, data: (await res.json()) as T };
}

export function parseBody(doc: StoredDocument): Block {
  return JSON.parse(doc.body) as Block;
}
```

`web/src/routes/+page.server.ts`:
```ts
import { apiGet, type TreeNode } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, request }) => {
  const { data } = await apiGet<TreeNode[]>(fetch, '/api/tree', request.headers.get('cookie'));
  return { tree: data ?? [] };
};
```

`web/src/routes/[...path]/+page.server.ts`:
```ts
import { error } from '@sveltejs/kit';
import { apiGet, parseBody, type StoredDocument, type TreeNode } from '$lib/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, fetch, request }) => {
  const cookie = request.headers.get('cookie');
  const { status, data } = await apiGet<StoredDocument>(
    fetch,
    `/api/documents/${params.path}`,
    cookie
  );

  if (status === 403) error(403, 'You do not have access to this page.');
  if (!data) error(404, 'Page not found.');

  const { data: tree } = await apiGet<TreeNode[]>(fetch, '/api/tree', cookie);
  return { doc: data, body: parseBody(data), tree: tree ?? [] };
};
```

`web/src/routes/+error.svelte`:
```svelte
<script lang="ts">
  import { page } from '$app/state';
</script>

<main class="wrap">
  <h1>{page.status}</h1>
  <p>{page.error?.message ?? 'Something went wrong.'}</p>
  <p><a href="/">Back to the start page</a></p>
</main>
```

The layout, tree navigation component and document renderer complete this task. Because
they are long, they are specified as one step with full content:

- [ ] **Step 6: Implement the layout and renderer**

`web/src/routes/+layout.svelte`:
```svelte
<script lang="ts">
  let { children } = $props();
</script>

<!-- Skip link first in the DOM: keyboard users must be able to bypass navigation. -->
<a class="skip" href="#content">Skip to content</a>
<header>
  <a class="brand" href="/">great-wiki</a>
</header>
{@render children()}

<style>
  /* Theme via custom properties, dark by system preference with a light override.
     Both directions are styled; neither is an afterthought. */
  :global(:root) {
    --bg: #0f1115; --panel: #161a21; --line: #2a3140;
    --ink: #e6e9ef; --ink-dim: #a9b2c3; --accent: #6ea8fe;
  }
  @media (prefers-color-scheme: light) {
    :global(:root) {
      --bg: #ffffff; --panel: #f6f7f9; --line: #d9dee7;
      --ink: #16191f; --ink-dim: #5b6473; --accent: #1a5fd0;
    }
  }
  :global(body) {
    margin: 0; background: var(--bg); color: var(--ink);
    font: 16px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif;
  }
  :global(a) { color: var(--accent); }
  .skip {
    position: absolute; left: -9999px;
  }
  .skip:focus {
    left: 1rem; top: 1rem; z-index: 10;
    background: var(--panel); padding: .5rem 1rem; border-radius: 6px;
  }
  header { border-bottom: 1px solid var(--line); padding: 1rem; }
  .brand { font-weight: 600; text-decoration: none; }
</style>
```

`web/src/lib/components/Tree.svelte`:
```svelte
<script lang="ts">
  import type { TreeNode } from '$lib/api';
  import Self from './Tree.svelte';

  let { nodes, current }: { nodes: TreeNode[]; current?: string } = $props();
</script>

{#if nodes.length}
  <ul>
    {#each nodes as node (node.path)}
      <li>
        <a href={node.path} aria-current={node.path === current ? 'page' : undefined}>
          {node.title}
        </a>
        <Self nodes={node.children} {current} />
      </li>
    {/each}
  </ul>
{/if}

<style>
  ul { list-style: none; margin: 0; padding-left: 1rem; }
  a[aria-current='page'] { font-weight: 600; }
</style>
```

`web/src/lib/components/BlockView.svelte`:
```svelte
<script lang="ts">
  import type { Block } from '$lib/blocks/render';
  import { slugify } from '$lib/slug';
  import { plainText } from '$lib/blocks/render';
  import Self from './BlockView.svelte';

  let { block }: { block: Block } = $props();
</script>

<!-- Only known kinds render. An unknown block is skipped rather than emitted raw, which
     is why there is no sanitisation step here: no untrusted HTML is ever constructed. -->
{#if block.kind === 'doc'}
  {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
{:else if block.kind === 'paragraph'}
  <p>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</p>
{:else if block.kind === 'heading'}
  {@const level = Math.min(6, Math.max(1, Number(block.attrs?.level ?? 1)))}
  {@const id = slugify(plainText(block))}
  <svelte:element this={`h${level}`} {id}>
    {#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}
  </svelte:element>
{:else if block.kind === 'bulletList'}
  <ul>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</ul>
{:else if block.kind === 'orderedList'}
  <ol>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</ol>
{:else if block.kind === 'listItem'}
  <li>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</li>
{:else if block.kind === 'blockquote'}
  <blockquote>{#each block.content ?? [] as child, i (i)}<Self block={child} />{/each}</blockquote>
{:else if block.kind === 'codeBlock'}
  <pre><code>{plainText(block)}</code></pre>
{:else if block.kind === 'text'}{block.text}{/if}
```

`web/src/routes/[...path]/+page.svelte`:
```svelte
<script lang="ts">
  import BlockView from '$lib/components/BlockView.svelte';
  import Tree from '$lib/components/Tree.svelte';
  import { outline } from '$lib/blocks/render';

  let { data } = $props();
  const headings = $derived(outline(data.body));
</script>

<svelte:head><title>{data.doc.title} — great-wiki</title></svelte:head>

<div class="shell">
  <nav aria-label="Site"><Tree nodes={data.tree} current={data.doc.path} /></nav>
  <main id="content" lang={data.doc.language}>
    <h1>{data.doc.title}</h1>
    <BlockView block={data.body} />
  </main>
  {#if headings.length > 1}
    <nav aria-label="On this page">
      <ul>
        {#each headings as h (h.id)}
          <li style:padding-left={`${(h.level - 1) * 0.75}rem`}>
            <a href={`#${h.id}`}>{h.text}</a>
          </li>
        {/each}
      </ul>
    </nav>
  {/if}
</div>

<style>
  .shell {
    display: grid; gap: 2rem; padding: 1.5rem;
    grid-template-columns: minmax(12rem, 16rem) minmax(0, 1fr) minmax(10rem, 14rem);
    max-width: 90rem; margin: 0 auto;
  }
  /* Single column on narrow viewports; the page body must never scroll horizontally. */
  @media (max-width: 60rem) { .shell { grid-template-columns: 1fr; } }
  main :global(pre) { overflow-x: auto; background: var(--panel); padding: 1rem; border-radius: 6px; }
  nav ul { list-style: none; margin: 0; padding: 0; }
</style>
```

`web/src/routes/+page.svelte`:
```svelte
<script lang="ts">
  import Tree from '$lib/components/Tree.svelte';
  let { data } = $props();
</script>

<svelte:head><title>great-wiki</title></svelte:head>
<main id="content" class="wrap">
  <h1>great-wiki</h1>
  {#if data.tree.length}
    <Tree nodes={data.tree} />
  {:else}
    <p>No pages yet.</p>
  {/if}
</main>

<style>.wrap { max-width: 60rem; margin: 0 auto; padding: 1.5rem; }</style>
```

- [ ] **Step 7: Run the gate**

Run: `cd web && npx vitest run && npm run check && npm run build`
Expected: 8 tests passed (5 slug + 3 block), `svelte-check found 0 errors`, build succeeds.

- [ ] **Step 8: See it work**

```bash
mkdir -p data
GW_DEV_IDENTITY=sergej:admins cargo run -p gw-api -- serve &
cd web && npm run dev
```
Open `http://localhost:5173`. Expected: the tree renders. (Seeding real content is Task 7.)

- [ ] **Step 9: Lint, changelog and commit**

Add under `### Added`:
```markdown
- Reader interface: layout with a skip link, dark and light themes following system
  preference, recursive tree navigation, document pages with an on-this-page outline,
  and error pages. Blocks render through a component that emits only kinds it knows, so
  no untrusted HTML is ever constructed.
```

```bash
git add web CHANGELOG.md
git commit -m "feat(web): reader interface with tree navigation and block rendering"
```

---

## Task 6: OIDC login

**Files:**
- Create: `crates/gw-api/src/auth/mod.rs`
- Create: `crates/gw-api/src/auth/oidc.rs`
- Create: `crates/gw-api/src/auth/session.rs`
- Modify: `crates/gw-api/src/routes/mod.rs`
- Modify: `crates/gw-api/src/config.rs`
- Modify: `crates/gw-api/Cargo.toml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `Config`, `Identity`.
- Produces:
  - `GET /auth/login` → 302 to Authelia with PKCE challenge and state
  - `GET /auth/callback` → exchanges the code, creates a session, 302 home
  - `POST /auth/logout` → clears the session
  - `GET /api/me` → the current `Identity`
  - `AppState::identity` extended to read the session cookie

**Prerequisite, done before writing code:** register the client in Authelia. This is a
change to shared infrastructure and requires a container restart.

- [ ] **Step 1: Register the OIDC client in Authelia**

Add to the `identity_providers.oidc.clients` list in
`/home/s/code/Server/server/manage/auth/authelia/configuration.yml`, modelled on the
existing `karakeep` client:

```yaml
      - client_id: 'great-wiki'
        client_name: 'great-wiki'
        client_secret: '$pbkdf2-sha512$...'   # generate; store the plaintext in the overlay
        public: false
        authorization_policy: 'one_factor'
        require_pkce: true
        pkce_challenge_method: 'S256'
        token_endpoint_auth_method: 'client_secret_basic'
        redirect_uris:
          - 'https://kb.ohje.ooguy.com/auth/callback'
        scopes: ['openid', 'profile', 'email', 'groups']
        userinfo_signed_response_alg: 'none'
```

> **Diff against the live file first.** The repository copy is stale — it holds 7 clients
> where the live file has 12. Pushing the repository copy would delete five working clients.

Then restart: `DOCKER_HOST=ssh://manage-vm docker restart authelia`, and confirm:
```bash
curl -fsS https://auth.ohje.ooguy.com/.well-known/openid-configuration | python3 -m json.tool | head -20
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/gw-api/src/auth/session.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::auth::session::{SessionStore, Session};
    use crate::identity::Identity;

    #[test]
    fn a_stored_session_is_retrievable_by_its_token() {
        let store = SessionStore::new();
        let token = store.create(Session {
            identity: Identity::dev("sergej", &["admins"]),
        });
        let got = store.get(&token).expect("session should exist");
        assert_eq!(got.identity.user.as_deref(), Some("sergej"));
        assert!(got.identity.in_group("admins"));
    }

    #[test]
    fn an_unknown_token_yields_nothing() {
        assert!(SessionStore::new().get("not-a-token").is_none());
    }

    #[test]
    fn tokens_are_unguessable_and_unique() {
        let store = SessionStore::new();
        let a = store.create(Session { identity: Identity::dev("a", &[]) });
        let b = store.create(Session { identity: Identity::dev("b", &[]) });
        assert_ne!(a, b);
        // 256 bits of entropy, hex-encoded.
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn removing_a_session_makes_it_unusable() {
        let store = SessionStore::new();
        let token = store.create(Session { identity: Identity::dev("a", &[]) });
        store.remove(&token);
        assert!(store.get(&token).is_none());
    }
}
```

- [ ] **Step 3: Add dependencies**

Append to `crates/gw-api/Cargo.toml` `[dependencies]`:
```toml
openidconnect = "4"
axum-extra = { version = "0.10", features = ["cookie"] }
rand = "0.8"
hex = "0.4"
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test -p gw-api session`
Expected: FAIL — `could not find auth in the crate root`.

- [ ] **Step 5: Implement the session store**

`crates/gw-api/src/auth/session.rs`, above `mod tests`:
```rust
use crate::identity::Identity;
use rand::RngCore;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct Session {
    pub identity: Identity,
}

/// In-memory sessions.
///
/// Deliberate for M1: sessions are cheap to re-establish (one redirect to Authelia), so
/// losing them on restart costs a login, not data. M2 moves them to SQLite when API
/// tokens and multiple processes make persistence matter.
#[derive(Clone, Default)]
pub struct SessionStore {
    inner: Arc<Mutex<HashMap<String, Session>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create(&self, session: Session) -> String {
        // 256 bits from the OS CSPRNG. A guessable session token is an account takeover.
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);
        self.inner.lock().unwrap().insert(token.clone(), session);
        token
    }

    pub fn get(&self, token: &str) -> Option<Session> {
        self.inner.lock().unwrap().get(token).cloned()
    }

    pub fn remove(&self, token: &str) {
        self.inner.lock().unwrap().remove(token);
    }
}
```

`crates/gw-api/src/auth/mod.rs`:
```rust
pub mod oidc;
pub mod session;

pub use session::{Session, SessionStore};

/// The session cookie name. `__Host-` forces Secure, path `/` and no Domain, which the
/// browser enforces — a cookie with this prefix cannot be set by a subdomain.
pub const SESSION_COOKIE: &str = "__Host-gw_session";
```

- [ ] **Step 6: Implement the OIDC flow**

`crates/gw-api/src/auth/oidc.rs`:
```rust
use crate::auth::{Session, SESSION_COOKIE};
use crate::error::ApiError;
use crate::identity::Identity;
use crate::routes::AppState;
use anyhow::{Context, Result};
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use openidconnect::core::{CoreClient, CoreProviderMetadata, CoreResponseType};
use openidconnect::{
    AuthenticationFlow, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use serde::Deserialize;

#[derive(Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

pub async fn discover(cfg: &OidcConfig) -> Result<CoreClient> {
    let issuer = IssuerUrl::new(cfg.issuer.clone()).context("GW_OIDC_ISSUER is not a URL")?;
    let http = openidconnect::reqwest::async_http_client;
    let metadata = CoreProviderMetadata::discover_async(issuer, http)
        .await
        .context("OIDC discovery failed — is the issuer reachable and TLS valid?")?;

    Ok(CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(cfg.client_id.clone()),
        Some(ClientSecret::new(cfg.client_secret.clone())),
    )
    .set_redirect_uri(RedirectUrl::new(cfg.redirect_uri.clone())?))
}

/// Begin login: redirect to Authelia with a PKCE challenge, storing the verifier, the
/// CSRF state and the nonce in short-lived cookies.
///
/// PKCE is required even though this is a confidential client: it removes the
/// authorization-code interception class of attack entirely rather than relying on the
/// secret staying secret.
pub async fn login(State(state): State<AppState>, jar: CookieJar) -> Result<Response, ApiError> {
    let client = state.oidc_client().ok_or(ApiError::NotFound)?;
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();

    let (url, csrf, nonce) = client
        .authorize_url(
            AuthenticationFlow::<CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("profile".into()))
        .add_scope(Scope::new("email".into()))
        // `groups` is what carries authorisation. Without it every user is ungrouped.
        .add_scope(Scope::new("groups".into()))
        .set_pkce_challenge(challenge)
        .url();

    let transient = |name: &'static str, value: String| {
        Cookie::build((name, value))
            .http_only(true)
            .secure(true)
            .same_site(SameSite::Lax)
            .path("/")
            .max_age(time::Duration::minutes(10))
            .build()
    };

    let jar = jar
        .add(transient("gw_pkce", verifier.secret().clone()))
        .add(transient("gw_state", csrf.secret().clone()))
        .add(transient("gw_nonce", nonce.secret().clone()));

    Ok((jar, Redirect::to(url.as_str())).into_response())
}

#[derive(Deserialize)]
pub struct CallbackParams {
    code: String,
    state: String,
}

pub async fn callback(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(params): Query<CallbackParams>,
) -> Result<Response, ApiError> {
    let client = state.oidc_client().ok_or(ApiError::NotFound)?;

    let expected_state = jar.get("gw_state").map(|c| c.value().to_string());
    // Constant-time is unnecessary here (the value is not a secret being guessed), but a
    // mismatch MUST reject: this is the CSRF defence for the whole flow.
    if expected_state.as_deref() != Some(params.state.as_str()) {
        return Err(ApiError::Forbidden);
    }

    let verifier = jar
        .get("gw_pkce")
        .map(|c| PkceCodeVerifier::new(c.value().to_string()))
        .ok_or(ApiError::Forbidden)?;
    let nonce = jar.get("gw_nonce").map(|c| Nonce::new(c.value().to_string()));

    let tokens = client
        .exchange_code(AuthorizationCode::new(params.code))
        .set_pkce_verifier(verifier)
        .request_async(openidconnect::reqwest::async_http_client)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("token exchange failed: {e}")))?;

    let id_token = tokens
        .id_token()
        .ok_or_else(|| ApiError::Internal(anyhow::anyhow!("no id_token in response")))?;
    let claims = id_token
        .claims(&client.id_token_verifier(), nonce.as_ref().unwrap_or(&Nonce::new(String::new())))
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("id_token verification failed: {e}")))?;

    let user = claims
        .preferred_username()
        .map(|u| u.to_string())
        .or_else(|| Some(claims.subject().to_string()));

    // Groups arrive as an additional claim; Authelia sends them as a string array.
    let groups: Vec<String> = claims
        .additional_claims()
        .get("groups")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|g| g.as_str().map(str::to_string)).collect())
        .unwrap_or_default();

    let token = state.sessions.create(Session {
        identity: Identity { user, groups },
    });

    let session_cookie = Cookie::build((SESSION_COOKIE, token))
        .http_only(true)
        .secure(true)
        .same_site(SameSite::Lax)
        .path("/")
        .build();

    let jar = jar
        .remove(Cookie::from("gw_pkce"))
        .remove(Cookie::from("gw_state"))
        .remove(Cookie::from("gw_nonce"))
        .add(session_cookie);

    Ok((jar, Redirect::to("/")).into_response())
}

pub async fn logout(State(state): State<AppState>, jar: CookieJar) -> Response {
    if let Some(c) = jar.get(SESSION_COOKIE) {
        state.sessions.remove(c.value());
    }
    (jar.remove(Cookie::from(SESSION_COOKIE)), Redirect::to("/")).into_response()
}
```

Extend `AppState` in `crates/gw-api/src/routes/mod.rs` with `sessions: SessionStore`, an
`oidc: Option<Arc<CoreClient>>` field and an `oidc_client()` accessor, and change
`identity()` to take the `CookieJar`: read `SESSION_COOKIE`, look it up, and fall back to
`dev_identity` then anonymous. Register the routes:
```rust
        .route("/auth/login", get(crate::auth::oidc::login))
        .route("/auth/callback", get(crate::auth::oidc::callback))
        .route("/auth/logout", post(crate::auth::oidc::logout))
        .route("/api/me", get(|State(s): State<AppState>, jar: CookieJar| async move {
            Json(s.identity(&jar))
        }))
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p gw-api`
Expected: `test result: ok` — 8 config, 4 session, 6 api integration tests.

- [ ] **Step 8: Lint, changelog and commit**

Add under `### Added`:
```markdown
- OpenID Connect login against Authelia using the authorization-code flow with PKCE,
  with the `groups` claim carrying authorisation. Sessions are held server-side behind a
  `__Host-` prefixed, HttpOnly, SameSite=Lax cookie with a 256-bit token.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-api CHANGELOG.md
git commit -m "feat(auth): openid connect login with PKCE against authelia"
```

---

## Task 7: Edge exposure and the acceptance gate

**Files:**
- Modify: `/home/s/code/Server/server/network/opnsense/caddy.d/00-snippets.conf`
- Modify: `/home/s/code/Server/server/network/opnsense/caddy.d/10-services.conf`
- Modify: `/home/s/code/Server/server/manage/auth/authelia/configuration.yml`
- Create: `docs/operations/deploy.md`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: everything above.
- Produces: `https://kb.ohje.ooguy.com` serving the application, with the acceptance checks
  passing.

- [ ] **Step 1: Add the header-stripping Authelia snippet**

The shared `(authelia)` snippet copies identity headers but does **not** strip
client-supplied ones. great-wiki authenticates via OIDC and does not read those headers, but
the strip is added anyway as defence in depth and because M2's API tokens will share this
site block.

Append to `caddy.d/00-snippets.conf`:
```
# (authelia_strip) — like (authelia), but removes any client-supplied Remote-* headers
# BEFORE forwarding. The shared (authelia) snippet only copies; nothing stops a client
# sending its own. Any application that reads these headers must import this variant.
(authelia_strip) {
	request_header -Remote-User
	request_header -Remote-Groups
	request_header -Remote-Email
	request_header -Remote-Name
	forward_auth manage.vm:9099 {
		uri /api/authz/forward-auth
		copy_headers Remote-User Remote-Groups Remote-Name Remote-Email
	}
}
```

- [ ] **Step 2: Add the site block**

Append to `caddy.d/10-services.conf`:
```
# --- great-wiki knowledge platform (github.com/Ch3fUlrich/great-wiki) --------
# NO `import authelia` here: the application performs its own OIDC login, so wrapping it
# in forward-auth would demand a second, redundant sign-in and break the /auth/callback
# redirect. Authelia gates it as an OIDC *client*, not as a proxy.
#
# The shared secret is the trust boundary. The application binds 0.0.0.0 (Caddy runs on
# this firewall, a different host from cloud.vm, so a loopback bind is unreachable) and
# therefore the port is LAN-reachable; the app refuses to start without this secret and
# rejects any request that does not carry it.
kb.ohje.ooguy.com {
	import secure_headers
	reverse_proxy cloud.vm:8092 {
		header_up X-GW-Proxy __GW_PROXY_SECRET__
	}
}
```

- [ ] **Step 3: Deploy safely**

**Never `scp *.conf`** — the live `00-snippets.conf` holds a real Cloudflare token where the
repository copy has a placeholder, and `10-services.conf` needs `__ACCOUNTS_PROXY_SECRET__`
and `__GW_PROXY_SECRET__` substituted.

```bash
# Back up first.
ssh opnsense "sh -c 'cp /usr/local/etc/caddy/caddy.d/10-services.conf /root/10-services.conf.bak'"
ssh opnsense "sh -c 'cp /usr/local/etc/caddy/caddy.d/00-snippets.conf /root/00-snippets.conf.bak'"

# Splice only the new blocks in, substituting the real secrets, then validate and reload.
ssh opnsense "sh -c 'caddy validate --config /usr/local/etc/caddy/Caddyfile --adapter caddyfile \
  && configctl caddy reload'"

# Confirm the firewall GUI still answers — a bad reload can lock you out.
curl -sk -o /dev/null -w '%{http_code}\n' https://192.168.178.76:8443/
```
Expected: `200`.

No DNS work is needed: Unbound holds a wildcard `local-data` for `ohje.ooguy.com` → the
firewall, and the public Dynu wildcard resolves externally. TLS is issued automatically per
hostname over HTTP-01 within about a minute of the reload.

- [ ] **Step 4: Run the acceptance checks**

```bash
# 1. The site answers over TLS.
curl -sk -o /dev/null -w '%{http_code}\n' https://kb.ohje.ooguy.com/api/health
#    Expect: 200

# 2. A public document is readable anonymously.
curl -sk -o /dev/null -w '%{http_code}\n' https://kb.ohje.ooguy.com/api/documents/oeffentlich
#    Expect: 200

# 3. A restricted document is refused anonymously.
curl -sk -o /dev/null -w '%{http_code}\n' https://kb.ohje.ooguy.com/api/documents/geheim
#    Expect: 403

# 4. A forged identity header changes nothing — the app authenticates by session, not header.
curl -sk -H 'Remote-User: admin' -H 'Remote-Groups: admins' \
  -o /dev/null -w '%{http_code}\n' https://kb.ohje.ooguy.com/api/documents/geheim
#    Expect: 403. If this returns 200, STOP — the identity model is compromised.

# 5. A restricted title does not leak into the navigation tree.
curl -sk https://kb.ohje.ooguy.com/api/tree | grep -c 'Geheim'
#    Expect: 0

# 6. Bypassing the proxy is refused even from the LAN.
curl -s -o /dev/null -w '%{http_code}\n' http://cloud.vm:8092/api/health
#    Expect: 403 — the shared secret is absent. NOT 200.

# 7. Login completes end to end.
#    In a browser: open https://kb.ohje.ooguy.com/auth/login, sign in at Authelia,
#    land back on the site, then GET /api/me.
#    Expect: {"user":"sergej","groups":["admins",...]} — a non-empty groups array.
#    An empty groups array means the `groups` scope is missing from the client registration.
```

- [ ] **Step 5: Write the operations document**

`docs/operations/deploy.md` covering: the hostname and port, the shared-secret contract,
how to roll back the Caddy config from the `.bak` files, how to restart the service, where
the database lives, and — explicitly — that the database is **not** disposable, unlike the
derived search index that arrives in M7.

- [ ] **Step 6: Changelog and commit**

Add under `### Added`:
```markdown
- Edge exposure at `https://kb.ohje.ooguy.com` with automatic TLS, an OIDC client
  registered in Authelia, and a shared-secret proxy boundary. Adds an `authelia_strip`
  Caddy snippet that removes client-supplied `Remote-*` headers, which the shared
  `authelia` snippet does not do.
```

```bash
git add docs/operations CHANGELOG.md
git commit -m "feat(deploy): expose kb.ohje.ooguy.com with OIDC login and a proxy boundary"
```

---

## Milestone exit criteria

- [ ] All seven acceptance checks in Task 7 pass.
- [ ] `just ci` passes.
- [ ] Signing in through Authelia lands back on the site with a non-empty `groups` array
      at `/api/me`.
- [ ] The tree renders, a document renders, an unknown path shows the error page.
- [ ] `curl http://cloud.vm:8092/api/health` returns 403, not 200.

## Self-Review

**Spec coverage.** M1 implements spec §4 (the document and identity parts of the data
model), §5 (relative defaults, bind and port rules), §6.1–6.3 (permission filtering in the
retriever, OIDC, the development shim), and the §12 requirements that apply to a reader:
dark and light themes, skip link, `aria-current`, `lang` attribute, responsive layout,
horizontally scrollable code blocks. Editing, media, search, datasets and the graph are
later milestones by design.

**Placeholders.** None. Every step has complete file content or an exact command with
expected output. Task 6 Step 6's `AppState` extension is described rather than fully
restated because the surrounding file is given in Task 4 — the fields and accessor names
are named exactly.

**Type consistency.** `Block`/`BlockKind` field names (`kind`, `attrs`, `content`, `text`)
are identical in Rust and TypeScript, and `#[serde(rename_all = "camelCase")]` makes
`bulletList` match on both sides. `StoredDocument`'s fields match the TypeScript interface
field for field, including `parent_path` and `doc_type` in snake_case. `may_read` has one
definition and Task 4's tests exercise it through the router. `Identity` is constructed by
`dev()` in tests and by the OIDC callback in production — one type, two producers.

**Deliberate gap.** `may_read` in M1 knows only visibility levels. M2 replaces it with the
full permission engine; the comment on the function says so, and `Store::tree` carries the
same note, so neither is left as an unfiltered variant that a later handler could
accidentally call.
