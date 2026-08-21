-- Tasks, boards and projects (piece 3 of the links/topics/tasks design).
--
-- Two tables, and one invariant that runs through both of them: **every task has exactly
-- one governing page.** That page is the anchor document when the task was written as a
-- line in a page (D-1), or the project's home page when it is a standalone card. It is
-- what decides who may see the card, who may move it, and who may be given it — see
-- `crates/gw-store/src/tasks.rs`, which is where that rule is applied. The schema's job is
-- to make "no governing page" unrepresentable, so that a query written in a hurry cannot
-- produce a row nothing can authorise. The Security section of the design is explicit that
-- an aggregate view is a disclosure surface: a board card reveals that a page exists and
-- what it is called.
--
-- Neither table is WITHOUT ROWID, and that is a departure from 0009 rather than an
-- oversight. `links` is all key and nothing else — the primary key *is* the row, so the
-- table is its own index and there is nothing left for a rowid to point at. A task is the
-- opposite shape: a text primary key and eleven columns of state, including a title of
-- unbounded length. WITHOUT ROWID stores such a row in the index B-tree it is too big for,
-- and every secondary index below would repeat the 36-byte uuid primary key in place of an
-- 8-byte rowid. `documents`, `revisions` and `principals` are all ordinary rowid tables
-- with text primary keys for the same reason; these two follow them, not `links`.

