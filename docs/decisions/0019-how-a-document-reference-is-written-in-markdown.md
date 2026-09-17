# 0019 — How a document reference is written in markdown, and what one discloses

**Status:** Accepted (2026-09-17)

## Context

D-5 decided that a link to another page of this wiki stores the target's **document id**, not
its path: the title and the path are resolved when the page is *read*, so renaming or moving a
page cannot break an inbound link, backlinks and graph edges stay correct with no rewriting,
and no move ever has to edit somebody else's page. The alternatives were stated and rejected
there — storing paths (moves break links silently, discovered by clicking) and rewrite-on-move
(a move edits other people's pages, files a revision on each, needs write permission on all of
them, and leaves the tree half-rewritten when refused).

Half the machinery has existed since piece 1 and none of it was reachable.
`gw_core::Mark::link_to_doc` builds such a mark, `Mark::target_doc` reads one,
`gw_store::links` collects them for the graph, `gw_api::export::comparable` admits `doc`
through `LINK_ATTRS`, and `BlockView` drew one as a non-navigating `<span>`. **Nothing in the
system wrote one** — `links.rs`'s own module comment said so outright — so the collector
accepted a wiki-path `href` as well, in order not to leave the graph permanently edgeless.

This decision closes that gap, and it has to answer three questions the code could not:

1. What a reference looks like in an exported markdown file, in both directions.
2. Where an `href` somebody typed becomes an id, and with whose permissions.
3. What a reference renders as when the reader may not read the page it points at.

## Decision

### A reference is written `[Titel](dok:<id> "/aktueller/pfad")` — a scheme, not a resolved path

Both halves live in `gw_core::markdown`, beside `attachment_destination` and
`attachment_reference`: `document_destination` writes one and `document_reference` reads one.

**This is [ADR 0015](0015-how-a-placed-file-is-written-in-markdown.md)'s argument, generalised**,
and both of its halves transfer.

*A scheme rather than a bare shape*, because a bare shape is a guess and this project does not
make those. And *both halves in the lower crate*, because two copies of one rule in two crates
stop agreeing the day one of them is edited, and what that costs is an export that refuses
every page holding a reference.

**Rejected: exporting the resolved path**, `[Titel](/darm/labor)`. It is the obvious spelling
and it is the one combination that permanently breaks the backup. `gw_core::markdown` has no
store, so it reads a path back as `Mark::link_to_url` and can do nothing else; the stored
`{doc: id}` and the re-imported `{href: "/darm/labor"}` differ; `comparable()` compares a link
mark's attributes against `LINK_ATTRS` and finds one missing and one extra; `render_file`
refuses the page. `run()` then writes every other page and continues, so the symptom is an
export directory quietly missing that page under a `FIDELITY` file calling the directory a
faithful copy of the database — and a document that cannot be exported cannot be exported
later either.

**Rejected: a new `BlockKind`.** A reference to a page inside a sentence is inline, and the
mark already exists.

### The target's path rides along in the link-title slot, as a fallback and never as an address

The id alone would have charged the backup path a real price, and it is avoidable. A seed
mints every id anew — `SeedMeta` has no `id` key — so **no `dok:` in a restored corpus can
ever match anything**: an export loaded into a fresh, empty database would keep every word and
lose every internal connection. That is a regression against what a pre-identity export did,
and "the words are intact, the connection is not" is not a cost worth paying when the file
already knows the path.

So the exporter writes the target's **current** path in CommonMark's link-title slot, which
every parser already reads and which nothing else in this system writes (`gw_core::markdown`
throws a link title away everywhere else, and `BlockView` renders none). `document_fallback`
writes it and `document_fallback_path` reads it, both in `gw_core::markdown`, and the writer
asks the reader rather than restating its rules.

`gw_store::links::settle_references` is where the pair ends, inside the publish transaction,
before the edges are read out of the body. **The order between its two outcomes is the whole
decision:**

- **The id names a live document here** — the reference stands and the path is dropped, *even
  when the two disagree*. A page renamed after the export has a stale path in the file and a
  perfectly good identity, and preferring the path would reintroduce exactly the breakage D-5
  exists to prevent, quietly, on a restore.
