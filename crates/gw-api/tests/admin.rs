//! The admin API: principals, teams, path-scoped grants and the audit log.
//!
//! Every test here is about who may do what, so each one names the principal it speaks
//! as. The fixture holds four:
//!
//! - `chef` — an `admins` member, so an instance admin by baseline (D-M2-1).
//! - `gast` — a local account with no groups and no grants. Nobody.
//! - `lektor` — a local account holding `admin` on `/raum` and nothing else. A SPACE
//!   admin: no baseline, so every instance-wide endpoint must refuse them.
//! - `leser` — a local account holding `read` on `/raum`. Being able to read a space is
//!   not being able to administer it, and that distinction is a test.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use gw_auth::{Permission, Subject};
use gw_store::{AuditEntry, Store};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

/// The password used wherever a test creates an account. Long enough to pass
/// `validate_password_strength`, and asserted against so no response or audit row can
/// quietly carry it.
const PASSPHRASE: &str = "ein-vollkommen-brauchbarer-satz";

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();

    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();
    store
        .create_local_principal("gast", "Gast", None, "$argon2id$fake")
        .await
        .unwrap();
    let lektor = store
        .create_local_principal("lektor", "Lektor", None, "$argon2id$fake")
        .await
        .unwrap();
    let leser = store
        .create_local_principal("leser", "Leser", None, "$argon2id$fake")
        .await
        .unwrap();

    store
        .add_grant(
            "/raum",
            Subject::Principal(lektor.id.clone()),
            Permission::Admin,
        )
        .await
        .unwrap();
    store
        .add_grant(
            "/raum",
            Subject::Principal(leser.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();
    store.create_team("gaeste", "Gäste").await.unwrap();

    // `/anderer-raum` deliberately carries no grants at all: it is the path a space admin
    // must be refused on.
    Arc::new(store)
}

async fn id_of(store: &Arc<Store>, username: &str) -> String {
    store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"))
        .0
        .id
}

/// A router whose requests arrive as the stored principal called `who`, or anonymously
/// when it is `None`.
///
/// The principal is looked up rather than invented, so a test cannot assert against
/// groups, teams or an active flag the database does not actually hold.
async fn router(store: &Arc<Store>, who: Option<&str>) -> axum::Router {
    let state = match who {
        Some(username) => {
            let (principal, _) = store
                .principal_by_username(username)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
            gw_api::AppState::for_test_principal(Arc::clone(store), &principal)
        }
        None => gw_api::AppState::for_test(Arc::clone(store), None),
    };
    gw_api::build_router(state)
}

/// A corpus that reports every password as heavily breached.
///
/// Exists so one test can prove the breach check is actually CONSULTED by admin account
/// creation. That endpoint originally called the length-only validator directly, so
/// accounts created by an administrator skipped the corpus entirely while the policy
/// looked implemented — a defect no test could see, because every test password was
/// long enough and none of them asked whether the corpus was reached at all.
struct BreachedCorpus;

impl gw_auth::breach::BreachRange for BreachedCorpus {
    fn fetch<'a>(&'a self, _prefix: &'a str) -> gw_auth::breach::BreachFuture<'a> {
        // The range API answers `SUFFIX:COUNT` lines. The suffix of SHA-1("password"),
        // whose prefix is 5BAA6.
        Box::pin(async { Ok("1E4C9B93F3F0682250B6CF8331B7EE68FD8:9545824".to_string()) })
    }
}

async fn send(
    store: &Arc<Store>,
    who: Option<&str>,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder().method(method).uri(uri);
    let request = match body {
        Some(value) => builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => builder.body(Body::empty()).unwrap(),
    };

    let response = router(store, who).await.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn get(store: &Arc<Store>, who: Option<&str>, uri: &str) -> (StatusCode, Value) {
    send(store, who, Method::GET, uri, None).await
}

async fn post(
    store: &Arc<Store>,
    who: Option<&str>,
    uri: &str,
    body: Value,
) -> (StatusCode, Value) {
    send(store, who, Method::POST, uri, Some(body)).await
}

async fn delete(
    store: &Arc<Store>,
    who: Option<&str>,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    send(store, who, Method::DELETE, uri, body).await
}

/// The whole audit log, read as the instance admin.
async fn audit(store: &Arc<Store>) -> Vec<AuditEntry> {
    let (chef, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    store.audit_for(&chef, 500).await.unwrap().entries
}

/// The mutating endpoints that concern the whole instance rather than one subtree.
/// Ordered so each one's precondition is satisfied by the ones before it.
fn instance_wide_mutations(principal_id: &str) -> Vec<(Method, String, Option<Value>)> {
    vec![
        (
            Method::POST,
            "/api/admin/principals".into(),
            Some(json!({
                "username": "neu",
                "display_name": "Neu",
                "password": PASSPHRASE,
            })),
        ),
        (
            Method::POST,
            "/api/admin/teams".into(),
            Some(json!({"slug": "redaktion", "name": "Redaktion"})),
        ),
        (
            Method::POST,
            "/api/admin/teams/redaktion/members".into(),
            Some(json!({"principal_id": principal_id})),
        ),
        (
            Method::DELETE,
            format!("/api/admin/teams/redaktion/members/{principal_id}"),
            None,
        ),
        (
            Method::POST,
            format!("/api/admin/principals/{principal_id}/active"),
            Some(json!({"active": false})),
        ),
    ]
}

/// The mutating endpoints scoped to one path. Both name `/raum`, which the fixture's
/// space admin administers — so these are exactly the requests that must NOT be refused
/// by an instance-wide gate.
fn path_scoped_mutations() -> Vec<(Method, String, Option<Value>)> {
    vec![
        (
            Method::POST,
            "/api/admin/acl".into(),
            Some(json!({
                "path": "/raum",
                "subject": {"kind": "team", "id": "gaeste"},
                "permission": "read",
            })),
        ),
        (
            Method::DELETE,
            "/api/admin/acl".into(),
            Some(json!({
                "path": "/raum",
                "subject": {"kind": "team", "id": "gaeste"},
                "permission": "read",
            })),
        ),
    ]
}

/// Every mutating request in the admin API. One list, so a new endpoint cannot be added
/// while quietly escaping the audit-row test.
fn mutations(principal_id: &str) -> Vec<(Method, String, Option<Value>)> {
    let mut all = instance_wide_mutations(principal_id);
    all.extend(path_scoped_mutations());
    all
}

/// Every instance-wide endpoint, mutating or not.
fn instance_wide(principal_id: &str) -> Vec<(Method, String, Option<Value>)> {
    let mut all = vec![
        (Method::GET, "/api/admin/principals".into(), None),
        (Method::GET, "/api/admin/teams".into(), None),
        // No `path`: the index of every path carrying a grant is instance-wide, because
        // it names subtrees the caller may administer none of.
        (Method::GET, "/api/admin/acl".into(), None),
    ];
    all.extend(instance_wide_mutations(principal_id));
    all
}

// -------------------------------------------------------------------------------------
// Refusals. Each of these must fail if the check it covers is deleted — see the comments.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_caller_with_no_admin_baseline_is_refused_every_instance_wide_endpoint() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    for (method, uri, body) in instance_wide(&gast) {
        let (status, response) = send(&store, Some("gast"), method.clone(), &uri, body).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} answered {status}: {response}"
        );
    }
    // The audit log is not instance-wide — a space admin reads their own subtree of it —
    // but somebody who administers nothing has nothing to read, and 200 with an empty
    // list would be an invitation to keep asking.
    assert_eq!(
        get(&store, Some("gast"), "/api/admin/audit").await.0,
        StatusCode::FORBIDDEN
    );
    assert!(
        audit(&store).await.is_empty(),
        "a refused request wrote to the audit log"
    );
}

