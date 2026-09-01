-- Topics (piece 2 of the links/topics/tasks design): a subject a page is about, and the
-- tree those subjects form.
--
-- D-4 decided that topics are NOT nodes in the graph — nodes are pages, edges are links
-- somebody deliberately wrote — so nothing here ever joins `links`. The consequence D-4
-- states is what these two tables are for: a topic is reachable only through its own
-- listing, so the listing has to exist and has to be correct.
--
-- **A topic has no creation step.** D-2 of the owner's three decisions: anyone may type a
-- new one, there is no managed list, and nothing pre-registers a name. A row here exists
-- because a page carries it, and `gw_store::topics` deletes one again when the last page in
-- its subtree lets go of it. That is what makes "this topic exists" and "this topic has at
-- least one document" the same statement — which is the sentence the whole disclosure rule
-- in `docs/decisions/0011-what-a-topic-discloses.md` rests on.

CREATE TABLE tags (
    id         TEXT PRIMARY KEY,               -- uuid v7: sortable by creation time

    -- The canonical key: `/medizin/darm`. Every segment is `gw_core::slugify` of the name
    -- somebody typed, joined with `/` and carrying a leading one, exactly as
    -- `documents.path` is built. Two reasons it is stored rather than derived:
    --
    -- 1. **It is how two spellings become one topic.** `Medizin`, `medizin` and `MEDIZIN`
    --    all slugify to `medizin`, so the UNIQUE below is what makes "free text, but
    --    reused" true — the second person to type a topic gets the first person's topic
    --    rather than a near-duplicate nobody can tell apart in a list.
    -- 2. **It makes a subtree one indexed comparison.** Listing `Medizin` means listing
    --    `Medizin` and everything under it (see `Store::topic_documents` for why), which is
    --    the same prefix-on-a-segment-boundary question `documents` already answers this
    --    way. Walking `parent_id` instead would be a recursive query for the one operation
    --    this table exists to serve.
    --
    -- The cost, stated because it is real: RENAMING a topic would have to rewrite every
    -- descendant's path. Nothing renames one today — a topic is dropped by untagging the
    -- last page that carries it — and the day something does, this column is the work.
    path       TEXT NOT NULL UNIQUE,

    -- The last segment as somebody actually typed it: `Darm`, not `darm`. First writer
    -- wins, because a topic is one topic and a list showing it two ways is the duplication
    -- the slug above exists to prevent. What a reader sees is therefore the spelling of
    -- whoever got there first, which is stated in the ADR rather than left to be found.
    --
    -- A name may not contain `/` — that character is the nesting separator and nothing
    -- else, so `Vor/Nachteile` IS the topic `Nachteile` inside `Vor`. `gw_store::topics`
    -- refuses to build one that does, which is why there is no CHECK for it here: the
    -- constraint that matters is the one below, which no writer can talk its way past.
    name       TEXT NOT NULL,

    -- The tree (D-3 of the owner's three decisions). NULL for a top-level topic.
    --
    -- ON DELETE CASCADE: a topic inside a topic that no longer exists has no place to be.
    -- The pruning in `gw_store::topics` deletes leaves upward and never relies on this, but
    -- a schema in which `Medizin › Darm` can outlive `Medizin` is a schema that can hold a
    -- path whose prefix names nothing.
    parent_id  TEXT REFERENCES tags(id) ON DELETE CASCADE,

    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX tags_parent ON tags(parent_id);

-- **What stops `A` inside `B` inside `A`.**
--
-- The answer is not "the Rust code is careful". It is that `parent_id` is forced to agree
-- with `path`: a row's parent must be its own path with the last segment removed. A cycle
-- needs each of two paths to be a strict prefix of the other, and no two strings can be,
-- so a cycle is not merely refused — it is unrepresentable, the same way
-- `CHECK ((doc_id IS NULL) <> (project_id IS NULL))` in 0010 makes a task with no
-- governing page unrepresentable.
--
-- A trigger rather than a CHECK because the condition is about ANOTHER row, which a CHECK
-- cannot see. On INSERT and on UPDATE both: nothing in this crate updates `path` or
-- `parent_id` today, and the day something does — a rename, a move — this is the guard that
-- has to refuse the version of it that would build a loop, rather than a comment saying
-- nobody does that.
--
-- The three clauses of a valid parent, in order: its path is a proper prefix ending at a
-- `/`; there is at least one character after that `/`; and there is no second `/` in the
-- remainder, so a parent is exactly ONE segment up rather than any ancestor.
CREATE TRIGGER tags_parent_must_be_one_segment_up_insert
BEFORE INSERT ON tags
FOR EACH ROW WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(
        ABORT,
        'a topic''s parent must be its own path with the last segment removed'
    )
    WHERE NOT EXISTS (
        SELECT 1 FROM tags p
         WHERE p.id = NEW.parent_id
           AND substr(NEW.path, 1, length(p.path) + 1) = p.path || '/'
           AND length(NEW.path) > length(p.path) + 1
           AND instr(substr(NEW.path, length(p.path) + 2), '/') = 0
    );
END;

CREATE TRIGGER tags_parent_must_be_one_segment_up_update
BEFORE UPDATE OF path, parent_id ON tags
FOR EACH ROW WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(
        ABORT,
        'a topic''s parent must be its own path with the last segment removed'
    )
    WHERE NOT EXISTS (
        SELECT 1 FROM tags p
         WHERE p.id = NEW.parent_id
           AND substr(NEW.path, 1, length(p.path) + 1) = p.path || '/'
           AND length(NEW.path) > length(p.path) + 1
           AND instr(substr(NEW.path, length(p.path) + 2), '/') = 0
    );
END;

-- The other half of the same invariant: a top-level topic is exactly one segment. Without
-- it, `/medizin/darm` could be stored with no parent at all — no cycle, but a path whose
-- prefix names nothing, which breaks the subtree listing in the quiet direction.
CREATE TRIGGER tags_root_is_exactly_one_segment_insert
BEFORE INSERT ON tags
FOR EACH ROW WHEN NEW.parent_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'a top-level topic is exactly one segment')
    WHERE substr(NEW.path, 1, 1) <> '/'
       OR length(NEW.path) < 2
       OR instr(substr(NEW.path, 2), '/') <> 0;
END;

CREATE TRIGGER tags_root_is_exactly_one_segment_update
BEFORE UPDATE OF path, parent_id ON tags
FOR EACH ROW WHEN NEW.parent_id IS NULL
BEGIN
    SELECT RAISE(ABORT, 'a top-level topic is exactly one segment')
    WHERE substr(NEW.path, 1, 1) <> '/'
       OR length(NEW.path) < 2
       OR instr(substr(NEW.path, 2), '/') <> 0;
END;

-- Which page is about which topic. All key and nothing else, so WITHOUT ROWID for the
-- reason 0009 gives for `links`: the primary key IS the row, the table is its own index,
-- and there is nothing left for a rowid to point at. (0010's two tables are the opposite
-- shape and are deliberately not WITHOUT ROWID; the difference is explained there.)
--
-- Both sides CASCADE. A page's topics are facts about that page and outlive neither it nor
-- the topic: a row pointing at a purged page would put a document in a listing that nothing
-- could ever authorise, which is the one thing every aggregate view in this system must not
-- be able to produce.
CREATE TABLE document_tags (
    doc_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
    PRIMARY KEY (doc_id, tag_id)
) WITHOUT ROWID;

-- The listing query: every page filed under one topic.
CREATE INDEX document_tags_tag ON document_tags(tag_id);

-- **`projects.tag_id` deliberately does NOT gain a foreign key here, and 0010 asked for the
-- decision to be made once `tags` existed. It is made: no.**
--
-- Not because the constraint would be wrong — it is exactly the constraint one wants — but
-- because SQLite cannot add one to an existing table, and the supported way of faking it
-- would destroy data on this particular schema:
--
--   * `ALTER TABLE ... ADD CONSTRAINT` does not exist in SQLite at all.
--   * The documented twelve-step rebuild ends in `DROP TABLE projects`. With foreign keys
--     ON — which they are; `Store::open` sets `foreign_keys(true)` — DROP TABLE performs an
--     implicit DELETE of every row first, and `tasks.project_id REFERENCES projects(id) ON
--     DELETE CASCADE` fires on it. Every standalone card in the wiki would be deleted by a
--     migration whose stated purpose was to add a constraint.
--   * The rebuild cannot turn foreign keys off around itself either: `PRAGMA foreign_keys`
--     is a no-op inside a transaction, and sqlx runs each migration in one.
--   * And the rename-first variant is worse, silently. Since SQLite 3.25 `ALTER TABLE
--     RENAME` rewrites REFERENCES clauses in *other* tables to follow the new name, so
--     `projects` → `projects_old` would repoint `tasks.project_id` at `projects_old` and
--     leave the new table referenced by nothing at all.
--
-- What replaces the constraint is the same thing a foreign key would have bought, written
-- where it can be read: `Store::set_project_tag` and `Store::create_project` resolve a
-- topic PATH to an id and refuse one that names no topic, so a dangling `tag_id` cannot be
-- written through this crate; and `gw_store::topics`' pruning consults `projects` by hand
-- before deleting an empty topic, so a project's topic cannot be deleted out from under it.
-- That second one is the honest price of the missing constraint: a foreign key would have
-- enforced it without anybody having to remember, and instead there is a query that a
-- future prune has to keep making. It is mutation-tested for that reason.
--
-- Switch back if `projects` is ever rebuilt for another reason. At that moment the FK costs
-- one line in a table definition that is being written out anyway, and none of the above
-- applies.
