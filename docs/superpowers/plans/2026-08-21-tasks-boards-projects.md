# Piece 3 — tasks, boards and projects

**Design:** [2026-08-15-links-topics-tasks-design.md](../specs/2026-08-15-links-topics-tasks-design.md),
decisions D-1 through D-11. That document says *what* was chosen and why the rejected options
were rejected. This one says *how*, and answers the one question D-10 deliberately left open.

Piece 3 is the largest of the five by a wide margin. It is also the first that cannot be
built at all until something else is built first, so the order below is not a preference.

## The floor comes first, and it is five layers deep

D-11 found it while the design was being written: `gw_core::markdown` had no task variant in
`BlockKind` and no way to keep a checkbox.

**Step 1 is done (2026-08-21), and it corrected the diagnosis.** The marker was not being
dropped by `Unsupported::TaskListMarker` — that arm was unreachable, because
`Options::ENABLE_TASKLISTS` was off and pulldown-cmark emitted no marker at all. The brackets
arrived as ordinary text, so `- [ ] etwas` imported as a bullet whose *words were* "[ ] etwas".
The converter now enables the extension, `BlockKind` carries `TaskList` and `TaskItem`, and the
variant has been replaced by `Unsupported::OrderedTaskList` for the one loss that is left.
**`gw_api::export` still refuses a `taskList`**, so step 1's export half is the next thing to
build and nothing above it can be tested end to end until it is.

**And step 5 is now urgent, not last.** Three lists mirror `BlockKind` by hand and *none* of
them is derived from it, so adding a kind breaks nothing and no suite goes red:

| Where | What it is | Consequence while it lacks `taskList`/`taskItem` |
|---|---|---|
| `web/src/lib/blocks/render.ts` → `BlockKind`, and `BlockView.svelte` | the reader | the checklist renders as **nothing** — an unknown block is skipped by design |
| `web/src/lib/editor/extensions.ts` → `SERVER_BLOCK_KINDS`, pinned against a hand-copied `SERVER_KINDS` in `extensions.test.ts` | the editor's schema | **data loss.** `extensions.test.ts`'s own header documents it: `createNodeFromYElement` catches the unknown node name and *deletes the element from the Y.Doc*, broadcasts the deletion, and the janitor files it as a revision. Silently. |
| `crates/gw-collab/src/fixtures.rs::one_per_kind` | the CRDT round-trip fixtures | the new kinds are simply untested through Yjs |

Nothing existing is at risk — the seeded content holds zero checkbox lines — but a page
seeded with one is destroyed the first time somebody opens it in the editor. Land the web
schema before any content with a checkbox does.

So D-6 — *any checkbox line is a task* — is not implementable today. A checkbox has to survive
import, export, the CRDT, the editor and the store before a single card can appear on a board.
This is the same five-layer floor piece 0 needed for marks, for the same reason: `Block`
mirrors ProseMirror exactly, so anything the editor can express has to exist in `Block` too.

One consequence worth stating because it is easy to get backwards: the model is
`taskList > taskItem{checked}`, matching TipTap's own extensions, **not** a `checked`
attribute on `listItem`. A translation layer between our model and the editor's is precisely
what mirroring ProseMirror exists to avoid.

### Mixed lists split; they do not upgrade

```
- [ ] a
- plain
```

A run of consecutive checkbox items becomes a `taskList`; a run of plain items stays a
`bulletList`. The alternative — upgrading the whole list and giving the plain item
`checked: false` — would fabricate a task from a line nobody marked, which is exactly the
cost D-6 was weighed against and accepted only because it does not happen. Splitting keeps
"which lines are tasks" exactly as written.

### The converter does not mint ids

The data model gives a task block a uuid in `attrs`. That uuid is minted by the **store**,
during reconciliation on publish — never by `gw_core::markdown::convert`, which must stay a
deterministic pure function. `gw_api::export::render_file` re-imports its own output and
compares it against the stored document; a randomly minted id would fail that comparison on
every export, forever. `comparable()` must therefore reduce a `taskItem`'s attrs to `checked`
alone, the same way `reduce_marks` already reduces a link's attrs to `href`/`doc`.

## D-10's open question, answered

D-10 gave a task a status, a due date and an assignee, and then said plainly that nothing in
this system answers *who may assign whom* — and that the plan must state a rule and a test
rather than leave assignment ungoverned. The rule:

> A task's **governing page** is its anchor document, or — for a standalone task — its
> project's home page.
>
> 1. You may create or modify a task, **including setting its assignee**, if you may *write*
>    the governing page.
> 2. You may only assign a task to somebody who may *read* the governing page.
> 3. You may always **unassign**, if you may write the governing page.

Clause 2 is the one that matters and the one to test hardest. Assigning somebody to a task on
a page they cannot open would hand them an obligation they are unable to see — and the
assignment itself would tell them the page exists, which is the disclosure the design's
Security section is entirely about. It is the board's version of the same leak a graph edge
would be.

Clause 3 exists so that a stale assignee can be cleared after that person loses read access.
Without it a task could become permanently stuck to somebody who can no longer see it, and the
only fix would be deleting the task — discarding the due date somebody set, which is the
outcome D-8 exists to prevent.

Assignment is governed by the page, not by the project, because permissions in this system
are a property of the tree and nothing else. A second answer would be a second source of
truth.

## Order of work

| | Step | Touches | Depends on |
|---|---|---|---|
| 1 | `taskList`/`taskItem` in `Block`; import, export, round-trip | `gw-core` | — |
| 2 | `tasks` + `projects` tables, permission-filtered accessors, the D-10 rule | `gw-store` | — |
| 3 | Reconciliation on publish: block ↔ record, detach per D-8 | `gw-store` | 1, 2 |
| 4 | Task, board and project endpoints | `gw-api` | 2, 3 |
| 5 | TipTap `TaskList`/`TaskItem`, checkbox rendered *from the record* | `web` | 1, 4 |
| 6 | The board itself — three fixed columns, drag to change status | `web` | 4 |

Steps 1 and 2 are independent and were built in parallel. Everything after 3 is a straight
line, because a board with no reconciliation shows an empty column and proves nothing.

## What reconciliation must not do

D-2 is the whole design in one sentence: **the page owns the words, the record owns the
state.** On publish, the document's task blocks are reconciled against the table — text is
taken from the block, state is left untouched. Dragging a card changes only the record and
never rewrites a page.

Get this backwards and the board files revisions nobody typed, needs write permission on a
page for a drag, and collides in the CRDT when two people move cards that came from one
document.

A record whose block no longer appears is **detached**, not deleted, and stays on its board
with a marker (D-8). Retyping a line therefore produces a *new* task and leaves the old one
visibly detached, rather than quietly mutating one task into another.

## Out of scope

Per-project columns (D-9 chose fixed ones and says why going fixed → configurable later is a
contained migration and the reverse is not), task comments, recurring tasks, notifications,
and any task view that is not the board or the page the task was written on.

## The property to write mutation tests against

The design names it: every aggregate view here is a disclosure surface. A board card reveals
that a page exists and what it is called, exactly as a graph edge does. Every card, row and
count must be filtered through the same permission-checked accessor a page read uses — and
because of D-3 that filtering is **per document**, not per subtree. A project spanning two
subtrees with different grants is normal, not exceptional.

This is the property most likely to be quietly lost by an aggregate query written in a hurry,
which is why it is the one that gets mutation-tested rather than merely unit-tested.
