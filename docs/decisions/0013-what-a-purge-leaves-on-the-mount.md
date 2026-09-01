# 0013 — What a purge leaves on the mount

**Status:** Accepted (2026-09-01)

## Context

[D-16](../superpowers/plans/2026-08-24-attachments-and-trash.md) makes attachments
content-addressed: a blob's name is the SHA-256 of its contents, so one PDF attached to two
pages is one file, and a download is authorised against **the page it was reached through**.
That is two tables — `blobs` is an index of the mount, `attachments` is a page's `Anhänge`
list — and the separation is what makes the access rule expressible at all.

It also creates a question the design does not answer on its own. An `attachments` row
cascades away when its page is destroyed; the bytes underneath do not belong to that page, so
what happens to them?

The question is sharpest at a purge, because [D-14](../superpowers/plans/2026-08-24-attachments-and-trash.md)
makes a purge the **only** operation in this system that loses data, and gives as its reason
that "a wiki that cannot forget a mistakenly-imported document is a wiki with a permanent
mistake in it". A purge that destroys every trace of a page except the 40 MB scan attached to
it has not obviously forgotten it.

Two things constrain the answer, and both of them are already decided elsewhere.

**[ADR 0012](0012-what-a-purge-destroys.md): the preview *is* the purge, rolled back.** There
is no second query that counts what a purge would destroy, because two statements describing
one truth can be edited apart, and this is the operation where being wrong cannot be undone.
`Store::purge_document` runs the whole thing either way and a `Purge::Preview` rolls the
transaction back.

**A filesystem is not in the transaction.** `unlink` does not roll back, and the mount is NFS
— `/mnt/cloud` answers `Stale file handle` inside a container while the host is fine.

## Decision — a purge takes the list and leaves the bytes, and says so

`Store::purge_document` destroys the pages, their revisions, their cards, their edges, their
topic filings and their **attachment rows**. It removes no file from the mount and no row from
`blobs`. It reports both numbers: `attachments`, the list entries that went, and
`blobs_orphaned`, the stored files that no page references any more.

### Why an unlink cannot go in the purge

**Inside the transaction, on both modes:** a preview would destroy files. It runs the same
code and rolls the database back; the mount does not roll back with it. Describing a
destruction would perform half of it.

**Inside the transaction, on `Commit` only:** the preview and the purge stop being the same
code path. That is precisely the thing ADR 0012 exists to prevent — the moment they differ,
the numbers an administrator confirms are produced by something other than what happens, and
the drift is silent.

**After the commit:** the commit can succeed and the unlink fail, or the process can die in
between, leaving `blobs` rows for files that are gone. Worse, the window is open to an upload:
another request that stores the same bytes and inserts a row would have its file deleted by an
unlink already in flight, and the page that would break is one nobody deleted. That is the
failure D-16 rejects most-restrictive-wins for — a change to one page reaching out and
altering another.

**Before the commit:** a failed commit rolls the rows back and the bytes are already gone, so
a live page's attachment now points at nothing. Same failure, other direction.

There is no ordering of "destroy rows" and "unlink files" whose worst case is not a *live*
page losing its file. Leaving the bytes has a worst case of wasted disk.

### Why the `blobs` row survives its last reference

The obvious tidying — delete the blob row along with the attachments and leave only the file —
is rejected, and this is the part that is least obvious.

**A row nobody references is a worklist. A file nobody indexed is a directory walk.** With the
row, "what is on this mount that nothing points at" is one indexed query, answerable inside a
transaction, on a table that no concurrent upload can change without taking the store's single
connection. Without it, the only way to find an orphan is to walk `blobs/` and compare against
the database — and that walk races every upload in flight: a file written a millisecond before
its row is inserted looks exactly like an orphan. A reclamation built on a directory walk
would eventually delete a file somebody had just uploaded.

`attachments.sha256 REFERENCES blobs(sha256)` therefore carries **no** `ON DELETE` clause. A
blob row cannot be deleted while an attachment points at it: the database refuses instead of
silently detaching a page's file. That refusal is what a future sweep is built on.

