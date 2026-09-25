# Rename and move — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A page can be renamed and moved — with its whole subtree — from a dialog on the page
that shows, before anything happens, who gains and who loses reading access; the old address
forwards to the new one for whoever may read the page.

**Architecture:** One store operation, `Store::move_document`, runs the move inside a
transaction and measures the access change *across the move itself* (before-verdicts from the
live tables, after-verdicts from inside the transaction), then commits or rolls back — the
preview **is** the move, exactly as the purge preview is the purge (ADR 0012). A `forwards`
table maps an old path to a document **id**; two triggers on `documents` drop a forward the
moment any row takes its path, so "until reused" is a property of the schema, not of every
caller. The API adds `GET|POST /api/move/{*path}` and `GET /api/forwards/{*path}`; the web
page gets a no-JavaScript dialog driven by the address (`?verschieben=1`), the same mechanism
the delete question uses.

**Tech Stack:** Rust (axum, sqlx/SQLite), SvelteKit 2 / Svelte 5, vitest.

**Source of the decisions:** roadmap section *2026-09-24 — rename and move come next, and how
they behave* (`2026-08-07-great-wiki-roadmap.md`). ADR 0019 (identity links), ADR 0022
(refusals), ADR 0012 (preview = act).

## Global Constraints

- A move **never rewrites another page's body** (ADR 0019). Links resolve by id at read time.
- Every retrieval filters by the caller's permissions in the retriever; `permits` in
  `gw_store::acl` remains the only interpreter of a visibility plus grants.
- ADR 0022: a refusal may only differ from "absent" for a caller who may read the page.
- Fail closed: anything not established as allowed is refused.
- Every task ends green: `cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check`; `cd web && npm run check && npx vitest run && npm run build`.
- `CHANGELOG.md` entry in the commit that earns it; ADR for the non-obvious choices.

## Decisions taken while planning (recorded in ADR 0023)

The owner's four decisions leave these open; each is settled here and written down:

1. **Grants written on the moved pages travel with them** (`acl` rows under the old prefix are
   re-pathed). Leaving them would orphan them onto whatever later takes the old address;
   dropping them would silently unfence a page somebody fenced. This is not the rejected
   "copy effective grants" option: nothing inherited is materialised. The preview shows the
   net effect either way, because it is measured after the rows moved.
2. **Pending invitations** naming a path in the subtree travel too (they are grants not yet
   written). Accepted and revoked ones are history and stay.
3. **Destination parent at the top level** requires administering the instance — the root has
   no page to hold a write grant, so `path_admin("/")` (baseline admin) is the gate.
