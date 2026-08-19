# Marks, Links and the Graph — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give `Block` inline marks and links, then build links, backlinks and a permission-filtered graph on top.

**Architecture:** `Block` gains a `marks` field shaped exactly like ProseMirror's, so the editor, the CRDT and the database keep one representation with no translation layer. An internal link stores the target document's **id**, resolved to a path only at render time, so moving a page cannot break an inbound link. Publishing extracts links into a `links` table inside the same transaction that writes the revision; that table *is* the graph.

**Tech Stack:** Rust 1.97 (gw-core, gw-store, gw-api, gw-collab), `pulldown-cmark`, SQLite via sqlx, SvelteKit 2 + Svelte 5, TipTap 3.30, `yrs` 0.27.

## Global Constraints

- **The test helpers below do not exist yet — write them.** `collect_text_leaves`,
  `paragraph_with_text`, `first_text_leaf`, `fixture_with_three_pages` and
  `body_linking_to` are named by the tests in this plan and are yours to write in the same
  test module. They are named rather than spelled out because their bodies are obvious and
  their names are not; keep the names exactly as written so later tasks match.

- **Spec:** `docs/superpowers/specs/2026-08-15-links-topics-tasks-design.md`. Decisions D-1..D-5 are binding.
- **D-5:** an internal link stores the target **document id**, never a path.
- **Security:** every node, edge and backlink is filtered through `Store::document_for` and the single `can()`. Never a second authorisation path. Filtering is **per document**, not per subtree.
- **Architecture rule 1:** a page body changes only by publishing a revision. Link extraction joins that transaction; it does not become a second write path.
- **Architecture rule 2:** nothing outside `gw-store` obtains an unfiltered document or tree. Public accessors take a `Principal`.
- **Interface language is German.** Every visible string.
- **TDD:** write the failing test, watch it fail *for the right reason* (a compile error is not a failing test), then implement.
- **Verification:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and for web work `cd web && npm run check && npx vitest run && npm run build`.
- **Never read, quote or commit `content-darm/`** — gitignored personal medical information about a child. Use `content-example/`.
- Commit after each task. Do not push; the orchestrator pushes.

---

## File Structure

| File | Responsibility |
|---|---|
| `crates/gw-core/src/block.rs` | `Mark`, `MarkKind`, `Block.marks` |
| `crates/gw-core/src/markdown.rs` | markdown → marks (import) |
| `crates/gw-api/src/export.rs` | marks → markdown (export) |
| `crates/gw-collab/src/doc.rs` | marks across the CRDT boundary |
| `web/src/lib/editor/extensions.ts` | editor schema: mark types |
| `web/src/lib/editor/EditorToolbar.svelte` | formatting controls |
| `web/src/lib/blocks/render.ts` + `BlockView.svelte` | rendering marks |
| `crates/gw-store/migrations/0009_links.sql` | the `links` table |
| `crates/gw-store/src/links.rs` | extraction, backlinks, graph — all permission-checked |
| `crates/gw-api/src/routes/links.rs` | HTTP for backlinks and graph |
| `web/src/lib/components/Backlinks.svelte` | backlinks panel |
| `web/src/routes/graph/+page.svelte` | graph view |

---

## Task 1: `Mark` and `Block.marks`

**Files:**
- Modify: `crates/gw-core/src/block.rs`
- Test: `crates/gw-core/src/block.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub enum MarkKind { Strong, Em, Code, Strike, Link }` — serde `rename_all = "camelCase"`, `#[non_exhaustive]`
  - `pub struct Mark { pub kind: MarkKind, pub attrs: serde_json::Map<String, serde_json::Value> }`
  - `Block.marks: Vec<Mark>` — `#[serde(default, skip_serializing_if = "Vec::is_empty")]`
  - `pub fn Mark::link_to_doc(id: &str) -> Mark` and `pub fn Mark::link_to_url(url: &str) -> Mark`
  - `pub fn Mark::target_doc(&self) -> Option<&str>` — `Some` only for a `Link` carrying `doc`

