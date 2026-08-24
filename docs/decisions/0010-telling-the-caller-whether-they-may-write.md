# 0010 — Telling the caller whether they may write

**Status:** Accepted (2026-08-24)

## Context

Nothing on the wire said whether the caller may **write** a page. So every control that needs
write — »Bearbeiten«, »Neues Projekt«, delete, and »… nach Läuft verschieben« on a card —
was offered to whoever was signed in, and the true answer arrived only as a refusal after
the person pressed it.

Five pieces of work recorded this independently and each correctly declined to fix it,
because each would have had to invent the answer locally:

- `web/src/routes/[...path]/+page.svelte` — "What would fix it is one boolean from the API."
- `web/src/routes/[...path]/history/+page.svelte` — the same, for restoring.
- `web/src/routes/projekte/+page.svelte` — the same, for creating and deleting a project.
- `crates/gw-api/src/routes/tasks.rs` — a board is a disclosure surface and this layer takes
  no permission decision of its own.
- `web/src/lib/board.ts` — left a seam, `movable?: boolean`, optional and never sent, with a
  comment saying which field it was waiting for.

The client cannot compute it. `/api/me` reports groups and a baseline, and D-M2-8 is explicit
that **no baseline confers write** — an instance admin with no grant is refused like anybody
else. The only endpoint that knew was the collaboration socket, and asking it means either a
WebSocket handshake (which allocates a room on the server, on every page view) or a POST
(which would publish somebody else's live session as a revision).

The offer was therefore made on "are you signed in", which is not the question. Two failures
follow, and both were live: somebody with read-only access is invited to edit and refused
after typing, and — the direction nobody notices — a page whose grants make it writable is
never distinguishable from one that is not, so the interface cannot mark a read-only card as
read-only rather than simply losing the move.

## Decision

**One boolean, called `may_write`, on the responses where a control is offered — and it is
produced by the same `permits()` call the write itself goes through, never computed beside
it.**

Carried on three responses today:

| Response | Field | Means |
|---|---|---|
| `GET /api/documents/{path}` | `may_write` | Write on this page |
| `GET /api/projects`, and every other `ProjectView` | `may_write` | Write on the project's **home page** |
| a board card, on both boards and on `GET /api/tasks/document/{path}` | `may_write` | Write on the card's **governing page** |

### One answer, not two

`gw_store::acl::permits` is the only function in this system that interprets a visibility and
a set of grants. `Store::document_access_with_baseline` resolves the page and the grants that
reach it **once**, then asks `permits` for the caller's action and for `Action::Write` — and
`Store::document_for`, which every write in the store already goes through, is now that
function with the second answer dropped. So the bit is not a second opinion about writing
computed next to the read: it is the opinion, from the same inputs, taken before anybody
presses anything.

That is the shape this codebase kept arriving at from other directions and it was chosen for
the same reason each time:

- `Store::document_for_id` — "a second way IN to one authorisation body, never a second
  answer".
- `Store::assignee_named` — the verdict and the name it licenses are **one value**, so there
  is no version of the code in which the gate is intact and the name leaks anyway.
- `TaskPage` — constructible only from a document the accessor answered with, so naming a
  page without having asked has nothing to build from.

`DocumentAccess` is the same trick one field further: a value that can only be built by
having asked cannot be built by having forgotten to ask. `Task::may_write` and
`Project::may_write` are read off it, never re-derived — `Store::governing_document`, the one
choke point in `gw_store::tasks`, now hands back a `DocumentAccess` rather than a bare
document, so the call that decides whether a card may be *seen* is the call that says whether
it may be *changed*.

### It costs nothing

`permits` is pure. The bit adds **no query at all** — not one per response and not one per
row. `Store::board_for` already memoised the accessor's answer per page; the memo now holds
the page and the verdict together (`Governed`) instead of the page alone, so a board of forty
cards over four pages asks four times, exactly as it did before.
`crates/gw-store/tests/board_query_count.rs` counts the statements and fails if that changes,
because the tempting way to add a write verdict to a card — ask "may I write this one?" per
row — reads like one line and is N+1 across the whole corpus.

### What `may_write` does **not** promise

It is the verdict on the *page*, so it licenses everything gated on that verdict: opening an
editing session, saving what is being typed, making the page a project's home, and moving,
retitling, reassigning or deleting a card the page governs.

**Filing a revision needs one thing more.** `Store::publish_revision` refuses an author who is
not a signed-in, active account, before it asks the permission question at all, because a
revision records an author and a byline nobody chose is worth nothing
(`an_anonymous_caller_cannot_publish_even_where_anyone_may_write`). On a path carrying
`anyone: write` — a public share link — an anonymous caller therefore gets `may_write: true`,
may edit and may save, and still cannot publish or restore.

That is not a defect to be papered over by narrowing the field, and narrowing it was
considered and rejected: `may_write && authenticated` would be wrong for the four controls
that do not need an author, and would make the one field mean something different on each of
the three responses. The composition belongs where both halves are already known — the
interface has `authenticated` from `/api/me` — and it is stated on the field itself, in
`DocumentAccess::may_write`, rather than left to be rediscovered.

### The name

`may_write` on all three responses and on both store types. `movable` was available — the
board seam already declares it — and was not taken: the three responses carry the *same* bit
about the same permission, and a card called `movable` beside a page called `may_write` is
two names for one answer, which is how two answers start. `movable` is also meaningless of a
document. **This is a rename of one line in `web/src/lib/board.ts` when the UI is wired up;**
`readOnly()`'s `=== false` check already has the right shape for a field that may be absent.

## Disclosure

**The bit is asserted only about pages the caller may already read, and it says nothing the
caller could not establish by trying.**

Three arguments, and the second is the one that settles it:

1. **The surface is bounded by the read.** `may_write` does not exist unless the read that
   produced it was permitted. A page somebody may not read answers 403 with no body; a page
   that is not there answers 404. A card whose page they may not read is omitted from the
   board **entirely** — not shown read-only, which would be the disclosure with the name
   filed off — and a project whose home page they may not read is omitted from the listing.
   `a_page_the_caller_may_not_read_says_nothing_about_writing_it` pins that structurally.

2. **It is a fact about the reader, not about the page.** ADR 0009 had to weigh a real
   disclosure: a name is *somebody else's*, and putting it on a card told a reader something
   about a third party. This is the caller's own reach, and they can already establish it
   exactly — press the control, get 403 or 200. The change is that they learn it before
   acting rather than after, which is a change in courtesy, not in what is knowable. It is
   the same thing `/api/me` already does with groups and baseline, one scope narrower.

3. **It cannot be a number about what was hidden.** The board's standing rule is that nothing
   in the answer may count what the filtering removed — no total, no `omitted`, no id of a
   card that was dropped. `may_write` is per row and identical whether the board holds one
   card or forty, so there is no arithmetic to do with it and nothing about the omitted rows
   to recover from it. That is why it is allowed onto a card while a count is not, and the
   structural key tests were updated deliberately rather than relaxed.

One thing it does newly say, stated rather than left implied: a reader of a page learns
whether *they* may edit it, which on a page with no grants of their own tells them their
access is read-only. That is the intended effect.

## Consequences

- The interface can stop offering controls it knows will be refused, and can mark a card
  read-only instead of losing the move. Each of the five places above becomes a one-line
  read of a field that is now there.
- **The bit and the refusal cannot drift.** They are the same `permits()` call on the same
  inputs. `the_write_bit_agrees_with_what_a_write_actually_does` (gw-store) and
  `may_write_on_the_wire_agrees_with_what_a_write_actually_does` (gw-api) assert exactly that
  and assert it *by performing the write*, for four callers who are refused for four
  different reasons — a grant, a grant that stops at Read, the admin baseline D-M2-8 says
  confers no write, and nobody. Neither test compares the boolean against a written-down
  expectation, and both refuse to pass if all four callers answer the same way.
- **Nothing may compute this bit a second time.** A handler that decides "may I write this"
  for itself reopens this decision, whatever it agrees with today. The store's accessor is
  the only place it is produced.
- `may_write` is derived per request and never stored. A stored one is a verdict that
  outlives the access it was granted under, which is the same rule ADR 0009 sets for a card's
  assignee name.

## Switch-back criteria

Revisit if a write is ever gated on something other than `Action::Write` on one page — a
per-document lock, a workflow state, an approval step. At that moment `may_write` would stop
being the whole answer for at least one control, and the honest fix is another named bit
derived the same way, not a broader reading of this one.
