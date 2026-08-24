# Piece 4 — attachments, and the trash that should have come first

**Design:** [2026-08-15-links-topics-tasks-design.md](../specs/2026-08-15-links-topics-tasks-design.md).
Roadmap entry: **M5 — Media & attachments**. The owner confirmed early that they need files
beside their notes; this is the piece that stops the wiki being prose-only.

Two features, planned together because they share one property: both are the first things in
this system that can **destroy** something, and everything else here has been carefully
built so that nothing can.

## Owner's decisions, 2026-08-24

### D-14: Trash, then purge on request

Deleting a page moves it to a Papierkorb. It leaves the tree, its ACL still applies, and it
is restorable. **Real deletion exists but is a second, deliberate act** — `endgültig löschen`,
for an administrator — and that is what cascades away the page's tasks and revisions.

Rejected: trash alone (nothing could ever be truly removed, and a wiki that cannot forget a
mistakenly-imported document is a wiki with a permanent mistake in it) and automatic purge
after a period (it makes "it vanished" possible again, which is the outcome
[D-8](../specs/2026-08-15-links-topics-tasks-design.md) exists to prevent — the same argument,
applied to a page instead of a card).

**Consequence:** a purge is the only operation in this system that loses data. It therefore
gets the treatment the ACL editor gets — it says what it is about to destroy, by name and by
count, before it does it, and the count includes the things that cascade.

### D-15: An attachment is both inline and listed

An image or PDF renders in the text where it was put, **and** every attached file appears in
an `Anhänge` list on the page.

Rejected: inline only (a file attached and then cut from the prose becomes unreachable while
still occupying the mount — invisible litter, and the sort nobody finds) and list only
(an image cannot sit beside the paragraph explaining it, which is most of what images are
for in a medical reference).

**Consequence:** the list is the authority on what is attached, and the inline block is a
*reference* to it. Removing an inline block must not detach the file — the same shape as
[D-2](../specs/2026-08-15-links-topics-tasks-design.md), where deleting a checkbox line
detaches a task rather than destroying its due date.

### D-16: A download is authorised against the page, not the blob

Blobs are content-addressed by SHA-256, so attaching one PDF to two pages stores one copy.
A download is then authorised against **the page it was reached through**.

Rejected: one copy per page (simple and unmistakable, but it stores duplicates of exactly the
files most likely to be large) and most-restrictive-wins (attaching a file to a private page
would silently break the public page that already showed it — a change to one page reaching
out and altering another is the failure D-5 rejected rewrite-on-move for).

**The reasoning, because it will look wrong to somebody:** the restriction was never about the
bytes. Two pages holding the same PDF are two statements about who may see *that page*, and
the identical bytes underneath are an implementation detail of how it is stored. A URL must
therefore never be `/blob/<sha>` — it is `/<page>/anhang/<name>`, and the sha never appears
in an address a reader can hold.

### D-17: 250 MB per file

Enough for scan bundles and short video. A single accidental upload is noticeable but not
damaging.

## What this shares with everything already built

**A download path is a disclosure surface**, exactly like a board card or a graph edge. It is
also the *worst* one, because it returns bytes rather than a title: a leak here does not
reveal that a page exists, it hands over the contents. Every download resolves its page
through the same permission-checked accessor a page read uses, per document, before a single
byte is served.

**Storage split is an architecture rule, not a preference**
([AGENTS.md](../../../AGENTS.md#architecture-rules--the-ones-that-are-not-negotiable) rule 5):
metadata in SQLite on NVMe, blobs on `/mnt/cloud/great-wiki/media/`. Never a database on NFS.

**Type detection is by magic bytes, never by extension**, and the declared type is never
echoed back. A file that claims to be a PNG and is not must be served as what it is, or as
nothing — the browser is the thing being protected here.

**No parser runs in the request path.** Text extraction from PDF and Office files is a known
attack surface and belongs in a background job, which is also where the roadmap puts it.

## Order of work

| | Step | Touches |
|---|---|---|
| 1 | Trash: soft delete, restore, and the purge that names what it destroys | `gw-store`, `gw-api` |
| 2 | `BlobStore` trait + filesystem implementation, content-addressed | `gw-store` |
| 3 | Upload pipeline: magic-byte typing, 250 MB cap, dedup | `gw-api` |
| 4 | Download, authorised per page, never by sha | `gw-api` |
| 5 | The `Anhänge` list, and the inline block | `gw-core`, `web` |

Trash first, deliberately: it is smaller, it is the thing the wiki most visibly lacks, and
step 5 needs a `BlockKind` — which means the four-mirror problem
([block.rs](../../../crates/gw-core/src/block.rs)) and is the step most likely to go wrong.

## Out of scope

Image derivatives and thumbnails, video transcoding, range requests, text extraction and the
search index that consumes it. All are M5/M7 and none of them block a file being stored and
fetched.