### What reclamation will look like, when it is built

Out of scope for this piece, and specified here so that whoever builds it does not have to
re-derive the constraints:

- It is a **deliberate act**, like the purge it follows. `endgültig löschen` empties the
  Papierkorb; a second, separate operation empties the mount. Never a timer and never a side
  effect of a request — the argument D-14 makes against automatic purge applies unchanged.
- It takes the store's connection and does the whole thing in **one transaction**:
  `SELECT` the unreferenced blobs, delete their rows, unlink their files, commit.
  `Store::open` fixes the pool at one connection, so an upload cannot interleave — it needs
  the same connection to insert its row.
- The residual risk is a crash between the unlink and the commit, whose outcome is a `blobs`
  row for a file that is gone. That state is already handled: a download answers 503, and
  re-uploading the file repairs it, because `PendingBlob::publish` renames into place even
  when the digest is already known.

## Two smaller decisions that follow from the same shape

**A download whose bytes are missing answers 503, never 404.** `open_read` cannot tell "never
stored" from "the mount will not hand it over", and 404 would say the attachment does not
exist — sending whoever investigates to the database, which is the one place the problem is
not. 503 says the wiki knows about this file and is failing to serve it, which is both true
and retryable. It is only reachable *after* the page has been authorised, so it discloses
nothing to somebody who may not read the page: the same request from an unauthorised caller is
403, and `a_file_whose_bytes_have_gone_answers_503_and_not_404` asserts both on one file.

**Deduplication is invisible.** Attaching a file whose bytes are already stored produces the
same status and the same body as attaching bytes nobody has ever sent, and takes the same work
— `publish` renames either way rather than skipping when the digest is known. If it did not,
possessing a file would be a test for whether somebody else had put it on a page you cannot
read: a disclosure about a *page*, obtained without ever naming one, which is AGENTS.md rule 2
applied to a write. The cost is one rename per duplicate upload, which is nothing beside the
transfer that preceded it.

## Consequences

- **The wiki cannot yet forget a file completely, and that is a real gap, not a nuance.**
  Purging the last page that carried a mistakenly-uploaded scan removes every trace of it from
  the database and leaves the bytes on the mount, reachable by anybody with shell access to
  the NAS. Until the sweep exists, "delete it from `/mnt/cloud/great-wiki/media/blobs/` by
  hand" is the honest answer, and `blobs_orphaned` in the purge report is what tells an
  administrator there is something to do.
- **Growth is bounded by distinct files, not by attachments.** Attaching the same PDF to
  forty pages stores one copy; detaching it from all forty still stores one copy. The waste
  is one copy per distinct file ever uploaded, which for a wiki of this size is measured in
  gigabytes at worst.
- **Detaching is not a destruction either**, and for the same reasons. `Store::detach` removes
  the row; the bytes stay. D-15 makes the list the authority on what is attached, so letting
  go of an entry is an edit.
- **A purge report now carries two numbers about files**, and they mean different things. A
  console that shows `attachments` alone would read as "and the files are gone", which is the
  misreading this whole document exists to prevent.

## Switch-back criteria

Revisit if any of these becomes true:

- **The mount fills up, or orphans become a measurable fraction of it.** The fix is the sweep
  described above, and it is a piece of work rather than a change to this decision — the
  decision *is* that reclamation is separate and deliberate.
- **Somebody needs a file provably gone within a fixed window** — a takedown, a
  misfiled record, anything where "an administrator will run the sweep eventually" is not an
  answer. That is the case that would justify accepting the unlink-after-commit window, and
  the price would have to be paid explicitly: the purge would need to hold the store's
  connection across the unlink, and the preview would have to stop being the same code path,
  which means ADR 0012 gets revisited at the same time. Do not do one without the other.
- **The store ever runs with more than one connection.** `Store::open` fixes the pool at one,
  and every safety argument above about interleaving uploads rests on it. If that changes, the
  sweep needs a real lock and this document needs rewriting before the sweep is written.
