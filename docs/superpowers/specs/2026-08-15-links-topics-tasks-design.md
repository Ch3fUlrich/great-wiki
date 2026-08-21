# Links, topics, tasks and attachments — design

**Date:** 2026-08-15
**Status:** Accepted by the owner, 2026-08-15
**Supersedes the sequencing in:** the roadmap's M4 / M5 / M7 / M9 / M13 split

## Why

The owner keeps their knowledge in Joplin and wants to stop. Their reasons, in their words:
linking files, groups and topics is too difficult, building a graph is too difficult, it
forces an app, it has no web interface, and multiple users are behind a paywall.

The last three great-wiki already answers. This design is about the first two — and about a
requirement Joplin does not attempt at all:

> it should be possible to have the documents that are under a group be part of a project
> … a lot of documents can have not only a number of text, but also a to-do list and other
> plans. those should be linkable in the document files as well as visualized via blocks of
> todos or kanban tasks (slices of the whole).

So: a document is prose *and* project material, and a to-do written while planning must also
appear on a board without being copied there.

**Migrating the existing Joplin content is explicitly not required.** The owner said so. That
removes an importer from the critical path and frees the content model from having to mirror
Joplin's structure.

## The floor

`Block` is `{kind, attrs, content, text}`. It has no marks and no href, so **a link cannot be
represented in this system at all**. Every feature below is blocked on the same change.

The same gap is already costing something today: `gw-collab`'s CRDT carries inline marks
faithfully — a browser can bold a word and the emphasis is in the document — but publishing
snapshots to `Block`, which drops it. The editor therefore ships with no formatting controls
at all, deliberately, rather than offering something the system throws away. Lifting this is
overdue on its own merits.

## Decisions

Each was chosen by the owner from stated alternatives. The reasoning is recorded because the
rejected options were reasonable and someone will propose them again.

### D-1: Tasks exist both inside documents and standalone

A to-do can be written as a line in a page, and a card can be created on a board that belongs
to no page. Rejected: document-only (a board could not hold anything not already written
somewhere) and record-only (a page would stop being the whole truth).

The cost is that "task" must not mean two different things. It does not: there is **one task
record**, with an optional anchor to the block that authored it. Standalone means the anchor
is null. Every view, query and permission check sees one kind of row.

### D-2: The page owns the words, the record owns the state

For an anchored task, the **text lives in the document block** — it is prose, it round-trips
through markdown, and it is edited by editing the page. The **workflow state** (status,
assignee, due date, position) lives on the record. Dragging a card changes only the record and
never rewrites a page. The page renders its checkbox *from the record*, so the two cannot
disagree.

Rejected: the board editing the page, which would file revisions nobody typed, need write
permission on that page for a drag, and collide in the CRDT when two people move cards from
one document. Also rejected: read-only cards, which would exclude exactly the tasks people
write while planning.

Consequence to accept: a raw markdown export carries the task's text but not its column. The
file is a faithful copy of the prose, not of the board.

### D-3: A project is a home subtree plus tagged extras

A project has a home page, and everything beneath it belongs to that project. Documents
elsewhere can additionally be pulled in by tag.

Rejected: subtree only (cannot pull in a page that lives elsewhere) and tag only (loses the
free alignment with permissions).

**Consequence, and it is load-bearing:** membership can no longer be decided per branch, so
every project query filters **per document** through the permission-checked accessor. A
project view spanning two subtrees with different grants is normal, not exceptional.

### D-4: The graph shows documents and the links between them, nothing else

Nodes are pages. Edges are links somebody deliberately wrote, plus their backlinks. Topics
are **not** nodes.

Rejected: topics-as-hubs (the recommendation at the time) and everything-as-nodes. The owner
chose the sparse graph where every edge means a person made a connection.

**Consequence:** topics are invisible in the graph, so browsing by topic needs its own view —
a topic page listing its documents. That is now a requirement rather than a nicety, because
it is the only way topics are reachable.

### D-5: A link points at the page's identity, not its path

A link stores the document id. The title and path are resolved at render time, so renaming or
moving a page cannot break an inbound link and the link text follows the title. Backlinks and
graph edges stay correct with no rewriting.

Rejected: storing paths (moves break links silently, discovered by clicking) and
rewrite-on-move (a move would edit other people's pages, file a revision on each, need write
permission on all of them, and leave the tree half-rewritten when refused).

**Consequence:** markdown export resolves ids to paths at export time. A file on disk is a
snapshot, not a live pointer — consistent with the existing export, which already states that
it is a faithful copy of the database rather than of the markdown that was imported.

