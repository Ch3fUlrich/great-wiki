# 0021 — One person, one identity, merged by a verified email address

**Status:** Accepted (2026-09-17)

> **On the number.** 0020 was the last one taken when this was written and `docs/decisions/`
> held nothing above it. Another change was in flight at the same time and was told to take
> the next free number too; if it landed on 0021 as well, **that one keeps 0021 and this is
> 0022** — the tie-break was agreed in advance and is recorded here so that renumbering is a
> file rename and not an argument.

## Context

The invitation flow and the Authelia flow produced two different people out of one.

An invitation created a **local** principal with a password (`kind: 'local'`). A person
arriving through Authelia was mirrored into `principals` on first sign-in as a separate
`kind: 'oidc'` row with **no access at all**, to be granted by hand afterwards. Nothing
connected the two. The operations walkthrough of 17 September put it plainly: *"the
invitation flow does not apply to it… 'send them an invitation link' is not how a homelab
account gets in, and the console does not currently say so anywhere."* It also asked the
question this decision answers — *"whether a homelab person should be invitable at all"* —
and noted that the alternative, an invitation that pre-grants access to a username that will
arrive via OIDC, *"does not exist and would need designing"*.

For a family wiki that is not a tidiness problem. It is two credentials for one relative,
one of which is a password the owner had to convey through a chat app, and two histories
where there should be one. The roadmap's entry for the same day chose: **one person, one
identity, merged by email**, and named the key — *"the email Authelia asserts as verified,
never one the person types at sign-in."*

Reading the code to implement that turned up something worse, and it is the second reason
this decision exists. `upsert_oidc_principal` was an `INSERT … ON CONFLICT (username) DO
UPDATE`. An Authelia account whose `preferred_username` happened to equal an existing
**local** account's username silently took that account over — its id, its grants, its
revisions, its attachments — with no address checked, no verification involved and nothing
recorded. A homelab user called `oma` was one sign-in away from being the invited `oma`.
That was live, under a green suite, and it is fixed here.

## Decision

### The merge key is the address the provider asserts as verified

`email_verified: true` in the id token, or in the userinfo response **that also supplied the
address**. A sign-in whose address is unverified, or carries no `email_verified` claim at
all, or carries none this wiki can key on, **does not merge**: it gets exactly the previous
behaviour — a principal of its own, with no access — and the reason is written to the log at
`info` with the account's name beside it, because a relative whose sign-in lands in an empty
account will say "it does not work" and the owner needs to know which of the three it was.

An absent `email_verified` is treated identically to `false`. "Nobody said no" is not an
assertion, and the difference between the two is the whole of the attack: Authelia lets an
account hold an address nobody vouched for, so without the flag anybody who can edit their
own profile there types somebody else's address into it and signs in as them.

Nothing else is ever a merge key. Not `sub`, not `preferred_username`, and above all not an
address typed into a form here — the invitation's address is set by the **owner**, and the
acceptance page asks for a display name and a password and nothing else.

### The address is normalised once, in one place, and deliberately narrowly

`gw_store::canonical_email` is the only definition, and both sides call it:

1. Trim ASCII whitespace.
2. **Refuse anything outside printable ASCII** (`!`–`~`).
3. Require exactly one `@`, with a non-empty local part and a non-empty domain.
4. Lowercase both sides.

So `Oma@Example.de` and `oma@example.de ` are one person, which is what the owner asked for.

**On Unicode, deliberately:** an internationalised address is **refused as a key** rather
than folded to one. It is stored, displayed and usable — it simply never merges. Folding is
the dangerous option in both directions: `exämple.de` and `example.de` are different
registrations, and `оma@…` with a Cyrillic *о* renders identically to `oma@…` in every font a
person reads. Either, silently folded, hands one person's grants to whoever can register the
other. Supporting IDN properly means UTS-46 plus a confusables check, which is a decision of
its own and not a patch to a normalising function.

