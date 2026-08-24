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

/// Whether `principal_id` carries a per-account promotion row (0006).
async fn promoted(conn: &mut sqlx::SqliteConnection, principal_id: &str) -> Result<bool> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM instance_admins WHERE principal_id = ?1")
            .bind(principal_id)
            .fetch_optional(conn)
            .await?;
    Ok(row.is_some())
}

/// The baseline of a caller already known to be signed in and active, on ONE connection.
///
/// This is the single implementation of D-M2-1 and of the 0006 fallback. It takes a
/// connection rather than the pool so that the interlock can ask the same question inside
/// the transaction that is about to deactivate somebody: a second copy of this rule —
/// written in SQL against `group_roles`, say, because that counts in one statement — is
/// exactly how the count and the authorisation decision come to disagree, and the shape of
/// that disagreement is "the console says there are two administrators and there are none".
///
/// The activity and authentication checks live in [`Store::baseline_for`] and in the
/// counter, because each knows a different way to establish them: one has a `Principal`,
/// the other a database row.
pub(crate) async fn baseline_on(
    conn: &mut sqlx::SqliteConnection,
    principal_id: &str,
    groups: &[String],
) -> Result<Baseline> {
    // The fallback, first: it names one principal, so it can neither be widened by a
    // group nor narrowed by one.
    if promoted(&mut *conn, principal_id).await? {
        return Ok(Baseline::Admin);
    }
    if groups.is_empty() {
        return Ok(Baseline::Public);
    }

    // Only the `?` placeholders are built from the group COUNT; every group name is
    // bound. No value is ever formatted into SQL.
    let placeholders = vec!["?"; groups.len()].join(",");
    let sql = format!("SELECT baseline FROM group_roles WHERE group_name IN ({placeholders})");
    let mut query = sqlx::query_as::<_, (String,)>(&sql);
    for group in groups {
        query = query.bind(group);
    }
    let rows: Vec<(String,)> = query.fetch_all(conn).await?;

    Ok(rows
        .into_iter()
        .map(|(b,)| Baseline::from_stored(&b))
        .max()
        .unwrap_or_default())
}

/// How many ACTIVE principals hold the admin baseline, on ONE connection.
///
/// Active, because the floor exists to guarantee somebody can still sign in and
/// administer: a deactivated account has no sessions (D-M2-7) and cannot get one, so
/// counting it would let the last usable administrator suspend themselves next to a row
/// that merely looks like a second.
///
/// Written as a query for the candidates and a loop for the verdict, rather than as one
/// clever statement: the group half of the rule lives in a JSON column, and expressing it
/// in SQL would mean a second implementation of [`baseline_on`] that nothing keeps honest.
/// The loop is one indexed lookup per active account, on a path taken when somebody
/// deactivates an account — never per request.
pub(crate) async fn active_instance_admins(conn: &mut sqlx::SqliteConnection) -> Result<i64> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, groups FROM principals WHERE active = 1")
            .fetch_all(&mut *conn)
            .await?;

    let mut count = 0;
    for (id, groups) in rows {
        // An unreadable `groups` value confers nothing, exactly as an unrecognised
        // baseline string does. It must never be guessed upward into an administrator.
        let groups: Vec<String> = serde_json::from_str(&groups).unwrap_or_default();
        if baseline_on(&mut *conn, &id, &groups).await? >= Baseline::Admin {
            count += 1;
        }
    }
    Ok(count)
}

