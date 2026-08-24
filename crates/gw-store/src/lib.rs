pub mod acl;
pub mod admin;
pub mod audit;
pub mod crdt;
pub mod documents;
pub mod invites;
pub mod links;
pub mod login_attempts;
pub mod principals;
pub mod revisions;
pub mod sessions;
pub mod tasks;

pub use acl::{Baseline, DocumentAccess};
pub use admin::MembershipOutcome;
pub use audit::{AuditEntry, AuditPage};
pub use documents::{NewDocument, StoredDocument, TreeNode};
pub use invites::{
    AcceptOutcome, CreateInviteOutcome, InviteOffer, InviteSummary, NewInvite, RevokeInviteOutcome,
    INVITE_TTL_SECONDS,
};
pub use links::{Backlink, Graph, GraphEdge, GraphNode};
pub use login_attempts::{LoginScope, LOGIN_FAILURE_LIMIT, LOGIN_LOCKOUT_SECONDS};
pub use principals::TeamSummary;
pub use revisions::{Author, Revision, IMPORT_AUTHOR_ID, IMPORT_AUTHOR_NAME};
pub use sessions::SESSION_TTL_SECONDS;
pub use tasks::{NewTask, Project, Task, TaskHome, TaskOutcome, TaskPage, TaskStatus, TaskUpdate};

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;
use url::Url;

pub struct Store {
    /// Crate-private, for the same reason [`Store::tree`] is. A caller holding the pool can
    /// `SELECT * FROM documents` and has bypassed every check in this crate, which would
    /// leave the invariant true only of the methods and not of the type. Nothing outside
    /// `gw-store` needs it: a query that does not exist here yet should become a method
    /// here, where the permission engine is already in scope.
    pub(crate) pool: SqlitePool,
    /// This deployment's own public origin — scheme, host and port, nothing else — or
    /// `None` when it is not configured. [`links::wiki_path`]'s doc comment explains why
    /// `gw-store` cannot work this out on its own: inventing a hostname in the schema layer
    /// would be worse than treating an absolute URL that happens to point back here as
    /// external. It arrives through [`Store::with_public_origin`] rather than a second
    /// `open` argument or an environment read in this crate — this is a library, and both
    /// of those are the application's job.
    pub(crate) public_origin: Option<Url>,
}

impl Store {
    /// Open (creating if needed) and migrate.
    ///
    /// `max_connections(1)` is deliberate and load-bearing for tests: an in-memory SQLite
    /// database is private per connection, so a larger pool silently gives some queries an
    /// empty database. Production is a single-writer workload anyway.
    pub async fn open(url: &str) -> Result<Self> {
        ensure_parent_dir(url)?;

        let opts = SqliteConnectOptions::from_str(url)?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(opts)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self {
            pool,
            public_origin: None,
        })
    }

    /// Configure the origin this deployment is publicly reachable at, so that an absolute
    /// URL pasted back at this wiki (`https://wiki.example.com/darm/labor`, say) resolves to
    /// a page instead of being recorded as an external link forever. A builder rather than a
    /// second parameter on [`Store::open`]: dozens of existing call sites — mostly tests —
    /// construct a `Store` with no opinion about this, and `None` is exactly what they
    /// already mean, unchanged by this method existing. Compared by [`url::Url::origin`] —
    /// scheme, host AND port — never by hostname alone, so `http://` and `https://` on the
    /// same host, or the same scheme and host on two different ports, are different origins.
    pub fn with_public_origin(mut self, origin: Option<Url>) -> Self {
        self.public_origin = origin;
        self
    }
}

