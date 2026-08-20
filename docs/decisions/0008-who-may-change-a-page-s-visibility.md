# 0008 — Who may change a page's visibility

**Status:** Accepted (2026-08-20)

## Context

`documents.visibility` decides who reaches a page *without* any access entry: `public` is
the open internet, `internal` is everyone signed in whose Authelia group confers internal
reach, `restricted` is nobody. Until now nothing in the running system could write it. The
value arrived once, from frontmatter, at import, and `seed --update` deliberately compares
it and **refuses** to change it — a stray `visibility: public` in a bulk file drop would
publish a restricted page with nobody watching, and the refusal message has always said
"do that in the admin console, where it is one deliberate act with a name on it".

There was no such place. The access panel rendered the value as a badge, which reads as
settable state, and the owner asked for the control.

The question the control forces is who holds it. This wiki is internet-facing and the
corpus includes a child's medical records, so "made public by accident" is the most
expensive mistake the interface can enable.

Three candidate gates:

| Gate | What it means |
|---|---|
| `instance_admin` | Only somebody who administers the whole instance |
| `path_admin` | Somebody holding `Action::Admin` on the page's own path (instance admins pass through it) |
| Split | `path_admin` to narrow, `instance_admin` to widen to `public` |

## Decision

**`path_admin` on the page's own path** — the same gate as `/api/admin/acl`, in the same
shape, with the same audit obligation.

Three arguments, and the third is the one that settles it.

1. **Read must never widen, and neither must write.** `path_admin` asks `gw_auth::can`
   for `Action::Admin`, which no `read`, `comment` or `write` grant satisfies. Being able
   to edit a page is not being able to decide who may see it. `leser` in the admin test
   fixture holds `read` on `/raum` and is refused; the same test then gives `gast` `write`
   and it is refused too.
2. **The authority is bounded the way every other path-scoped power in this API is
   bounded** — to a subtree somebody was deliberately given. Whoever administers `/raum`
   decides about `/raum`, and D-M2-2 exists so that this does not decentralise instance
   administration along with it.
3. **Anybody who passes this gate can already publish the page.** `can()` answers a
   `Subject::Anyone` grant *before* it looks at whether the caller is signed in — that is
   what makes it a public share link — so `anyone: read` written through
   `/api/admin/acl` puts the page on the open internet just as effectively, and reaches
   the whole subtree while it is at it. A stricter gate on visibility would withhold
   nothing. It would push the same act onto the mechanism the console shows *less*
   clearly, and leave the page's badge still reading »Eingeschränkt« while the world reads
   the page. Two doors into one room with different locks is not a security boundary; it
   is a reporting error.

The split gate was rejected for the same reason as (3), plus one of its own: it would make
the console's own explanation false. The panel now says that visibility is one of four
ways into a page and that entries outrank it; a rule where narrowing and widening have
different owners cannot be explained in one sentence to the person making the decision.

### What the endpoint does besides check the gate

- **Refuses a value it does not understand**, rather than defaulting. `Visibility::default()`
  is `Restricted` — safe today, and one enum reordering away from publishing a page nobody
  asked to publish. Recognised values are canonicalised by the same `Visibility::from_str`
  the store reads the column back with, so there is exactly one definition of what counts
  as public.
- **404s on a path with no document.** A grant may be written on a path nothing occupies
  yet — that is deliberate, so access can be prepared before a page arrives. Visibility is
  a column; with no row there is nothing to set, and a 200 would tell somebody they had
  published something.
- **Writes the audit row in the same transaction as the UPDATE**, recording the value
  *before* as well as after: `document.visibility`, target and path both the page,
  `{"from": …, "to": …}`. The row is scoped to the page rather than instance-wide, so the
  space admin who made the change can read it back in their own log (0004). It is the only
  place that fact survives — `documents` keeps just the current value, and `updated_at` is
  deliberately not touched so that a metadata change is not misread as an edit by whoever
  wrote the last revision.

## Consequences

- **`seed --update` is unchanged and must stay unchanged.** Nothing here weakens it. The
  two are different acts: a file drop is two hundred files and nobody watching; this is one
  path, one person, one row with a name on it. If the refusal is ever relaxed, this ADR is
  the argument that says why it should not be.
- **A space admin can publish a page in their subtree to the internet.** That is not new
  authority — see (3) — but it is now *visible* authority, which is the point.
- **The console must keep saying what visibility does not do.** Lowering a page to
  `restricted` closes nothing an entry has opened, because `permits()` consults the grants
  first, and it does not reach descendants at all. Both sentences are in
  `web/src/lib/components/admin/reach.ts` and asserted, because a control that exists and
  does not do what the reader assumes is more dangerous than no control.

## Switch-back criteria

Revisit if path-scoped administration is ever delegated more widely than it is today —
specifically, if space admins are appointed for subtrees by somebody other than an instance
admin. Argument (3) rests on `anyone: read` being available to exactly the same people; if
that changes, both gates have to change together, or neither.
