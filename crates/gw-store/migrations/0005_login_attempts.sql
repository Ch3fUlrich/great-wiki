-- Failed sign-in counters, for the guest password form.
--
-- D-M2-11 put a username/password form on a public hostname. That makes throttling a
-- prerequisite of shipping it rather than a later hardening pass: without it, the form is
-- an offline-speed guessing oracle that anyone on the internet can drive.
--
-- Two independent counters, and EITHER one tripping is enough to refuse:
--
--   scope = 'account'  the submitted username, whether or not such an account exists
--   scope = 'address'  the client address the request came from
--
-- Independent because the two attacks are different shapes. Counting per account alone
-- lets a botnet spray one guess at each of a thousand accounts; counting per address
-- alone lets a distributed attempt hammer one account from a thousand places. Neither
-- counter subsumes the other, so both are kept and both are consulted.
--
-- The account counter is keyed on the SUBMITTED string, not on a principal id, for two
-- reasons. A guess at a username that does not exist must cost the same as a guess at one
-- that does, or the lockout itself becomes the enumeration oracle the generic failure
-- message exists to close. And there is no principal id to key on when the account is
-- imaginary.
--
-- In SQLite rather than in memory because a counter that a restart clears is a counter an
-- attacker can clear: `just serve` after a deploy would hand back the full budget, and a
-- crash loop would hand it back repeatedly.
CREATE TABLE login_attempts (
    scope           TEXT NOT NULL CHECK (scope IN ('account', 'address')),
    subject         TEXT NOT NULL,
    failures        INTEGER NOT NULL DEFAULT 0,
    last_failure_at TEXT NOT NULL DEFAULT (datetime('now')),
    -- NULL means "counting, not locked". A time in the past means the lockout has run
    -- out; the next failure starts a fresh window rather than resuming at the limit, so a
    -- five-minute lockout can never quietly become a permanent one.
    locked_until    TEXT,
    PRIMARY KEY (scope, subject)
);

-- Housekeeping only. `login_locked` compares against datetime('now') on every call, so a
-- row that has outlived its lockout stops refusing anybody whether or not anything has
-- got round to deleting it.
CREATE INDEX login_attempts_stale ON login_attempts (last_failure_at);
