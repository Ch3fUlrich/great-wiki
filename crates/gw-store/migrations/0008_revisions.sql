-- Revisions: the append-only history under every page.
--
-- Numbered 0008 and not the plan's 0004 because M2 shipped through 0007_invites.sql.
-- Nothing was renumbered; a migration that has already run somewhere is not a file you
-- get to rename.
--
-- Revisions are APPEND-ONLY and are never updated in place. That is what makes diff,
-- restore and blame ordinary queries rather than features, and it is what makes "restore"
-- safe: restoring publishes the old content as a NEW revision, so the history it was
-- restored from is still there afterwards. A restore that rewinds by deleting the
-- revisions after it turns "let me look at the old version" into data loss, which is the
-- one thing a history must never do.
CREATE TABLE revisions (
    id           TEXT PRIMARY KEY,             -- uuid v7: sortable by creation time
    -- ON DELETE CASCADE is the purge path (D-M3-6). Purging a page must destroy its
    -- history with it — that is the whole point of purge, as against trash — and a
    -- history left behind by a deleted page would be content nothing can reach, nothing
    -- can permission-check, and nothing will ever clean up.
    document_id  TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    -- What this revision was published on top of. NULL for the first one. The chain is
    -- what blame walks; it is recorded rather than derived from timestamps because two
    -- revisions can share a timestamp (see the index below) and "the one before" then has
    -- no answer.
    parent_id    TEXT REFERENCES revisions(id),
    body         TEXT NOT NULL,                -- Block tree as JSON, exactly as documents.body
    -- Why this edit happened, in the author's words. Optional: demanding one produces
    -- "update" on every row, which is worse than nothing because it looks like an answer.
    summary      TEXT,

    -- WHO, and why that is two columns rather than one.
    --
    -- `author_id` is the identity. Blame, "everything this person wrote" and any later
    -- contribution view join on it, and it survives a rename. It carries NO foreign key,
    -- for the same reason `audit_log.principal_id` and `instance_admins.granted_by` carry
    -- none: the record of who wrote something must not vanish — or block the deletion —
    -- when the account is removed. D-M3-4 is explicit that offboarding removes access and
    -- nothing else, and a history that forgets who wrote the current text when somebody
    -- leaves has destroyed most of what it was for.
    --
    -- `author_name` is a SNAPSHOT of the display name at the moment of publishing. A join
    -- to `principals` would render every revision by a departed colleague as "unknown",
    -- and D-M3-4 forbids exactly that. It is deliberately not kept in step with later
    -- renames: it says who wrote this, as they were called then.
    author_id    TEXT NOT NULL,
    author_name  TEXT NOT NULL,

    -- Of the stored JSON. The timeline shows "+412" against the previous revision, which
    -- is the cheapest useful signal about an edit that exists — recorded at write time
    -- because computing it later means loading every body to render a list of them.
    byte_size    INTEGER NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- The timeline query: one document, newest first.
--
-- `datetime('now')` has one-second resolution, so two revisions published in the same
-- second carry the SAME timestamp — which is every publish a person makes twice in a row,
-- not a rare case. Every read therefore breaks the tie on `id`, which is a uuid v7: it
-- orders by creation time to the millisecond, and the generator keeps a counter WITHIN a
-- millisecond, so the order is total rather than merely probable. That second half is a
-- property of the `uuid` crate and is pinned by `the_id_the_timeline_breaks_ties_on_is_
-- monotonic`, because when it breaks the timeline is simply wrong and never loud.
--
-- Ordering on the timestamp alone would show a rapid edit and its predecessor in an
-- arbitrary order, and would hand `parent_id` a chain that contradicts the list it is
-- drawn from.
CREATE INDEX revisions_document ON revisions (document_id, created_at DESC);

-- Append-only, enforced here rather than promised in a doc comment.
--
-- Nothing in this codebase updates a revision, and this trigger is what keeps that true
-- of code nobody has written yet — including a future importer or repair script, which is
-- exactly the sort of thing that rewrites history "just this once".
--
-- DELETE is deliberately NOT blocked: the cascade above is how purge (D-M3-6) works, and
-- a trigger that refused it would leave the one operation that is meant to destroy
-- history unable to.
CREATE TRIGGER revisions_are_append_only
BEFORE UPDATE ON revisions
BEGIN
    SELECT RAISE(ABORT, 'revisions are append-only: publish a new one instead');
END;

-- The live CRDT state, separate from revisions: it changes on every keystroke, whereas a
-- revision is a deliberate publish. Storing them together would make the revision table
-- churn and would blur "what was saved" with "what is being typed".
--
-- Created here, with the revisions it will be snapshotted into, because the two halves of
-- M3's storage belong to one schema change. Nothing reads it yet — the CRDT lands in
-- gw-collab and the socket that persists it in gw-api.
CREATE TABLE crdt_state (
    document_id  TEXT PRIMARY KEY REFERENCES documents(id) ON DELETE CASCADE,
    state        BLOB NOT NULL,               -- yrs encoded state vector + document
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

-- What the page currently shows, as a revision. `documents.body` still holds the content
-- itself so that reading a page stays one row and one query; this says which revision that
-- body came from, which is what makes the next publish's `parent_id` a fact rather than a
-- guess at "the newest one".
ALTER TABLE documents ADD COLUMN current_revision_id TEXT REFERENCES revisions(id);

-- Whether this page is a template, i.e. offered by "new from template" rather than being
-- one more page in the tree. A flag and not a document type: a template is an ordinary
-- page in every other respect, and a second doc_type would have to be handled by every
-- query that already switches on it.
ALTER TABLE documents ADD COLUMN is_template INTEGER NOT NULL DEFAULT 0;
