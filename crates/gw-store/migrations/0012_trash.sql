-- The Papierkorb (D-14): a page leaves the tree, keeps its ACL, and can be put back.
--
-- `documents.deleted_at` has been here since `0001_init.sql`, with the comment "soft delete;
-- M3 adds the trash UI" beside it. Nothing ever wrote it. This migration adds the three
-- columns that turn a nullable timestamp into an operation somebody can perform, undo and
-- — deliberately, as a second act — make permanent.
--
-- **Why the timestamp alone was not enough.** Trashing a page takes its subtree with it
-- (`gw_store::trash` says why: a page in the trash whose children are not is a tree with a
-- hole in it, and the hole is silent — `Store::tree` builds a child list by matching
-- `parent_path` against a parent it has already emitted, so an orphaned child vanishes from
-- the navigation AND from the markdown export while staying readable at its own URL). With
-- only `deleted_at`, restoring `/handbuch` would have to restore every trashed page beneath
-- it — including one somebody had deliberately thrown away last week. `deleted_root` is what
-- makes "put back exactly what went down with it" a query instead of a guess.

-- The page whose deletion put this row in the trash, BY ID.
--
-- Self-rooted (`deleted_root = id`) means "somebody deleted this page"; anything else means
-- "this went down with an ancestor". The trash listing is therefore the self-rooted rows,
-- and a restore is `WHERE deleted_root = ?` — one act in, one act out.
--
-- **An id and not a path**, for the reason `0010_tasks.sql` gives for `projects.home_doc`:
-- a path is the one column of `documents` that is meant to change, and a trash entry that
-- forgot its members because somebody renamed something would be unrestorable.
--
-- **NO foreign key, and that is a decision rather than an omission.** `REFERENCES
-- documents(id) ON DELETE CASCADE` is the obvious spelling and it would quietly break the
-- one thing a purge exists to do. A purge deletes a whole subtree in ONE statement and
-- reports what it destroyed from that statement's own `RETURNING` clause (ADR 0012). Add the
-- cascade and deleting the root row silently deletes its members before the outer DELETE
-- reaches them, so the statement returns fewer rows than it destroyed — a purge that
-- under-reports itself, which is the exact failure the report exists to prevent. NO ACTION
-- (SQLite's default) is worse still: it would abort the DELETE mid-statement. What replaces
-- the constraint is that nothing can produce a dangling value — a purge always takes the
-- whole subtree at a path, and a row's `deleted_root` always names itself or an ancestor —
-- and `purging_a_trashed_parent_takes_a_separately_trashed_child_with_it` is what holds it up.
ALTER TABLE documents ADD COLUMN deleted_root TEXT;

-- Who threw it away, and what they were called at the time.
--
-- Two columns for the reason `revisions` carries `author_id` and `author_name`: the id is
-- what code joins on, the name is a SNAPSHOT so the Papierkorb still says who emptied a
-- shelf after they have left. Neither carries a foreign key, for the same reason
-- `revisions.author_id` carries none — D-M3-4 says offboarding removes access and nothing
-- else, and a record that blocks an account's removal is a record that will be deleted by
-- hand instead.
--
-- Always present while a row is in the trash: `gw_store::trash` refuses to trash a page for
-- a caller who is not a signed-in, active account, exactly as `append_revision` refuses to
-- file one for `Author::Import`'s HTTP equivalent. A page on a path carrying `anyone: write`
-- is editable by somebody who has not said who they are (see `DocumentAccess::may_write`);
-- emptying the wiki into a trash that cannot say who did it is not the same act as editing a
-- paragraph.
ALTER TABLE documents ADD COLUMN deleted_by TEXT;
ALTER TABLE documents ADD COLUMN deleted_by_name TEXT;

-- The listing ("which acts are in the trash") and the restore ("which rows went down with
-- this one") are the same lookup from different directions, so one partial index serves
-- both. Partial because the interesting rows are a handful and the live ones are all of
-- them — the same shape as `documents_visible` in 0001, pointed the other way.
CREATE INDEX documents_trashed ON documents (deleted_root) WHERE deleted_at IS NOT NULL;

-- **The four columns are one fact, so they are written together or not at all.**
--
-- A row with `deleted_at` set and `deleted_root` NULL is invisible in the tree and belongs to
-- no trash entry: unreachable, unrestorable, and unnoticeable — the state this schema most
-- has to be unable to hold. A row with `deleted_root` set and `deleted_at` NULL is the
-- mirror: live, in the tree, and claimed by an entry that would restore it again.
--
-- A trigger rather than a CHECK because SQLite cannot add a CHECK to an existing table, and
-- the twelve-step rebuild that fakes one ends in `DROP TABLE documents` — which, with foreign
-- keys on (`Store::open` sets them), performs an implicit DELETE first and cascades away every
-- revision, link, task and topic filing in the wiki. `0011_tags.sql` works the same argument
-- through for `projects`; here the stakes are the entire corpus.
--
-- On INSERT and on UPDATE both. `BEFORE UPDATE OF ...` fires only when one of the named
-- columns is in the SET list, so an ordinary publish — which writes `body`,
-- `current_revision_id` and `updated_at` — pays nothing for this.
CREATE TRIGGER documents_trash_columns_agree_insert
BEFORE INSERT ON documents
FOR EACH ROW
BEGIN
    SELECT RAISE(
        ABORT,
        'deleted_at, deleted_root, deleted_by and deleted_by_name are one fact: set all four or none'
    )
    WHERE (NEW.deleted_at IS NULL) <> (NEW.deleted_root IS NULL)
       OR (NEW.deleted_at IS NULL) <> (NEW.deleted_by IS NULL)
       OR (NEW.deleted_at IS NULL) <> (NEW.deleted_by_name IS NULL);
END;

CREATE TRIGGER documents_trash_columns_agree_update
BEFORE UPDATE OF deleted_at, deleted_root, deleted_by, deleted_by_name ON documents
FOR EACH ROW
BEGIN
    SELECT RAISE(
        ABORT,
        'deleted_at, deleted_root, deleted_by and deleted_by_name are one fact: set all four or none'
    )
    WHERE (NEW.deleted_at IS NULL) <> (NEW.deleted_root IS NULL)
       OR (NEW.deleted_at IS NULL) <> (NEW.deleted_by IS NULL)
       OR (NEW.deleted_at IS NULL) <> (NEW.deleted_by_name IS NULL);
END;
