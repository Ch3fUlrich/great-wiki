# 0012 — What a purge destroys, and who may ask for it

**Status:** Accepted (2026-09-01)

## Context

[D-14](../superpowers/plans/2026-08-24-attachments-and-trash.md) settled what deleting a page
means: it goes to a **Papierkorb**, leaves the tree, keeps its ACL, and is restorable. Real
deletion exists as a **second, deliberate act** — `endgültig löschen` — and that is what
cascades away the page's revisions, cards and edges.

So a purge is the first and only operation in this system that loses data. Everything else
here was built the other way round on purpose: revisions are append-only and a restore
publishes a *new* revision rather than rewinding; a task whose line is deleted is marked
`detached` rather than dropped (D-8); an `acl` row may outlive the page it was written for.
Even the trash keeps everything.

Two questions about the purge have non-obvious answers, and both are the sort that look
settled until somebody meets the consequence.

## Decision 1 — the gate is `path_admin`, on the page's own path

`gw_api::routes::admin` has exactly two gates and the difference between them is the whole
design: `instance_admin` guards what concerns the **instance** — accounts, teams, group roles,
things that belong to no subtree — and `path_admin` guards what concerns **one subtree**, by
asking `can()` for `Action::Admin` on that path (an instance admin passes as well, because
the first grant on a fresh subtree has to be writable by somebody holding none there yet).

A page belongs to exactly one subtree, so a purge is gated by `path_admin` on that page's own
path. It is not a new check of the same shape: it is **the same function**, made `pub(crate)`
and called from `routes::trash`, so "who administers this page" keeps one answer.

That is also the gate `set_visibility` uses, and ADR 0008 gives the reason that carries over
unchanged: `Action::Admin` is satisfied by no amount of read and by no amount of write, so
being able to *edit* a page is structurally not being able to publish it — or, now, to
destroy it. Deleting a page is an edit and follows write; destroying it is not, and does not.
That asymmetry is the whole of D-14's "second, deliberate act", and putting the two acts
behind the same permission would have made the second one a formality.

**Rejected: `instance_admin`.** Tempting, because a purge is irrecoverable and instance
administration is the strongest thing this system has. It is wrong for the reason the two
gates exist at all: it would say that whoever administers `/praxis` may publish a restricted
page in it to the open internet but may not delete an empty one, which is not a coherent
boundary — it is a boundary drawn around *how alarming an operation sounds* rather than around
*what it reaches*. It would also make every emptying of the Papierkorb an escalation to one
of two people, which in practice means the trash never gets emptied.

**Rejected: admin on every page the purge would destroy.** This is the version that looks
most correct, and it does not work. A purge takes the whole subtree, because a page whose
parent has been destroyed can never be restored — there would be nowhere to put it back — and
a trash *entry* is reachable only as a whole: its members are not separately addressable. So
a subtree containing one page fenced off with its own narrower grants would become
**unpurgeable**, permanently, by anybody. A trash that cannot be emptied is a worse failure
than the one being avoided.

### The consequence, stated rather than discovered

Grants do not union up the tree — the nearest ancestor carrying any wins outright — so
`/praxis/intern` can be fenced off from whoever administers `/praxis`. If such a page is in
the trash under `/praxis`, then **purging `/praxis` destroys it, and names it in the report**.
This is the one place in the system where administering a path reaches a descendant that has
been deliberately narrowed away from it.

Three things make that acceptable rather than merely tolerated, and all three are load-bearing:

1. **Nothing gets there by accident.** Putting a subtree in the trash requires **write on
   every page in it** (`gw_store::trash`), so the fenced-off page was deleted by somebody
   entitled to delete it. A purge destroys what has already been deleted; it never reaches a
   live page, and refuses outright if one is inside the subtree.
2. **The report names it.** The administrator sees the page before they confirm — see
   Decision 2 — rather than discovering afterwards what went.
3. **The audit row survives it.** `document.purge` records every destroyed path, scoped to
   the page, so whoever administers the fenced-off subtree can read that it happened. It is
   the only surviving record; every other trace has just been destroyed.

## Decision 2 — the preview **is** the purge, rolled back

