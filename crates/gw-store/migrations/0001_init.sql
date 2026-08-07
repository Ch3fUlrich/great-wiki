-- Documents are the SOURCE OF TRUTH, not a derived cache. Deleting this database loses
-- content. Backup and the git export (M17, M12) are how it is protected.

CREATE TABLE documents (
    id            TEXT PRIMARY KEY,               -- uuid v7: sortable by creation time
    parent_path   TEXT,                            -- NULL for a root document
    -- Materialised path, e.g. '/handbook/onboarding'. Subtree queries and permission
    -- inheritance are both a prefix match on this, which an index serves directly.
    path          TEXT NOT NULL UNIQUE,
    slug          TEXT NOT NULL,
    doc_type      TEXT NOT NULL,
    title         TEXT NOT NULL,
    language      TEXT NOT NULL DEFAULT 'de',
    -- Fail closed: anything that does not say otherwise is restricted.
    visibility    TEXT NOT NULL DEFAULT 'restricted'
                  CHECK (visibility IN ('public', 'internal', 'restricted')),
    body          TEXT NOT NULL,                   -- Block tree as JSON
    sort_key      INTEGER NOT NULL DEFAULT 0,      -- sibling order, for drag-to-reorder
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now')),
    deleted_at    TEXT                             -- soft delete; M3 adds the trash UI
);

CREATE INDEX documents_parent   ON documents (parent_path, sort_key);
CREATE INDEX documents_prefix   ON documents (path);
CREATE INDEX documents_visible  ON documents (visibility) WHERE deleted_at IS NULL;
