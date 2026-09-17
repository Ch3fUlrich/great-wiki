# 0020 — How an embedded page and a heading's anchor are written in markdown, and what an embed discloses

**Status:** Accepted (2026-09-17)

## Context

D-27 asked for **transclusion**: a block that shows another page, or one section of it, live.
D-28 settled that it is framed with its source named, D-29 that an orphaned one stays and says
so, and D-30 that a quoted checklist is read-only with live state. The permission rule was
written before any of it was built, in
[the rich-blocks plan](../superpowers/plans/2026-09-02-rich-blocks.md#the-transclusion-permission-rule-if-it-is-ever-built),
and it is the rule this decision implements rather than one it invents.

Two questions the code could not answer had to be settled first, and they are what this record
is for:

1. **How an embed is spelled in an exported markdown file**, in both directions, and what it
   costs a restore.
2. **How a heading acquires an anchor that survives being re-worded**, and whether that anchor
   is in the file or only in the database.

[ADR 0015](0015-how-a-placed-file-is-written-in-markdown.md) (a placed file) and
[ADR 0019](0019-how-a-document-reference-is-written-in-markdown.md) (a document reference)
already answer the first question for two other constructs, and their reasoning transfers
almost unchanged. The second has no precedent at all: nothing in this system has ever needed a
stable name for a *part* of a page.

## Decision

### An embed is `![Beschriftung](einbettung:<ziel>#<abschnitt> "/pfad")` — an image, not a link

`gw_core::markdown` holds both halves, beside `attachment_destination`/`attachment_reference`
and `document_destination`/`document_reference`: `transclusion_destination` writes one and
`transclusion_reference` reads one. A scheme rather than a shape, German because this wiki is,
both halves in the lower crate because two copies of one rule in two crates stop agreeing the
day one is edited — ADR 0015's three arguments, unchanged.

**It goes through the image path rather than the link path**, and that is the one genuinely new
choice here. An embed is a top-level **atom** with no inline content, which is exactly what a
placement is and exactly what a link is not:

- CommonMark renders an image's description as a **plain string**, so the author's label cannot
  carry marks that would then have to round-trip. A link's content can, and a block that stored
  marked-up inline content would need somewhere to put it — `Block::attrs` holds strings.
- The "alone in its own top-level paragraph" rule
  (`Builder::placement_is_possible`, `Builder::settle_placements`) is then the *same rule*,
  reused rather than restated, and a reference in a position that cannot hold a block degrades
  to its own text exactly as an ordinary image does.
- The alt slot is already markdown's place for *what to show when the thing itself cannot be
  shown*, which is precisely what an embed's label is for: it is what a reader who may not read
  the target sees, and nothing else.

`<ziel>` is a document id **or** a rooted wiki path; `#<abschnitt>` is a heading's stable id, or
absent for a whole page. The stored block carries `doc` **or** `path` and never both — the
invariant `gw_core::Mark` states about a link, one layer up — plus `heading` and `label`.

**Rejected: a link, `[Titel](einbettung:<id>)`.** It reads more naturally and costs the two
things above.

**Rejected: a fenced block with YAML in it.** It would make an embed invisible to every other
markdown renderer *and* give a writer a place to put a fifth, sixth and seventh attribute —
which is the trap `CODE_BLOCK_ATTRS` exists to close.

**Rejected: a new `MarkKind`.** An embed is a block; a reference inside a sentence is what D-21
already built.

### The target's path rides in the link-title slot, exactly as a reference's does

ADR 0019's argument verbatim, because the cost is identical: a seed mints every document id
anew, so no `einbettung:<uuid>` in a restored corpus can match anything, and an export loaded
into a fresh database would keep every page and frame nothing at all.

So the exporter writes the target's **current** path in the title slot, `Store::references_for`
answers what that path is (and therefore withholds it for a target the exporting account may
not read), `settle_embeds` ends the pair inside the publish transaction, and `Store::resolve_embeds`
exchanges the path back for *this* database's id at the next publish, for a page the **author**
may read. The order between the two outcomes is the whole decision and it is 0019's: **a live
id wins against the path a file carried**, because a page renamed after the export has a stale
path and a perfectly good identity.

One thing is better here than for a link, and it is worth naming: an embed named by **address**
is a first-class stored form that the reader resolves through `Store::document_for` — the same
permission-checked accessor a page read ends in. So a restored corpus's frames *work*
immediately, rather than waiting for a republish. A link has the same property for the same
reason (an unresolved `href` is still a link); an embed simply needed it stated, because a
block-level reference could have been made id-only.

`path` is deliberately **absent from `EMBED_ATTRS`**, so `reduce()` discards it from both sides
of the round-trip comparison. Without that, a stored `{doc, label}` and the `{doc, path, label}`
its own markdown re-imports as would differ and every page holding an embed would be refused
from every export from then on — the `LINK_ATTRS` incident a fourth time.

### A heading's stable id is minted on publish, for every heading, and is written into the file

A section needs an anchor that survives an edit. A heading's words change and its position
certainly does, so the slug `Block::headings` already computes is not one: it is exactly right
for a fragment somebody copied out of the address bar an hour ago, and exactly wrong for this.

So `Heading` gains an `id` attribute — a uuid, minted by `gw_store::transclusion::mint_heading_ids`
inside `append_revision`, beside `reconcile_tasks`, on the same principle: identity is the
store's to mint, `gw_core::markdown` is a pure function and mints none, and what is **stored**
has to be what was minted into.

**For every heading rather than only the transcluded ones.** The alternative needs to know who
embeds this page *before* this page is published — knowledge of another document's body, at the
wrong moment, and unobtainable without reading pages the author may not read. Minting on the
*target's* behalf when an embed is created is worse: it writes to a page the author may have no
right to write. Minting for all costs one visible suffix per heading in the export and makes
the editor's section picker able to offer something the first time anybody looks.

**And the id is written into the markdown**, as ` {#<uuid>}` after the heading's words.
`heading_anchor` writes it, `heading_anchor_id` reads it, and the writer asks the reader rather
than restating its rules.

**Rejected: reducing it away like a task's id.** `TASK_ITEM_ATTRS` keeps `checked` and discards
the uuid beside it, and the parallel is exact — right up to the consequence. A task's id being
lost on a re-seed costs the board its cards' due dates, which is real and bounded. A heading's
id being lost on a re-seed orphans **every section embed in the corpus**, on pages nobody
touched, with **no fallback available** — because the anchor's whole point is that it is not
derivable from the heading's text, so there is nothing for a carried fallback to carry. That
turns a restored backup from "complete" into "readable", which is the exact trade ADR 0019
refused.

**Rejected: `Options::ENABLE_HEADING_ATTRIBUTES`.** pulldown-cmark will read `{#id}` for us, and
turning it on is a **global re-parse of every heading in the corpus**: a heading whose words
happen to end in `{…}` would come back as different text, differ from what is stored, and be
refused from every export from then on — a page nobody edited becoming unexportable, which this
project treats as disqualifying (the reason inline maths is out of scope, in the same plan).
Reading the suffix ourselves lets the rule be as narrow as it needs to be: **only a uuid-shaped
id counts**, which is ADR 0019's `dok:etwas` argument run again. `content-example` and
`content-darm` were both checked and contain no `{#` at all.

**The last suffix wins**, and that is what makes the pair total rather than usually right: the
exporter always writes the real id last, so `## Dosis {#0199…} {#<real>}` reads back as the text
it was. The one shape the syntax cannot carry is a heading that has **no** id and whose words
end in an anchor-shaped suffix; `gw_api::export` refuses that page loudly rather than writing a
file that would re-import as something else, and one publish (which mints the id) ends it for
good.

### What an embed discloses

> An embed is resolved through the store, **per document, against the reader**, by the same
> accessor a page read itself ends in. A target the reader may not read, one that does not
> exist, one in the Papierkorb and one that was purged all render **identically**: the frame is
> replaced by the author's own label, with no title, no address, no words and no count. The
> verdict is re-asked on every read, so a revoked grant empties the frame at the next request
> with nothing to clean up.

The split is the author/target line, exactly as it is for a reference. *The label is the
author's* — they wrote it, into a body this reader is already reading. *The title, the path and
the words are the target's*, and they are **live**, so a frame written when the reader could
still see the page would otherwise go on quoting it after their access was removed.

**What is NOT withheld is the stored target itself**, and this is the one place the rule is
weaker than it looks. `documents.body` is served verbatim to anyone who may read the host page,
so the `doc` or `path` the author wrote is in the response whatever the frame renders — as
`data-einbettung` on the frame, and in the page's own data either way. Withholding the block
would hide nothing and would make the page differ from what its author wrote, from what the
editor holds and from what the exporter writes; ADR 0011's stronger shape ("answer as if it
does not exist") is unachievable here for that reason.

So the disclosure has to be named rather than implied: a reader who may not read the target
nonetheless learns **that the author of this page named something**, and learns either a v7
uuid — whose first 48 bits are the millisecond the target was created, which is enough to
correlate a hidden page with a known event — or, for an embed this database has not identified
yet, **the address the author typed**. The second is not a new class: an internal `href` nobody
could resolve renders as the author's own address today, and blanking it would corrupt somebody's
page to hide a string they put in it. What must not happen, and does not, is that the address
becomes something a reader can follow: the stored target never reaches `safeHref`, and an
unresolved frame is not a link.

**Depth is one.** A transcluded page's own embeds render as their labels. Two reasons, and the
second is load-bearing: it closes the cycle question by construction rather than by remembering
to carry a visited set, and it bounds the authorisation cost — `Store::open` keeps
`max_connections(1)`, so per-embed authorisation raised to the power of the depth is a lever on
the availability of the whole deployment.

**A cycle is allowed to exist, is stopped at render, and is named.** Refusing A's publish because
B embeds A would require reading pages the author may not read, and `links` is a plain edge table
with no acyclicity constraint. So it is caught where both ends are known — a bounded walk, sharing
one budget across the page, through the same permission-checked accessor, so a ring passing
through a page this reader may not see is simply not found. That is right rather than merely
convenient: naming it would disclose that page's title.

**Sixteen embeds per page.** `MAX_REFERENCES_PER_PAGE`'s reasoning with the numbers moved:
expanding one costs an authorisation, a row, a JSON parse of somebody else's whole body and a
second resolution pass over what comes back. Over the cap the remainder render as their labels —
the state a forbidden target is already in — so the cap discloses nothing and needs no message.

### An embed records an ordinary `links` row and contributes nothing to `plain_text`

The row, because an embed is at least as strong a connection as a link — it puts another page's
words on this one — and because `collect()` walks **marks**, so a block attribute is invisible
to it unless it is read there deliberately. It inherits `graph_for`'s both-ends filter for free.

Nothing to `plain_text`, because that is computed in `gw_core`, which has no store and cannot
ask a permission question: feeding another page's words in would put content the reader may not
see into this page's anchor ids and, at M7, its search index. The known cost is the one ADR 0015
already records for placements — `diff_structure` fingerprints a block as kind plus text, so two
embeds look alike to the structural diff and swapping one target for another shows up as a
*design* change instead.

### A quoted page's own files and references belong to that page

Two smaller decisions that follow from "the frame shows somebody else's page":

- **Its references are resolved against the reader, one document further out.** `Embed::references`
  carries them. Reusing the host page's map would render every `dok:` inside a frame as unlinked
  words, which is the state that means *"you may not read that"*, asserted about pages the reader
  very possibly may.
- **A file placed in a quoted page is named, not drawn.** Resolving it against the host page's
  `Anhänge` list would make every picture inside a frame read the German "not attached" sentence
  — a false statement about somebody else's page — and guessing an address would bypass D-16,
  which authorises a download against the page it was reached through. The plan's version of this
  said "carries the embedded page's own list, or it does not render"; a third option satisfies
  what that was protecting without a second permission-checked fetch per frame, and it is what
  ships: the frame names the file and links to the page it is shown on.

## Consequences

- **Every exported heading now carries a uuid.** That is a visible change to what the backup
  looks like, and `FIDELITY_WARNING` says so in the directory rather than leaving it to be
  noticed. It is the price of a restore that keeps its frames.
- **A task's id is forgotten by a re-seed and a heading's is not**, and the export round-trip
  test now asserts both halves in one run. Before this change the corpus held no checklist at
  all, so the first half had never been exercised.
- **The editor must declare `heading.id` and all four of an embed's attributes**, or y-tiptap
  deletes them from the Y.Doc and broadcasts the deletion. For `heading.id` the blast radius
  reaches *other people's pages*: the next publish mints a fresh id and every embed of that
  section, anywhere in the wiki, becomes a D-29 orphan about a section nobody removed. This is
  `TaskItem::id` again with a wider reach, and it is pinned by
  `extensions.test.ts::keeps a heading's stable anchor, which every section embed of this page depends on`.
- **`BlockKind`'s doc comment understated its own mirrors and now says so.** Adding this kind
  moved seven, not four-plus-a-soft-fifth.
- **The graph's edge lines gained `data-von`/`data-nach`.** `content-example` gained an embedding
  page, an embed is an edge, and the behaviour harness's "exactly one edge" check had nothing to
  point at once there were two. Both ends' paths are already in the accessible list beside the
  drawing, so this discloses nothing new.
- **The editor's section picker needed a route of its own.** A heading's stable id lives in
  the target page's body, and the editor holds a Y.Doc of *this* page and has never seen that
  one — so `/_abschnitte/{pfad}` proxies `GET /api/documents/{path}` with the caller's own
  cookie and reduces the answer to a list of headings. It decides nothing: the API's 403 and
  404 are handed back untouched, and a page the caller may not read is not in the picker to be
  chosen in the first place. A heading with no anchor is left **out** rather than offered,
  because offering it would mean writing an embed with no anchor to write, and D-29's orphan
  frame is a state to reach by deletion rather than by construction.
- **A formula or a listing inside a frame is typeset and highlighted**, because the page's `load`
  walks the embedded bodies too. The per-page caps then bound the page as a whole, which is the
  right place for them: an embed is exactly a way to have more fences than the page appears to
  hold.

## Switch-back criteria

Revisit if any of these becomes true:

- **The seeder learns to preserve document ids** (an `id` key in frontmatter). The carried path
  becomes unnecessary — and so, arguably, does writing heading ids into the file, since a whole
  corpus would then restore by identity. It is a decision of its own, and it makes an id
  something a hand-written file can assert, which is a different threat model.
- **Depth is raised above one.** That makes a visited set mandatory rather than structural, and
  the cycle walk stops being a diagnostic and starts being load-bearing. Read `Embed::cycle` and
  `MAX_CYCLE_WALK` first.
- **Something else starts writing a heading suffix**, or an author needs `{#…}` at the end of a
  heading as literal text. The narrow uuid rule is what makes the current spelling safe; widening
  it is the `ENABLE_HEADING_ATTRIBUTES` decision by another route.
- **`documents.body` stops being served verbatim.** The disclosure paragraph above rests on it.
  If a reader is ever handed a filtered body, withholding an unreadable embed's target becomes
  possible and should be reconsidered on its merits.
- **An embed gains a fifth attribute.** `EMBED_ATTRS`, the editor's schema and this record move
  together, or a page is refused from the backup permanently.