**Link attrs are exactly one of two shapes** and this is load-bearing:
- internal: `{"doc": "<document id>"}` — per D-5
- external or unresolved: `{"href": "<url>"}`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_mark_round_trips_through_json_and_an_absent_marks_field_is_empty() {
    let b: Block = serde_json::from_str(
        r#"{"kind":"text","text":"hallo","marks":[{"kind":"strong"},
             {"kind":"link","attrs":{"doc":"019ff0"}}]}"#,
    )
    .unwrap();
    assert_eq!(b.marks.len(), 2);
    assert_eq!(b.marks[1].target_doc(), Some("019ff0"));

    // A block written before marks existed must still parse, and must not grow a key.
    let old: Block = serde_json::from_str(r#"{"kind":"text","text":"hallo"}"#).unwrap();
    assert!(old.marks.is_empty());
    assert_eq!(serde_json::to_string(&old).unwrap(), r#"{"kind":"text","text":"hallo"}"#);
}

#[test]
fn an_external_link_is_not_a_document_reference() {
    let m = Mark::link_to_url("https://example.org");
    assert_eq!(m.target_doc(), None, "an href must never be read as a document id");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p gw-core marks`
Expected: does not compile — `Mark` not found. **Add the types first, then re-run and confirm the assertions fail before implementing `target_doc`.**

- [ ] **Step 3: Implement**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum MarkKind { Strong, Em, Code, Strike, Link }

/// Inline formatting on a text leaf, shaped exactly like a ProseMirror mark.
///
/// A link carries EITHER `doc` (an internal target, per D-5) or `href` (external, or an
/// internal one that could not be resolved). Never both: `target_doc` reading an `href`
/// as an id would turn a URL into a document reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mark {
    pub kind: MarkKind,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

impl Mark {
    pub fn link_to_doc(id: &str) -> Self {
        let mut attrs = serde_json::Map::new();
        attrs.insert("doc".into(), serde_json::Value::String(id.to_string()));
        Mark { kind: MarkKind::Link, attrs }
    }

    pub fn link_to_url(url: &str) -> Self {
        let mut attrs = serde_json::Map::new();
        attrs.insert("href".into(), serde_json::Value::String(url.to_string()));
        Mark { kind: MarkKind::Link, attrs }
    }
    pub fn target_doc(&self) -> Option<&str> {
        if self.kind != MarkKind::Link { return None; }
        self.attrs.get("doc").and_then(|v| v.as_str())
    }
}
```

Add to `Block`:

```rust
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
```

Fix every `Block { .. }` literal the compiler now rejects. Export `Mark`, `MarkKind` from `crates/gw-core/src/lib.rs`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p gw-core && cargo test --workspace`
Expected: PASS. The workspace run matters — `Block` is constructed in several crates.

- [ ] **Step 5: Prove the guard is real**

Change `target_doc` to also read `href`. Run `cargo test -p gw-core`. Expected: `an_external_link_is_not_a_document_reference` FAILS. Revert.

- [ ] **Step 6: Commit**

```bash
git add crates/gw-core crates/ CHANGELOG.md
git commit -m "feat(core): Block can hold inline marks and links"
```

---

## Task 2: The markdown importer produces marks

**Files:**
- Modify: `crates/gw-core/src/markdown.rs`
- Test: `crates/gw-core/src/markdown.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `Mark`, `MarkKind` from Task 1.
- Produces: `convert()` emits `Block.marks`; `Unsupported::InlineMarks` and `Unsupported::LinkTarget` are **no longer emitted**. `Unsupported::Image` still is (images are piece 4).

The converter is an event loop over `pulldown_cmark`. Marks arrive as `Start(Tag::Strong)` … `End`. Carry an active-mark stack and stamp it onto each text leaf as it is pushed.

**A markdown link cannot be resolved to a document id here** — this crate has no store. So a link becomes `Mark::link_to_url(dest)`. Task 7 resolves internal ones on publish. Say so in a comment; do not invent a resolution.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn emphasis_and_links_survive_import_and_are_no_longer_reported_as_lost() {
    let c = convert("Ein **fetter** Satz mit [einem Link](https://example.org).");
    let text: Vec<_> = collect_text_leaves(&c.doc); // you write this helper — see the note below
    let fett = text.iter().find(|b| b.text.as_deref() == Some("fetter")).unwrap();
    assert!(fett.marks.iter().any(|m| m.kind == MarkKind::Strong), "bold was dropped");

    let link = text.iter().find(|b| b.text.as_deref() == Some("einem Link")).unwrap();
    let m = link.marks.iter().find(|m| m.kind == MarkKind::Link).unwrap();
    assert_eq!(m.attrs.get("href").and_then(|v| v.as_str()), Some("https://example.org"));

    assert!(
        !c.notes.iter().any(|n| matches!(n.construct, Unsupported::InlineMarks | Unsupported::LinkTarget)),
        "the converter still reports marks as lost: {:?}", c.notes
    );
}

#[test]
fn nested_emphasis_stamps_both_marks_on_the_inner_text() {
    let c = convert("*kursiv und **beides***");
    let text: Vec<_> = collect_text_leaves(&c.doc);
    let both = text.iter().find(|b| b.text.as_deref() == Some("beides")).unwrap();
    assert_eq!(both.marks.len(), 2, "an inner leaf carries every enclosing mark");
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p gw-core emphasis_and_links`
Expected: FAIL on `bold was dropped` — an assertion, not a compile error.

- [ ] **Step 3: Implement the mark stack**

Maintain `active: Vec<Mark>` in `Builder`. On `Start(Tag::Strong|Emphasis|Strikethrough)` push; on the matching `End` pop. `Tag::Link { dest_url, .. }` pushes `Mark::link_to_url(&dest_url)`. `Event::Code(s)` pushes a text leaf carrying `MarkKind::Code`. Stamp `active.clone()` onto every text leaf as it is created. Delete the `InlineMarks` and `LinkTarget` note emissions.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p gw-core && cargo test --workspace`
Expected: PASS. `crates/gw-api/tests/seed.rs` asserts on note counts — update those assertions to the new truth and say so in the commit.

- [ ] **Step 5: Prove the stack is real**

Replace `active.clone()` with `Vec::new()`. Run `cargo test -p gw-core`. Expected: both tests FAIL. Revert.

- [ ] **Step 6: Commit**

```bash
git add crates/gw-core crates/gw-api/tests/seed.rs
git commit -m "feat(core): markdown import keeps emphasis and link destinations"
```

---

## Task 3: The exporter renders marks

**Files:**
- Modify: `crates/gw-api/src/export.rs`
- Test: `crates/gw-api/tests/export_markdown.rs`

**Interfaces:**
- Consumes: `Mark`, `MarkKind`; `render(doc: &Block) -> Rendered`.
- Produces: nothing new; `render` gains mark output.

`render_file` already re-imports its own output and refuses to write a file that would come back different. That check now covers marks for free — which is exactly why this task is small and safe.

**A `doc` link must be resolved to a path before export.** `render` has no store, so `run()` resolves ids → paths and passes a map into the renderer. An id that resolves to nothing (deleted, or unreadable by this principal) is rendered as **plain text, not a broken link**, and reported as a `Refused` reason — never a dangling `[text]()`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn marks_round_trip_through_markdown() {
    let md = "Ein **fetter** Satz mit [einem Link](https://example.org) und `code`.";
    let doc = gw_core::markdown::convert(md).doc;
    let out = gw_api::export::render(&doc);
    assert!(out.markdown.contains("**fetter**"), "bold was lost: {}", out.markdown);
    assert!(out.markdown.contains("[einem Link](https://example.org)"));
    assert!(out.markdown.contains("`code`"));

    // The real proof: re-importing must give the same tree.
    let again = gw_core::markdown::convert(&out.markdown).doc;
    assert_eq!(serde_json::to_value(&doc).unwrap(), serde_json::to_value(&again).unwrap());
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p gw-api --test export_markdown marks_round_trip`
Expected: FAIL — the output has no `**`.

- [ ] **Step 3: Implement**

Wrap each text leaf in its marks, innermost last, escaping as the existing escaper does. Order marks deterministically (`strong`, `em`, `strike`, `code`, `link`) so two runs produce identical bytes.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p gw-api && cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Prove it**

Drop `strong` from the wrapper. Expected: the test FAILS on `bold was lost`. Revert.

- [ ] **Step 6: Commit**

```bash
git add crates/gw-api
git commit -m "feat(export): markdown export carries emphasis and links"
```

---

## Task 4: Marks across the CRDT boundary

**Files:**
- Modify: `crates/gw-collab/src/doc.rs`
- Test: `crates/gw-collab/src/doc.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `Mark`, `MarkKind`.
- Produces: `CollabDoc::to_block` emits marks; `from_block` writes Yjs formatting.

`crates/gw-collab/src/lib.rs` documents that marks live in the CRDT and are lost by `to_block`. **That statement becomes false with this task — update it.** The test `an_inline_mark_from_the_browser_keeps_its_text_and_loses_its_emphasis` must be rewritten to assert the emphasis now *survives*, and renamed.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_mark_survives_a_round_trip_through_the_crdt() {
    let mut block = paragraph_with_text("fett");
    block.content[0].content[0].marks = vec![Mark { kind: MarkKind::Strong, attrs: Default::default() }];
    let doc = CollabDoc::from_block(&block);
    let back = doc.to_block();
    let leaf = first_text_leaf(&back);
    assert_eq!(leaf.marks.len(), 1, "the CRDT dropped the mark");
    assert_eq!(leaf.marks[0].kind, MarkKind::Strong);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p gw-collab a_mark_survives`
Expected: FAIL — `leaf.marks` is empty.

- [ ] **Step 3: Implement**

`read_text` already uses `diff(txn, YChange::identity)`, which yields `(string, Option<Attrs>)` per chunk — map those attributes to `Mark`s. On write, apply formatting with `XmlTextRef::format` for each mark's range.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p gw-collab && cargo test --workspace`
Expected: PASS, including the renamed test and the reworded crate docs.

- [ ] **Step 5: Prove it**

Return `Vec::new()` from the attribute mapping. Expected: FAIL. Revert.

- [ ] **Step 6: Commit**

```bash
git add crates/gw-collab
git commit -m "feat(collab): a published snapshot keeps inline marks"
```

---

## Task 5: The editor offers formatting again

**Files:**
- Modify: `web/src/lib/editor/extensions.ts`, `web/src/lib/editor/EditorToolbar.svelte`, `web/src/lib/blocks/render.ts`, `web/src/lib/components/BlockView.svelte`
- Test: `web/src/lib/editor/extensions.test.ts`, `web/src/lib/components/BlockView.test.ts`

**Interfaces:**
- Consumes: nothing from Rust at runtime; the schema must match `MarkKind`.
- Produces: `EDITOR_MARK_NAMES: readonly string[]`.

The editor deliberately ships with **zero marks** because `Block` could not hold them. That reason is now gone. Enable exactly `strong`, `em`, `code`, `strike`, `link` — no others, and assert the set equals the server's `MarkKind` list the same way `SERVER_BLOCK_KINDS` is already asserted.

**`BlockView` must render marks** or the reader sees plain text while the editor shows bold. Render `link` with `doc` as an internal `<a href>` resolved by the server; `href` as an external link with `rel="noopener noreferrer"`.

- [ ] **Step 1: Write the failing tests**

```ts
it('offers exactly the marks the server can store', () => {
  expect([...EDITOR_MARK_NAMES].sort()).toEqual(['code', 'em', 'link', 'strike', 'strong']);
});

it('renders a bold run as <strong>', () => {
  const html = render(BlockView, { props: { block: textWithMark('fett', 'strong') } }).body;
  expect(html).toContain('<strong>fett</strong>');
});

it('never renders an external link without rel protection', () => {
  const html = render(BlockView, { props: { block: linkTo('https://example.org') } }).body;
  expect(html).toMatch(/rel="[^"]*noopener/);
});
```

- [ ] **Step 2: Run and watch them fail**

Run: `cd web && npx vitest run src/lib/editor/extensions.test.ts src/lib/components/BlockView.test.ts`
Expected: FAIL — `EDITOR_MARK_NAMES` undefined, no `<strong>`.

- [ ] **Step 3: Implement**

Add the five marks to `contentExtensions()`, export `EDITOR_MARK_NAMES` from the built schema, add toolbar toggles (Ark `ToggleGroup`, German labels: `Fett`, `Kursiv`, `Code`, `Durchgestrichen`, `Link`), and render marks in `BlockView`.

- [ ] **Step 4: Run the web gate**

Run: `cd web && npm run check && npx vitest run && npm run build`
Expected: all clean.

- [ ] **Step 5: Prove it**

Remove `strike` from the extension list. Expected: the set test FAILS naming `strike`. Revert.

- [ ] **Step 6: Commit**

```bash
git add web
git commit -m "feat(web): formatting controls, now that a revision can keep them"
```

---

## Task 6: The `links` table

**Files:**
- Create: `crates/gw-store/migrations/0009_links.sql`
- Test: `crates/gw-store/src/links.rs` (created next task; add a migration-applies test here)

- [ ] **Step 1: Write the migration**

```sql
-- The graph is this table. One row per ordered pair: a page linking to another twice is
-- one edge, because that is what a graph draws and what a backlinks panel lists.
--
-- Both sides CASCADE. A link is a fact about two documents and outlives neither: deleting
-- a page must not leave an edge pointing at nothing, which would be a node in the graph
-- with a title nobody can read.
CREATE TABLE links (
    from_doc TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    to_doc   TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    PRIMARY KEY (from_doc, to_doc)
) WITHOUT ROWID;

CREATE INDEX links_to_doc ON links(to_doc);
```

- [ ] **Step 2: Verify it applies to a POPULATED database**

```bash
cp data/great-wiki.db /tmp/mig-check.db
# NOT `gw-api -- check`: that subcommand only validates environment variables and never
# calls Store::open, which is where sqlx::migrate! runs. It would pass without touching
# the database at all. Open the store for real instead:
cat > /tmp/mig.rs <<'EOF'
fn main() { /* see below */ }
EOF
cargo run -p gw-store --example migration_check   # writes a throwaway example that calls
                                                  # Store::open(&env GW_DATABASE_URL) and
                                                  # prints the applied migration list
```
Expected: every migration recorded, including yours, against a database that already has
rows. A migration that only works on an empty database is a migration that fails in
production — and a check that never opens the database proves neither.

- [ ] **Step 3: Commit**

```bash
git add crates/gw-store/migrations/0009_links.sql
git commit -m "feat(store): the links table"
```

---

## Task 7: Extract links when a revision is published

**Files:**
- Create: `crates/gw-store/src/links.rs`
- Modify: `crates/gw-store/src/revisions.rs` (call from `append_revision`), `crates/gw-store/src/lib.rs`
- Modify: `scripts/mutate.sh`

**Interfaces:**
- Consumes: `Block`, `Mark::target_doc`, the open transaction inside `append_revision`.
- Produces:
  - `pub(crate) async fn replace_links(conn: &mut SqliteConnection, from_doc: &str, body: &Block) -> Result<()>`
  - `pub async fn Store::backlinks_for(&self, principal: &Principal, document_id: &str) -> Result<Vec<Backlink>>`
  - `pub struct Backlink { pub id: String, pub path: String, pub title: String }`

`replace_links` walks the body, collects every `Mark::target_doc`, deletes this document's rows and inserts the new set — **inside the caller's transaction**, so a failed publish leaves no edges for a revision that does not exist. It takes a connection, not the pool, exactly as `append_revision` does and for the same reason.

**`backlinks_for` filters per document** through `Store::document_for`. A backlink to a page the caller may not read is omitted entirely — it is not shown as "a page you cannot see", because that still reveals it exists.

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn publishing_records_the_links_in_the_body() {
    let (store, chef, from, a, b) = fixture_with_three_pages().await;
    store
        .publish_revision(&chef, &from, &body_linking_to(&[&a, &b]), None)
        .await
        .unwrap()
        .expect("the publish was refused");

    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT from_doc, to_doc FROM links ORDER BY to_doc")
            .fetch_all(&store.pool)
            .await
            .unwrap();
    assert_eq!(rows.len(), 2, "expected one edge per linked document, got {rows:?}");
    assert!(rows.iter().all(|(f, _)| *f == from));
}

#[tokio::test]
async fn republishing_replaces_rather_than_accumulates() {
    let (store, chef, from, a, b) = fixture_with_three_pages().await;
    store.publish_revision(&chef, &from, &body_linking_to(&[&a, &b]), None).await.unwrap();
    store.publish_revision(&chef, &from, &body_linking_to(&[&a]), None).await.unwrap();

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM links WHERE from_doc = ?1")
        .bind(&from)
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "the removed link is still an edge — edges accumulated instead of being replaced");
}

#[tokio::test]
async fn a_backlink_to_a_page_the_caller_cannot_read_is_not_listed() {
    // `leser` may read /ziel but NOT /geheim. /geheim links to /ziel.
    let back = store.backlinks_for(&leser, ziel_id).await.unwrap();
    assert!(back.is_empty(), "a backlink revealed a page the caller cannot read");
    // Anti-vacuity: chef DOES see it, so the fixture really contains the link.
    assert_eq!(store.backlinks_for(&chef, ziel_id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn a_failed_publish_leaves_no_edges() {
    let (store, chef, from, a, _b) = fixture_with_three_pages().await;

    // Fail at exactly the point where a body could be committed without its edges — or
    // edges without their revision. A trigger is the only way to force it from outside.
    sqlx::query(
        "CREATE TRIGGER refuse_revisions BEFORE INSERT ON revisions
         BEGIN SELECT RAISE(ABORT, 'nope'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap();

    let result = store.publish_revision(&chef, &from, &body_linking_to(&[&a]), None).await;
    assert!(result.is_err(), "the publish should have failed");

    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM links")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "edges survived a publish that did not happen");
}
```

- [ ] **Step 2: Run and watch them fail**

Run: `cargo test -p gw-store links`
Expected: FAIL on assertions once the module exists.

- [ ] **Step 3: Implement**

```rust
/// Every document this body links to, deduplicated, in no particular order.
fn targets(body: &Block, into: &mut BTreeSet<String>) {
    for mark in &body.marks {
        if let Some(doc) = mark.target_doc() {
            into.insert(doc.to_string());
        }
    }
    for child in &body.content {
        targets(child, into);
    }
}

/// Replace this document's edges. Takes a CONNECTION, not the pool, so it joins the
/// caller's transaction: a publish that fails afterwards must leave no edges behind for a
/// revision that does not exist. Same reasoning as `append_revision`.
pub(crate) async fn replace_links(
    conn: &mut sqlx::SqliteConnection,
    from_doc: &str,
    body: &Block,
) -> Result<()> {
    let mut found = BTreeSet::new();
    targets(body, &mut found);

    sqlx::query("DELETE FROM links WHERE from_doc = ?1")
        .bind(from_doc)
        .execute(&mut *conn)
        .await?;

    for to_doc in found {
        // A page linking to itself is not an edge worth drawing, and it would render as a
        // self-loop in the graph and as a backlink to the page you are already reading.
        if to_doc == from_doc {
            continue;
        }
        // OR IGNORE, not an error: a link to a document that has been deleted is a fact
        // about the body, not a reason to refuse the publish.
        sqlx::query("INSERT OR IGNORE INTO links (from_doc, to_doc) VALUES (?1, ?2)")
            .bind(from_doc)
            .bind(&to_doc)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}
```

`backlinks_for` reads candidate rows, then asks `document_for` per candidate and keeps only
those it answers `Some` for. Do not filter in SQL by path prefix: D-3 makes membership
per-document, and a prefix filter would be a second, weaker answer to a question `can()`
already answers.

- [ ] **Step 4: Run the tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 5: Add mutations and run them**

Add to `scripts/mutate.sh`:

```bash
mutation crates/gw-store/src/links.rs killed \
  's/        let Some(doc) = store.document_for(principal, &row.path, Action::Read).await? else {/        let Some(doc) = Some(\&row) else {/' \
  'links: a backlink names only a page the caller may actually read'
mutation crates/gw-store/src/links.rs killed \
  's/    sqlx::query("DELETE FROM links WHERE from_doc = ?1")/    sqlx::query("SELECT 1 FROM links WHERE from_doc = ?1")/' \
  'links: republishing replaces this page edges rather than accumulating them'
```

Run: `./scripts/mutate.sh links`
Expected: `2 mutations, all as expected`. **Run it only when no other agent is building** — it rewrites source in place.

- [ ] **Step 6: Commit**

```bash
git add crates/gw-store scripts/mutate.sh
git commit -m "feat(store): publishing extracts links, and backlinks are permission-filtered"
```

---

## Task 8: Backlinks over HTTP and on the page

**Files:**
- Create: `crates/gw-api/src/routes/links.rs`, `web/src/lib/components/Backlinks.svelte`
- Modify: `crates/gw-api/src/routes/mod.rs`, `web/src/routes/[...path]/+page.server.ts`, `web/src/routes/[...path]/+page.svelte`
- Test: `crates/gw-api/tests/links.rs`, `web/src/lib/components/Backlinks.test.ts`

**Interfaces:**
- Consumes: `Store::backlinks_for`.
- Produces: `GET /api/links/backlinks/{*path}` → `{"backlinks": [{"path": …, "title": …}]}`

**Route shape matters.** `crates/gw-api/src/routes/collab.rs` documents that matchit prefers a literal segment over `{*path}`, so a suffixed route under `/api/documents/{*path}` would shadow a real page. Put backlinks under its own prefix, as collab did.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn backlinks_are_refused_to_somebody_who_cannot_read_the_page() {
    let (status, _) = get(&store, None, "/api/links/backlinks/geheim").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
```

- [ ] **Step 2: Run and watch it fail** — Run: `cargo test -p gw-api --test links`. Expected: 404, no route.
- [ ] **Step 3: Implement the route and the panel.** German heading: `Verweist hierher`. Render nothing at all when the list is empty — an empty panel is furniture.
- [ ] **Step 4: Run both gates.** `cargo test --workspace` and `cd web && npm run check && npx vitest run && npm run build`.
- [ ] **Step 5: Prove it** — delete the permission check in the handler; expected: the test FAILS. Revert.
- [ ] **Step 6: Commit**

```bash
git add crates/gw-api web
git commit -m "feat(api): backlinks, filtered by what the caller may read"
```

---

## Task 9: The graph

**Files:**
- Modify: `crates/gw-store/src/links.rs`, `crates/gw-api/src/routes/links.rs`
- Create: `web/src/routes/graph/+page.svelte`, `web/src/routes/graph/+page.server.ts`
- Test: `crates/gw-api/tests/links.rs`, `web/src/routes/graph/page.test.ts`

**Interfaces:**
- Produces:
  - `pub async fn Store::graph_for(&self, principal: &Principal, root: Option<&str>) -> Result<Graph>`
  - `pub struct Graph { pub nodes: Vec<GraphNode>, pub edges: Vec<GraphEdge> }`
  - `pub struct GraphNode { pub id: String, pub path: String, pub title: String }`
  - `pub struct GraphEdge { pub from: String, pub to: String }`
  - `GET /api/links/graph?root=/darm`

**The property to test hardest:** an edge is emitted only when the caller may read **both** endpoints. An edge with one unreadable end reveals that page exists; a node label reveals its title. Filter per document (D-3), never per subtree.

Render with plain SVG and a small force layout — **no new dependency**. The corpus is tens of nodes, not thousands; a graph library is a bundle for a problem this does not have yet. Say so in a comment.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn an_edge_needs_both_ends_readable() {
    // /oeffentlich -> /geheim, and `leser` may read only /oeffentlich.
    let g = store.graph_for(&leser, None).await.unwrap();
    assert!(g.edges.is_empty(), "an edge leaked a page the caller cannot read");
    assert!(!g.nodes.iter().any(|n| n.path == "/geheim"), "a node leaked an unreadable title");
    // Anti-vacuity: chef sees exactly one edge, so the fixture really has one.
    assert_eq!(store.graph_for(&chef, None).await.unwrap().edges.len(), 1);
}
```

- [ ] **Step 2: Run and watch it fail** — Run: `cargo test -p gw-store an_edge_needs`. Expected: FAIL.
- [ ] **Step 3: Implement**

```rust
pub async fn graph_for(&self, principal: &Principal, root: Option<&str>) -> Result<Graph> {
    // Every candidate edge first, then the permission question ONCE per document rather
    // than once per edge — a page with forty links would otherwise be asked forty times.
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT from_doc, to_doc FROM links")
        .fetch_all(&self.pool)
        .await?;

    let mut readable: HashMap<String, GraphNode> = HashMap::new();
    let mut refused: HashSet<String> = HashSet::new();
    for id in rows.iter().flat_map(|(f, t)| [f.clone(), t.clone()]) {
        if readable.contains_key(&id) || refused.contains(&id) {
            continue;
        }
        match self.document_node_for(principal, &id, root).await? {
            Some(node) => { readable.insert(id, node); }
            None => { refused.insert(id); }
        }
    }

    // BOTH ends. An edge with one unreadable end reveals that the page exists, and drawing
    // it with an anonymous node still says "there is something here you may not see".
    let edges = rows
        .into_iter()
        .filter(|(f, t)| readable.contains_key(f) && readable.contains_key(t))
        .map(|(from, to)| GraphEdge { from, to })
        .collect();

    Ok(Graph { nodes: readable.into_values().collect(), edges })
}
```

`document_node_for` resolves the id to a path, applies the optional `root` prefix filter,
and then asks `document_for(.., Action::Read)` — the prefix narrows the view, the permission
check decides it. Never the other way round.

- [ ] **Step 4: Run both gates.**
- [ ] **Step 5: Add the mutation**

```bash
mutation crates/gw-store/src/links.rs killed \
  's/        if readable.contains(&edge.from) \&\& readable.contains(&edge.to) {/        if readable.contains(\&edge.from) || readable.contains(\&edge.to) {/' \
  'graph: an edge needs BOTH ends readable — one is a disclosure'
```

Run: `./scripts/mutate.sh graph`. Expected: `1 mutation, all as expected`.

- [ ] **Step 6: Commit**

```bash
git add crates/gw-store crates/gw-api web scripts/mutate.sh
git commit -m "feat(graph): documents and the links between them, both ends permission-checked"
```

---

## After the plan: re-import the corpus

Not a task in this plan, because it touches production and is the orchestrator's to run — but it is the reason Task 2 matters, and **the last cheap moment is immediately after Task 5**.

The owner's 35 pages lost their emphasis and all 89 source URLs at import, before marks existed. `content-darm/` still holds them. Re-importing with `seed --update --as sergej` restores them — and **overwrites anything edited in the wiki in the meantime**, which is why it happens before the editor is used in anger rather than after.