One thing found while walking this through a real browser and worth writing down, because
it means the refusal is rarely the thing an owner meets: **`<input type="email">` converts
an internationalised domain to punycode before anything here reads the value.** Typing
`oma@exämple.de` into the console submits `oma@xn--exmple-cua.de` — ASCII, matchable, and
accepted. That is correct and is the reason the rule is stated as "a homograph must never
become the address it imitates" rather than "non-ASCII is rejected": the punycode form is
precisely *not* `oma@example.de`, so nothing is confused with anything. The outright
refusal is what a client that is not a browser e-mail field gets, and Group Q in the
behaviour harness pins the browser's behaviour so that it cannot quietly become the Latin
lookalike instead.

**On the local part, deliberately:** RFC 5321 makes it case-sensitive and this ignores that.
No mail provider a family uses distinguishes `Oma` from `oma`, the owner's decision names the
two as one person, and honouring the RFC here would leave exactly the duplicate this exists
to remove. The cost — a provider that really does distinguish them could let two of its
accounts claim one identity here — is recorded rather than handled, because defending against
it fails every real user to defend against none.

### The order a sign-in is resolved in, and who wins

1. **Known by this Authelia handle** (`principals.oidc_username`) → refresh and sign in.
2. **A verified address that exactly one live, un-merged account carries** → merge onto that
   account. It keeps its id, so every grant, revision, attachment and audit line it already
   owns is simply still there; there is nothing to copy.
3. **Two accounts carry it** → **refused** (403). "Which of these is this person" has no safe
   guess, and guessing hands one of them the other's access. The owner resolves it by hand.
4. **Nobody carries it** → a new principal, mirrored from the verified claims, as before.
5. **…unless the Authelia handle is already somebody else's `username`** → **refused** (403).
   This is the takeover described above. Both refusals answer 403 and neither says which one
   applied: a caller who could tell them apart would learn whether a given username exists.

**Who wins on a conflict: the identity that already exists, in both directions.** The
Authelia sign-in loses (3 and 5). And so does the invitation — see below.

### An invitation must name an address, and may not name one already in use

The address is **required**. An invitation without one guarantees a second account for one
person and nothing repairs it afterwards but withdrawing the link and making another; making
it optional is how half the accounts end up unmergeable. The form asks for it as a required
field and says on screen what it is for, because "required" with no reason is a field people
fill in with anything.

An invitation whose address is already carried by an account, or by another **outstanding**
invitation, is **refused** (409, naming who holds it). A withdrawn, spent or expired
invitation does not hold an address — otherwise a typo would be permanent.

**The reverse merge is therefore not performed, and that is the decision, not an omission.**
An Authelia person who already exists here and is later invited by the same address does not
get a password attached to their principal. The brief asked whether that is even desirable.
It is not: the invitation link *is* a credential, handed to one person over a chat app, and
an invitation that lands on an existing identity is a way to **set a password on an account
that already holds grants** — a mistyped address aims that at the owner. The owner already
has a way to give an existing person access and it is »Zugriff«; the refusal says so.

The same question is asked again inside the acceptance transaction, because a month can pass
between the link being handed over and it being clicked. A refusal there **rolls back the
consuming UPDATE**, so the link stays live: the address now belongs to somebody, which is
something for the owner to look at rather than a reason to silently burn a link the
recipient is holding.

The creation-time refusal is an existence oracle — an administrator learns that an address
already has an account here. That is bounded by the gate in front of it, which has already
established that the caller administers the space they are inviting into, and it is the same
population »Personen« already lists in full.

### Merging widens nothing

A merged principal's reach is its **own direct grants** ∪ **the baseline `group_roles`
already confers on the verified groups**. There is no third source: `baseline_on` is
unchanged, and it is the same union every mirrored Authelia account has had all along.
Authelia's groups arrive with no reach of their own — a group with no row in `group_roles`
confers `public`, which is the fail-closed default expressed as the absence of a row.

A **deactivated** account is not a merge candidate. "This person is gone" must not be undone
by them signing in the other way; the sign-in falls through to an account of its own.

An account that already answers to a different Authelia handle is not a candidate either, so
a second Authelia account asserting the same verified address gets a principal of its own
rather than a second claim on the first.

### Names

Authelia is authoritative for **groups** on every sign-in it completes, merged or not — that
is why losing a group there takes effect at the next sign-in. It is authoritative for the
**display name** only of an account it is the sole source of. A merged account's name was
typed by the person themselves on the acceptance page, and having it replaced by whatever the
homelab directory holds is a rename nobody asked for.