impl Store {
    pub async fn add_grant(
        &self,
        path: &str,
        subject: Subject,
        permission: Permission,
    ) -> Result<()> {
        let (kind, id) = subject_columns(&subject);
        let perm = permission_column(permission);
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
    /// writes Authelia's user database — confers `public` only. A per-account promotion
    /// (0006) confers `admin` on its own, whatever the groups say.
    ///
    /// Authentication is checked BEFORE anything is read, for the same reason `can()`
    /// checks it first: a `groups` list on a request that is not signed in is not
    /// evidence of anything, and a deactivated account's groups are stale by definition.
    pub async fn baseline_for(&self, principal: &Principal) -> Result<Baseline> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(Baseline::Public);
        }
        let mut conn = self.pool.acquire().await?;
        baseline_on(&mut conn, &principal.id, &principal.groups).await
    }

    /// Whether `principal_id` carries the per-account promotion (0006).
    ///
    /// The stored fact only. Whether it takes effect is [`Store::baseline_for`]'s answer,
    /// which also refuses a deactivated account — so this must not be used as a gate.
    pub async fn is_instance_admin(&self, principal_id: &str) -> Result<bool> {
        let mut conn = self.pool.acquire().await?;
        promoted(&mut conn, principal_id).await
    }

    /// How many people can administer this instance right now.
    ///
    /// The number the interlock protects, for a console that wants to say "you are the
    /// last one" before the refusal does. The refusal itself counts inside its own
    /// transaction — see [`crate::admin`] — because a number read here is already stale
    /// by the time anything acts on it.
    pub async fn active_instance_admin_count(&self) -> Result<i64> {
        let mut conn = self.pool.acquire().await?;
        active_instance_admins(&mut conn).await
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
        let baseline = self.baseline_for(principal).await?;
        self.document_for_with_baseline(principal, path, action, baseline)
            .await
    }

    /// Fetch a document by its **id**, only if `principal` may perform `action` on it.
    ///
    /// [`Store::document_for`] reached by an id, and deliberately nothing more: the id is
    /// resolved to a path and the path is put through the same accessor, so there is one
    /// authorisation answer in this crate and two ways in rather than two answers. Deciding
    /// it here would be the second one, and the second one is always the one that gets it
    /// wrong — it is the one nobody remembers when the rules change.
    ///
    /// **Why an id-keyed accessor exists at all.** Aggregate views hold ids, not paths: a
    /// board card knows its `doc_id`, a revision knows its `document_id`, a link knows both
    /// ends by id. Without this they had two options and both were bad — make the caller
    /// supply a path and hope it names the same document, or resolve the page unchecked on
    /// the grounds that the row was already filtered somewhere else. The second is the one
    /// that rots: it is true today and it makes the safety of one query a property of a
    /// different query, maintained in another module.
    ///
    /// Returning `None` for "no such document" and for "not permitted" alike is the same
    /// closed conflation [`Store::document_for`] makes, and for the same reason: this layer
    /// does not decide whether existence may be revealed.
    ///
    /// The round trip cannot land on a different document: `documents.path` is UNIQUE
    /// across every row including soft-deleted ones. A page in the trash is refused here
    /// exactly as it is there, because [`Store::document_by_path_unchecked`] will not
    /// resolve it.
    pub async fn document_for_id(
        &self,
        principal: &Principal,
        document_id: &str,
        action: Action,
    ) -> Result<Option<StoredDocument>> {
        let baseline = self.baseline_for(principal).await?;
        self.document_for_id_with_baseline(principal, document_id, action, baseline)
            .await
    }

    /// [`Store::document_for_id`], with the caller's baseline already resolved.
    ///
    /// The same hoist [`Store::document_for_with_baseline`] exists for, for the same
    /// reason and with the same warning attached: the baseline is a property of the caller,
    /// so an aggregate view asking about forty documents resolves it once — and a baseline
    /// belonging to somebody else is a hole that reads like an optimisation. `pub(crate)`
    /// deliberately, so it cannot be handed one from outside.
    pub(crate) async fn document_for_id_with_baseline(
        &self,
        principal: &Principal,
        document_id: &str,
        action: Action,
        baseline: Baseline,
    ) -> Result<Option<StoredDocument>> {
        let Some(path) = self.document_path_unchecked(document_id).await? else {
            return Ok(None);
        };
        self.document_for_with_baseline(principal, &path, action, baseline)
            .await
    }

    /// [`Store::document_for`], with the caller's baseline already resolved.
    ///
    /// The whole of `document_for` lives here, and `document_for` is one line calling it —
    /// so this is not a second answer to the authorisation question, it is the *only* one,
    /// reached two ways. What differs is who pays for the baseline: a caller asking about
    /// ONE document has no reason to care, while an aggregate view over the whole corpus
    /// does, because the baseline is a property of the caller and not of the document. It is
    /// the same hoist [`Store::tree_for`] performs on its walk, for the same reason and with
    /// the same consequence: re-querying it per document costs a round trip each and only
    /// invites the answer to drift within a single response.
    ///
    /// `pub(crate)` deliberately. Outside this crate the accessor is `document_for`, which
    /// cannot be handed a baseline belonging to somebody else.
    pub(crate) async fn document_for_with_baseline(
        &self,
        principal: &Principal,
        path: &str,
        action: Action,
        baseline: Baseline,
    ) -> Result<Option<StoredDocument>> {
        let Some(doc) = self.document_by_path_unchecked(path).await? else {
            return Ok(None);
        };
        let visibility = Visibility::from_str(&doc.visibility).unwrap_or_default();
        let grants = self.grants_for_path(path).await?;
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
        self.nearest_grants(ancestors(path)).await
    }

    /// The grants that would apply at `path` if `path` carried none of its own.
    ///
    /// The console needs this to describe a revoke honestly. Removing the LAST row on a
    /// path is not a local change: the nearest ancestor with any rows wins outright, so
    /// once this path has none, the ancestor's set applies here — and across every page
    /// below that carries nothing of its own. [`Store::effective_grants`] cannot answer
    /// it, because a path is its own first ancestor and therefore names ITSELF the moment
    /// it holds a single row.
    ///
    /// It never subtracts: a grant only ever widens, so what comes back is what somebody
    /// would REGAIN, never what they would lose.
    pub async fn grants_above(&self, path: &str) -> Result<(Option<String>, Vec<Grant>)> {
        // `skip(1)`, not `parent path then ancestors`: `ancestors` already stops below the
        // root and truncates at segment boundaries, and re-deriving the parent here would
        // be a second copy of that rule for `/darm` versus `/darmflora` to disagree over.
        self.nearest_grants(ancestors(path).into_iter().skip(1).collect())
            .await
    }

    /// The rows of the first candidate that has any. One walk, so `effective_grants` and
    /// `grants_above` cannot come to disagree about what "nearest" means.
    async fn nearest_grants(
        &self,
        candidates: Vec<String>,
    ) -> Result<(Option<String>, Vec<Grant>)> {
        for candidate in candidates {
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
pub(crate) fn subject_columns(subject: &Subject) -> (&'static str, Option<String>) {
    match subject {
        Subject::Principal(i) => ("principal", Some(i.clone())),
        Subject::Team(i) => ("team", Some(i.clone())),
        Subject::Group(i) => ("group", Some(i.clone())),
        Subject::Anyone => ("anyone", None),
        Subject::Authenticated => ("authenticated", None),
    }
}

pub(crate) fn permission_column(permission: Permission) -> &'static str {
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
    use crate::Store;
    use gw_auth::{Grant, Permission, Subject};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    #[test]
    fn ancestors_are_nearest_first_and_stop_below_the_root() {
        assert_eq!(ancestors("/a/b/c"), vec!["/a/b/c", "/a/b", "/a"]);
        assert_eq!(ancestors("/a"), vec!["/a"]);
        assert!(ancestors("/").is_empty());
        assert!(ancestors("").is_empty());
    }

    #[tokio::test]
    async fn what_would_apply_here_without_this_path_s_own_grants() {
        // The fact the revoke dialog is built on. Removing the LAST row on a path is not
        // a local change: the nearest ancestor with any rows wins outright, so the page
        // — and everything under it that carries nothing — falls back to that ancestor.
        // `effective_grants` cannot say so, because a path is its own first ancestor.
        let store = store().await;
        store
            .add_grant("/a", Subject::Team("oben".into()), Permission::Read)
            .await
            .unwrap();
        store
            .add_grant("/a/b", Subject::Team("hier".into()), Permission::Write)
            .await
            .unwrap();

        let (source, grants) = store.effective_grants("/a/b").await.unwrap();
        assert_eq!(
            source.as_deref(),
            Some("/a/b"),
            "a path is its own ancestor"
        );
        assert_eq!(grants.len(), 1);

        let (above, resuming) = store.grants_above("/a/b").await.unwrap();
        assert_eq!(above.as_deref(), Some("/a"));
        assert_eq!(
            resuming,
            vec![Grant {
                subject: Subject::Team("oben".into()),
                permission: Permission::Read
            }]
        );

        // Nothing above a top-level path, and it says so rather than naming itself.
        assert_eq!(store.grants_above("/a").await.unwrap(), (None, Vec::new()));

        // Segment-based, exactly as `ancestors` is: `/ab` is not under `/a`.
        assert_eq!(
            store.grants_above("/ab/c").await.unwrap(),
            (None, Vec::new())
        );
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

    // --- the per-account promotion (0006) ---------------------------------------------

    #[tokio::test]
    async fn a_promoted_account_holds_the_admin_baseline_with_no_groups_at_all() {
        // The fallback D-M2-1 cannot provide. A local account has no Authelia groups and
        // never will — great-wiki does not write that database — so if this were derived
        // from groups there would be no way back into an instance whose administrators
        // have left.
        let store = store().await;
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        assert!(gast.groups.is_empty(), "the fixture must have no groups");
        assert_eq!(store.baseline_for(&gast).await.unwrap(), Baseline::Public);

        assert!(store.set_instance_admin(&gast.id, true).await.unwrap());
        assert_eq!(store.baseline_for(&gast).await.unwrap(), Baseline::Admin);

        // And it is exactly reversible.
        assert!(store.set_instance_admin(&gast.id, false).await.unwrap());
        assert_eq!(store.baseline_for(&gast).await.unwrap(), Baseline::Public);
    }

    #[tokio::test]
    async fn promoting_one_account_changes_nobody_else_s_baseline() {
        // The owner's constraint: the promotion names one person. A `group_roles` row
        // would promote everybody holding that group — including people added to it later
        // in Authelia, where nothing here can see it happen — which is why the fallback is
        // not implemented as one.
        let store = store().await;
        let promoted = store
            .upsert_oidc_principal("chef", "Chef", None, &["redaktion".into()])
            .await
            .unwrap();
        let colleague = store
            .upsert_oidc_principal("kollege", "Kollege", None, &["redaktion".into()])
            .await
            .unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();

        store.set_instance_admin(&promoted.id, true).await.unwrap();

        assert_eq!(
            store.baseline_for(&promoted).await.unwrap(),
            Baseline::Admin
        );
        assert_eq!(
            store.baseline_for(&colleague).await.unwrap(),
            Baseline::Public,
            "promoting one member of `redaktion` promoted the whole group"
        );
        assert_eq!(store.baseline_for(&guest).await.unwrap(), Baseline::Public);
        assert_eq!(store.active_instance_admin_count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_promotion_does_not_survive_deactivation_or_signing_out() {
        // Authentication and activity are checked before the promotion is read, exactly as
        // they are before the groups: a row in `instance_admins` is not evidence about a
        // request that has not said who it is.
        let store = store().await;
        let mut gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store.set_instance_admin(&gast.id, true).await.unwrap();
        assert_eq!(store.active_instance_admin_count().await.unwrap(), 1);

        // Two separate facts, and they are separate on purpose. `baseline_for` answers
        // about the principal a REQUEST is carrying, so the flag on that principal is what
        // decides. The count answers about the instance, so it reads the stored rows —
        // nobody hands it a principal to be wrong about.
        store.set_principal_active(&gast.id, false).await.unwrap();
        gast.active = false;
        assert_eq!(store.baseline_for(&gast).await.unwrap(), Baseline::Public);
        assert_eq!(
            store.active_instance_admin_count().await.unwrap(),
            0,
            "a deactivated promotion still counted as an administrator"
        );

        let mut forged = gw_auth::Principal::anonymous();
        forged.id = gast.id.clone();
        assert_eq!(store.baseline_for(&forged).await.unwrap(), Baseline::Public);
    }

    #[tokio::test]
    async fn the_authelia_group_path_is_untouched_by_the_promotion() {
        // The fallback is additional, never a replacement: D-M2-1 has to keep working
        // exactly as it did, including for somebody who is both.
        let store = store().await;
        let by_group = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
            .await
            .unwrap();
        let by_both = store
            .upsert_oidc_principal("beides", "Beides", None, &["admins".into()])
            .await
            .unwrap();
        let by_users = store
            .upsert_oidc_principal("kollege", "Kollege", None, &["users".into()])
            .await
            .unwrap();
        store.set_instance_admin(&by_both.id, true).await.unwrap();

        assert_eq!(
            store.baseline_for(&by_group).await.unwrap(),
            Baseline::Admin
        );
        assert_eq!(
            store.baseline_for(&by_users).await.unwrap(),
            Baseline::Internal
        );
        assert_eq!(store.active_instance_admin_count().await.unwrap(), 2);

        // Demoting somebody who also holds the group takes nothing away — great-wiki
        // cannot revoke an Authelia group, and must not pretend it can.
        store.set_instance_admin(&by_both.id, false).await.unwrap();
        assert_eq!(store.baseline_for(&by_both).await.unwrap(), Baseline::Admin);
    }

    // --- the same accessor, reached by an id ------------------------------------------

    /// [`Store::document_for_id`] is [`Store::document_for`] reached by an id, and must
    /// never be a second answer to the same question.
    ///
    /// Asserted as an **equivalence** over every caller and every action rather than as a
    /// handful of cases. A second implementation can satisfy a list of examples and still
    /// drift on the one nobody listed, and the reason this accessor exists is that the
    /// alternative — an aggregate view resolving its pages by id, unchecked, because the
    /// rows were already filtered somewhere else — is safe only for as long as that
    /// somewhere else stays right.
    #[tokio::test]
    async fn a_document_asked_for_by_id_answers_exactly_what_its_path_answers() {
        use crate::{Author, NewDocument};
        use gw_auth::Action;
        use gw_core::{Block, DocumentType, Visibility};

        let store = store().await;
        let body: Block = serde_json::from_str(r#"{"kind":"doc"}"#).unwrap();
        let id = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Befunde".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();

        let mut accounts = Vec::new();
        for username in ["fremder", "leser", "chefin"] {
            accounts.push(
                store
                    .create_local_principal(username, username, None, "x")
                    .await
                    .unwrap(),
            );
        }
        let [fremder, leser, chefin] = <[_; 3]>::try_from(accounts).unwrap();
        store
            .add_grant(
                "/befunde",
                Subject::Principal(leser.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        store
            .add_grant(
                "/befunde",
                Subject::Principal(chefin.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        let mut permitted = 0;
        for who in [&fremder, &leser, &chefin] {
            for action in [Action::Read, Action::Write] {
                let by_path = store.document_for(who, "/befunde", action).await.unwrap();
                let by_id = store.document_for_id(who, &id, action).await.unwrap();
                assert_eq!(
                    by_path.map(|doc| doc.id),
                    by_id.as_ref().map(|doc| doc.id.clone()),
                    "{} asking to {action:?} the page by id got a different answer than \
                     the same question asked by path",
                    who.username
                );
                permitted += usize::from(by_id.is_some());
            }
        }
        assert_eq!(
            permitted, 3,
            "the fixture must permit some of these and refuse others, or the equivalence \
             above holds vacuously"
        );

        // An id nothing names is `None`, exactly as a path nothing occupies is — and not
        // an error, because this layer does not decide whether existence may be revealed.
        assert!(store
            .document_for_id(
                &chefin,
                "0192f000-0000-7000-8000-000000000000",
                Action::Read
            )
            .await
            .unwrap()
            .is_none());
    }
}