D-14 requires a purge to say what it is about to destroy, by name and by count, including
what cascades. The obvious implementation is a `SELECT` that mirrors the `DELETE`'s `WHERE`
clause and counts the same joins. It is rejected.

Two statements written to describe one truth are two statements that can be edited apart.
The day they drift, the number an administrator reads and confirms is not the number of
things that go — and this is the one operation where being wrong cannot be undone. The same
objection defeats `rows_affected()`, which counts what one statement matched and knows
nothing about what cascaded out of it.

So `Store::purge_document` takes a `Purge::Preview | Purge::Commit` and **runs the whole
thing either way**, inside one transaction:

- the names come out of `DELETE … RETURNING path, title` — the destroying statement itself;
- every other number is a **difference in a whole-table total measured across that DELETE**
  (`revisions`, `tasks`, `projects`, `links`, `document_tags`, `tags`, after
  `prune_empty_topics` has run in the same transaction), so it describes what actually
  disappeared rather than what a predicate says should have;
- a `Preview` then rolls the transaction back and records nothing.

A preview and the purge it describes therefore cannot report different totals, because they
are the same code path executed the same way. `GET /api/trash/purge/{path}` is the
description and `POST` to the same address is the act.

**What this costs, stated plainly.** A `GET` opens a write transaction and takes SQLite's
write lock for its duration. On a single-writer wiki of tens of pages that is microseconds
and it is an administrative action, not a request path. It is still a `GET` that writes and
undoes, which will surprise somebody reading the route table; this document is why.

**What it buys beyond the numbers.** The purge asserts, inside the transaction, that the
count of destroyed documents equals the number of rows `RETURNING` handed back, and refuses
the whole thing otherwise. That is not decoration: the obvious spelling of
`documents.deleted_root` — a foreign key with `ON DELETE CASCADE` — would delete an entry's
members before the outer `DELETE` reached them, so the statement would return fewer rows than
it destroyed and the purge would silently under-report itself. `0012_trash.sql` records why
that column carries no foreign key; this assertion is what would catch it being added.

## Disclosure

**A purge report is a disclosure surface, so the preview is gated exactly as the purge is.**
It names every page in the subtree, including any the caller could not otherwise read, so a
read-only preview is not a read-only disclosure. There is one gated body and the two verbs are
ways into it, rather than a "harmless" description with a lighter check.

**The trash listing is a different surface and filters differently.** It authorises every page
through the same body a page read ends in, so a page you could not see before deleting it is
not one you can see in the trash, and the count beside an entry is the pages *you* may read in
it — never a total, and with no field beside it that could count what was hidden (ADR 0011).

## Consequences

- **`path_admin` is now used by two features and must stay one function.** A second "who
  administers this page" written anywhere is a second answer, and the second answer is the one
  that gets it wrong when the rules change.
- **A purge never withdraws a grant.** An `acl` row is a fact about a path, not about a
  document — it may be written where no page exists, so that access can be prepared before one
  arrives. A page created later at a purged path therefore inherits whatever policy the path
  still carries, which is the same thing that has always been true of a path nobody has used
  yet.
- **A purge never reaches a live page.** The trash is the only way in, and a subtree holding
  one is refused rather than destroyed. Purging is not a faster delete.
- **Both halves are mutation-tested.** Swapping the gate for "is there a caller at all",
  committing the preview, zeroing a report count and deleting the live-page guard are each an
  entry in `scripts/mutate.sh`, because a gate nothing would notice being removed is the
  failure mode this project has shipped three times.

## Switch-back criteria

Revisit if either becomes true:

- **A subtree is genuinely fenced off from its parent's administrator in practice.** Today
  this is a shape the ACL permits and nobody uses. If it becomes a working arrangement —
  a shared wiki where `/praxis/personal` is administered by somebody else entirely — then a
  purge that reaches into it stops being a documented consequence and becomes a bug. The fix
  is not a stricter gate but **an addressable trash entry per fenced subtree**, so its own
  administrator can purge it first and the outer purge finds nothing left to reach.
- **The corpus grows enough that a whole-table count per purge is measurable.** The fix is
  then counting by primary key against the ids the `DELETE` returned — still inside the same
  transaction and still measured across the statement — and *not* a `SELECT` beside the
  `DELETE`, which is the thing this decision exists to refuse.
