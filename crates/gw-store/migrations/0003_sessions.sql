-- Browser sessions.
--
-- In SQLite rather than in memory. Attribution is the reason: a revision or a comment is
-- signed by the principal a session resolved to, so a restart that forgot every session
-- would silently reattribute in-flight work to nobody. It also means one process is not
-- quietly a requirement of the design.
--
-- Only the SHA-256 of the token is stored, never the token. The plaintext exists in
-- exactly one place — the browser's cookie — so reading every row of this table hands
-- over no working session. The lookup hashes what it was given and compares hashes, which
-- is why the column is the primary key: an index on a value nobody can reverse.
CREATE TABLE sessions (
    token_hash   TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    -- Enforced on every lookup, not merely recorded. A row that has passed this instant
    -- resolves to nobody even though it is still present.
    expires_at   TEXT NOT NULL
);

-- Deactivating a principal deletes its sessions in the same transaction (D-M2-7); this
-- index is what makes that a lookup rather than a scan.
CREATE INDEX sessions_principal ON sessions (principal_id);
CREATE INDEX sessions_expiry ON sessions (expires_at);
