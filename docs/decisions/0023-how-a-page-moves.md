# 0023 — How a page moves, and what a move takes with it

**Status:** Accepted (2026-09-25)

## Context

Nothing could rename or move a page. ADR 0019 built links on document identity precisely so
that a move would break nothing, and until now that was proved only by a Rust test running a
direct `UPDATE`. The owner settled four things on 2026-09-24 (roadmap, *rename and move come
next*):

1. **Access follows the new place, and the dialog shows it first.** Before confirming, the
   dialog names who gains and who loses reading access, computed by the permission engine
   against the destination. Widening needs admin rights on the destination. A move that only
   removes access is allowed with write on the source and on the destination parent (the
   recommended default, taken).
2. **The old address forwards, until reused** — only for a caller who may read the page; a
   page later created there takes the address and the forward is dropped.
3. **A page moves with its whole subtree**: one move, one audit entry.
4. **A dialog first**, working with a keyboard and without JavaScript; sidebar dragging is a
   later layer that opens the same dialog.

And one constraint from ADR 0019: a move never rewrites another page's body.

That leaves several questions the decisions do not answer, and this ADR answers them.

## Decision

### The preview is the move, rolled back

`Store::move_document` has two modes and one body. Both carry the move out inside a
transaction; after the rows have moved, it reads who may read each moved page through
`acl::grants_on` (the nearest-ancestor lookup, on the transaction's connection) and
`acl::permits` (the only interpreter of a visibility and a set of grants). Before-verdicts come
from the same two functions against the live tables. A preview then rolls back; a commit
records the audit entry and commits.

This is ADR 0012's argument for the purge, applied again: a preview computed by a second
piece of code is a prediction, and the day it disagrees with the act, somebody confirmed a
different move from the one that happened. Measuring across the act also makes the preview
honest about things nobody would think to model — a grant left behind on a destination path
by an earlier purge, for instance, applies to what arrives there, and the preview shows it.

`grants_on` exists because `Store::open` gives the pool one connection: asking the pool from
inside the transaction would wait for itself. `grants_for_path` now delegates to it, so there
is still one rule.

The confirmation re-measures. The POST carries the fields the preview was asked with, and the
store refuses a commit that would widen access for a caller who does not administer the
destination, whatever the preview said a minute earlier.

### Who counts

Every **active** account, and "everybody without an account" (`Principal::anonymous()`), who
is reachable through an `anyone` grant. Deactivated accounts read public pages and nothing
else, and a move does not change a page's visibility, so they can never gain or lose.

The preview names people to somebody who may write every moved page and the destination
parent. That is a disclosure — who can read a page — and it is made to the person about to
change exactly that, which is what the owner's decision asks for. It is not made to anybody
who may not write the page: a caller who fails that check is refused before anything is
measured, with ADR 0022's answer.

### Grants written on the moved pages travel with them

`acl` rows at or under the old path are re-pathed with the pages; so are invitations still
pending for those paths. Accepted and revoked invitations are records and stay.

The owner rejected *copying today's effective grants onto the page* (grants pile up and drift
from the tree). This is not that: nothing inherited is materialised. The two alternatives
were worse:

- **Leave them at the old path.** They would govern whatever is created there next — a
  stranger's page inheriting a fence, or a fence silently lifted from the page that moved.
- **Drop them.** A page somebody deliberately fenced off with its own grants would be
  unfenced by being moved, which is a widening the mover never saw as one.

The preview measures the effect either way, so what travels is shown before it travels.

### What may be moved, by whom

- A signed-in, active account — a move is recorded under a name, and on a path carrying
  `anyone: write` the write bit alone would let somebody who has not said who they are move
  a page. The same rule `trash_document` applies.
- Write on **every** page that moves, live or in the trash, for the reason `crate::trash`
  gives about a subtree somebody else fenced off.
