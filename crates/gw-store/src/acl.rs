use crate::{Store, StoredDocument, TreeNode};
use anyhow::Result;
use gw_auth::{can, Action, Grant, Permission, Principal, Subject};
use gw_core::Visibility;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::str::FromStr;

/// The default reach a principal has before any grant is consulted (D-M2-1).
///
/// Derived from the verified Authelia `groups` claim through the `group_roles` table, so
/// access follows the homelab account rather than being maintained twice. Ordered, so the
/// strongest baseline across a principal's groups is simply the maximum.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Serialize, Deserialize, Hash,
)]
#[serde(rename_all = "lowercase")]
pub enum Baseline {
    /// The fail-closed default: no groups, or no group with a row in `group_roles`.
    #[default]
    Public,
    /// Public plus `internal`. Still no `restricted` content without a grant.
    Internal,
    /// May read anything.
    Admin,
}

impl Baseline {
    pub fn as_str(self) -> &'static str {
        match self {
            Baseline::Public => "public",
            Baseline::Internal => "internal",
            Baseline::Admin => "admin",
        }
    }

    /// Read a stored value. An unrecognised one confers nothing beyond public — a value
    /// this code does not understand must never be guessed upward.
    fn from_stored(value: &str) -> Self {
        match value {
            "admin" => Baseline::Admin,
            "internal" => Baseline::Internal,
            _ => Baseline::Public,
        }
    }
}

#[derive(FromRow)]
struct GrantRow {
    subject_kind: String,
    subject_id: Option<String>,
    permission: String,
}

fn to_grant(row: GrantRow) -> Option<Grant> {
    let subject = match row.subject_kind.as_str() {
        "principal" => Subject::Principal(row.subject_id?),
        "team" => Subject::Team(row.subject_id?),
        "group" => Subject::Group(row.subject_id?),
        "anyone" => Subject::Anyone,
        "authenticated" => Subject::Authenticated,
        // An unrecognised subject kind confers nothing. Never guess.
        _ => return None,
    };
    let permission = match row.permission.as_str() {
        "read" => Permission::Read,
        "comment" => Permission::Comment,
        "write" => Permission::Write,
        "admin" => Permission::Admin,
        _ => return None,
    };
    Some(Grant {
        subject,
        permission,
    })
}

/// Every ancestor path of `path`, nearest first: '/a/b/c' -> ['/a/b/c', '/a/b', '/a'].
fn ancestors(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = path.to_string();
    while !current.is_empty() && current != "/" {
        out.push(current.clone());
        match current.rfind('/') {
            Some(0) | None => break,
            Some(i) => current.truncate(i),
        }
    }
    out
}

/// The effective decision: the permission engine, plus the D-M2-1 baseline.
///
/// `can()` remains the only thing that interprets a grant. This function calls it and
/// then applies exactly two adjustments, both of which exist because default reach now
/// follows the verified Authelia group rather than the mere fact of holding an account:
///
/// - it **narrows** `internal`, which `can()` would give to anyone signed in; and
/// - it **widens** `restricted` for the `admin` baseline, and for reads only.
///
/// Authentication is still checked before any group is looked at: `can()` runs first, and
/// `baseline_for` refuses to look up a baseline for a caller who is not signed in and
/// active. There is no path around that ordering.
fn permits(
    principal: &Principal,
    action: Action,
    visibility: Visibility,
    grants: &[Grant],
    baseline: Baseline,
) -> bool {
    // A grant decides on its own, at any visibility. Presenting the document at its
    // strictest level is how this crate asks the engine "is there a grant for this
    // caller?" without keeping a second copy of the subject-matching rules here: at
    // `Restricted`, `can()` consults only grants and `Anyone` share links.
    if can(principal, action, Visibility::Restricted, grants) {
        return true;
    }

    // No grant applies, so what remains is default reach. Comment, write and admin are
    // never conferred by default — only ever by a grant.
    if action != Action::Read {
        return false;
    }

    match visibility {
        // `can()` owns this rule, including the part that matters: a public read survives
        // deactivation, so suspending an account does not make the public site vanish.
        Visibility::Public => can(principal, action, visibility, grants),
        Visibility::Internal => baseline >= Baseline::Internal,
        Visibility::Restricted => baseline >= Baseline::Admin,
    }
}

