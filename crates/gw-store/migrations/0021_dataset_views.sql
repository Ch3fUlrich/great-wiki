-- Saved dataset views (ADR 0029 cl. 6): config, never data. A view is a kind plus a JSON
-- config naming field keys; every key is validated by gw-core `FieldKey::parse` and checked
-- against the dataset's own fields before it is stored. Goes with its page (CASCADE).
CREATE TABLE dataset_view (
    id         TEXT PRIMARY KEY,
    doc_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    kind       TEXT NOT NULL CHECK (kind IN (
        'table', 'board', 'form', 'calendar', 'gallery', 'timeline')),
    name       TEXT NOT NULL,
    config     TEXT NOT NULL DEFAULT '{}',
    position   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX dataset_view_doc ON dataset_view (doc_id, position, created_at);
