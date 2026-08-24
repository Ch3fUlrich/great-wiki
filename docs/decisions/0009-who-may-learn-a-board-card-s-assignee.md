# 0009 — Who may learn what a board card's assignee is called

**Status:** Accepted (2026-08-24)

## Context

D-10 gave a task an assignee and stated who may set one: whoever may **write** the task's
governing page, and only ever onto somebody who may **read** it (clause 3). What it never
said is what a board may then *display*. The card carried the raw principal id, so the
interface rendered »Zuständig: 0199c0de-…« — an identifier that names nobody, in the one
place on a card a person has to be able to read at a glance. The owner asked for the name.

The design's Security section calls a board a disclosure surface, and this is a second thing
to disclose on it. The id was already on the wire; a display name is more legible and more
identifying, and this wiki is internet-facing with a corpus that includes a child's medical
records. So "put the name on the card" forces the question of who is told.

Three candidate gates:

| Gate | What it means |
|---|---|
| `instance_admin` | Only somebody who administers the instance is shown a name |
| None | Whoever may see the card is shown the name, because the id is there anyway |
| D-10 clause 3, re-asked at read time | The name is shown while the person named may still **read** the page the card is governed by |

The first is the gate that already answers a neighbouring question. `Store::list_principals`
takes no principal at all; `gw_api::routes::admin::list_principals` gates it behind
`instance_admin`, and that *is* this system's answer to "who may read the directory of
accounts".

## Decision

**D-10 clause 3, asked again when the card is read** — `Store::assignee_named`, which is the
one function that answers clause 3 at all.

1. **The admin gate answers a different question.** Enumerating accounts is learning that
   arbitrary people exist. A card is not the directory: it names *one* account, put there
   deliberately by somebody who may write the page, and — by clause 3 — only ever an account
   that may read that page. So the person named and the person reading the card are both
   readers of the same page. Reusing `instance_admin` would also mean a board names nobody
   unless an administrator is looking, which is not a board; and it lives in `gw-api`, a
   layer above the code that would have to consult it.

2. **The wiki already discloses a person's name to a page's readers.** `revisions.author_name`
   — the same `byline` string, display name falling back to username — is rendered in the
   history of every page to everybody who may read it. Naming a card's assignee to the same
   audience is not a new class of disclosure; it is the existing one, one surface further on.

3. **Re-asking, rather than trusting the stored fact, is what makes revocation work.**
   Clause 3 held when the assignment was made. Access can be taken away afterwards, and a
   name resolved once and copied onto the row would stay on every board until somebody
   noticed and cleared the card. Asked at read time, taking somebody's read away — or
   suspending their account — takes their name off every board at the next request, with
   nothing to clean up.

4. **One function, not two.** The verdict and the name are the same value:
   `assignee_named` returns what to call the person exactly when clause 3 would still permit
   the assignment, so there is no version of this code in which the gate is intact and the
   name leaks anyway. That is the trick `TaskPage` already plays with a document — the value
   that authorises the read is the value that names the thing — and it is why one mutation
   stands behind both.

### What a card shows when it may not name them

**The id it already carried, and no name** (`assignee_name: null`).

- Not a placeholder. Inventing a word for a person is the store's business least of all, and
  a German UI string in `gw-store` is the wrong layer.
- Not a card that forgets its assignee. Clause 4 exists precisely so a stale assignment can
  be cleared, and there is nothing to clear on a card that says it rests on nobody.
- **Not an oracle for "does this account exist".** The foreign key on `tasks.assignee` means
  every id on a card already belongs to a real account, so there is no non-existent account
  for the answer to be about; and the same empty answer covers an account that may not read
  the page and one that has been suspended. The write path already makes exactly that
  conflation — `TaskOutcome::AssigneeMayNotRead` is returned both for "cannot read it" and
  for "not an account" — so nothing is answered on the read path that was not answered on
  the write path already.

## Consequences

- Revoking a read, or suspending an account, is now visible on every board: the card says
  who it rests on by id and no longer by name. That is a legible signal that an assignment
  has gone stale, and the way out is the one clause 4 already provides.
- A reader of a page learns that a named co-reader of it exists. That is the deliberate
  disclosure, and it is bounded by the page: nothing about accounts outside its readership
  is reachable through a board.
- Resolving names costs one question per (person, page) pair per request rather than one per
  card, memoised in `AssigneeNames`. The pair is load-bearing: D-3 lets a project span pages
  with different grants, so a verdict memoised on the person alone would carry one page's
  answer across a whole board.
- `Task::assignee_name` is derived at read time and is never stored. Nothing may write it to
  a column later without reopening this decision, because a stored name is a name that
  outlives the access it was granted under.