impl Store {
    pub async fn add_grant(
        &self,
        path: &str,
        subject: Subject,
        permission: Permission,
    ) -> Result<()> {
        let (kind, id) = match &subject {
            Subject::Principal(i) => ("principal", Some(i.clone())),
            Subject::Team(i) => ("team", Some(i.clone())),
            Subject::Group(i) => ("group", Some(i.clone())),
            Subject::Anyone => ("anyone", None),
            Subject::Authenticated => ("authenticated", None),
        };
        let perm = match permission {
            Permission::Read => "read",
            Permission::Comment => "comment",
            Permission::Write => "write",
            Permission::Admin => "admin",
        };
        sqlx::query(
            "INSERT OR IGNORE INTO acl (id, path, subject_kind, subject_id, permission) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(path)
        .bind(kind)
        .bind(id)
        .bind(perm)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The grants that apply at `path`.
    ///
    /// The NEAREST ancestor that has any grants wins outright; grants are not unioned up
    /// the tree. Unioning would make it impossible to *narrow* access on a subtree — you
    /// could only ever widen it — and narrowing is the common case.
    pub async fn grants_for_path(&self, path: &str) -> Result<Vec<Grant>> {
        for candidate in ancestors(path) {
            let rows: Vec<GrantRow> = sqlx::query_as(
                "SELECT subject_kind, subject_id, permission FROM acl WHERE path = ?1",
            )
            .bind(&candidate)
            .fetch_all(&self.pool)
            .await?;
            if !rows.is_empty() {
                return Ok(rows.into_iter().filter_map(to_grant).collect());
            }
        }
        Ok(Vec::new())
    }

    /// The whole group-to-baseline mapping, for the admin console.
    pub async fn group_roles(&self) -> Result<Vec<(String, Baseline)>> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT group_name, baseline FROM group_roles ORDER BY group_name")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows
            .into_iter()
            .map(|(g, b)| (g, Baseline::from_stored(&b)))
            .collect())
    }

    /// Map an OIDC group to a baseline reach, or change the mapping of one.
    ///
    /// The point of the table is that this is a row and not a release: a new homelab
    /// group does not need a deploy to become meaningful here.
    pub async fn set_group_role(&self, group: &str, baseline: Baseline) -> Result<()> {
        sqlx::query(
            "INSERT INTO group_roles (group_name, baseline) VALUES (?1, ?2) \
             ON CONFLICT (group_name) DO UPDATE SET baseline = excluded.baseline",
        )
        .bind(group)
        .bind(baseline.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The default reach `principal` has before any grant is consulted (D-M2-1).
    ///
    /// The strongest baseline across the principal's groups wins. An unmapped group, or
    /// no groups at all — which is every local guest account, since great-wiki never
    /// writes Authelia's user database — confers `public` only.
    ///
    /// Authentication is checked BEFORE the groups are read, for the same reason `can()`
    /// checks it first: a `groups` list on a request that is not signed in is not
    /// evidence of anything, and a deactivated account's groups are stale by definition.
    pub async fn baseline_for(&self, principal: &Principal) -> Result<Baseline> {
        if !principal.is_authenticated() || !principal.active || principal.groups.is_empty() {
            return Ok(Baseline::Public);
        }

        // Only the `?` placeholders are built from the group COUNT; every group name is
        // bound. No value is ever formatted into SQL.
        let placeholders = vec!["?"; principal.groups.len()].join(",");
        let sql = format!("SELECT baseline FROM group_roles WHERE group_name IN ({placeholders})");
        let mut query = sqlx::query_as::<_, (String,)>(&sql);
        for group in &principal.groups {
            query = query.bind(group);
        }
        let rows: Vec<(String,)> = query.fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|(b,)| Baseline::from_stored(&b))
            .max()
            .unwrap_or_default())
    }

    /// Fetch a document only if `principal` may perform `action` on it.
    ///
    /// Returning `None` for both "absent" and "not permitted" is deliberate at this layer:
    /// the caller decides whether to reveal existence. The HTTP layer maps them differently.
    pub async fn document_for(
        &self,
        principal: &Principal,
        path: &str,
        action: Action,
    ) -> Result<Option<StoredDocument>> {
        let Some(doc) = self.document_by_path_unchecked(path).await? else {
            return Ok(None);
        };
        let visibility = Visibility::from_str(&doc.visibility).unwrap_or_default();
        let grants = self.grants_for_path(path).await?;
        let baseline = self.baseline_for(principal).await?;
        Ok(permits(principal, action, visibility, &grants, baseline).then_some(doc))
    }

    /// The navigation tree, filtered to what `principal` may read.
    ///
    /// This REPLACES `Store::tree()`, which M1 left unfiltered. A restricted title in the
    /// navigation is a disclosure even when the body is protected.
    pub async fn tree_for(&self, principal: &Principal) -> Result<Vec<TreeNode>> {
        let all = self.tree().await?;
        // Resolved once for the whole walk: the baseline is a property of the caller, not
        // of the node, and re-querying it per node would only invite it to drift.
        let baseline = self.baseline_for(principal).await?;
        self.filter_tree(all, principal, baseline).await
    }

