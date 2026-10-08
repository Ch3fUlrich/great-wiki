-- Comments (ADR 0025): a comment is governed by exactly one page, its `doc_id`.
--
-- Whoever may Read the page may read and write comments on it; there is no comment
-- permission. Every access goes through the permission-checked document accessor, so
-- nothing here is queried across pages and ids are uuids (no sequence to count by).
--
-- **Never deleted.** There is no DELETE on this table anywhere in the code: an anchored
-- comment whose passage is gone is flagged `orphaned`, a settled thread is `resolved_*`.
-- The rows leave only with their page (CASCADE on a purge; trashing keeps them).
--
-- Replies are flat: `parent_id` names a top-level comment of the same page (enforced by
-- the store, which can ask the question; a CHECK cannot).
--
-- `anchor_start` / `anchor_end` are opaque Yjs relative positions; both or neither.
-- `anchor_quote` is page text and is returned only to readers of the page.
-- `author_id` goes NULL with the account; `author_name` is the snapshot that remains.
CREATE TABLE comments (
    id           TEXT PRIMARY KEY,
    doc_id       TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    parent_id    TEXT REFERENCES comments(id),
    author_id    TEXT REFERENCES principals(id) ON DELETE SET NULL,
    author_name  TEXT NOT NULL,
    body         TEXT NOT NULL,
    anchor_start BLOB,
    anchor_end   BLOB,
    anchor_quote TEXT,
    orphaned     INTEGER NOT NULL DEFAULT 0,
    resolved_at  TEXT,
    resolved_by  TEXT,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    CHECK ((anchor_start IS NULL) = (anchor_end IS NULL))
);

CREATE INDEX comments_doc ON comments (doc_id, created_at);
