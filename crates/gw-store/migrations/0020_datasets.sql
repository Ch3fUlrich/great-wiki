-- Datasets (ADR 0029): a dataset is a page (`documents.doc_type = 'dataset'`; that column has
-- no CHECK, so nothing to extend). Rows inherit the page's access: there is no per-row ACL,
-- and every access goes through the permission-checked document accessor.
--
-- `dataset_field` is typed metadata; `dataset_row."values"` is one JSON object keyed by field
-- key. `key` is validated by gw-core `FieldKey::parse` before it reaches SQL. `kind` must
-- match gw-core `FieldKind::ALL` (a test asserts every kind is accepted).
-- Both tables go with their page (CASCADE on a purge; trashing keeps them).
CREATE TABLE dataset_field (
    id         TEXT PRIMARY KEY,
    doc_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    key        TEXT NOT NULL,
    label      TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN (
        'text', 'number', 'bool', 'date', 'select', 'multi_select', 'tags',
        'url', 'person', 'file', 'relation', 'rollup', 'formula')),
    config     TEXT NOT NULL DEFAULT '{}',
    position   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (doc_id, key)
);

CREATE INDEX dataset_field_doc ON dataset_field (doc_id, position);

CREATE TABLE dataset_row (
    id         TEXT PRIMARY KEY,
    doc_id     TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    "values"   TEXT NOT NULL DEFAULT '{}',
    version    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX dataset_row_doc ON dataset_row (doc_id, created_at, id);