-- A project is a home page plus, later, a tag that pulls in documents from elsewhere (D-3).
CREATE TABLE projects (
    id         TEXT PRIMARY KEY,               -- uuid v7: sortable by creation time

    -- The home page, BY ID and not by path, per D-5 — "a link points at the page's
    -- identity, not its path". The design's data-model sketch says `home_path`, and a path
    -- is what the permission accessor takes, so this is worth stating plainly: a path is
    -- the one column of `documents` that is meant to change. Storing it here would mean
    -- that moving the home page silently un-homes the project, and that whatever page next
    -- occupies the vacated path silently becomes the project's home — which is not a
    -- cosmetic bug, because the home page is what decides who may read the board. The path
    -- is one lookup away from the id (`Store::may` does exactly that lookup, and says why),
    -- and derived at query time it is the CURRENT path, which is what D-3's subtree
    -- membership needs after a move.
    --
    -- ON DELETE CASCADE: a project is defined by its home page. Purging that page leaves
    -- no subtree, no home and nobody who could be authorised to see the board, so the
    -- project goes with it — the same reasoning 0008 gives for revisions, where purge is
    -- meant to destroy a page and everything about it.
    home_doc   TEXT NOT NULL UNIQUE REFERENCES documents(id) ON DELETE CASCADE,

    -- D-3's "tagged extras". No foreign key: `tags` is piece 2 and does not exist yet, and
    -- SQLite resolves a foreign key's parent table when the statement runs rather than when
    -- the table is created — so an FK named here would turn every INSERT into "no such
    -- table: main.tags" the moment foreign keys are on, which they are (see `Store::open`).
    -- Nothing reads this column yet; it is here so that piece 2 adds the join and not a
    -- migration of every existing project.
    tag_id     TEXT,

    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE tasks (
    id         TEXT PRIMARY KEY,               -- uuid v7

    -- The anchor (D-1): the page the line was written in, and the block within it. Both
    -- NULL means standalone — a card created on a board that belongs to no page.
    --
    -- ON DELETE CASCADE, deliberately, and the alternative deserves its reasons. D-8 keeps
    -- a task whose *line* has been deleted, because deleting it would silently discard a
    -- due date and an assignee somebody set. That is the `detached` column below, and the
    -- anchor stays: the page is still there, still says who may read it, and the card sits
    -- on its board marked "this page no longer mentions it". A PURGE is a different event.
    -- The page is gone and so are its grants, and a card carrying text copied out of it
    -- would outlive the ACL that was hiding that text — a restricted page's words, on
    -- whatever board the card happened to sit on, with nothing left to check them against.
    -- So the cards go with the page, exactly as its revisions and its CRDT state already do
    -- (0008), and for the same stated reason: destroying a page and everything about it is
    -- what purge is for.
    doc_id     TEXT REFERENCES documents(id) ON DELETE CASCADE,
    -- The uuid the task block carries in its `attrs`. No foreign key exists to point it at
    -- — blocks live inside `documents.body` as JSON — and reconciling it against the body
    -- on publish is a later task, out of scope here.
    block_id   TEXT,

    -- The words. For an ANCHORED task these are a copy of the block's text and the page
    -- owns them (D-2): the board renders this, the page renders itself, and reconciliation
    -- keeps this in step. For a STANDALONE task there is no block, so this is the task.
    -- Kept NOT NULL for both, because a detached card still has to be readable.
    title      TEXT NOT NULL,

    -- D-9's fixed columns, enforced here and not only in Rust. The schema is the single
    -- source of truth for what a status may be: `TaskStatus` in `tasks.rs` mirrors this
    -- list, and `every_status_the_rust_enum_knows_is_accepted_by_the_schema` is what stops
    -- the two drifting. Note the non-ASCII `ä` in `Läuft`: SQLite compares text with the
    -- BINARY collation, byte for byte, so the composed form written here (U+00E4) and a
    -- decomposed `a` + U+0308 are two different statuses as far as this constraint is
    -- concerned. Both files are UTF-8 and both spell it composed.
    status     TEXT NOT NULL DEFAULT 'Offen'
               CHECK (status IN ('Offen', 'Läuft', 'Fertig')),

    -- Who is meant to do it (D-10). A foreign key, unlike `revisions.author_id`, and the
    -- difference is the point: an author is HISTORY and must survive the account being
    -- removed, whereas an assignee is CURRENT STATE — an obligation resting on somebody who
    -- no longer exists is not a record worth keeping, it is a stale name on a card. It also
    -- means "assigned to an id that is not an account" cannot be stored at all, which is
    -- the fail-closed half of the assignment rule that `tasks.rs` states in full.
    assignee   TEXT REFERENCES principals(id) ON DELETE SET NULL,
    due_at     TEXT,

    -- The project a STANDALONE card is filed under. An anchored task does not carry one:
    -- D-3 already says which project it belongs to — the one whose home subtree its page
    -- lives in, or one whose tag its page carries — and storing a second answer here would
    -- let the two disagree about a card that is visibly on a page. The CHECK below makes
    -- that structural rather than a convention.
    --
    -- ON DELETE CASCADE: a standalone card belongs to its project and to nothing else. It
    -- has no page of its own, so deleting the project would otherwise leave a row whose
    -- governing page has gone — see the CHECK.
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,

    -- Order within a column, for drag-to-reorder. Not unique: two cards may collide, and
    -- reads break the tie on `id` so the board is stable rather than merely usually stable.
    position   INTEGER NOT NULL DEFAULT 0,

    -- D-8. Set when the line that authored this task no longer appears in its page; the
    -- card stays on the board with a marker rather than vanishing. Only reconciliation
    -- (a later task) sets it.
    detached   INTEGER NOT NULL DEFAULT 0 CHECK (detached IN (0, 1)),

    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now')),

    -- EXACTLY ONE governing page. Anchored to a document, or filed under a project, never
    -- both and never neither. "Never neither" is the security half: a task with no page
    -- behind it is a row no permission check can ever answer for, and fail-closed would
    -- make it invisible to everybody forever, which is a leak of storage rather than of
    -- data but is still a row nobody can explain. "Never both" is the D-3 half above.
    CHECK ((doc_id IS NULL) <> (project_id IS NULL)),

    -- A block id names a block inside a document, so it is meaningless without one.
    CHECK (doc_id IS NOT NULL OR block_id IS NULL),

    -- "The page no longer mentions this line" presupposes a page. A standalone card has
    -- none, so it can never be detached from anything.
    CHECK (detached = 0 OR doc_id IS NOT NULL)
);

-- The board query: one project's standalone cards, in column order.
CREATE INDEX tasks_project ON tasks (project_id, status, position);

-- The other half of a board, and the reconciliation pass that is not written yet: the
-- tasks anchored in one document.
CREATE INDEX tasks_document ON tasks (doc_id);
