-- Attachments (D-15, D-16, D-17): the files that sit beside the prose.
--
-- Two tables, because they answer two different questions and only one of them is about
-- access:
--
--   * `blobs` is the INDEX OF THE MOUNT. One row per distinct sequence of bytes, keyed by
--     its SHA-256, which is also its address under `$GW_MEDIA_DIR/blobs/`. It says nothing
--     about who may see anything.
--   * `attachments` is the `Anhänge` LIST — "this page carries this file, under this name".
--     It is the only thing that says a file is attached (D-15), and it is what a download
--     is authorised through (D-16).
--
-- **The split is the whole of D-16.** One PDF on two pages is one row in `blobs` and two
-- rows here, and each of those two rows is a separate statement about who may see a
-- *page*. Nothing anywhere joins a blob to a permission, and nothing can: `blobs` has no
-- column naming a document. A download therefore cannot be authorised against the bytes
-- even by accident — there is nothing there to authorise against.
--
-- The consequence that must not be lost: **a sha is never an address a reader can hold.**
-- `gw_store::attachments` resolves (page, filename) to a sha and `gw_api::routes::
-- attachments` never serialises one, so the only way to the bytes is through a page whose
-- ACL was just consulted. If a sha ever appears in a URL, D-16 is undone — the page check
-- becomes optional and the blob becomes the resource.

CREATE TABLE blobs (
    -- Lowercase hex, 64 characters, and the schema says so rather than trusting the code
    -- that writes it.
    --
    -- This is not tidiness: this column IS the path on disk
    -- (`blobs/ab/cd/abcd…`), so a value holding `..` or `/` would be a directory traversal
    -- written by the database. `gw_store::blobs` builds the path from the digest it
    -- computed itself and never from a request, which is the real defence; this is the one
    -- that survives somebody adding a second writer. `NOT GLOB '*[^0-9a-f]*'` is how SQLite
    -- spells "every character is a lowercase hex digit" — a bare `GLOB '[0-9a-f]*'` would
    -- only constrain the first one.
    sha256     TEXT PRIMARY KEY
               CHECK (length(sha256) = 64 AND sha256 NOT GLOB '*[^0-9a-f]*'),

    -- The size the bytes actually were, measured while hashing them. Never a
    -- `Content-Length` a client asserted.
    byte_size  INTEGER NOT NULL CHECK (byte_size > 0),

    -- The type the BYTES are, sniffed from their leading magic number by
    -- `gw_store::blobs::sniff` — never the `Content-Type` an upload declared, and never
    -- guessed from the filename. This is the value the download echoes back, so a file
    -- claiming to be a PNG and not being one is served as what it is, or refused. An upload
    -- whose bytes match nothing in the allowlist never reaches this table at all.
    media_type TEXT NOT NULL,

    created_at TEXT NOT NULL DEFAULT (datetime('now'))
) WITHOUT ROWID;

-- The `Anhänge` list. D-15: this is the AUTHORITY on what is attached to a page, and an
-- inline block in the body is a *reference* to a row here.
--
-- Which is why nothing in this table is derived from `documents.body`, and nothing may
-- become so. Publishing a revision that no longer mentions a file must leave its row
-- exactly where it is — the same shape D-2 gives a task whose checkbox line was cut, and
-- the reason the reconciliation `gw_store::tasks` performs on publish has no counterpart
-- here. Detaching is its own act, on its own address, and it is the only thing that removes
-- a row.
CREATE TABLE attachments (
    id               TEXT PRIMARY KEY,          -- uuid v7: sortable by creation time

    -- The page this file is attached to, BY ID, for the reason `0012_trash.sql` gives for
    -- `deleted_root` and `0010_tasks.sql` for `projects.home_doc`: `path` is the one column
    -- of `documents` that is meant to change, and an attachment that lost its page because
    -- somebody renamed something would be a file nobody could reach and nobody could find.
    --
    -- ON DELETE CASCADE, so a purge takes the list with the page. It is a purge that has to
    -- report it (ADR 0013), not this constraint — the cascade is what makes the count a
    -- measurement across the DELETE rather than a second query beside it.
    doc_id           TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,

    -- The bytes. **No ON DELETE anything**, and that is the decision in ADR 0013 expressed
    -- as a constraint: a `blobs` row outlives its last reference, so an orphan is a row
    -- somebody can list rather than a file only a directory walk could find. The absent
    -- cascade also means the reverse is refused — deleting a blob row that an attachment
    -- still points at raises rather than silently detaching a page's file, which is what a
    -- future reclamation sweep needs to be safe.
    sha256           TEXT NOT NULL REFERENCES blobs(sha256),

    -- What the file is called on this page, as somebody named it. One path segment and
    -- nothing else: it is half of the address a download is reached through
    -- (`/api/attachment/{filename}/{page}`), so a name holding `/` would be a second page
    -- path, and `.`/`..` would be a directory.
    --
    -- It never becomes a filesystem path — the file on disk is named by its digest — so
    -- this CHECK is not what stops a traversal. It is what stops an UNREACHABLE row: a name
    -- that cannot be expressed in the address is a file attached to a page and downloadable
    -- from nowhere.
    filename         TEXT NOT NULL
                     CHECK (filename <> '' AND filename NOT IN ('.', '..')
                            AND instr(filename, '/') = 0),

    -- Who attached it, and what they were called at the time. Two columns for the reason
    -- `revisions` and `0012_trash.sql` both give: the id is what code joins on, the name is
    -- a SNAPSHOT so the list still says who put a file there after they have left. Neither
    -- carries a foreign key, for the same reason `revisions.author_id` carries none.
    uploaded_by      TEXT,
    uploaded_by_name TEXT NOT NULL,
    uploaded_at      TEXT NOT NULL DEFAULT (datetime('now')),

    -- One name per page. An upload onto a name already taken is a conflict the caller is
    -- told about, never a silent replacement: replacing bytes under a name an inline block
    -- already points at would change what a paragraph shows without touching the page, and
    -- the same file under two names on one page is two rows over one blob, which is fine.
    --
    -- This index also serves the listing — every attachment of one page, in name order — so
    -- there is deliberately no second index on `doc_id`.
    UNIQUE (doc_id, filename)
);

-- The reverse question, and the only one that is not about a page: "does anything still
-- reference these bytes?". A purge measures the orphans it creates with it, and the
-- reclamation sweep ADR 0013 describes would be driven by it.
CREATE INDEX attachments_blob ON attachments (sha256);