    async fn filter_tree(
        &self,
        nodes: Vec<TreeNode>,
        principal: &Principal,
        baseline: Baseline,
    ) -> Result<Vec<TreeNode>> {
        let mut out = Vec::new();
        for mut node in nodes {
            let visibility = Visibility::from_str(&node.visibility).unwrap_or_default();
            let grants = self.grants_for_path(&node.path).await?;
            if !permits(principal, Action::Read, visibility, &grants, baseline) {
                // Skipping the whole branch is correct: a child cannot be more visible
                // than a parent the caller cannot see, or the tree would leak structure.
                continue;
            }
            let children = std::mem::take(&mut node.children);
            node.children = Box::pin(self.filter_tree(children, principal, baseline)).await?;
            out.push(node);
        }
        Ok(out)
    }
}

impl Store {
    /// The grants that apply at `path`, together with the path they are defined on.
    ///
    /// The access-first console has to answer "why does this person reach this page?",
    /// and "because /rundgang says so" is a different answer from "because this page
    /// says so" — the first is changed somewhere else, and an administrator who edits
    /// the wrong one has widened access to a whole subtree by accident.
    ///
    /// `None` for the source means no ancestor carries any grant, so reach here is
    /// whatever the baseline confers and nothing more.
    pub async fn effective_grants(&self, path: &str) -> Result<(Option<String>, Vec<Grant>)> {
        for candidate in ancestors(path) {
            let rows: Vec<GrantRow> = sqlx::query_as(
                "SELECT subject_kind, subject_id, permission FROM acl WHERE path = ?1",
            )
            .bind(&candidate)
            .fetch_all(&self.pool)
            .await?;
            if !rows.is_empty() {
                let grants = rows.into_iter().filter_map(to_grant).collect();
                return Ok((Some(candidate), grants));
            }
        }
        Ok((None, Vec::new()))
    }

    /// The grants written on `path` itself, inherited ones excluded.
    ///
    /// This is what a revoke operates on: you cannot remove an inherited grant from
    /// here, and offering a control that appears to would be a lie about what happened.
    pub async fn grants_defined_at(&self, path: &str) -> Result<Vec<Grant>> {
        let rows: Vec<GrantRow> =
            sqlx::query_as("SELECT subject_kind, subject_id, permission FROM acl WHERE path = ?1")
                .bind(path)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().filter_map(to_grant).collect())
    }

    /// Remove one grant defined at `path`. Returns whether a row was actually removed.
    ///
    /// The boolean is not decoration. A revoke that silently matched nothing — because
    /// the grant was inherited, or already gone — must not report success to someone who
    /// is about to conclude that access has been withdrawn.
    pub async fn remove_grant(
        &self,
        path: &str,
        subject: &Subject,
        permission: Permission,
    ) -> Result<bool> {
        let (kind, id) = subject_columns(subject);
        let result = sqlx::query(
            "DELETE FROM acl WHERE path = ?1 AND subject_kind = ?2 \
             AND subject_id IS ?3 AND permission = ?4",
        )
        .bind(path)
        .bind(kind)
        .bind(id)
        .bind(permission_column(permission))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Every path that carries at least one grant, with how many. The console's index.
    pub async fn paths_with_grants(&self) -> Result<Vec<(String, i64)>> {
        Ok(
            sqlx::query_as("SELECT path, COUNT(*) FROM acl GROUP BY path ORDER BY path")
                .fetch_all(&self.pool)
                .await?,
        )
    }
}

/// The stored spelling of a subject. One definition, so an insert and a delete cannot
/// disagree about how `Anyone` is written and leave a grant that can never be revoked.
fn subject_columns(subject: &Subject) -> (&'static str, Option<String>) {
    match subject {
        Subject::Principal(i) => ("principal", Some(i.clone())),
        Subject::Team(i) => ("team", Some(i.clone())),
        Subject::Group(i) => ("group", Some(i.clone())),
        Subject::Anyone => ("anyone", None),
        Subject::Authenticated => ("authenticated", None),
    }
}

fn permission_column(permission: Permission) -> &'static str {
    match permission {
        Permission::Read => "read",
        Permission::Comment => "comment",
        Permission::Write => "write",
        Permission::Admin => "admin",
    }
}

#[cfg(test)]
mod tests {
    use super::{ancestors, Baseline};

    #[test]
    fn ancestors_are_nearest_first_and_stop_below_the_root() {
        assert_eq!(ancestors("/a/b/c"), vec!["/a/b/c", "/a/b", "/a"]);
        assert_eq!(ancestors("/a"), vec!["/a"]);
        assert!(ancestors("/").is_empty());
        assert!(ancestors("").is_empty());
    }

    #[test]
    fn an_unrecognised_stored_baseline_falls_back_to_public() {
        assert_eq!(Baseline::from_stored("superadmin"), Baseline::Public);
        assert_eq!(Baseline::from_stored(""), Baseline::Public);
        assert_eq!(Baseline::from_stored("admin"), Baseline::Admin);
    }

    #[test]
    fn baselines_are_ordered_so_the_strongest_group_wins() {
        assert!(Baseline::Admin > Baseline::Internal);
        assert!(Baseline::Internal > Baseline::Public);
        assert_eq!(Baseline::default(), Baseline::Public);
    }
}