#[tokio::test]
async fn a_space_admin_is_refused_the_instance_wide_endpoints() {
    // The load-bearing half of D-M2-2: administering a space must not decentralise
    // instance administration. `lektor` holds `admin` on `/raum`, so a gate that asked
    // "does this caller administer anything?" would let them list every account.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    for (method, uri, body) in instance_wide(&gast) {
        let (status, response) = send(&store, Some("lektor"), method.clone(), &uri, body).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} answered {status}: {response}"
        );
    }
}

#[tokio::test]
async fn an_anonymous_caller_is_refused_every_endpoint() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    let mut all = instance_wide(&gast);
    all.push((Method::GET, "/api/admin/audit".into(), None));
    all.push((Method::GET, "/api/admin/acl?path=/raum".into(), None));

    for (method, uri, body) in all {
        let (status, response) = send(&store, None, method.clone(), &uri, body).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} answered {status}: {response}"
        );
    }
}

#[tokio::test]
async fn an_anonymous_caller_is_refused_even_where_anyone_holds_admin() {
    // THE test for the authentication check, and the reason it exists as its own gate
    // rather than being left to `can()`.
    //
    // `can()` answers an `Anyone` grant BEFORE it looks at whether the caller is signed
    // in — deliberately, because that is how a public share link works. So on a path
    // carrying `Anyone: admin`, `can(anonymous, Action::Admin, ...)` is TRUE, and an
    // admin API that consulted only the engine would hand the whole ACL editor to a
    // request that has not said who it is.
    //
    // Without this grant every assertion below would hold whether or not authentication
    // is checked at all — no grant would match any subject — which is exactly the vacuous
    // shape that has bitten this project three times.
    let store = fixture().await;
    store
        .add_grant("/offen", Subject::Anyone, Permission::Admin)
        .await
        .unwrap();

    let grant = json!({
        "path": "/offen",
        "subject": {"kind": "team", "id": "gaeste"},
        "permission": "read",
    });

    assert_eq!(
        get(&store, None, "/api/admin/acl?path=/offen").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&store, None, "/api/admin/acl", grant.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        delete(&store, None, "/api/admin/acl", Some(grant)).await.0,
        StatusCode::FORBIDDEN
    );
    // The same for the audit log: `Anyone: admin` on a path makes the scoped read
    // reachable, so only the authentication check stands between an anonymous request
    // and who-did-what.
    assert_eq!(
        get(&store, None, "/api/admin/audit").await.0,
        StatusCode::FORBIDDEN
    );

    assert_eq!(
        store.grants_defined_at("/offen").await.unwrap().len(),
        1,
        "a refused request changed the ACL"
    );
}