/// Create the directory the database file lives in.
///
/// `create_if_missing` creates the *file* and never its directory, and the default
/// configuration points at `./data/great-wiki.db` — a path a fresh clone does not have.
/// Without this, the first command anyone runs after cloning fails on a directory the
/// application itself chose.
fn ensure_parent_dir(url: &str) -> Result<()> {
    // In-memory databases have no path. `mode=memory` is the URI spelling of the same.
    if url.contains(":memory:") || url.contains("mode=memory") {
        return Ok(());
    }
    let path = url
        .strip_prefix("sqlite://")
        .or_else(|| url.strip_prefix("sqlite:"))
        .unwrap_or(url);
    // Query parameters are part of the URL, not of the filename.
    let path = path.split('?').next().unwrap_or(path);

    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gw_core::{Block, DocumentType, Visibility};

    fn body(text: &str) -> Block {
        serde_json::from_str(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}"#
        ))
        .unwrap()
    }

    fn new_doc(parent: Option<&str>, title: &str, vis: Visibility) -> NewDocument {
        NewDocument {
            parent_path: parent.map(str::to_string),
            doc_type: DocumentType::Page,
            title: title.to_string(),
            slug: None,
            language: "de".into(),
            visibility: vis,
            body: body("hallo"),
            sort_key: 0,
        }
    }

    #[tokio::test]
    async fn insert_then_fetch_by_path() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Größe und Maß", Visibility::Public),
                None,
            )
            .await
            .unwrap();

        let got = store
            .document_by_path_unchecked("/groesse-und-mass")
            .await
            .unwrap()
            .expect("document should exist");
        assert_eq!(got.title, "Größe und Maß");
        assert_eq!(got.visibility, "public");
        assert!(got.body.contains("hallo"));
    }

    #[tokio::test]
    async fn path_is_derived_from_parent_and_slug() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Handbuch", Visibility::Public),
                None,
            )
            .await
            .unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(Some("/handbuch"), "Erste Schritte", Visibility::Public),
                None,
            )
            .await
            .unwrap();

        let child = store
            .document_by_path_unchecked("/handbuch/erste-schritte")
            .await
            .unwrap();
        assert!(child.is_some(), "child path must nest under its parent");
    }

    #[tokio::test]
    async fn duplicate_path_is_rejected_rather_than_overwriting() {
        // Silent overwrite on slug collision is data loss. It must be an error.
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Notes", Visibility::Public),
                None,
            )
            .await
            .unwrap();
        let second = store
            .create_document(
                Author::Import,
                &new_doc(None, "Notes", Visibility::Public),
                None,
            )
            .await;
        assert!(second.is_err(), "a colliding path must fail loudly");
    }

    #[tokio::test]
    async fn opening_a_database_creates_the_directory_it_lives_in() {
        // The default configuration points at ./data/great-wiki.db, which a fresh clone
        // does not have. `create_if_missing` creates the file, never the directory.
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("nested/great-wiki.db");
        let store = Store::open(&format!("sqlite://{}", db.display()))
            .await
            .unwrap();
        assert!(db.exists());
        drop(store);
    }

    #[tokio::test]
    async fn resolved_path_matches_the_path_the_insert_actually_uses() {
        // The seeder pre-checks collisions with `resolved_path`. If it could disagree
        // with the insert, the error message would name a path that is not the one that
        // collided.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let doc = new_doc(Some("/handbuch"), "Größe und Maß", Visibility::Public);
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Handbuch", Visibility::Public),
                None,
            )
            .await
            .unwrap();
        store
            .create_document(Author::Import, &doc, None)
            .await
            .unwrap();
        assert_eq!(doc.resolved_path().unwrap(), "/handbuch/groesse-und-mass");
        assert!(store
            .document_by_path_unchecked(&doc.resolved_path().unwrap())
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn missing_document_is_none_not_an_error() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        assert!(store
            .document_by_path_unchecked("/nope")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn tree_nests_children_under_parents_in_sort_order() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Handbuch", Visibility::Public),
                None,
            )
            .await
            .unwrap();

        let mut b = new_doc(Some("/handbuch"), "Beta", Visibility::Public);
        b.sort_key = 2;
        let mut a = new_doc(Some("/handbuch"), "Alpha", Visibility::Public);
        a.sort_key = 1;
        store
            .create_document(Author::Import, &b, None)
            .await
            .unwrap();
        store
            .create_document(Author::Import, &a, None)
            .await
            .unwrap();

        let tree = store.tree().await.unwrap();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].path, "/handbuch");
        let kids: Vec<&str> = tree[0].children.iter().map(|c| c.slug.as_str()).collect();
        assert_eq!(
            kids,
            vec!["alpha", "beta"],
            "children must respect sort_key"
        );
    }

    // ---------------------------------------------------------------------------------
    // Identity, ACLs and effective permissions (M2 Task 3, including D-M2-1).
    // ---------------------------------------------------------------------------------

    use gw_auth::{Action, Permission, Principal, Subject};

    async fn seeded() -> Store {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Handbuch", Visibility::Restricted),
                None,
            )
            .await
            .unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(Some("/handbuch"), "Onboarding", Visibility::Restricted),
                None,
            )
            .await
            .unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Öffentlich", Visibility::Public),
                None,
            )
            .await
            .unwrap();
        store
            .create_document(
                Author::Import,
                &new_doc(None, "Intern", Visibility::Internal),
                None,
            )
            .await
            .unwrap();
        store
    }

    fn titles(nodes: &[TreeNode]) -> Vec<&str> {
        nodes.iter().map(|n| n.title.as_str()).collect()
    }

    #[tokio::test]
    async fn an_oidc_principal_is_created_on_first_login_and_updated_after() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let first = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["users".into()])
            .await
            .unwrap();
        let second = store
            .upsert_oidc_principal(
                "sergej",
                "Sergej Maul",
                None,
                &["users".into(), "admins".into()],
            )
            .await
            .unwrap();

        assert_eq!(
            first.id, second.id,
            "the same user must not create a second principal"
        );
        assert_eq!(second.display_name, "Sergej Maul");
        // Groups are refreshed from the verified claim: losing a group must take effect.
        assert_eq!(second.groups, vec!["users", "admins"]);
    }

    #[tokio::test]
    async fn oidc_groups_are_replaced_on_upsert_never_merged() {
        // Merging would make removal impossible: someone dropped from `admins` in
        // Authelia would keep the reach forever.
        let store = Store::open("sqlite::memory:").await.unwrap();
        store
            .upsert_oidc_principal("sergej", "Sergej", None, &["users".into(), "admins".into()])
            .await
            .unwrap();
        let after = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["users".into()])
            .await
            .unwrap();
        assert_eq!(after.groups, vec!["users"]);
        assert_eq!(
            store.baseline_for(&after).await.unwrap(),
            Baseline::Internal,
            "losing `admins` must lose the admin baseline"
        );
    }

    #[tokio::test]
    async fn a_local_principal_carries_its_credential_and_no_groups() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "$argon2id$fake")
            .await
            .unwrap();
        assert_eq!(guest.kind, gw_auth::PrincipalKind::Local);
        assert!(guest.groups.is_empty());

        let (_, hash) = store.principal_by_username("gast").await.unwrap().unwrap();
        assert_eq!(hash.as_deref(), Some("$argon2id$fake"));

        // An OIDC principal has no credential row at all, rather than a NULL to compare.
        store
            .upsert_oidc_principal("sergej", "Sergej", None, &[])
            .await
            .unwrap();
        let (_, none) = store
            .principal_by_username("sergej")
            .await
            .unwrap()
            .unwrap();
        assert!(none.is_none());
    }

    #[tokio::test]
    async fn team_membership_loads_onto_the_principal() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store.create_team("editors", "Redaktion").await.unwrap();
        store.add_team_member("editors", &guest.id).await.unwrap();

        let (loaded, _) = store.principal_by_username("gast").await.unwrap().unwrap();
        assert_eq!(loaded.teams, vec!["editors"]);
    }

    #[tokio::test]
    async fn deactivating_a_principal_is_visible_on_the_next_load() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        assert!(guest.active);
        store.set_principal_active(&guest.id, false).await.unwrap();
        let (loaded, _) = store.principal_by_username("gast").await.unwrap().unwrap();
        assert!(!loaded.active);
    }

    #[tokio::test]
    async fn grants_inherit_down_the_tree() {
        let store = seeded().await;
        store
            .add_grant(
                "/handbuch",
                Subject::Team("editors".into()),
                Permission::Read,
            )
            .await
            .unwrap();

        let grants = store.grants_for_path("/handbuch/onboarding").await.unwrap();
        assert_eq!(grants.len(), 1, "a child must inherit its parent's grants");
        assert_eq!(grants[0].permission, Permission::Read);
    }

    #[tokio::test]
    async fn a_descendant_grant_overrides_rather_than_adds() {
        let store = seeded().await;
        store
            .add_grant(
                "/handbuch",
                Subject::Team("editors".into()),
                Permission::Admin,
            )
            .await
            .unwrap();
        store
            .add_grant(
                "/handbuch/onboarding",
                Subject::Team("editors".into()),
                Permission::Read,
            )
            .await
            .unwrap();

        let grants = store.grants_for_path("/handbuch/onboarding").await.unwrap();
        assert_eq!(
            grants.len(),
            1,
            "the nearest ancestor with grants wins outright"
        );
        assert_eq!(grants[0].permission, Permission::Read);
    }

    #[tokio::test]
    async fn a_descendant_grant_can_narrow_access_a_parent_grant_gave() {
        // The reason grants do not union upward: narrowing a subtree must be possible.
        let store = seeded().await;
        store
            .add_grant(
                "/handbuch",
                Subject::Team("editors".into()),
                Permission::Admin,
            )
            .await
            .unwrap();
        store
            .add_grant(
                "/handbuch/onboarding",
                Subject::Team("leitung".into()),
                Permission::Read,
            )
            .await
            .unwrap();
        let editor = Principal::test("ed", &[], &["editors"]);

        assert!(store
            .document_for(&editor, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert!(
            store
                .document_for(&editor, "/handbuch/onboarding", Action::Read)
                .await
                .unwrap()
                .is_none(),
            "a nearer grant that does not name the caller must take the subtree away"
        );
    }

    #[tokio::test]
    async fn the_tree_hides_documents_the_principal_cannot_read() {
        let store = seeded().await;
        let guest = Principal::test("guest", &[], &[]);

        let tree = store.tree_for(&guest).await.unwrap();
        assert_eq!(
            titles(&tree),
            vec!["Öffentlich"],
            "restricted branches must not appear at all"
        );
    }

    #[tokio::test]
    async fn a_granted_team_member_sees_the_restricted_branch() {
        let store = seeded().await;
        store
            .add_grant(
                "/handbuch",
                Subject::Team("editors".into()),
                Permission::Read,
            )
            .await
            .unwrap();
        let editor = Principal::test("ed", &[], &["editors"]);

        let tree = store.tree_for(&editor).await.unwrap();
        let handbuch = tree
            .iter()
            .find(|n| n.path == "/handbuch")
            .expect("branch should appear");
        assert_eq!(
            handbuch.children.len(),
            1,
            "the granted subtree comes with it"
        );
    }

    #[tokio::test]
    async fn document_for_enforces_the_action_not_just_readability() {
        let store = seeded().await;
        store
            .add_grant(
                "/handbuch",
                Subject::Team("editors".into()),
                Permission::Read,
            )
            .await
            .unwrap();
        let editor = Principal::test("ed", &[], &["editors"]);

        assert!(store
            .document_for(&editor, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert!(store
            .document_for(&editor, "/handbuch", Action::Write)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn document_for_is_none_for_an_absent_path() {
        // Absent and forbidden are both `None` here on purpose; the HTTP layer decides
        // which to reveal.
        let store = seeded().await;
        let editor = Principal::test("ed", &[], &["editors"]);
        assert!(store
            .document_for(&editor, "/gibt-es-nicht", Action::Read)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_deactivated_principal_loses_granted_access_immediately() {
        let store = seeded().await;
        store
            .add_grant(
                "/handbuch",
                Subject::Team("editors".into()),
                Permission::Admin,
            )
            .await
            .unwrap();
        let mut editor = Principal::test("ed", &[], &["editors"]);
        editor.active = false;

        assert!(store
            .document_for(&editor, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .tree_for(&editor)
            .await
            .unwrap()
            .iter()
            .all(|n| n.path != "/handbuch"));
    }

    // --- D-M2-1: default reach follows the verified Authelia group ---------------------

    #[tokio::test]
    async fn a_local_principal_with_no_groups_reaches_only_public_content() {
        // The row that matters most: "I gave someone an account" must never silently
        // become "I gave them the internal wiki". Local guests have no Authelia groups.
        let store = seeded().await;
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();

        assert_eq!(store.baseline_for(&guest).await.unwrap(), Baseline::Public);
        assert!(store
            .document_for(&guest, "/oeffentlich", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert!(
            store
                .document_for(&guest, "/intern", Action::Read)
                .await
                .unwrap()
                .is_none(),
            "an account by itself must not confer the internal wiki"
        );
        assert!(store
            .document_for(&guest, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            titles(&store.tree_for(&guest).await.unwrap()),
            ["Öffentlich"]
        );
    }

    #[tokio::test]
    async fn the_users_group_reaches_internal_but_not_restricted() {
        let store = seeded().await;
        let member = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["users".into()])
            .await
            .unwrap();

        assert_eq!(
            store.baseline_for(&member).await.unwrap(),
            Baseline::Internal
        );
        assert!(store
            .document_for(&member, "/intern", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert!(
            store
                .document_for(&member, "/handbuch", Action::Read)
                .await
                .unwrap()
                .is_none(),
            "`users` is public plus internal, never restricted without a grant"
        );
        assert_eq!(
            titles(&store.tree_for(&member).await.unwrap()),
            ["Intern", "Öffentlich"]
        );
    }

    #[tokio::test]
    async fn the_admins_group_reaches_restricted_content_without_a_grant() {
        let store = seeded().await;
        let admin = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
            .await
            .unwrap();

        assert_eq!(store.baseline_for(&admin).await.unwrap(), Baseline::Admin);
        assert!(store
            .document_for(&admin, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert_eq!(
            titles(&store.tree_for(&admin).await.unwrap()),
            ["Handbuch", "Intern", "Öffentlich"]
        );
    }

    #[tokio::test]
    async fn the_admin_baseline_does_not_confer_writing() {
        // Reach is about reading. Writing still needs a grant, so a stray `admins`
        // membership cannot silently become edit rights on every page.
        let store = seeded().await;
        let admin = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
            .await
            .unwrap();
        assert!(store
            .document_for(&admin, "/handbuch", Action::Write)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn an_unmapped_group_confers_public_only() {
        let store = seeded().await;
        let outsider = store
            .upsert_oidc_principal("extern", "Extern", None, &["lieferanten".into()])
            .await
            .unwrap();

        assert_eq!(
            store.baseline_for(&outsider).await.unwrap(),
            Baseline::Public,
            "a group with no row confers nothing beyond public"
        );
        assert_eq!(
            titles(&store.tree_for(&outsider).await.unwrap()),
            ["Öffentlich"]
        );
    }

    #[tokio::test]
    async fn the_strongest_baseline_across_the_groups_wins() {
        let store = seeded().await;
        let both = store
            .upsert_oidc_principal(
                "sergej",
                "Sergej",
                None,
                &["lieferanten".into(), "users".into(), "admins".into()],
            )
            .await
            .unwrap();
        assert_eq!(store.baseline_for(&both).await.unwrap(), Baseline::Admin);
    }

    #[tokio::test]
    async fn mapping_a_new_group_is_a_row_not_a_release() {
        let store = seeded().await;
        let outsider = store
            .upsert_oidc_principal("extern", "Extern", None, &["lieferanten".into()])
            .await
            .unwrap();
        assert!(store
            .document_for(&outsider, "/intern", Action::Read)
            .await
            .unwrap()
            .is_none());

        store
            .set_group_role("lieferanten", Baseline::Internal)
            .await
            .unwrap();

        assert_eq!(
            store.baseline_for(&outsider).await.unwrap(),
            Baseline::Internal
        );
        assert!(store
            .document_for(&outsider, "/intern", Action::Read)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn the_seeded_mapping_is_admins_and_users_only() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let mut roles = store.group_roles().await.unwrap();
        roles.sort();
        assert_eq!(
            roles,
            vec![
                ("admins".to_string(), Baseline::Admin),
                ("users".to_string(), Baseline::Internal),
            ]
        );
    }

    #[tokio::test]
    async fn an_anonymous_caller_carrying_a_forged_group_gets_no_baseline() {
        // Authentication before groups, always. A `groups` list on a request that is not
        // signed in is not evidence of anything.
        let store = seeded().await;
        let mut forged = Principal::anonymous();
        forged.groups = vec!["admins".into()];

        assert_eq!(store.baseline_for(&forged).await.unwrap(), Baseline::Public);
        assert!(store
            .document_for(&forged, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .document_for(&forged, "/intern", Action::Read)
            .await
            .unwrap()
            .is_none());
        assert_eq!(
            titles(&store.tree_for(&forged).await.unwrap()),
            ["Öffentlich"]
        );
    }

    #[tokio::test]
    async fn a_deactivated_admin_loses_the_baseline_but_keeps_public_reads() {
        let store = seeded().await;
        let mut admin = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
            .await
            .unwrap();
        store.set_principal_active(&admin.id, false).await.unwrap();
        admin.active = false;

        assert_eq!(store.baseline_for(&admin).await.unwrap(), Baseline::Public);
        assert!(store
            .document_for(&admin, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_none());
        assert!(
            store
                .document_for(&admin, "/oeffentlich", Action::Read)
                .await
                .unwrap()
                .is_some(),
            "suspending an account must not make the public site disappear"
        );
    }

    #[tokio::test]
    async fn a_grant_still_reaches_a_principal_with_no_baseline() {
        // The baseline narrows the DEFAULT. An explicit grant is unaffected by it.
        let store = seeded().await;
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .add_grant(
                "/handbuch",
                Subject::Principal(guest.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();

        assert_eq!(store.baseline_for(&guest).await.unwrap(), Baseline::Public);
        assert!(store
            .document_for(&guest, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert_eq!(
            titles(&store.tree_for(&guest).await.unwrap()),
            ["Handbuch", "Öffentlich"],
            "the granted subtree plus public pages, and nothing else"
        );
    }

    #[tokio::test]
    async fn an_anyone_grant_reaches_an_anonymous_caller_at_that_permission_only() {
        let store = seeded().await;
        store
            .add_grant("/handbuch", Subject::Anyone, Permission::Read)
            .await
            .unwrap();
        let anon = Principal::anonymous();

        assert!(store
            .document_for(&anon, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_some());
        assert!(store
            .document_for(&anon, "/handbuch", Action::Write)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn an_unrecognised_subject_kind_confers_nothing() {
        // A row written by a future version, or by hand, must never be guessed at. The
        // CHECK constraint is suspended for the length of the insert so the test can
        // produce the row a CHECK is meant to prevent, and prove the code refuses it too.
        let store = seeded().await;
        for statement in [
            "PRAGMA ignore_check_constraints = ON",
            "INSERT INTO acl (id, path, subject_kind, subject_id, permission) \
             VALUES ('x', '/handbuch', 'zukunft', NULL, 'read')",
            "PRAGMA ignore_check_constraints = OFF",
        ] {
            sqlx::query(statement).execute(&store.pool).await.unwrap();
        }

        assert!(store.grants_for_path("/handbuch").await.unwrap().is_empty());
        let anon = Principal::anonymous();
        assert!(store
            .document_for(&anon, "/handbuch", Action::Read)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn soft_deleted_documents_are_excluded() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let id = store
            .create_document(
                Author::Import,
                &new_doc(None, "Temporär", Visibility::Public),
                None,
            )
            .await
            .unwrap();
        sqlx::query("UPDATE documents SET deleted_at = datetime('now') WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();

        assert!(store
            .document_by_path_unchecked("/temporaer")
            .await
            .unwrap()
            .is_none());
        assert!(store.tree().await.unwrap().is_empty());
        assert!(
            !store.document_exists("/temporaer").await.unwrap(),
            "a deleted document must not keep answering the existence check that picks \
             404 over 403"
        );
    }

    // ---------------------------------------------------------------------------------
    // Sessions (M2 Task 6). Persisted, hashed, expiring, and re-read on every lookup.
    // ---------------------------------------------------------------------------------

    /// Sessions are stored by digest, so the tests speak in digests too. The real one is
    /// SHA-256 and lives in `gw_api::auth::session`, next to the generator; the store
    /// treats the value as opaque, and a test that pretended otherwise would be asserting
    /// against a layer that does not hash anything.
    fn digest(token: &str) -> String {
        format!("digest-of-{token}")
    }

    #[tokio::test]
    async fn a_session_resolves_to_the_principal_that_owns_it() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("t"), SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let resolved = store
            .principal_for_session(&digest("t"))
            .await
            .unwrap()
            .expect("a live session must resolve");
        assert_eq!(resolved.id, guest.id);
        assert_eq!(resolved.username, "gast");
    }

    #[tokio::test]
    async fn an_expired_session_resolves_to_nobody() {
        // Expiry is enforced in the lookup, not by a sweeper: the row is still there.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("abgelaufen"), -60)
            .await
            .unwrap();

        assert_eq!(store.session_count_for(&guest.id).await.unwrap(), 1);
        assert!(store
            .principal_for_session(&digest("abgelaufen"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn an_unknown_session_token_resolves_to_nobody() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        assert!(store
            .principal_for_session(&digest("erfunden"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn only_the_digest_is_stored_never_the_token() {
        // A database disclosure must not hand over live sessions. The row holds what the
        // caller hashed and nothing that resembles the token itself.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("geheim"), SESSION_TTL_SECONDS)
            .await
            .unwrap();

        let (stored,): (String,) = sqlx::query_as("SELECT token_hash FROM sessions")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(stored, digest("geheim"));
        assert!(
            !stored.contains("geheim") || stored != "geheim",
            "the plaintext token must never be the stored value"
        );
    }

    #[tokio::test]
    async fn deleting_a_session_stops_it_resolving() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("t"), SESSION_TTL_SECONDS)
            .await
            .unwrap();

        assert!(store.delete_session(&digest("t")).await.unwrap());
        assert!(store
            .principal_for_session(&digest("t"))
            .await
            .unwrap()
            .is_none());
        assert_eq!(store.session_count_for(&guest.id).await.unwrap(), 0);
        // Deleting twice is not an error, it is simply no longer there.
        assert!(!store.delete_session(&digest("t")).await.unwrap());
    }

    #[tokio::test]
    async fn deactivating_a_principal_deletes_its_sessions() {
        // D-M2-7, the half that a per-request permission read cannot deliver on its own:
        // the cookie must stop resolving, not merely stop conferring anything.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        for token in ["laptop", "telefon"] {
            store
                .create_session(&guest.id, &digest(token), SESSION_TTL_SECONDS)
                .await
                .unwrap();
        }

        store.set_principal_active(&guest.id, false).await.unwrap();

        assert_eq!(
            store.session_count_for(&guest.id).await.unwrap(),
            0,
            "every session, on every device, not just the one that made the request"
        );
        assert!(store
            .principal_for_session(&digest("laptop"))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn reactivating_a_principal_does_not_bring_its_sessions_back() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("t"), SESSION_TTL_SECONDS)
            .await
            .unwrap();

        store.set_principal_active(&guest.id, false).await.unwrap();
        store.set_principal_active(&guest.id, true).await.unwrap();

        assert!(
            store
                .principal_for_session(&digest("t"))
                .await
                .unwrap()
                .is_none(),
            "invalidated means gone; they sign in again"
        );
    }

    #[tokio::test]
    async fn the_principal_behind_a_session_is_read_fresh_not_captured_at_sign_in() {
        // D-M2-7: a change made after the session existed must be visible through it.
        let store = Store::open("sqlite::memory:").await.unwrap();
        let member = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["admins".into()])
            .await
            .unwrap();
        store
            .create_session(&member.id, &digest("t"), SESSION_TTL_SECONDS)
            .await
            .unwrap();

        store.create_team("editors", "Redaktion").await.unwrap();
        store.add_team_member("editors", &member.id).await.unwrap();
        store
            .upsert_oidc_principal("sergej", "Sergej Maul", None, &["users".into()])
            .await
            .unwrap();

        let resolved = store
            .principal_for_session(&digest("t"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(resolved.teams, vec!["editors"], "a team added afterwards");
        assert_eq!(resolved.groups, vec!["users"], "groups as they are now");
        assert_eq!(resolved.display_name, "Sergej Maul");
    }

    #[tokio::test]
    async fn creating_a_session_clears_out_expired_rows() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("alt"), -60)
            .await
            .unwrap();
        store
            .create_session(&guest.id, &digest("neu"), SESSION_TTL_SECONDS)
            .await
            .unwrap();

        assert_eq!(
            store.session_count_for(&guest.id).await.unwrap(),
            1,
            "the expired row is not kept for ever"
        );
    }

    #[tokio::test]
    async fn principal_by_id_matches_principal_by_username() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let guest = store
            .create_local_principal("gast", "Gast", None, "$argon2id$fake")
            .await
            .unwrap();
        store.create_team("gaeste", "Gäste").await.unwrap();
        store.add_team_member("gaeste", &guest.id).await.unwrap();

        let (by_id, hash_by_id) = store.principal_by_id(&guest.id).await.unwrap().unwrap();
        let (by_name, hash_by_name) = store.principal_by_username("gast").await.unwrap().unwrap();
        assert_eq!(by_id.id, by_name.id);
        assert_eq!(by_id.teams, by_name.teams);
        assert_eq!(by_id.teams, vec!["gaeste"]);
        assert_eq!(hash_by_id, hash_by_name);
    }

    #[tokio::test]
    async fn document_exists_answers_only_the_question_the_status_code_needs() {
        // What separates 404 from 403 at the HTTP layer, and the only thing a caller may
        // learn about a path without presenting a principal.
        let store = seeded().await;
        assert!(store.document_exists("/handbuch").await.unwrap());
        assert!(store.document_exists("/oeffentlich").await.unwrap());
        assert!(!store.document_exists("/gibt-es-nicht").await.unwrap());
    }
}
