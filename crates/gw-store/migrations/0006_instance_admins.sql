-- The per-account instance-admin promotion, and the reason it is a table.
--
-- D-M2-1 makes Authelia's `admins` group the source of truth for real system
-- administrators, and ADR 0002 forbids great-wiki from ever writing Authelia's user
-- database. Between those two, an instance whose last Authelia administrator has left has
-- no in-app way back: nobody here can add themselves to a group that lives somewhere else.
--
-- This table is the FALLBACK for exactly that, and nothing more. A row promotes the one
-- principal it names. It is deliberately NOT another `group_roles` mapping: a group row
-- promotes everybody who currently holds that group and everybody who is added to it
-- later, in Authelia, where great-wiki cannot see the change happen. Space-level access
-- stays with teams and path grants; this is instance administration only.
--
-- A TABLE rather than a column on `principals`:
--
--   * Presence is the whole rule, and ABSENCE is the fail-closed default — the same shape
--     `group_roles` uses. A column has a value on every row, so a migration default, a
--     bad UPDATE or a `CHECK` nobody wrote can hand out administration to everyone at
--     once. A row can only ever be wrong about one person.
--   * It carries provenance. `granted_by` and `granted_at` answer "who did this, and
--     when" from the row itself; a boolean column answers neither, and the audit log,
--     which does record it, is a log — it can be read past, and it does not tell you the
--     CURRENT set at a glance.
--   * `principals` is re-read on every single request (D-M2-7). Nothing is gained by
--     widening that row with a column that matters on a handful of administrative paths.
--
-- `granted_by` carries no foreign key, for the same reason `audit_log.principal_id`
-- carries none: it records who acted, and that record must not disappear or block a
-- deletion when the account that acted is removed. `principal_id` does have one, with
-- ON DELETE CASCADE — a promotion that outlived its principal would be a row conferring
-- administration on an id nobody can sign in as.
CREATE TABLE instance_admins (
    principal_id TEXT PRIMARY KEY REFERENCES principals(id) ON DELETE CASCADE,
    granted_by   TEXT,
    granted_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