- Write on the new parent. The destination is read before it is written, so a destination the
  caller may not see is refused in the words an absent one is (ADR 0022), and only somebody
  who can see it is told the refusal is about writing.
- **The top level needs instance administration.** The root has no page to hold a grant, so
  `path_admin("/")` — the admin baseline — is the gate, including for renaming a page that is
  already at the top level.
- **Widening** — anybody gaining read on any moved page — needs `path_admin` on the new
  parent. The API asks it and hands the verdict to the store, for the reason `crate::admin`
  gives: a second copy of that gate in the store would be a second rule to disagree with.

An occupied destination is refused and named, including one held by a page in the trash (it
still owns its address). That tells a writer of the destination parent that an address under
it is taken — one bit, told to a writer rather than to a prober, which is the line
`crate::trash` draws for its own refusal.

### Rename changes the address, and the dialog can keep it

The dialog carries the title and the last segment of the address, prefilled with the current
slug. A new title therefore does not change the URL unless the address field is changed too;
a blank address field is derived from the title, as creating a page does.

### Forwards are by id, ended by triggers, and answered with 307

`forwards(old_path → document_id)`. The reader is sent to the document's **current** path, so
a page moved twice forwards from both old addresses with no chain to follow. Two triggers on
`documents` delete the forward at any path a row takes — a page created there, or a page moved
there, including the same page moving back — so "until reused" holds for every writer of
`documents.path`, present and future, rather than for the ones that remembered.

The forward is resolved through `Store::document_for_id`, so it answers only somebody who may
read the page where it is now. `GET /api/forwards/{path}` answers everyone else with the same
404 bytes as an address nothing ever left. The web loader asks it only after a 404, and
redirects with **307**: a 301/308 is cached by the browser indefinitely, and this forward ends
the moment the address is reused.

Trashed descendants get forwards too; they answer nothing while in the trash and start
working if the page is restored.

### The audit entry

One `document.move` entry: target the old path, scope the **new** path (the people who
administer the page from now on are the ones entitled to read how it arrived), detail
`{from, to, title_before, title, pages}`.

## Consequences

- **ADR 0019's property is now proved by use.** The store test
  `a_move_leaves_every_other_body_and_history_alone_and_a_reference_follows_it` snapshots
  every body and the revision count across a move, and resolves a reference afterwards.
- **An open editing session survives a move that leaves the editor in reach, and ends with
  one that does not.** The socket joins on a path but is re-authorised by **document id**
  (`collab::authorise_id` → `Store::document_for_id(.., Action::Write)`), which resolves to
  the page's current path and asks the same accessor as a handshake. An editor who lost write
  or read by the move (or whose page went to the trash) gets the policy close at the next
  tick or update; one who keeps write stays connected, in the same room (rooms are keyed by
  document id). Before this the session was closed for everybody. The view-as refusal is
  shared with the handshake (`collab::editor`). Mutations `collab:` in `scripts/mutate.sh`.
- **Links typed as a path to the old address** (an `href` that was never resolved to an id)
  keep the old path and work through the forward — until the address is reused.
- **Sub-routes of an old address** (`/alt/history`) are forwarded by the same rule as the
  page: the loader asks `GET /api/forwards/alt` (the page, not the sub-route) after its own
  404 and redirects 307 to `<new path>/history` with the query kept. A caller who may not read
  the page at its new address gets the forward API's 404 and so the ordinary missing page. The
  history page is the only `[...path]/*` route the web app has; a new one must do the same.
- **Visibility does not change on a move.** It is a property of the page, not of its place.
  If the owner later wants "public" to follow the tree too, the preview is where that would
  show.

## Switch-back criteria

- **A second preview implementation appears** — in the client, or a "quick" access diff that
  does not move the rows. That is the failure this design exists to prevent.
- **Grants become something other than path rows** (e.g. grants on document ids). Then
  "travel with the page" is free and the re-pathing goes away.
- **A move ever rewrites a body.** That contradicts ADR 0019, not just this.