#[tokio::test]
async fn a_deactivated_space_admin_is_refused_their_own_subtree() {
    // D-M2-7 through the admin API. `can()` checks `active`, but only after the `Anyone`
    // branch, and `baseline_for` refuses a deactivated account outright — so this is the
    // gate's own check being tested, not the engine's.
    let store = fixture().await;
    let lektor = id_of(&store, "lektor").await;
    store.set_principal_active(&lektor, false).await.unwrap();

    let grant = json!({
        "path": "/raum",
        "subject": {"kind": "team", "id": "gaeste"},
        "permission": "read",
    });
    assert_eq!(
        get(&store, Some("lektor"), "/api/admin/acl?path=/raum")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&store, Some("lektor"), "/api/admin/acl", grant)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn reading_a_space_is_not_administering_it() {
    // `leser` holds `read` on `/raum`. If the path gate asked for `Action::Read` instead
    // of `Action::Admin`, every reader of a space would be able to grant access to it.
    let store = fixture().await;
    let grant = json!({
        "path": "/raum",
        "subject": {"kind": "team", "id": "gaeste"},
        "permission": "read",
    });

    assert_eq!(
        get(&store, Some("leser"), "/api/admin/acl?path=/raum")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&store, Some("leser"), "/api/admin/acl", grant.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        delete(&store, Some("leser"), "/api/admin/acl", Some(grant))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        get(&store, Some("leser"), "/api/admin/audit").await.0,
        StatusCode::FORBIDDEN,
        "reading a space must not confer reading who was granted access to it"
    );
}

// -------------------------------------------------------------------------------------
// Principals.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn an_instance_admin_lists_principals_creates_an_account_and_deactivates_it() {
    let store = fixture().await;

    let (status, listed) = get(&store, Some("chef"), "/api/admin/principals").await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = listed
        .as_array()
        .expect("a list")
        .iter()
        .map(|p| p["username"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["chef", "gast", "lektor", "leser"]);

    let (status, created) = post(
        &store,
        Some("chef"),
        "/api/admin/principals",
        json!({
            "username": "neu",
            "display_name": "Neue Person",
            "email": "neu@example.invalid",
            "password": PASSPHRASE,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["username"], "neu");
    assert_eq!(created["kind"], "local");
    assert_eq!(created["active"], true);

    // Rule 6, checked against the wire rather than against intent.
    let body = created.to_string();
    assert!(!body.contains(PASSPHRASE), "the password came back: {body}");
    assert!(
        !body.contains("argon2"),
        "a password hash came back: {body}"
    );

    // The account really exists, with a credential that is not the plaintext.
    let (person, hash) = store
        .principal_by_username("neu")
        .await
        .unwrap()
        .expect("the account must exist");
    let hash = hash.expect("a local account has a credential");
    assert!(hash.starts_with("$argon2id$"), "{hash}");
    assert!(!hash.contains(PASSPHRASE));

    let (status, updated) = post(
        &store,
        Some("chef"),
        &format!("/api/admin/principals/{}/active", person.id),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{updated}");
    assert_eq!(updated["active"], false);

    // Deactivated accounts stay listed: "cannot sign in" must not look like "was never
    // here", or somebody creates a duplicate.
    let (_, listed) = get(&store, Some("chef"), "/api/admin/principals").await;
    let neu = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["username"] == "neu")
        .expect("a deactivated account must still be listed");
    assert_eq!(neu["active"], false);
}

#[tokio::test]
async fn a_weak_password_is_refused_and_creates_nothing() {
    let store = fixture().await;
    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/principals",
        json!({"username": "schwach", "display_name": "Schwach", "password": "kurz"}),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{response}");
    assert!(store
        .principal_by_username("schwach")
        .await
        .unwrap()
        .is_none());
    assert!(
        audit(&store).await.is_empty(),
        "a rejected creation wrote an audit row"
    );
}

#[tokio::test]
async fn a_duplicate_username_is_refused() {
    let store = fixture().await;
    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/principals",
        json!({"username": "gast", "display_name": "Zweiter Gast", "password": PASSPHRASE}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
}

#[tokio::test]
async fn activating_an_account_that_does_not_exist_is_not_a_success() {
    let store = fixture().await;
    let (status, _) = post(
        &store,
        Some("chef"),
        "/api/admin/principals/gibt-es-nicht/active",
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(audit(&store).await.is_empty());

    // The same request against an id that does exist, so this cannot pass merely because
    // the route is absent — a missing route answers 404 too.
    let (status, _) = post(
        &store,
        Some("chef"),
        &format!(
            "/api/admin/principals/{}/active",
            id_of(&store, "gast").await
        ),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

// -------------------------------------------------------------------------------------
// Teams.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn creating_a_team_writes_one_audit_row_naming_it() {
    let store = fixture().await;
    let (status, created) = post(
        &store,
        Some("chef"),
        "/api/admin/teams",
        json!({"slug": "reviewers", "name": "Gegenlesen"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let entries = audit(&store).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    let entry = &entries[0];
    assert_eq!(entry.action, "team.create");
    assert_eq!(entry.target.as_deref(), Some("reviewers"));
    assert_eq!(
        entry.path, None,
        "creating a team belongs to no subtree, so it is instance-wide (0004)"
    );
    assert_eq!(
        entry.principal_id.as_deref(),
        Some(id_of(&store, "chef").await.as_str()),
        "the audit row must name who did it"
    );

    let (_, teams) = get(&store, Some("chef"), "/api/admin/teams").await;
    let slugs: Vec<&str> = teams
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["slug"].as_str().unwrap())
        .collect();
    assert_eq!(slugs, vec!["gaeste", "reviewers"]);
}

#[tokio::test]
async fn adding_a_member_to_a_team_that_does_not_exist_is_not_a_success() {
    // `add_team_member` selects the team id by slug, so a typo inserts no rows. Reporting
    // 200 would tell an administrator that access was given when it was not.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/teams/tippfehler/members",
        json!({"principal_id": gast}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{response}");
    assert!(
        audit(&store).await.is_empty(),
        "a membership that was never written was recorded anyway"
    );

    let (loaded, _) = store.principal_by_username("gast").await.unwrap().unwrap();
    assert!(loaded.teams.is_empty());

    // The same request against a team that does exist, so the 404 above cannot be the
    // router's 404 for a route nobody registered.
    let (status, _) = post(
        &store,
        Some("chef"),
        "/api/admin/teams/gaeste/members",
        json!({"principal_id": gast}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn adding_a_member_who_does_not_exist_is_not_a_success() {
    let store = fixture().await;
    let (status, _) = post(
        &store,
        Some("chef"),
        "/api/admin/teams/gaeste/members",
        json!({"principal_id": "gibt-es-nicht"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(audit(&store).await.is_empty());

    // And the same request with an id that exists, so the 404 is about the principal and
    // not about the route.
    let (status, _) = post(
        &store,
        Some("chef"),
        "/api/admin/teams/gaeste/members",
        json!({"principal_id": id_of(&store, "gast").await}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn memberships_are_added_removed_and_audited() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/teams/gaeste/members",
        json!({"principal_id": gast}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);

    let (_, teams) = get(&store, Some("chef"), "/api/admin/teams").await;
    assert_eq!(teams[0]["members"][0], gast.as_str());

    // Adding twice is idempotent: the state is what was asked for, and nothing changed,
    // so nothing is recorded.
    let before = audit(&store).await.len();
    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/teams/gaeste/members",
        json!({"principal_id": gast}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["changed"], false);
    assert_eq!(audit(&store).await.len(), before);

    let (status, response) = delete(
        &store,
        Some("chef"),
        &format!("/api/admin/teams/gaeste/members/{gast}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);

    // Removing what is not there is NOT idempotent success: after a removal that removed
    // nothing, access may well still be in place, which is the opposite of what the
    // administrator concluded.
    let (status, _) = delete(
        &store,
        Some("chef"),
        &format!("/api/admin/teams/gaeste/members/{gast}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let entries = audit(&store).await;
    let actions: Vec<&str> = entries.iter().map(|e| e.action.as_str()).collect();
    assert!(actions.contains(&"team.member.add"), "{actions:?}");
    assert!(actions.contains(&"team.member.remove"), "{actions:?}");
    assert!(
        entries.iter().all(|e| e.path.is_none()),
        "membership belongs to no subtree, so it is instance-wide (0004): {entries:?}"
    );
    assert_eq!(
        entries
            .iter()
            .find(|e| e.action == "team.member.add")
            .and_then(|e| e.target.as_deref()),
        Some("gaeste")
    );
}

// -------------------------------------------------------------------------------------
// Access control.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_space_admin_manages_grants_on_their_own_subtree() {
    let store = fixture().await;
    let lektor = id_of(&store, "lektor").await;

    let (status, view) = get(&store, Some("lektor"), "/api/admin/acl?path=/raum").await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["path"], "/raum");
    assert_eq!(view["inherited_from"], "/raum");
    assert_eq!(view["effective"].as_array().unwrap().len(), 2);
    assert_eq!(view["defined_here"].as_array().unwrap().len(), 2);

    // A descendant with no grants of its own: the same grants, and the console must be
    // able to say they come from somewhere else.
    let (status, view) = get(
        &store,
        Some("lektor"),
        "/api/admin/acl?path=/raum/unterseite",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["inherited_from"], "/raum");
    assert_eq!(view["effective"].as_array().unwrap().len(), 2);
    assert!(view["defined_here"].as_array().unwrap().is_empty());

    // And they may grant on that descendant, because the admin grant inherits down.
    //
    // Their own admin grant first, deliberately: the first grant written on a path
    // replaces everything it inherited, so writing somebody else's grant first would end
    // this space admin's reach over the page they were configuring. That is what
    // `the_first_grant_on_a_path_replaces_what_it_inherited` is about.
    let (status, response) = post(
        &store,
        Some("lektor"),
        "/api/admin/acl",
        json!({
            "path": "/raum/unterseite",
            "subject": {"kind": "principal", "id": lektor},
            "permission": "admin",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);

    let entries = audit(&store).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].action, "acl.grant");
    assert_eq!(entries[0].target.as_deref(), Some("/raum/unterseite"));
    assert_eq!(
        entries[0].path.as_deref(),
        Some("/raum/unterseite"),
        "a grant concerns the subtree it is written on, not the whole instance"
    );
    assert_eq!(entries[0].principal_id.as_deref(), Some(lektor.as_str()));

    // Granting the same thing twice is idempotent and records nothing the second time.
    let (status, response) = post(
        &store,
        Some("lektor"),
        "/api/admin/acl",
        json!({
            "path": "/raum/unterseite",
            "subject": {"kind": "principal", "id": lektor},
            "permission": "admin",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["changed"], false);
    assert_eq!(audit(&store).await.len(), 1);

    // The page now carries its own grants, and the view says so.
    let (_, view) = get(
        &store,
        Some("lektor"),
        "/api/admin/acl?path=/raum/unterseite",
    )
    .await;
    assert_eq!(view["inherited_from"], "/raum/unterseite");
    assert_eq!(view["defined_here"].as_array().unwrap().len(), 1);

    // Revoking what is defined here works, and is recorded.
    let (status, response) = delete(
        &store,
        Some("lektor"),
        "/api/admin/acl",
        Some(json!({
            "path": "/raum/unterseite",
            "subject": {"kind": "principal", "id": lektor},
            "permission": "admin",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);
    assert!(store
        .grants_defined_at("/raum/unterseite")
        .await
        .unwrap()
        .is_empty());

    let entries = audit(&store).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert!(entries.iter().any(|e| e.action == "acl.revoke"));
}

#[tokio::test]
async fn the_first_grant_on_a_path_replaces_what_it_inherited() {
    // A consequence of the inheritance model, pinned here because it is surprising and
    // because the console has to be built around it.
    //
    // The nearest ancestor carrying any grants wins OUTRIGHT — that is what makes it
    // possible to narrow a subtree rather than only ever widen it. So the first grant
    // written on a page that had none is not additive: it replaces everything that page
    // inherited, including the grant the author is administering it through. Writing
    // "let the guests read this page" therefore hands the page to the guests and takes it
    // away from you, in one request.
    //
    // It fails closed — an administrator loses reach, nobody gains any — and the fix is
    // ordering: write your own grant first, as
    // `a_space_admin_manages_grants_on_their_own_subtree` does. Recorded as a test rather
    // than left for somebody to discover on a live wiki.
    let store = fixture().await;
    let grant = json!({
        "path": "/raum/unterseite",
        "subject": {"kind": "team", "id": "gaeste"},
        "permission": "read",
    });

    let (status, _) = post(&store, Some("lektor"), "/api/admin/acl", grant.clone()).await;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(
        get(
            &store,
            Some("lektor"),
            "/api/admin/acl?path=/raum/unterseite"
        )
        .await
        .0,
        StatusCode::FORBIDDEN,
        "the grant that replaced the inherited set did not carry the author with it"
    );
    // The space itself is untouched: only the page they wrote on changed.
    assert_eq!(
        get(&store, Some("lektor"), "/api/admin/acl?path=/raum")
            .await
            .0,
        StatusCode::OK
    );
    // And an instance admin can always undo it.
    assert_eq!(
        delete(&store, Some("chef"), "/api/admin/acl", Some(grant))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        get(
            &store,
            Some("lektor"),
            "/api/admin/acl?path=/raum/unterseite"
        )
        .await
        .0,
        StatusCode::OK,
        "removing the narrowing grant restores what the page inherited"
    );
}

#[tokio::test]
async fn adding_a_grant_to_a_path_that_already_has_some_is_additive() {
    // The other half, so the trap above is not mistaken for "every grant is destructive".
    // `/raum` already carries grants, so writing another one adds to them and the space
    // admin keeps administering it.
    let store = fixture().await;
    let (status, _) = post(
        &store,
        Some("lektor"),
        "/api/admin/acl",
        json!({
            "path": "/raum",
            "subject": {"kind": "team", "id": "gaeste"},
            "permission": "read",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, view) = get(&store, Some("lektor"), "/api/admin/acl?path=/raum").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(view["defined_here"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn a_space_admin_is_refused_a_space_they_do_not_administer() {
    let store = fixture().await;
    let grant = json!({
        "path": "/anderer-raum",
        "subject": {"kind": "team", "id": "gaeste"},
        "permission": "read",
    });

    assert_eq!(
        get(&store, Some("lektor"), "/api/admin/acl?path=/anderer-raum")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&store, Some("lektor"), "/api/admin/acl", grant.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        delete(&store, Some("lektor"), "/api/admin/acl", Some(grant))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    assert!(
        store
            .grants_defined_at("/anderer-raum")
            .await
            .unwrap()
            .is_empty(),
        "a refused grant was written anyway"
    );
    assert!(audit(&store).await.is_empty());
}

#[tokio::test]
async fn a_neighbouring_path_does_not_inherit_by_string_prefix() {
    // `/raum-anderes` starts with `/raum`. Inheritance walks path SEGMENTS, and a prefix
    // match here would hand a whole unrelated space to whoever administers `/raum`.
    let store = fixture().await;
    assert_eq!(
        get(&store, Some("lektor"), "/api/admin/acl?path=/raum-anderes")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn revoking_an_inherited_grant_is_refused_rather_than_reported_as_done() {
    // The grant lives on `/raum`. Answering 200 here would tell an administrator that
    // access had been withdrawn from `/raum/unterseite` while it is still in force.
    let store = fixture().await;
    let lektor = id_of(&store, "lektor").await;

    let (status, response) = delete(
        &store,
        Some("lektor"),
        "/api/admin/acl",
        Some(json!({
            "path": "/raum/unterseite",
            "subject": {"kind": "principal", "id": lektor},
            "permission": "admin",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{response}");

    assert_eq!(
        store.grants_defined_at("/raum").await.unwrap().len(),
        2,
        "the inherited grant must still be exactly where it was"
    );
    assert!(
        audit(&store).await.is_empty(),
        "a revoke that removed nothing was recorded as if it had"
    );

    // The identical revoke at the path the grant is actually defined on succeeds, so the
    // 404 above is about where the grant lives and not about a route that does not exist.
    let (status, _) = delete(
        &store,
        Some("lektor"),
        "/api/admin/acl",
        Some(json!({
            "path": "/raum",
            "subject": {"kind": "principal", "id": lektor},
            "permission": "admin",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn an_instance_admin_grants_on_a_path_that_has_no_grants_yet() {
    // Bootstrapping. Nobody holds `admin` on `/anderer-raum`, so if the path gate looked
    // only at grants there would be no way to write the first one.
    let store = fixture().await;
    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/acl",
        json!({
            "path": "/anderer-raum",
            "subject": {"kind": "team", "id": "gaeste"},
            "permission": "read",
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        store
            .grants_defined_at("/anderer-raum")
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_grant_on_a_path_nothing_could_inherit_from_is_refused() {
    // Inheritance walks path segments upward and stops below the root, so each of these
    // would be stored and then match no document ever. They fail closed — nobody gains
    // anything — which is exactly why it has to be refused loudly: the administrator
    // reads 200, believes the handbook is shared, and finds out months later.
    // The message is asserted, not just the status. All three are 400 whichever rule
    // catches them, so the status alone cannot tell whether the administrator was told
    // something they can act on — and "path must not end with `/`" is useless advice for
    // `/`, where removing the slash leaves nothing at all.
    let store = fixture().await;
    for (path, expected) in [
        ("raum", "start with"),
        ("/", "root"),
        ("/raum/", "end with"),
    ] {
        let (status, response) = post(
            &store,
            Some("chef"),
            "/api/admin/acl",
            json!({
                "path": path,
                "subject": {"kind": "team", "id": "gaeste"},
                "permission": "read",
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "`{path}`: {response}");
        assert!(
            response["error"]
                .as_str()
                .unwrap_or_default()
                .contains(expected),
            "`{path}` was refused with advice that does not apply to it: {response}"
        );
        assert!(store.grants_defined_at(path).await.unwrap().is_empty());
    }
    assert!(audit(&store).await.is_empty());

    // But one already stored on such a path stays removable — a validator on the revoke
    // would trap it there for ever.
    store
        .add_grant("/", Subject::Team("gaeste".into()), Permission::Read)
        .await
        .unwrap();
    let (status, _) = delete(
        &store,
        Some("chef"),
        "/api/admin/acl",
        Some(json!({
            "path": "/",
            "subject": {"kind": "team", "id": "gaeste"},
            "permission": "read",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(store.grants_defined_at("/").await.unwrap().is_empty());
}

#[tokio::test]
async fn a_malformed_path_is_still_refused_before_it_is_diagnosed() {
    // Rule: a caller who may not touch this path learns 403 and nothing else. The order
    // of the two checks is what decides that, and it is easy to write the other way round.
    let store = fixture().await;
    let body = json!({
        "path": "/",
        "subject": {"kind": "team", "id": "gaeste"},
        "permission": "read",
    });
    assert_eq!(
        post(&store, None, "/api/admin/acl", body.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&store, Some("gast"), "/api/admin/acl", body).await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn every_subject_kind_survives_the_wire() {
    // The JSON spelling of a `Subject` is part of the API. `anyone` and `authenticated`
    // carry no id, and a serialisation that quietly dropped one of them would turn a
    // narrow grant into a different one.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    for subject in [
        json!({"kind": "principal", "id": gast}),
        json!({"kind": "team", "id": "gaeste"}),
        json!({"kind": "group", "id": "users"}),
        json!({"kind": "anyone"}),
        json!({"kind": "authenticated"}),
    ] {
        let (status, response) = post(
            &store,
            Some("chef"),
            "/api/admin/acl",
            json!({"path": "/wire", "subject": subject, "permission": "comment"}),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{subject}: {response}");
        assert_eq!(response["changed"], true, "{subject}");
    }

    let (_, view) = get(&store, Some("chef"), "/api/admin/acl?path=/wire").await;
    let kinds: Vec<&str> = view["defined_here"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| g["subject"]["kind"].as_str().unwrap())
        .collect();
    for expected in ["principal", "team", "group", "anyone", "authenticated"] {
        assert!(
            kinds.contains(&expected),
            "{expected} missing from {kinds:?}"
        );
    }
}

#[tokio::test]
async fn the_grant_index_lists_every_path_that_carries_one() {
    let store = fixture().await;
    let (status, index) = get(&store, Some("chef"), "/api/admin/acl").await;
    assert_eq!(status, StatusCode::OK, "{index}");
    assert_eq!(index["paths"][0]["path"], "/raum");
    assert_eq!(index["paths"][0]["grants"], 2);
}

// -------------------------------------------------------------------------------------
// The audit log.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_space_admin_reads_their_own_subtree_of_the_audit_log_and_no_more() {
    let store = fixture().await;

    // One instance-wide row and one scoped to `/raum`, both written through the API.
    post(
        &store,
        Some("chef"),
        "/api/admin/teams",
        json!({"slug": "reviewers", "name": "Gegenlesen"}),
    )
    .await;
    post(
        &store,
        Some("chef"),
        "/api/admin/acl",
        json!({
            "path": "/raum",
            "subject": {"kind": "team", "id": "reviewers"},
            "permission": "read",
        }),
    )
    .await;

    let (status, page) = get(&store, Some("lektor"), "/api/admin/audit").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let actions: Vec<&str> = page["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        vec!["acl.grant"],
        "a space admin must see their subtree and not the instance-wide rows"
    );

    let (status, page) = get(&store, Some("chef"), "/api/admin/audit").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);
    assert_eq!(page["truncated"], false);
}

#[tokio::test]
async fn the_audit_limit_is_honoured_and_bounded() {
    let store = fixture().await;
    for slug in ["a", "b", "c"] {
        post(
            &store,
            Some("chef"),
            "/api/admin/teams",
            json!({"slug": slug, "name": slug}),
        )
        .await;
    }

    let (_, page) = get(&store, Some("chef"), "/api/admin/audit?limit=2").await;
    assert_eq!(page["entries"].as_array().unwrap().len(), 2);

    // A nonsensical limit must not become "everything" or an error.
    let (status, page) = get(&store, Some("chef"), "/api/admin/audit?limit=0").await;
    assert_eq!(status, StatusCode::OK, "{page}");
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn no_audit_row_ever_carries_a_password() {
    let store = fixture().await;
    post(
        &store,
        Some("chef"),
        "/api/admin/principals",
        json!({"username": "neu", "display_name": "Neu", "password": PASSPHRASE}),
    )
    .await;

    let (_, page) = get(&store, Some("chef"), "/api/admin/audit").await;
    let text = page.to_string();
    assert!(!text.contains(PASSPHRASE), "{text}");
    assert!(!text.contains("argon2"), "{text}");
    assert!(
        text.contains("principal.create"),
        "the row that must not carry it must nonetheless exist: {text}"
    );
}

#[tokio::test]
async fn every_mutating_endpoint_writes_exactly_one_audit_row() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    for (method, uri, body) in mutations(&gast) {
        let before = audit(&store).await.len();
        let (status, response) = send(&store, Some("chef"), method.clone(), &uri, body).await;
        assert!(
            status.is_success(),
            "{method} {uri} answered {status}: {response}"
        );
        let after = audit(&store).await.len();
        assert_eq!(
            after,
            before + 1,
            "{method} {uri} wrote {} audit rows, not one",
            after - before
        );
    }
}

#[tokio::test]
async fn an_administrator_cannot_create_an_account_with_a_breached_password() {
    // The password is long enough, so the length floor alone would let it through. The
    // only thing that can refuse it is the corpus actually being consulted.
    let store = fixture().await;
    let (principal, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    let mut state = gw_api::AppState::for_test_principal(Arc::clone(&store), &principal);
    state.corpus = Arc::new(BreachedCorpus);
    let app = gw_api::build_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/admin/principals")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "username": "geleakt",
                        "display_name": "Geleakt",
                        "password": "password"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "a breached password was accepted, so the corpus is not being consulted"
    );

    assert!(
        store
            .principal_by_username("geleakt")
            .await
            .unwrap()
            .is_none(),
        "a refused password still created an account"
    );
}
