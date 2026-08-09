-- Give the audit log a scope, so it can be read by the people it concerns.
--
-- D-M2-6: the audit log is readable by instance admins, AND by anyone holding `admin`
-- on a subtree — for the entries concerning that subtree only. Someone who administers
-- a space needs to see who was granted access to it; they must not thereby see who was
-- granted access to anything else, or which accounts exist elsewhere.
--
-- That requires knowing what each entry is ABOUT, and `target` cannot answer it: for
-- `team.create` the target is a team name, for `acl.grant` it is a path, for
-- `principal.deactivate` it is a principal id. Deriving a scope by guessing at the
-- shape of that string would be a security decision made by a heuristic.
--
-- So scope is explicit and separate:
--
--   path IS NULL  -> the entry is INSTANCE-WIDE. Creating an account, creating a team,
--                    changing a group mapping: none of these belong to a subtree, and
--                    all of them are readable by instance admins only.
--   path = '...'  -> the entry concerns that document path and everything under it.
--
-- NULL is therefore the closed default: an entry recorded without a stated scope is
-- visible to fewer people, never more. A future action that forgets to set a path leaks
-- nothing; it merely becomes invisible to space admins, which is a bug you notice by its
-- absence rather than a disclosure you notice too late.
ALTER TABLE audit_log ADD COLUMN path TEXT;

-- The scoped read is "entries at or under this prefix, newest first". Without this,
-- every space admin's console does a full scan of a table that only ever grows.
CREATE INDEX audit_path_at ON audit_log (path, at DESC);
