-- One person, one identity: an Authelia account is merged into a local one by email.
--
-- Until now an invitation created a `local` principal with a password, and the same
-- person arriving through Authelia was mirrored as a SECOND, `oidc` principal with no
-- access at all. Two rows, two histories, two credentials, and the relative who was
-- invited had to be told which button to press. This migration adds the two columns that
-- let one row answer to both.
--
-- `oidc_username` — the Authelia handle, which is NOT the same string as `username`.
--
--   `username` is the handle the person was invited under and the one every screen shows;
--   `oidc_username` is `preferred_username` out of the verified token. A merged principal
--   has both and they differ (`oma` and `erika.mueller`), so one column cannot carry both.
--
--   It is also what the OIDC sign-in now LOOKS UP BY, and that is a fix rather than a
--   refactor: the previous `ON CONFLICT (username) DO UPDATE` meant an Authelia account
--   whose `preferred_username` happened to equal an existing LOCAL account's username
--   silently took that account over — its grants, its history, its everything — with no
--   address and no verification involved anywhere. Keyed on this column, that cannot
--   happen: an Authelia handle that is somebody else's `username` and matches no verified
--   address is refused outright (`OidcSignIn::UsernameHeldByAnother`).
--
--   UNIQUE, partially: two principals answering to one Authelia account is the duplicate
--   this whole change exists to prevent. NULL for every account that has never signed in
--   through Authelia, and SQLite's partial index leaves those alone.
--
-- `email_canonical` — the merge key, derived and never typed.
--
--   NOT unique, deliberately. »Person anlegen« can still write two accounts with the same
--   address, and an existing database may already hold such a pair; a UNIQUE index would
--   make this migration fail on real data and, worse, would turn a duplicate into a 500
--   at the moment somebody creates an account. Instead the merge REFUSES when more than
--   one account carries the address (`OidcSignIn::AmbiguousEmail`) — fail closed — and
--   `Store::merge_candidates` lists every such pair for the owner to resolve by hand.
--
-- WHY THE EMAIL HALF IS NOT BACKFILLED HERE
--
-- There is exactly one definition of the merge key and it is `gw_store::canonical_email`
-- in Rust: trim ASCII whitespace, refuse anything that is not printable ASCII (so no IDN
-- and no homograph is ever folded into the address it imitates), require exactly one `@`
-- with a non-empty side each, then lowercase. Writing that a second time in SQL — where
-- `trim()` trims spaces only and `lower()` is ASCII-only in a way that happens to agree
-- today — would be two rules that can disagree, and the way they would disagree is by
-- merging two people. So the backfill runs in `Store::open`, in Rust, over the rows this
-- migration leaves NULL, and it is idempotent: an address with no key stays NULL and is
-- simply re-examined next start, on a table with tens of rows.
--
-- The `oidc_username` half IS backfilled here, because it is a copy and not a rule: every
-- existing `oidc` principal was created by `upsert_oidc_principal` with its Authelia
-- handle as its username, which is exactly what the new column means.

ALTER TABLE principals ADD COLUMN oidc_username TEXT;
ALTER TABLE principals ADD COLUMN email_canonical TEXT;

CREATE UNIQUE INDEX principals_oidc_username
    ON principals (oidc_username) WHERE oidc_username IS NOT NULL;
CREATE INDEX principals_email_canonical
    ON principals (email_canonical) WHERE email_canonical IS NOT NULL;

UPDATE principals SET oidc_username = username WHERE kind = 'oidc';

-- The same key on the invitation, so "is this address already spoken for?" is one indexed
-- lookup across both tables rather than a scan of every outstanding link. Backfilled in
-- Rust for the reason above.
ALTER TABLE invites ADD COLUMN email_canonical TEXT;
CREATE INDEX invites_email_canonical
    ON invites (email_canonical) WHERE email_canonical IS NOT NULL;
