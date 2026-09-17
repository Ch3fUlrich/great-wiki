# 0022 — What a refusal may say about a page it refuses

**Status:** accepted · **Date:** 2026-09-17 · **Supersedes the status-code split argued in
`crates/gw-api/src/routes/docs.rs`**

## The decision

A refusal about a page **may only distinguish itself from "there is no page here" for a
caller who may already read that page.** Everything else answers `404 Not Found`, with the
same headers and the same bytes an address holding nothing answers.

It is one function — `gw_api::routes::docs::withheld_or_absent` — and every path-keyed
handler in `gw-api` ends its failing branch there. Nothing else in the crate decides it.

## What was decided before, and why it was right

The first version of `GET /api/documents/{*path}` checked existence before permission, so an
address holding nothing answered 404 and a page the caller may not read answered 403. The
module comment argued it out:

> Collapsing both to 404 would hide configuration mistakes behind a status code that says
> "you spelled it wrong"; collapsing both to 403 would confirm the existence of every path
> somebody guesses.

That is a real trade and the answer was defensible. Backlinks, the revision list, the
attachment list, the per-page task list, the editing socket, the project endpoints, the topic
endpoints and the delete all copied it, several of them naming `docs.rs` in their comments.

It was right because of who could meet it. The wiki had **one person** in it. The only caller
who could be handed a 403 was the owner, and for the owner the 403 was a diagnostic — "this
page is there and your grants do not reach it" sends you to the access panel, where a 404
would send you to check your spelling. ADR 0009, ADR 0011 and the per-document filtering were
all written against that threat model.

## Why it stopped being right

The invitation flow put a second person in, and
[`docs/operations/invite-walkthrough-2026-09-17.md`](../operations/invite-walkthrough-2026-09-17.md)
walked it as that person. Four endpoints answered:

```
/api/links/backlinks/rundgang/nur-intern     403      .../rundgang/gibt-es-nicht   404
/api/revisions/document/rundgang/nur-intern  403      .../rundgang/gibt-es-nicht   404
/api/attachments/rundgang/nur-intern         403      .../rundgang/gibt-es-nicht   404
/api/tasks/document/rundgang/nur-intern      403      .../rundgang/gibt-es-nicht   404
```

Two status codes is one bit, and a bit you can ask for about any address you can type is an
enumeration oracle. A signed-in relative could map which addresses hold a page and which do
not, without reading a word of any of them.

The objection — "addresses are guessable words, so what is disclosed is nearly nothing" — is
half true and is the wrong half. Addresses **are** guessable. The **titles** are not, and a
title is one grant away from an address that is known to exist. "There is a page at
`/medizin/befund-2026-08`" is a disclosure about a family that holds medical notes, and it is
exactly what the permission filter is there to prevent.

The convention was not even uniform: `/api/topics/tagged/{*path}` had answered 404 for both
all along, and ADR 0011 argues for it in the same words this ADR now uses everywhere.

## What replaces it, and the one place it deviates

The owner's decision, recorded in the roadmap on 2026-09-17, reads: *"404 for both to anyone
who is not an admin on the path; an admin still sees 403, so the diagnostic survives for
exactly the person who can act on it."*

Implemented literally, the 403 branch would be **unreachable**, and this ADR records why
rather than leaving a reader to rediscover it:

- `path_admin` passes on an `Admin` grant on the path, and `Permission::Admin` satisfies
  `Action::Read` in `gw_auth::can` — so that caller reads the page.
- `path_admin` also passes on `Baseline::Admin`, and `gw_store::acl::permits` widens *every*
  restricted read for that baseline — so that caller reads the page too.

An admin of a path can therefore never be refused a read of what is at it. They are shown the
page, which is a **better** diagnostic than a status code ever was: a grant mistake shows up
as "I can see this and the person I granted it to cannot", in the console that can fix it.

So the condition is **"may the caller read this page?"** rather than "does the caller
administer this path?". It gives an admin everything the decision asked for, and it is the
sufficient condition as well as the necessary one, which is what keeps the endpoints needing
**write** honest: somebody who may read a page but not change it meets a refusal
legitimately, and answering "there is no such page" to a reader who is looking at it would be
a lie the interface then has to relay to them.

The 403 that survives is exactly that one. `PUT /api/topics/document/{page}`,
`DELETE /api/documents/{page}`, `POST /api/projects`, the attachment upload and detach, and
the editing socket all still say "you may not do that" to a reader — and say nothing at all
to somebody who may not read the page.

## What it costs

- **`GET /api/documents/{*path}` can no longer answer 403.** The German refusal screen an
  invited reader meets on a page they were not granted is now the *missing-page* one. The
  403 sentence is kept in `web/src/lib/refusals.ts` and still thrown, because the loader maps
  a status rather than deciding one.
- **A misconfigured grant looks like a typo to the person who hit it.** They ask, and the
  person they ask can see the page. That is the trade, taken deliberately.
- **One extra store read on a refused request.** `withheld_or_absent` asks the
  permission-checked accessor again to decide the status code. Only on the failing path; a
  request that succeeds pays nothing.

## What is deliberately not in this

- **Timing.** Two responses identical in body and headers can still differ in how long they
  took, and this ADR does not claim otherwise. What it does claim is that the change did not
  make it worse: both cases now go through the *same* call, where before the absent case
  skipped the permission read entirely.
- **Ids.** A task id, a project id and a revision id are uuids nobody guesses, so there is no
  existence to protect and they were already 404 for everything unreachable. Unchanged.
- **The Papierkorb.** `POST /api/trash/restore/{page}` decides between 403 and 404 on whether
  the entry appears in the caller's own filtered trash listing — which is the same rule in a
  different accessor: it distinguishes itself only for somebody who can already see the entry.
  Unchanged.
- **Aggregate views.** The tree, the board, the graph, the topic index and the trash listing
  ask about no particular page, so there is no existence a status code could confirm. They
  answer 200 and whatever the caller is entitled to, which for somebody entitled to nothing
  is an empty one. Unchanged, and the reasoning is already on each handler.

## What would make this worth revisiting

A deployment where every account is an administrator — a single-person wiki again — would put
the old trade back, and the argument above says so in its own terms. It is not worth a
configuration switch: a switch that turns disclosure back on is a switch somebody leaves on.

## The fence

`crates/gw-api/tests/withheld.rs` sweeps every path-keyed request in the API and asserts that
a withheld page and an absent one produce the same status, headers and bytes, for a signed-in
caller with no grant and for an anonymous one. Two anti-vacuity tests stop it passing because
everything answers 404: an admin of the path is shown the page, and a reader who may not write
is told what was refused. `scripts/mutate.sh` breaks the helper four ways. Behaviour check P11
runs the same comparison end to end against a real server.