### D-6: Any checkbox line is a task (2026-08-20)

`- [ ] Stuhlprobe einschicken` written anywhere in a page is a task, with no block to mark
and nothing to opt into. Markdown's own syntax is the interface.

Rejected: requiring an explicit task block (a concept to learn, a step to forget, and a
to-do that never appears is the failure you notice last) and restricting it to project pages
(makes projects a prerequisite for writing a to-do).

The cost is that a throwaway checklist becomes board material. Measured before accepting it:
the owner's 35 live pages contain **zero** checkbox lines against 232 bullet points, so
boards start empty and fill only with what is written deliberately.

### D-7: Existing checkbox lines become tasks on first import (2026-08-20)

No date-dependent rule. Two identical lines in one page must not behave differently because
of when they were written — that is impossible to explain and worse to debug. Safe here for
the reason above: there are none to adopt.

### D-8: A detached task stays on its board, marked (2026-08-20)

When the line that authored a task is deleted, the record is kept (D-2's reasoning: deleting
would silently discard a due date and assignee somebody set). It stays **on the board** with
a marker saying its page no longer mentions it.

Rejected: moving it to a separate list (a place you must remember to visit is a place nobody
visits) and auto-archiving after a period (makes "it vanished" possible again, which is the
outcome the detached state exists to prevent).

### D-9: Fixed columns — Offen, Läuft, Fertig (2026-08-20)

The same three on every board, built in. A task's status is a small closed set the schema can
enforce, every board reads the same way, and no query has to handle a status set it cannot
know in advance.

Rejected: per-project columns (a task moved between projects lands in a column that may not
exist) and renameable labels over fixed states (a column called something else that behaves
like `Läuft` misleads as easily as it helps). Going from fixed to configurable later is a
contained migration; the reverse is not.

### D-10: A task carries status, due date and assignee (2026-08-20)

The full record. Dates matter for a treatment plan, which is what this wiki holds.

**Open question this creates, to be answered in the plan, not here:** nothing in this system
answers *who may assign whom*. Today there is one account, so any answer is untested. The
plan must state a rule and a test for it rather than leaving assignment ungoverned.

### D-11: The floor — a checkbox cannot be represented today (2026-08-20)

Discovered while planning, and it changes the size of this piece. `gw_core::markdown` maps
`Event::TaskListMarker` to `Unsupported::TaskListMarker` and **drops it**; `BlockKind` has no
task variant. So `- [ ] etwas` imports as a plain bullet with the checkbox gone.

D-6 ("any checkbox line is a task") therefore cannot be built until a checkbox survives
import, export, the CRDT and the editor — the same five layers piece 0 needed for marks, and
for the same reason.

`Block` mirrors ProseMirror exactly, deliberately, so this follows the editor's own model:
`taskList` and `taskItem` as block kinds, matching TipTap's extensions, rather than a
`checked` attribute on `listItem`. A translation layer is the thing that model exists to
avoid.

**Import closed, 2026-08-21 — and the diagnosis above was one step off.** The
`Unsupported::TaskListMarker` arm was never reached: `Options::ENABLE_TASKLISTS` was off, so
pulldown-cmark emitted no marker at all and the brackets arrived as ordinary text. `- [ ] etwas` did not
import as a plain bullet — it imported as a bullet whose *words were* "[ ] etwas", in the
page, in the search index and in every anchor derived from it. The converter now enables the
extension, `BlockKind` has `TaskList` and `TaskItem`, and a mixed list splits into a task run
and a plain run rather than fabricating `checked: false` on a line nobody marked. The variant
is gone; a numbered list holding checkboxes reports `ordered-task-list` instead, because a
task list is unordered and those lines lose their numbers.

Four layers of the floor remain: export (`gw_api::export` refuses a `taskList` today), the
store's reconciliation, the CRDT and the editor.

## Data model

```
documents ──< links >── documents     from_doc, to_doc, from_block   ← the graph is this table
     │
     ├──< document_tags >── tags      topics; browsable, not in the graph
     │
     └──< tasks                       id, anchor(doc_id, block_id) NULL ⇒ standalone
                                      title (used only when standalone)
                                      status, assignee, due_at, project_id, position
projects                              id, home_path, tag_id (for extras)
attachments                           id, doc_id, blob path, media type, size, uploaded_by
```

### Task identity, which is the crux

A task block carries a uuid in `attrs`, minted when the line is created. On publish the
document's task blocks are reconciled against the table: text is taken from the block, state
is left untouched.

A record whose block no longer appears is **detached**, not deleted. Deleting it would
silently discard a due date and an assignee that somebody set on a board. Retyping a line
therefore produces a new task and leaves the old one visibly detached, rather than mutating
one task into another or losing it.