4. **"Widening"** = at least one person gains read on at least one page of the subtree.
   Widening needs `path_admin` on the destination parent. Narrowing needs only write on every
   moved page and on the destination parent (the roadmap's recommended default).
5. **"Who"** = every active account plus "anyone without an account" (anonymous). Deactivated
   accounts can only read public pages, and a move does not change visibility, so they can
   never appear.
6. **The forward is a 307**, not 301/308: a permanent redirect is cached by browsers
   indefinitely, and the decision is that the forward ends when the address is reused.
7. **Rename changes the address.** The dialog carries title and address segment (prefilled
   with the current slug), so a title typo fix need not change the URL.
8. **The audit entry** (`document.move`) is scoped to the new path, target = the old path.
9. **An open editing session on a moved page is closed** by its next re-authorisation (it is
   keyed by the old path). Fail-closed; re-keying the socket by id is follow-up work.

## File Structure

- Create `crates/gw-store/migrations/0015_forwards.sql` — `forwards` table + two triggers.
- Create `crates/gw-store/src/moves.rs` — `Store::move_document`, `Store::forward_for`, types
  `MoveRequest`, `MoveMode`, `MoveOutcome`, `MovePlan`, `ReaderChange`.
- Modify `crates/gw-store/src/acl.rs` — `grants_on(conn, path)` (nearest-ancestor lookup on
  one connection; `grants_for_path` delegates to it); `permits` becomes `pub(crate)`.
- Modify `crates/gw-store/src/trash.rs` — `SUBTREE` and `refuse_a_hole_in_the_tree` become
  `pub(crate)` for reuse.
- Modify `crates/gw-store/src/lib.rs` — `mod moves; pub use moves::*`.
- Create `crates/gw-api/src/routes/moves.rs` — `GET/POST /api/move/{*path}`,
  `GET /api/forwards/{*path}`.
- Modify `crates/gw-api/src/routes/mod.rs` — merge the routes.
- Create `crates/gw-api/tests/moves.rs`; modify `crates/gw-api/tests/withheld.rs` (probes).
- Create `web/src/lib/moves.ts` (+ `moves.test.ts`) — wire types, endpoint builders, German
  wording, destination options from the tree.
- Modify `web/src/routes/[...path]/+page.server.ts` — forward on 404; preview in load;
  `verschieben` action.
- Modify `web/src/routes/[...path]/+page.svelte` — "Verschieben" link and dialog.
- Tests in `web/src/routes/[...path]/server.test.ts` and `page.test.ts`.
- Create `docs/decisions/0023-how-a-page-moves.md`; modify `CHANGELOG.md`, roadmap handoff.

---

### Task 1: Forwards table and store lookup

**Files:** Create `crates/gw-store/migrations/0015_forwards.sql`; create
`crates/gw-store/src/moves.rs` (forward half); modify `crates/gw-store/src/lib.rs`.

**Produces:** `Store::forward_for(&self, principal: &Principal, path: &str) -> Result<Option<String>>`
(the current path of the page that used to live at `path`, only if the caller may read it).

- [ ] Write failing tests in `moves.rs`:
  `a_forward_names_the_current_path_for_a_reader`,
  `a_forward_is_silent_for_somebody_who_may_not_read_the_page`,
  `creating_a_page_at_a_forwarded_address_drops_the_forward` (insert a forward by SQL, then
  `create_document` at that path → `forward_for` is `None` and the row is gone),
  `a_forward_to_a_page_in_the_trash_is_silent`.
- [ ] Run `cargo test -p gw-store moves` — fails (no table / no fn).
- [ ] Migration:

```sql
CREATE TABLE forwards (
    old_path    TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX forwards_document ON forwards (document_id);
CREATE TRIGGER forwards_end_when_a_page_arrives_insert AFTER INSERT ON documents
FOR EACH ROW BEGIN DELETE FROM forwards WHERE old_path = NEW.path; END;
CREATE TRIGGER forwards_end_when_a_page_arrives_update AFTER UPDATE OF path ON documents
FOR EACH ROW BEGIN DELETE FROM forwards WHERE old_path = NEW.path; END;
```

- [ ] `forward_for`: `SELECT document_id FROM forwards WHERE old_path = ?1`, then
  `document_for_id(principal, id, Action::Read)` → `Some(document.path)`.
- [ ] Tests pass; commit `feat(store): an address a page left can name where it went`.

### Task 2: `Store::move_document`

**Files:** `crates/gw-store/src/moves.rs`, `acl.rs`, `trash.rs`.

**Produces:**

```rust
pub struct MoveRequest { pub parent: Option<String>, pub title: String, pub slug: Option<String> }
pub enum MoveMode { Preview, Commit }
pub enum MoveOutcome { Refused, Blocked(String), Planned(MovePlan) }
pub struct MovePlan {
    pub from: String, pub to: String, pub title: String,
    pub pages: usize,                 // live pages that move, the page included
    pub gains: Vec<ReaderChange>, pub losses: Vec<ReaderChange>,
    pub refusal: Option<String>,      // complete plan that may not be carried out (widening)
    pub committed: bool,
}
pub struct ReaderChange { pub anonymous: bool, pub name: String, pub username: Option<String>, pub pages: usize }
impl Store {
    pub async fn move_document(&self, principal: &Principal, path: &str, request: &MoveRequest,
        administers_destination: bool, mode: MoveMode) -> Result<MoveOutcome>;
}
```

Order of checks (each a test first):
1. not signed in / inactive → `Refused`; no write on the page → `Refused`.
2. empty title → `Blocked`; slug that slugifies to nothing → `Blocked`.
3. parent `Some(p)`: `p == path` or under `path` → `Blocked`; `document_for(p, Read)` is
   `None` → `Blocked("there is no page at p")` (same words for absent and withheld);
   readable but `document_access(p, Write)` is `None` → `Blocked("may not create under p")`.
   parent `None` and `!administers_destination` → `Blocked`.
4. `to == from && title unchanged` → `Blocked("nothing changes")`.
5. any row (live or trashed) at or under `to` when `to != from` → `Blocked` naming it.
6. every row under `from` (live via `document_access_with_baseline`, trashed via
   `trashed_document_access`) must be writable → `Blocked`.
7. People: active principals from `list_principals` plus `Principal::anonymous()`;
   baselines resolved once; before-verdicts via `grants_for_path` on the pool.
8. Transaction: re-path `documents` (path, parent_path; root also slug, title, sort_key =
   last among new siblings when the parent changes), `acl`, pending `invites`; insert a
   forward for every old **live** path; `refuse_a_hole_in_the_tree`; after-verdicts with
   `grants_on(&mut tx, new_path)` and `permits`.
9. gains non-empty and `!administers_destination` → `refusal = Some(..)`, rollback.
   Preview → rollback. Commit → `record_audit("document.move", target from, scope to,
   {from, to, title_before, title, pages})`, commit.

Tests (all in `moves.rs`): `a_move_takes_the_subtree_and_leaves_other_bodies_alone`
(bodies + revision counts of every other page byte-identical), `a_reference_by_id_follows_the_move`
(`references_for` names the new path), `grants_written_on_the_page_travel_with_it`,
`pending_invitations_travel_with_the_page`, `a_preview_changes_nothing`,
`the_preview_names_who_gains_and_who_loses`, `widening_without_admin_on_the_destination_is_refused_and_changes_nothing`,
`narrowing_needs_only_write`, `a_rename_in_place_changes_title_and_address_and_forwards`,
`moving_back_to_an_old_address_drops_its_forward`, `a_chain_of_moves_forwards_to_the_current_address`,
`a_page_cannot_move_into_its_own_subtree`, `an_occupied_address_is_refused`,
`a_subpage_you_may_not_write_blocks_the_move`, `an_unreadable_destination_reads_as_absent`,
`the_top_level_needs_instance_administration`, `a_trashed_subpage_moves_with_its_parent`,
`one_move_one_audit_entry`.

- [ ] Commit `feat(store): a page moves with its subtree, and the move measures who it lets in`.

### Task 3: API

**Files:** `crates/gw-api/src/routes/moves.rs`, `routes/mod.rs`, tests `moves.rs`, `withheld.rs`.

- `GET /api/move/{*path}?parent=/x&title=T&slug=s` → 200 `MovePlanView` (preview; includes
  `refusal`). `parent` absent or empty = top level.
- `POST /api/move/{*path}` JSON `{parent, title, slug}` → 200 plan with `committed: true`;
  `refusal` → 409 with it; `Blocked` → 409; `Refused` → `withheld_or_absent`.
- `administers_destination` = `path_admin(state, jar, parent.unwrap_or("/")).is_ok()`.
- `GET /api/forwards/{*path}` → 200 `{ "path": "/new" }` or 404 (`ApiError::NotFound`).
- Tests: preview does not move; commit moves and GET at the new path works; a reader gets 403,
  a stranger 404; widening by a writer is 409 and changes nothing, by an admin commits;
  forward endpoint 200 for reader, 404 for stranger. Withheld sweep gains the `POST` and
  `GET` move probes (write-keyed) and the forward probe.
- [ ] Commit `feat(api): moving a page, previewing it, and following an old address`.

### Task 4: Web

**Files:** `web/src/lib/moves.ts`, `moves.test.ts`, `[...path]/+page.server.ts`,
`+page.svelte`, `server.test.ts`, `page.test.ts`.

- `moves.ts`: `MOVE_PARAM = 'verschieben'`, `MOVE_REGION_ID = 'gw-verschieben'`,
  `moveApiPath(path)`, `movePreviewApiPath(path, {ziel,titel,adresse})`,
  `forwardApiPath(path)`, `moveHref(path)`, `destinations(tree, path)` (flattened tree minus
  the page's own subtree, with depth), `describeMove(status, message)` (German),
  `readerChangeText(change, pages)`.
- Loader: on 404 ask `forwardApiPath`; found → `redirect(307, new + url.search)`. When
  `?verschieben=1` and `ziel` present → preview via API; expose `verschieben`, `vorschau`,
  `vorschauFehler`, form values.
- Action `verschieben`: POST; failure → `fail(status, {wo:'verschieben', fehler})`; success →
  `redirect(303, plan.to)`.
- Page: link "Verschieben" beside "Löschen" (same offer rule: `may_write && authenticated`);
  section `#gw-verschieben` with a GET form (hidden `verschieben=1`; `titel`, `adresse`,
  `ziel` select) and, when a preview exists, the lists and a POST form.
- [ ] Commit `feat(web): a page can be renamed and moved from a dialog that shows the access change`.

### Task 5: Record it

- ADR 0023, CHANGELOG entry, roadmap handoff update; full gate green; browser check via
  `just dev`/Playwright of the dialog, preview, move and forward.
- [ ] Commit `docs: how a page moves`.