- **The id names nothing here** — which is what a fresh seed produces — so the mark becomes an
  ordinary `href` to the carried path. `replace_links` records the edge from it, and the next
  publish exchanges it for *this* database's id through `resolve_references`. A restored corpus
  is back where it started rather than merely readable.
- **No path was carried** — an older export, or one whose target the exporting account could
  not read — so the reference stays unresolved and renders as the author's own text.

A mark reaching `documents.body` therefore still carries an id **or** an address and never
both, which is the invariant `gw_core::Mark` states and `Mark::target_doc` depends on.

**The path comes from `Store::references_for` and from nowhere else.** That is the same
permission-checked, capped resolver the reader uses, so a reference to a page the exporting
account may not read contributes no path and is written bare — the file cannot become a way
to learn a restricted page's address.

**The fallback must not leak into the round-trip comparison.** The stored mark is `{doc}`; its
own markdown re-imports as `{doc, path}`. `path` is deliberately absent from `LINK_ATTRS`, so
`reduce()` discards it from both sides, exactly as it discards the attributes TipTap used to
add. Without that, a page holding a reference would be refused from every export from then on
— the `LINK_ATTRS` incident a third time.

### A `dok:` that is not shaped like a document id is an ordinary link, not a broken reference

`document_reference` accepts exactly what `Uuid::now_v7().to_string()` writes — eight, four,
four, four and twelve hex digits — and returns `None` for anything else, which the importer
turns back into a plain `href`.

This is not leniency; it is the same class of failure the decision above avoids. `dok:etwas`
is a string anybody with write access could have typed into the old link control at any point
in the past. Today it stores as an `href`, exports as `[T](dok:etwas)` and re-imports as the
same `href`. The moment a reader accepted anything after the colon, that page would re-import
as `{doc: "etwas"}`, differ from the stored `{href: "dok:etwas"}`, and be refused from every
export from then on — **a page nobody edited becoming unexportable**, which this project treats
as disqualifying.

The live database was checked before the scheme existed, against a copy and never against
`data/` itself: **zero** link marks in `documents.body` or `revisions.body` whose `href` begins
`dok:`, and zero `doc` marks of any kind. `content-example/` and `content-darm/` hold no `dok:`
either. The parser is written to be safe regardless, because the corpus that was checked is
not the only corpus this code will ever meet.

### The writer refuses what it cannot read back

`document_destination` asks `document_reference` rather than restating its rules, exactly as
`attachment_destination` does, so the two halves cannot drift. `None` is a refusal that names
the page.

That refusal is load-bearing rather than tidy. Nothing validates a mark's attributes on the
write path — `gw_collab::attrs_to_marks` copies whatever the Yjs attribute carries — and
`Renderer::wrap` interpolates a link destination with **no escaping at all**. A stored `doc` of
`x) [siehe](https://angreifer.example/` would otherwise export as
`[Titel](dok:x) [siehe](https://angreifer.example/`: a second link the author never wrote, in
the owner's backup file. The round-trip comparison would also catch that, and that is not the
design — it catches it only because the smuggled text happens to re-parse differently.

A uuid contains no character markdown reads as markup, so no escaping is needed rather than
merely omitted.

### An `href` becomes an id on publish, in the store, with the author's permissions

`Store::resolve_references` runs when a revision is published: every `href` naming a page of
this wiki (`links::wiki_path`, unchanged) that **the author may read** becomes `{doc: <id>}`,
and the `href` is dropped in the same breath — a mark carrying both can disagree with itself
the day the target moves, and `gw_core::Mark` says a link carries one or the other.

**Rejected: resolving in the editor.** A client that turned a typed path into an id would be a
second answer to a permission question, computed where the answer cannot be trusted, and kept
in step with the server's for ever.

**Rejected: resolving on import.** `Store::create_document` has no principal to authorise
against, and the importer's job is fidelity to the file. It also makes the restore-safety cost
below strictly worse: an unresolved `href` in a re-seeded corpus is re-resolved on the next
publish, while a baked-in id is not.