## Security

**A graph is a disclosure surface, and so is every aggregate view here.** An edge to a page
the viewer may not read reveals that the page exists; the node label reveals its title. The
same is true of a backlinks panel, a board card and a project listing.

Every node, edge, card and row must be filtered through the same permission-checked accessor
a page read uses — `Store::document_for` and the one `can()` — never a second answer. Because
of D-3 this filtering is per document, not per subtree.

This is the property most likely to be got wrong by an aggregate query written in a hurry, so
it is the property to write mutation tests against.

## Decomposition

Five pieces. Each gets its own implementation plan.

| | Piece | Delivers | Depends on |
|---|---|---|---|
| **0** | Marks and links in `Block` | Formatting stops being lost; links become expressible | — |
| **1** | Links, backlinks, graph | Page-to-page connections and a graph | 0 |
| **2** | Topics | Tagging and browsing by subject | — |
| **3** | Tasks, boards, projects | Kanban and to-dos over documents | 0 |
| **4** | Attachments | Files stored beside notes | — |

**Build order chosen: 0, then 1.** Then 2, 3 and 4 in an order to be decided.

Piece 0 is unavoidable and small. Piece 1 is what the owner named first. Piece 3 is the
largest by a wide margin. Piece 4 is required — the owner confirmed they need files with
their notes — and is a subsystem of its own: blob storage on the existing NFS mount, metadata
in SQLite, and a download path that is its own disclosure surface.

## Scope of the current round (pieces 0 and 1)

**In:**

- `Block` gains inline marks (strong, emphasis, code, strikethrough) and a link mark carrying
  either a document id or an external URL.
- The markdown importer and exporter carry marks and links both ways, replacing the
  `inline-marks` and `link` loss notes the seeder currently emits.
- The editor gains the formatting controls it deliberately withheld, and a way to link to a
  page.
- Publishing extracts links into a `links` table.
- A backlinks panel on every page, permission-filtered.
- A graph view of documents and links, permission-filtered, scoped to a page or a subtree.

**Out, deliberately:**

- Topics, tasks, boards, projects, attachments — pieces 2, 3 and 4.
- Importing from Joplin. Not required, per the owner.
- Full-text search. Still M7; the graph is not a search substitute and should not pretend.
- External link checking. A link to the outside world is not this system's to validate.

## Risks

- **The existing 35 pages have no marks to recover.** Their emphasis and their 89 source URLs
  were dropped at import, before the CRDT existed. Piece 0 makes marks *possible*; it does not
  restore what was already lost. Recovering those means re-importing from the markdown in
  `content-darm/`, which still holds them. That re-import should happen once piece 0 lands,
  and it is the last cheap moment — anything edited in the wiki first would be overwritten.
- **Reconciliation on publish is new machinery on the write path.** Publishing already writes
  a revision inside one transaction; extracting links joins it. A failure must take the whole
  thing back, exactly as document creation and its first revision already do.
- **The graph's query is the one most likely to leak.** See Security.
- **One refusable page fails the whole export, and page content is writer-controlled.**
  `export::run` collects refusals and `main.rs` turns a non-empty list into a `bail!`, so a
  single page the round-trip guard rejects means no backup at all. Anyone with write on one
  page can produce such a page — a literal newline in a text leaf is enough, and the
  collaboration socket applies raw CRDT updates without schema validation, so `attrs_to_marks`
  will carry any object-valued attribute into a mark. Pre-existing and not introduced by piece
  0. The fix is per-page skip or an `--allow-incomplete` flag, or validating CRDT-derived
  blocks at the collab boundary rather than at the exporter.
- **`render()` can still report `problems: []` for markdown it cannot express.** Found while
  building piece 0 and left open deliberately. `Renderer::delimited` implements only half of
  CommonMark's flanking rule — it handles a delimiter abutting a space but not one abutting
  punctuation — and nothing stops two adjacent delimiter runs fusing. `**Vor-**und Nachteile`
  is enough. Exhaustively: 364 of 2304 two-leaf paragraphs and 205 of 2744 generated sources
  render to something that re-imports as a different document, while reporting no problem.
  It is **pre-existing and 80% smaller** than before that pass, `render_file` re-imports and
  refuses before writing so the export command cannot corrupt a file, and neither corpus
  reaches it. What is wrong is the contract: `Rendered::problems`' own doc says empty means
  the tree was fully expressible, `blocks_to_markdown` is public, and the visible symptom is
  a page refused as "a bug in the exporter" for perfectly ordinary markdown.
