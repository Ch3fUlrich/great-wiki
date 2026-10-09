-- The access epoch (ADR 0027): a counter that moves whenever the answer to "may this
-- principal do that to this page?" may have changed.
--
-- The triggers are the point. A bump written into each store method is a bump somebody
-- forgets when the next writer is added, and the failure is silent: an open editing socket
-- keeps working for a person who lost the page. A trigger cannot be forgotten, is atomic
-- with the change it announces, and is narrower than "any write to the table" where it can
-- be — a login refreshing an unchanged group list, or a page being published, moves nothing.
--
-- The counter row is only ever written by these triggers. The server does not READ it: it
-- listens to the write (an update hook, published at commit) and keeps the number in memory.
CREATE TABLE access_epoch (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    n  INTEGER NOT NULL
);
INSERT INTO access_epoch (id, n) VALUES (1, 0);

-- Grants.
CREATE TRIGGER access_epoch_acl_insert AFTER INSERT ON acl
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_acl_update AFTER UPDATE ON acl
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_acl_delete AFTER DELETE ON acl
BEGIN UPDATE access_epoch SET n = n + 1; END;

-- Accounts: deactivation, a changed OIDC group list, removal.
CREATE TRIGGER access_epoch_principal_update AFTER UPDATE ON principals
WHEN OLD.active IS NOT NEW.active OR OLD.groups IS NOT NEW.groups
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_principal_delete AFTER DELETE ON principals
BEGIN UPDATE access_epoch SET n = n + 1; END;

-- Teams, group baselines and instance admins.
CREATE TRIGGER access_epoch_team_member_insert AFTER INSERT ON team_members
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_team_member_delete AFTER DELETE ON team_members
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_group_role_insert AFTER INSERT ON group_roles
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_group_role_update AFTER UPDATE ON group_roles
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_group_role_delete AFTER DELETE ON group_roles
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_instance_admin_insert AFTER INSERT ON instance_admins
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_instance_admin_delete AFTER DELETE ON instance_admins
BEGIN UPDATE access_epoch SET n = n + 1; END;

-- A session ending (logout, deactivation, expiry sweep). Creating one grants nothing to a
-- socket that is already open.
CREATE TRIGGER access_epoch_session_delete AFTER DELETE ON sessions
BEGIN UPDATE access_epoch SET n = n + 1; END;

-- A page moving, changing visibility, entering or leaving the trash, or going.
CREATE TRIGGER access_epoch_document_update AFTER UPDATE ON documents
WHEN OLD.path IS NOT NEW.path
  OR OLD.parent_path IS NOT NEW.parent_path
  OR OLD.visibility IS NOT NEW.visibility
  OR OLD.deleted_at IS NOT NEW.deleted_at
BEGIN UPDATE access_epoch SET n = n + 1; END;
CREATE TRIGGER access_epoch_document_delete AFTER DELETE ON documents
BEGIN UPDATE access_epoch SET n = n + 1; END;