**This replaces a paragraph that said the opposite.** `links.rs` used to state that the body is
never rewritten on the way past, because canonicalising would edit what somebody wrote without
being asked — and named what leaving it alone costs in the next sentence: moving the target
afterwards leaves the edge correct and *the link in the body stale*, which is precisely the
breakage D-5 exists to prevent. The owner chose to pay the first cost rather than the second.

**And it runs just before the transaction rather than inside it**, which is a compromise and is
recorded as one. The rewrite belongs with `replace_links` — both derive from the body, both
must describe the revision actually stored, and `reconcile_tasks` does exactly that on the
transaction's own connection. This cannot: authorising a path needs `Store::document_for`,
which goes to the pool, and `Store::open` gives the pool **one** connection; asking it for a
second while a transaction holds the first waits until it times out. So `publish_revision`
resolves first and hands `append_revision` the rewritten body. The body that is stored, the
body the edges are read out of and the body a reader is given are one body, and a rollback
discards all of it. What is genuinely outside the transaction is only the author's own
permission verdicts, a moment stale; the **reader's** verdict, which is the one that discloses
anything, is re-asked on every read.

### What a reference discloses

> A reference is resolved through the store, per target, against **the reader**. A target the
> reader may not read, one that does not exist, one that is in the Papierkorb and one that was
> purged all render **identically**: the reference's own text, unlinked, with no address, no
> title and no tooltip. The verdict is re-asked on every read, so a revoked grant empties the
> reference at the next request with nothing to clean up.

The split is the author/target line, and it is the whole of the rule.

*The words are the author's.* They are in the body, the author wrote them, the reader is
already reading that body, and blanking them would corrupt a sentence in order to hide
something the sentence does not contain.

*The current path and the current title are the target's.* They are exactly what
`backlinks_for` and `graph_for` refuse to disclose — a path says a page exists and where, a
title says what it is about. Worse than either in isolation, they are **live**: a reference
written when the reader could still see the page would otherwise go on reporting that page's
new name after a rename, to somebody whose access was removed.

The four cases answer identically because distinguishing them is itself the disclosure: "you
may not see this" and "there is nothing here" differ only in confirming that something exists.
It is the same closed conflation `Store::document_for` makes everywhere.

**Rejected: a batch `ids → (path, title)` query with no per-target check**, on the reasoning
that the ids came out of a body the reader may already read. They did, and that says nothing
whatever about the pages they point at: the reference would render as a working link to a
restricted page's path, and a click would yield 403 rather than 404 because the API splits
those deliberately.

**A reference to the page it is written on resolves normally**, and that is a deliberate
departure from the plan's first draft, which folded self-reference in with the four above for
uniformity. It is the one case that is not a disclosure at all — the reader is already reading
that page — and the draft was written before an `href` was resolved on publish. With that in
place, folding it in would mean an ordinary same-page link silently becoming unlinked text the
first time its own page was saved. It is still not a graph edge; `replace_links` has always
dropped self-loops, and that is unchanged.

### The picker is filtered by being the tree

The editor's link dialog offers pages by title. That listing is `GET /api/tree`, which is
`Store::tree_for` — filtered per document through the same `can()` a page read goes through,
with a refused branch skipped whole — and **nothing says how many were left out**, because a
count of what was hidden is the same disclosure with the name filed off.

An unfiltered picker would be a whole-corpus existence-and-title oracle for exactly the person
in the threat model: somebody with write on one page who wants to know whether
`/darm/befund-mueller` exists. Building the picker on the listing that already exists means
there is no second listing to filter differently, and `TreeNode` gained an `id` rather than a
new endpoint gaining a second answer.

### The number of references one page resolves is capped

256 distinct ids, in either direction. Nothing caps how many marks a body holds — it is JSON
over the collaboration socket — and every resolution is one authorisation through the single
SQLite connection the whole application shares, so a page carrying a few thousand references,
fetched repeatedly, is a lever on the availability of the entire deployment. Over the cap the
remainder are simply not resolved, which is the state a forbidden target is already in, so the
cap discloses nothing and needs no message.

