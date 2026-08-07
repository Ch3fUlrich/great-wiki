CREATE TABLE principals (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('oidc', 'local')),
    username      TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    email         TEXT,
    -- OIDC groups, refreshed from the verified claim on every login. NULL for local.
    groups        TEXT NOT NULL DEFAULT '[]',
    active        INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    last_seen_at  TEXT
);

-- Separate table so an OIDC principal has no credential row at all, rather than a NULL
-- column that a bug could compare against.
CREATE TABLE credentials (
    principal_id  TEXT PRIMARY KEY REFERENCES principals(id) ON DELETE CASCADE,
    password_hash TEXT NOT NULL,
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE teams (
    id          TEXT PRIMARY KEY,
    slug        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE team_members (
    team_id      TEXT NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    principal_id TEXT NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, principal_id)
);

-- A grant on a path. Inheritance is by prefix: a grant on '/handbook' applies to
-- '/handbook/onboarding' unless that path has its own grants.
CREATE TABLE acl (
    id           TEXT PRIMARY KEY,
    path         TEXT NOT NULL,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('principal','team','group','anyone','authenticated')),
    subject_id   TEXT,
    permission   TEXT NOT NULL CHECK (permission IN ('read','comment','write','admin')),
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (path, subject_kind, subject_id, permission)
);

CREATE INDEX acl_path ON acl (path);

CREATE TABLE audit_log (
    id           TEXT PRIMARY KEY,
    at           TEXT NOT NULL DEFAULT (datetime('now')),
    principal_id TEXT,
    action       TEXT NOT NULL,
    target       TEXT,
    detail       TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX audit_at ON audit_log (at DESC);

-- D-M2-1: default reach follows the verified Authelia group.
--
-- This is a TABLE and not a match arm on purpose. Adding a homelab group must be a row,
-- not a release, and the mapping has to be inspectable in the admin console alongside
-- everything else that decides who sees what.
--
-- A group with no row here — and a principal with no groups at all, which is every local
-- guest account — confers `public` only. That is the fail-closed default and it is
-- expressed by the ABSENCE of a row, so forgetting to configure a group can never widen
-- access. A `public` row is therefore redundant but permitted, so an administrator can
-- record "this group deliberately gets nothing extra" rather than leaving it ambiguous.
CREATE TABLE group_roles (
    group_name  TEXT PRIMARY KEY,
    baseline    TEXT NOT NULL CHECK (baseline IN ('public', 'internal', 'admin')),
    created_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO group_roles (group_name, baseline) VALUES
    ('admins', 'admin'),
    ('users',  'internal');
