-- The old address of a moved page forwards to wherever the page is now — until something else
-- takes that address (roadmap, 2026-09-24; ADR 0023).
--
-- **By id, not by path.** A forward names the DOCUMENT, and the reader is sent to that
-- document's current path, resolved at the moment of asking. So a page moved twice leaves two
-- forwards that both land on where it is now, with no chain to follow and nothing to rewrite
-- on the second move — the argument `links` makes for storing ids (ADR 0019), applied to the
-- one kind of reference that lives outside this database: a bookmark.
--
-- ON DELETE CASCADE because a forward to a purged page is a forward to nothing. A page in the
-- trash keeps its forwards and they answer nothing while it is there, since the accessor that
-- resolves them refuses a trashed row — exactly as its own address does.
CREATE TABLE forwards (
    old_path    TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX forwards_document ON forwards (document_id);

-- "Until reused", made a property of the schema rather than of every writer.
--
-- Any row that takes a path ends the forward that pointed away from it: a page created there
-- (the seeder, a future create endpoint, anything), and a page moved there — including the
-- page moving back to where it came from. Written as triggers because the alternative is a
-- DELETE that every present and future writer of `documents.path` has to remember, and the
-- one that forgets turns an address with a real page at it into a redirect away from that
-- page. A trashed row counts: it still occupies its path (`documents.path` is UNIQUE across
-- soft-deleted rows), so nothing else can be at that address anyway.
CREATE TRIGGER forwards_end_when_a_page_arrives_insert
AFTER INSERT ON documents
FOR EACH ROW
BEGIN
    DELETE FROM forwards WHERE old_path = NEW.path;
END;

CREATE TRIGGER forwards_end_when_a_page_arrives_update
AFTER UPDATE OF path ON documents
FOR EACH ROW
BEGIN
    DELETE FROM forwards WHERE old_path = NEW.path;
END;