## Consequences

- **A mark's attributes are not null-filtered by y-tiptap, and a node's are.** This is the
  reusable lesson and it cost the most to find. The editor's `Link` mark must *declare* `doc`,
  or `computeAttrs` drops it, `updateYFragment` writes the loss back and broadcasts it, and
  somebody typing one word in the paragraph destroys the reference for everybody — silently,
  with the export refusing the page afterwards. But a *declared* attribute is one ProseMirror
  **mints**: every ordinary external link becomes `{href: "https://…", doc: null}` the first
  time its paragraph is touched. `LINK_ATTRS` keeps a key without inspecting its value, so it
  is blind to that, and every page holding an external link would be refused from the backup —
  the `target`/`rel`/`class`/`title` incident again, from the other side. So `reduce()` now
  drops a **null-valued** allow-listed attribute, on marks and on the block allow-lists alike.
  Neither edit is safe without the other. `TaskItem`'s `id: { default: null }` is free only
  because a node's attributes *are* filtered (`createTypeFromElementNode` skips a null value),
  so that pattern does not transfer to a mark without this change.
- **Identity buys move-safety, and the fallback is what stops it costing restore-safety.**
  Without the carried path, an export re-seeded into a fresh database would leave every `dok:`
  pointing at nothing. With it, a restored corpus keeps every connection whose target was
  restored too — by path, exactly as a pre-identity export did. Two residual costs remain and
  are real: a link whose target the exporting account could not read carries no path and stays
  unresolved, and a link whose target seeds *later* in the directory walk records no graph edge
  until something republishes the page (pre-existing `replace_links` behaviour for every
  address-shaped link, since a foreign key cannot point at a row that is not there yet). The
  link itself works in both directions of that; only the edge waits.
- **`FIDELITY_WARNING` says so.** It enumerates what the format can and cannot carry, and a new
  scheme that was not named there would be a silent change to what the backup means. It now
  names the scheme and states the re-seeding cost in the directory itself.
- **The `doc` value never becomes an address in the reader.** It is an arbitrary string that
  reached `documents.body` over the collaboration socket without passing any validation. What
  becomes an address is the *path the server resolved*, and it goes through `safeHref` like
  every other one.

### Correction to ADR 0015, 2026-09-17

ADR 0015's Context says that a block which does not survive the round-trip comparison "makes
`export` refuse the page, and **one refusal fails the whole run**". The second half is wrong,
and it is repeated in about sixteen places in this tree. `export::run` pushes a `Refused` and
**continues**, writing every other page; only the CLI's exit code fails, through
`is_complete()` and `main.rs`'s `bail!`.

The correction makes the consequence worse rather than better, which is why it is recorded
here rather than left: the directory exists, is missing pages, and looks like a backup. This
ADR was written from 0015's reasoning and deliberately not from that sentence.

An Accepted ADR records what was decided and when, so 0015 is not edited; it carries a pointer
to this note.

## Switch-back criteria

Revisit if any of these becomes true:

- **The seeder learns to preserve ids** (an `id` key in frontmatter). That would make the
  carried path unnecessary for a restore, and it is a decision of its own — it also makes ids
  something a hand-written file can assert, which is a different threat model.
- **Something else starts writing a link title.** The fallback owns that slot for `dok:`
  destinations only; anything that wanted a title on an ordinary link would have to say how the
  two coexist, and `LINK_ATTRS` would have to move in the same change.
- **Pages gain a move or rename operation.** Nothing in this system renames a page today, so
  the property this decision exists for is proved by tests and not yet by use. A move
  implementation must not also rewrite bodies — that is the alternative D-5 rejected — and if
  it does, this decision is what it contradicts.
- **The export gains a mode meant for other tools to read.** A "portable" export would want
  resolved paths; that is a different artefact and should be a second mode, exactly as 0015
  says about bare filenames.
- **A second resolver appears anywhere** — in the client, in `gw-api`, in a batch query. That
  is the failure this decision is written to prevent, not a change to it.