`username` is likewise never overwritten. It is the handle every screen shows and every grant
was written against; the Authelia handle lives beside it in `oidc_username`, and both strings
are true at once.

### Sessions

**Signing out ends the session that was presented, and only that one.** One principal may
hold several sessions — two browsers, two credentials — and "abmelden" has never meant
"everywhere". Signing out of great-wiki still does not touch the Authelia session, which is
unchanged and already documented.

What the merge changes is **deactivation**, and it changes it for the better: D-M2-7 deletes
every session a principal holds, and after a merge that is one principal, so deactivating a
merged account closes both doors at once. Before, it closed one and left the other signing
in. Both halves are pinned by tests.

### Audit

The merge writes `identity.merge`, instance-wide, naming **both halves** — the local
username, the Authelia handle, the canonical address and the groups. An auditor a year later
has to be able to see that `erika.mueller` and `oma` became one row and on the strength of
which address. `invite.create` now records the canonical address too, so the two rows read
together.

### Nothing is merged retroactively

Accounts that already exist are **not** merged by this change, and there is no endpoint that
would. Two accounts sharing an address may be one person with two credentials, or may be a
shared family address that two people genuinely use — and the second, merged, is one person
silently holding the other's grants. That is the owner's call.

What exists instead is a listing, and it only reads:

```
GET /api/admin/identity/candidates          # instance admins only
```

```bash
curl -s --cookie "__Host-gw_session=$TOKEN" \
     https://wiki.example.com/api/admin/identity/candidates | jq .
```

It groups every address that more than one account carries, naming each account, its kind,
whether it is active and whether it has already been merged. **It has not been run against
`data/great-wiki.db`.** Resolving a pair is done by hand under »Personen«.

## Alternatives rejected

- **Leave the two flows apart and explain the difference.** What the walkthrough found: two
  flows for one family is the thing that gets explained wrong, and the console said nothing
  about it anywhere.
- **Pre-grant to an Authelia username on the invitation.** A reference to an account that
  does not exist yet, and a username is not a thing a relative knows.
- **Merge on the `email` claim without `email_verified`.** One profile edit in Authelia away
  from being anybody.
- **Merge on `sub`.** Stable and opaque, and it says nothing about who the person is here —
  there is no way for an invitation written before they ever signed in to name one.
- **A UNIQUE index on `email_canonical`.** It would fail the migration on a database that
  already holds a duplicate, and afterwards would turn »Person anlegen« typing a duplicate
  address into a 500. The merge refuses an ambiguous address instead, which is fail-closed,
  and the listing above surfaces the pairs.
- **Backfill the key in SQL.** Two implementations of the normalising rule that can disagree,
  and the way they would disagree is by merging two people. The column is added by the
  migration and filled in Rust at startup, idempotently, by the one function.
- **Let an invitation add a password to an existing identity.** Rejected above: it turns a
  link somebody was sent into a way into an account that already has grants.

## Consequences

- `POST /api/admin/invites` now **requires** `email`, and answers 400 for one it cannot key
  on and 409 for one already in use. This is a breaking change to that endpoint; the console
  is the only caller.
- `principals` gains `oidc_username` (partial-UNIQUE) and `email_canonical` (indexed).
  `invites` gains `email_canonical`. Migration `0014_identity_merge.sql`, plus a Rust
  backfill in `Store::open`.
- `Principal` gains `oidc_username`, and »Personen« shows a merged account as
  *Lokal + Authelia (handle)* rather than as *Lokal*, so that an owner about to deactivate
  one can see it closes both doors.
- The username-collision takeover is fixed, and the fix is visible: an Authelia sign-in whose
  handle is somebody else's username is now refused where it used to succeed silently.
- `upsert_oidc_principal` keeps its signature and becomes the no-merge-key form, so the
  development shim and thirty test call sites are unchanged.
- Still not walked against a real Authelia. Everything here is exercised against the
  stand-in provider in `crates/gw-api/tests/auth.rs`, which signs real RS256 tokens — but
  the shape of Authelia's own `email_verified` claim is a thing to confirm on `cloud.vm`
  before the first relative arrives. If Authelia does not send it in the id token, the
  userinfo path already handles it; if it does not send it at all, **nobody will ever merge**
  and the log will say so on every sign-in.
