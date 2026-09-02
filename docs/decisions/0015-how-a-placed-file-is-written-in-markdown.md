# 0015 — How a placed file is written in markdown

**Status:** Accepted (2026-09-02)

## Context

D-15 decided that a file appears both in a page's `Anhänge` list and, optionally, *inline* —
an image where it belongs, a card for everything else. The list half shipped first. This is
about the inline half, and specifically about what it looks like on the way out.

`gw_api::export::render_file` re-imports its own output and compares it against the stored
tree. A block that does not survive that comparison makes `export` refuse the page, and one
refusal fails the whole run. So the markdown spelling of a placement is not cosmetic: it has
to be a spelling the importer reads back as exactly the same block, or the owner's backup
path stops working the first time somebody puts a picture in a page.

The obvious spelling is markdown's own image syntax, `![Beschreibung](befund.png)`.

## Decision

**A placed file is written `![Beschreibung](anhang:befund.png)`** — a scheme, not a bare
name — **alone in its own paragraph, at the top level of the document.**

### Why a scheme rather than a bare filename

A bare relative name would make the meaning of a document depend on what happens to be
attached to it. Every ordinary `![x](bild.png)` in every markdown file ever imported —
including files written years ago for some other tool, which is precisely the corpus this
wiki was built to receive — would silently become a reference to a file nobody attached. The
page would then render a German sentence saying the file is missing, about a picture whose
author never made any claim about this wiki at all.

`anhang:` is a **statement**. Nothing writes it by accident, and nothing that predates this
feature can contain it. An import either finds that statement or it does not.

The cost is that the export is no longer valid CommonMark that another renderer will resolve:
`anhang:befund.png` is an address no other tool can follow. That is the correct trade here and
it is consistent with what the export already says about itself — `EXPORT-README.txt` states
that the directory is a faithful copy of *the database*, not of hand-written markdown, and
that the files themselves are not in it. A reference that another renderer silently resolved
to the wrong thing would be worse than one it visibly cannot resolve.

### Why alone in a paragraph, and why top-level only

Both rules exist to keep two schemas in agreement, and the consequence of disagreement is
data loss rather than a rendering fault.

**Alone in its paragraph** is markdown's only way of saying "this is a block". An image
sharing a paragraph with words is inline content, and reading it as a block would change
where the surrounding text's leaf boundaries fall.

**Top-level only** is the important one. `listItem`'s ProseMirror content expression is
`paragraph block*`, so a placement inside a list item is a node ProseMirror cannot construct —
and TipTap's answer to a node it cannot construct is to **delete the element from the CRDT and
broadcast the deletion**, which the next sweep files as a revision. A placement the exporter
writes somewhere the importer will not read one back is therefore not a cosmetic mismatch: it
is a page that can never be exported again, and, if it reaches an editor, a page that loses
content silently.

Anywhere other than those two positions, the reference degrades to exactly what an ordinary
image degrades to — the same tree, leaf for leaf — and is reported as `Unsupported::Image`.

### Where the syntax lives

Both directions live in `gw_core::markdown`, for the reason `MARK_ORDER` does: two copies in
two crates drift, and the cost of drift here is an export that refuses every page holding a
picture.

## Consequences

- **The block stores only `filename`**, never the page. The page is where the block *is* — a
  placement is a top-level block of one document's body — so storing it would be an address
  that outlives a move, and resolving against the current page's list is what keeps a client
  from ever assembling an address of its own (ADR 0013's `D-16` reasoning).
- **A reference to a file that is not attached is a real state**, not an error. It is produced
  as written on import, stated plainly on render, and written out unchanged on export — a page
  whose file was detached is still a page that can be backed up.
- **No `comparable()` reduction exists for a placement.** The editor declares exactly the two
  attributes the importer writes, so they are compared whole, and a test pins that: adding a
  reduction goes red. That is deliberate, because the two reductions that already exist were
  each switched off once without a test noticing.

## Switch-back criteria

Revisit if either becomes true:

- **The export gains a mode meant for other tools to read.** A "portable" export would want
  bare relative names and the files beside them; that is a different artefact from this one,
  and it should be a second mode rather than a change to this spelling.
- **Placements become legal inside list items or table cells.** That is a ProseMirror schema
  question first — the content expressions would have to admit an atom — and the importer's
  top-level rule must move in the same change, or the two disagree in the direction that loses
  content rather than the direction that merely refuses.
