-- Full-text search over pages (M7; ADR 0024, on the engine choice of ADR 0003).
--
-- `body_text` is the plain text of `body`'s block tree, written in the same statement as
-- `body` by the two places a body is written (`create_document`, `append_revision`). Nullable
-- because ALTER cannot compute it: rows that predate this migration are filled in by
-- `Store::open` (Rust owns the one flattener, `gw_core::body_plain_text`) and indexed by a
-- `rebuild` straight after. A NULL is therefore only ever "not yet backfilled".
ALTER TABLE documents ADD COLUMN body_text TEXT;

-- An FTS5 EXTERNAL-CONTENT table: the index holds tokens and nothing else, and reads the
-- text back from `documents` when it has to (snippets, `rebuild`). The text exists once.
--
-- The index holds NO path and NO state — not `deleted_at`, not visibility. A hit is a rowid;
-- everything about the page is read from `documents` at query time, so a move or a rename
-- needs no index write, trash/restore is a `deleted_at IS NULL` condition, and no permission
-- fact can go stale in here because none is kept here.
--
-- `remove_diacritics 2` folds umlauts (Müller finds muller) for this German/English corpus.
-- `content_rowid` is the implicit rowid of `documents`: the table has a TEXT primary key, so
-- a `VACUUM` may renumber it. Nothing runs VACUUM; if one ever does, the repair is the same
-- `rebuild` the open-time backfill already issues.
CREATE VIRTUAL TABLE search_pages USING fts5(
    title,
    body_text,
    content = 'documents',
    content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2'
);

-- Rows that already exist are indexed now, by title (their `body_text` is still NULL, which
-- FTS5 reads as no text). It matters that every row is in the index BEFORE any trigger can
-- fire: the update and delete triggers hand FTS5 the values being removed, and removing
-- values that were never indexed corrupts an external-content index ("database disk image is
-- malformed"). `Store::open` then fills `body_text` and rebuilds again.
INSERT INTO search_pages (search_pages) VALUES ('rebuild');

-- Triggers, because the alternative is an index write that every present and future writer of
-- `documents.title` / `body_text` has to remember, and the one that forgets leaves a page
-- findable by words it no longer contains. An external-content index is only correct if the
-- 'delete' command is given exactly the values that were indexed, so the update trigger
-- deletes the OLD row's values and inserts the NEW ones.
CREATE TRIGGER search_pages_insert
AFTER INSERT ON documents
BEGIN
    INSERT INTO search_pages (rowid, title, body_text)
    VALUES (new.rowid, new.title, new.body_text);
END;

CREATE TRIGGER search_pages_update
AFTER UPDATE OF title, body_text ON documents
BEGIN
    INSERT INTO search_pages (search_pages, rowid, title, body_text)
    VALUES ('delete', old.rowid, old.title, old.body_text);
    INSERT INTO search_pages (rowid, title, body_text)
    VALUES (new.rowid, new.title, new.body_text);
END;

-- A purge is a hard `DELETE FROM documents` (ADR 0012). This trigger is what makes "a purge
-- destroys" true of the index too: without it the tokens of a destroyed page, and its words
-- in any snippet, would outlive the page.
CREATE TRIGGER search_pages_delete
AFTER DELETE ON documents
BEGIN
    INSERT INTO search_pages (search_pages, rowid, title, body_text)
    VALUES ('delete', old.rowid, old.title, old.body_text);
END;
