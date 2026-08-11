-- Invites: a link that creates an account (D-M2-2, D-M2-3, D-M2-12, D-M2-20, D-M2-21).
--
-- An invite is a CREDENTIAL. Whoever holds the link can create an account and walk into
-- whatever the row below says, without ever presenting a password to anybody. That is what
-- makes it worth having — no credential passes through chat or email (D-M2-3) — and it is
-- also why this table is shaped the way it is.
--
-- ONLY THE HASH. `token_hash` is the SHA-256 of the token, exactly as `sessions` stores a
-- session. The plaintext exists in two places and neither of them is here: the link the
-- console showed once, and whatever the inviter pasted it into. A copy of this database is
-- therefore not a bag of live invitations, and the lookup hashes what it was given and
-- compares hashes — which is why the column is UNIQUE: an index on a value nobody can
-- reverse.
--
-- WHAT AN INVITE CARRIES. D-M2-20 says both, because they answer different questions:
-- `path` + `permission` is "read this one page", `team_id` is "everything this team can
-- reach". Both are optional individually and the second CHECK refuses an invite that
-- carries NEITHER. That refusal is deliberate rather than a missing feature: an invite
-- with no grant mints an account that reaches only what needs no account at all, and it
-- reopens exactly the gap — "make the account now, give it access later" — that D-M2-20
-- exists to close. Somebody who really wants an account with nothing attached can be
-- created directly in the console, where it is an explicit act rather than an oversight.
--
-- SINGLE USE, AND HOW. `accepted_at` is NULL until somebody accepts. Consumption is one
-- UPDATE carrying its own precondition — `WHERE accepted_at IS NULL AND revoked_at IS NULL
-- AND expires_at > datetime('now')` — inside the transaction that also creates the
-- account, so two simultaneous accepts cannot both see an unspent invite. A SELECT
-- followed by an UPDATE would leave precisely that window, and the second accept would
-- fail on the username's UNIQUE constraint if it failed at all: an error rather than a
-- refusal, and only by luck.
--
-- STATE IS DERIVED, NEVER STORED. There is no `state` column, because a stored one can
-- disagree with the timestamps beside it. `expired` is `expires_at <= datetime('now')`,
-- read at the moment of asking, in the same spirit as `sessions.expires_at`: a row that has
-- outlived its window stops working the instant it does, whether or not anything has got
-- round to noticing.
CREATE TABLE invites (
    id            TEXT PRIMARY KEY,             -- uuid v7, and NOT a secret
    -- SHA-256 of the token, hex. Never the token.
    token_hash    TEXT NOT NULL UNIQUE,

    -- Who made it. No foreign key, for the reason `audit_log.principal_id` and
    -- `instance_admins.granted_by` carry none: it records who acted, and that record must
    -- not vanish — or block a deletion — when the account that acted is removed. The
    -- invite page resolves it to a display name and says "somebody" when it cannot.
    invited_by    TEXT,

    -- The account this invite will create. Fixed at creation, because the acceptance page
    -- asks for a display name and a password and nothing else: letting the recipient
    -- choose a username would let one invite be redeemed as any name at all, including one
    -- an ACL already names.
    username      TEXT NOT NULL,
    email         TEXT,

    -- The direct grant, if there is one. Written on `path` at acceptance exactly as
    -- `POST /api/admin/acl` would write it, so there is one kind of grant in this database
    -- rather than a second kind that only invites produce.
    path          TEXT,
    permission    TEXT CHECK (permission IN ('read','comment','write','admin')),

    -- The team, if there is one. ON DELETE CASCADE rather than SET NULL: an invite into a
    -- team that no longer exists must not survive as an invite into nothing, quietly
    -- creating an account with no access.
    team_id       TEXT REFERENCES teams(id) ON DELETE CASCADE,

    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    -- Thirty days (D-M2-21). Stored rather than computed at read time so that changing the
    -- constant does not silently extend or expire every link already handed out.
    expires_at    TEXT NOT NULL,

    revoked_at    TEXT,
    revoked_by    TEXT,
    accepted_at   TEXT,
    -- The account it produced. ON DELETE SET NULL: `accepted_at` is what makes the invite
    -- spent, so losing the pointer must never make a spent invite live again.
    accepted_principal_id TEXT REFERENCES principals(id) ON DELETE SET NULL,

    -- A path with no permission is a grant that confers nothing; a permission with no path
    -- is a grant with nowhere to apply. Both are somebody believing they granted access.
    CHECK ((path IS NULL) = (permission IS NULL)),
    -- D-M2-20, stated where it cannot be forgotten by a future caller.
    CHECK (path IS NOT NULL OR team_id IS NOT NULL)
);

-- The console's list, and the space-admin filter that reads it: entries are checked one
-- distinct path at a time against the permission engine, so the path is the column that
-- is looked up.
CREATE INDEX invites_path ON invites (path);
-- Newest first, which is the order an administrator wants and the order a spent-invite
-- sweep would want too.
CREATE INDEX invites_created ON invites (created_at DESC);
