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
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{AuditEntry, Author, NewDocument, Store, SESSION_TTL_SECONDS};
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

    // Two real DOCUMENTS, because visibility is a property of a document and not of a
    // path. Everything else in this file works on the `acl` table alone, which happily
    // holds grants for paths nothing lives at; `/api/admin/visibility` cannot, and a
    // fixture with no documents would let its 404 branch pass for the wrong reason.
    page(&store, None, "raum", "Raum", Visibility::Restricted).await;
    page(
        &store,
        Some("/raum"),
        "unterseite",
        "Unterseite",
        Visibility::Restricted,
    )
    .await;

    // `/anderer-raum` deliberately carries no grants at all: it is the path a space admin
    // must be refused on.
    Arc::new(store)
}

/// A document at a known path. The slug is given explicitly rather than derived from the
/// title, because these paths are asserted against by name.
async fn page(
    store: &Store,
    parent: Option<&str>,
    slug: &str,
    title: &str,
    visibility: Visibility,
) {
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap();
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: parent.map(str::to_string),
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility,
                body,
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();
}

/// The stored visibility of a document, read straight from the store.
async fn visibility_of(store: &Arc<Store>, path: &str) -> String {
    let (chef, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    store
        .document_for(&chef, path, gw_auth::Action::Read)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("`{path}` must exist in the fixture"))
        .visibility
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

/// A password long enough to clear [`gw_auth::password::MIN_PASSWORD_LENGTH`], so that the
/// only thing left that can refuse it is the corpus.
///
/// This is load-bearing and was got wrong here once: the test below submitted `"password"`,
/// which is eight characters against a floor of twelve, so it was refused by the length
/// check and passed whether or not the corpus was ever consulted — the exact vacuity the
/// stub underneath exists to rule out.
const BREACHED_BUT_LONG: &str = "geleaktespasswort";

/// A corpus that reports exactly one password as heavily breached, and it is the one the
/// test submits.
///
/// Exists so one test can prove the breach check is actually CONSULTED by admin account
/// creation. That endpoint originally called the length-only validator directly, so
/// accounts created by an administrator skipped the corpus entirely while the policy
/// looked implemented — a defect no test could see, because every test password was
/// long enough and none of them asked whether the corpus was reached at all.
///
/// The answer is computed from the password rather than written out as a constant. A
/// hard-coded digest is a second place the password lives, and when this one drifted from
/// the other the corpus answered about a password nobody was submitting — which reads as
/// "not breached" and lets the account through.
struct BreachedCorpus;

impl gw_auth::breach::BreachRange for BreachedCorpus {
    fn fetch<'a>(&'a self, _prefix: &'a str) -> gw_auth::breach::BreachFuture<'a> {
        // The range API answers `SUFFIX:COUNT` lines, the suffix being everything after
        // the five-character prefix that was queried.
        Box::pin(async {
            Ok(format!(
                "{}:9545824\r\n",
                &gw_auth::breach::sha1_hex(BREACHED_BUT_LONG)[5..]
            ))
        })
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
            format!("/api/admin/principals/{principal_id}/instance-admin"),
            Some(json!({"admin": true})),
        ),
        // Before the deactivation below, and that ordering is load-bearing: a deactivated
        // account cannot be viewed as, so this would answer 400 and the loop would read it
        // as a missing audit row rather than as the refusal it is.
        (
            Method::POST,
            format!("/api/admin/view-as/{principal_id}"),
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
        // An invite carrying a path grant is bounded by that path, so it is gated by it —
        // the same door as the grant above, which is why it belongs in this list and not
        // the instance-wide one. An invite carrying a TEAM is instance-only and is covered
        // by tests/invites.rs, where the distinction is the point.
        (
            Method::POST,
            "/api/admin/invites".into(),
            Some(json!({
                "username": "eingeladen",
                "path": "/raum",
                "permission": "read",
            })),
        ),
        // How open a page is belongs to the page, so it is gated by the page's own path —
        // the same door as the grant above. `internal` rather than `public` only because
        // the fixture page starts `restricted` and this list needs each entry to actually
        // change something; which value is written is not what this list is about.
        (
            Method::POST,
            "/api/admin/visibility".into(),
            Some(json!({"path": "/raum", "visibility": "internal"})),
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
        // Who could be promoted names every active account. Instance-wide by the same
        // argument as listing principals.
        (Method::GET, "/api/admin/admins/candidates".into(), None),
        // The group-to-baseline mapping is instance-wide configuration: it decides reach
        // over the WHOLE corpus, not over any one subtree.
        (Method::GET, "/api/admin/roles".into(), None),
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
    // And for publishing. This is the worst of the three if it were open: `visibility:
    // public` puts the page on the open internet, and `Anyone: admin` is exactly the
    // grant an anonymous caller passes `can()` with.
    assert_eq!(
        post(
            &store,
            None,
            "/api/admin/visibility",
            json!({"path": "/offen", "visibility": "public"}),
        )
        .await
        .0,
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
// The administrative safety interlock.
//
// An instance admin who deactivates the last active administrator locks everybody out of
// administration for good: there is no in-app recovery, and great-wiki never writes
// Authelia's user database (ADR 0002), so there is nothing to fix it from either. Every
// test below is about the floor of one, and about the per-account promotion that exists
// so the floor can be raised without touching Authelia.
// -------------------------------------------------------------------------------------

/// Promote or demote through the API, as `chef`.
async fn set_instance_admin(
    store: &Arc<Store>,
    who: &str,
    id: &str,
    admin: bool,
) -> (StatusCode, Value) {
    post(
        store,
        Some(who),
        &format!("/api/admin/principals/{id}/instance-admin"),
        json!({ "admin": admin }),
    )
    .await
}

#[tokio::test]
async fn the_last_instance_admin_cannot_deactivate_themselves() {
    // `chef` is the fixture's only administrator, by the `admins` group. Answering 200
    // here is the bug this interlock exists for: the account is suspended, its sessions
    // are swept, and nobody can ever reach the admin console again.
    let store = fixture().await;
    let chef = id_of(&store, "chef").await;
    store
        .create_session(&chef, "chef-digest", SESSION_TTL_SECONDS)
        .await
        .unwrap();

    let (status, response) = post(
        &store,
        Some("chef"),
        &format!("/api/admin/principals/{chef}/active"),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");

    let (loaded, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    assert!(loaded.active, "a refused deactivation took effect anyway");
    // The session sweep happens in the same transaction as the flag, so a refusal that
    // only rolled back the UPDATE would still have signed the last administrator out.
    assert_eq!(
        store.session_count_for(&chef).await.unwrap(),
        1,
        "a refused deactivation still ended the administrator's session"
    );
    assert_eq!(
        get(&store, Some("chef"), "/api/admin/principals").await.0,
        StatusCode::OK,
        "the administrator lost the console to a request that was refused"
    );
    assert!(
        audit(&store).await.is_empty(),
        "a deactivation that did not happen was recorded as if it had"
    );
}

#[tokio::test]
async fn an_admin_may_be_deactivated_while_another_active_admin_remains() {
    // The other half of the rule, without which the interlock would simply be "admins
    // cannot be deactivated". `gast` holds no groups at all, so the per-account promotion
    // is the only thing that can make them count.
    let store = fixture().await;
    let chef = id_of(&store, "chef").await;
    let gast = id_of(&store, "gast").await;

    let (status, response) = set_instance_admin(&store, "chef", &gast, true).await;
    assert_eq!(status, StatusCode::OK, "{response}");

    let (status, response) = post(
        &store,
        Some("chef"),
        &format!("/api/admin/principals/{chef}/active"),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["active"], false);

    // `gast` is now the last one, and the same request is refused for them.
    let (status, response) = post(
        &store,
        Some("gast"),
        &format!("/api/admin/principals/{gast}/active"),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
}

#[tokio::test]
async fn a_promoted_account_administers_the_instance_with_no_groups_at_all() {
    // The fallback D-M2-1 does not provide: `gast` is a local account, so there is no
    // Authelia group that could ever confer this, and great-wiki must not write one.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    assert_eq!(
        get(&store, Some("gast"), "/api/admin/principals").await.0,
        StatusCode::FORBIDDEN,
        "the fixture's guest must start with nothing, or this test proves nothing"
    );

    let (status, response) = set_instance_admin(&store, "chef", &gast, true).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);
    assert_eq!(response["instance_admin"], true);

    let (status, listed) = get(&store, Some("gast"), "/api/admin/principals").await;
    assert_eq!(status, StatusCode::OK, "{listed}");

    // And it is reversible.
    let (status, response) = set_instance_admin(&store, "chef", &gast, false).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);
    assert_eq!(response["instance_admin"], false);
    assert_eq!(
        get(&store, Some("gast"), "/api/admin/principals").await.0,
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn promoting_one_account_promotes_nobody_else() {
    // The owner's constraint, checked rather than assumed: the promotion names one
    // principal, so it cannot have a side effect on the other members of any group or
    // team. A `group_roles` row would have exactly that side effect, which is why this is
    // not implemented as one.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    // Everybody here shares a team with the promoted account, so a promotion that leaked
    // through team membership would show up.
    for username in ["gast", "lektor", "leser"] {
        let id = id_of(&store, username).await;
        post(
            &store,
            Some("chef"),
            "/api/admin/teams/gaeste/members",
            json!({"principal_id": id}),
        )
        .await;
    }

    set_instance_admin(&store, "chef", &gast, true).await;

    for username in ["lektor", "leser"] {
        assert_eq!(
            get(&store, Some(username), "/api/admin/principals").await.0,
            StatusCode::FORBIDDEN,
            "promoting `gast` promoted `{username}` as well"
        );
    }
}

#[tokio::test]
async fn demoting_the_last_instance_admin_is_refused() {
    // Demotion is the other way to reach zero, and it must be refused by the same rule
    // rather than by a second one written next to it.
    let store = fixture().await;
    let chef = id_of(&store, "chef").await;
    let gast = id_of(&store, "gast").await;

    // Asserted, not assumed. `set_instance_admin` returns a status this test previously
    // discarded, so a promotion that failed left `gast` un-promoted and every assertion
    // below measured the wrong situation.
    let (promoted, body) = set_instance_admin(&store, "chef", &gast, true).await;
    assert_eq!(promoted, StatusCode::OK, "promoting gast failed: {body}");

    let (status, body) = post(
        &store,
        Some("chef"),
        &format!("/api/admin/principals/{chef}/active"),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, response) = set_instance_admin(&store, "gast", &gast, false).await;
    assert_eq!(status, StatusCode::CONFLICT, "{response}");
    assert_eq!(
        get(&store, Some("gast"), "/api/admin/principals").await.0,
        StatusCode::OK,
        "a refused demotion took effect anyway"
    );
}

#[tokio::test]
async fn a_deactivated_instance_admin_does_not_count_toward_the_floor() {
    // The floor is ACTIVE administrators. A suspended account cannot sign in, so counting
    // it would let the last usable administrator suspend themselves next to a row that
    // looks like a second one.
    let store = fixture().await;
    let chef = id_of(&store, "chef").await;
    let gast = id_of(&store, "gast").await;

    set_instance_admin(&store, "chef", &gast, true).await;
    let (status, _) = post(
        &store,
        Some("chef"),
        &format!("/api/admin/principals/{gast}/active"),
        json!({"active": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "deactivating the second admin");

    let (status, response) = post(
        &store,
        Some("chef"),
        &format!("/api/admin/principals/{chef}/active"),
        json!({"active": false}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a deactivated admin was counted as an administrator: {response}"
    );
}

#[tokio::test]
async fn the_candidates_are_active_non_admins_most_recently_active_first() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let leser = id_of(&store, "leser").await;
    let lektor = id_of(&store, "lektor").await;

    // Two more accounts, so the list has something to order: one Authelia account and one
    // local one, neither of which has ever been seen.
    store
        .upsert_oidc_principal("kollege", "Kollege", None, &["users".into()])
        .await
        .unwrap();
    post(
        &store,
        Some("chef"),
        "/api/admin/principals",
        json!({"username": "neu", "display_name": "Neu", "password": PASSPHRASE}),
    )
    .await;

    // Excluded: already an administrator by promotion, already one by Authelia group
    // (`chef`), and deactivated (`lektor`). Promoting any of them would add nobody.
    set_instance_admin(&store, "chef", &leser, true).await;
    store.set_principal_active(&lektor, false).await.unwrap();
    // `gast` has signed in; nobody else has.
    store
        .create_session(&gast, "gast-digest", SESSION_TTL_SECONDS)
        .await
        .unwrap();

    let (status, list) = get(&store, Some("chef"), "/api/admin/admins/candidates").await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let names: Vec<&str> = list
        .as_array()
        .expect("a list")
        .iter()
        .map(|c| c["username"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["gast", "kollege", "neu"],
        "candidates must exclude administrators and deactivated accounts, most recently \
         active first"
    );

    // The console shows where the account comes from and what it already carries, so the
    // person choosing a successor is not choosing from a list of bare usernames.
    let gast_entry = &list[0];
    assert_eq!(gast_entry["id"], gast.as_str());
    assert_eq!(gast_entry["kind"], "local");
    assert!(gast_entry["groups"].as_array().unwrap().is_empty());
    assert!(
        gast_entry["last_active_at"].as_str().is_some(),
        "a candidate with a session must report when they were last active: {gast_entry}"
    );

    let kollege = &list[1];
    assert_eq!(kollege["kind"], "oidc", "the Authelia accounts are marked");
    assert_eq!(kollege["groups"][0], "users");
    assert_eq!(
        kollege["last_active_at"],
        Value::Null,
        "an account with no session and no audit entry has no derived activity"
    );
}

#[tokio::test]
async fn a_caller_with_no_admin_baseline_is_refused_both_interlock_endpoints() {
    // Instance-wide, both of them: the candidate list names every active account, and the
    // promotion hands over the whole instance.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    for who in [None, Some("gast"), Some("lektor"), Some("leser")] {
        assert_eq!(
            get(&store, who, "/api/admin/admins/candidates").await.0,
            StatusCode::FORBIDDEN,
            "{who:?} read the candidate list"
        );
        let (status, _) = post(
            &store,
            who,
            &format!("/api/admin/principals/{gast}/instance-admin"),
            json!({"admin": true}),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{who:?} promoted an account");
    }

    assert_eq!(
        get(&store, Some("gast"), "/api/admin/principals").await.0,
        StatusCode::FORBIDDEN,
        "a refused promotion took effect anyway"
    );
    assert!(
        audit(&store).await.is_empty(),
        "a refused request wrote to the audit log"
    );
}

#[tokio::test]
async fn promotion_and_demotion_each_write_one_instance_wide_audit_row() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    set_instance_admin(&store, "chef", &gast, true).await;
    let entries = audit(&store).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].action, "principal.promote");
    assert_eq!(entries[0].target.as_deref(), Some(gast.as_str()));
    assert_eq!(
        entries[0].path, None,
        "administering the instance belongs to no subtree (0004)"
    );
    assert_eq!(
        entries[0].principal_id.as_deref(),
        Some(id_of(&store, "chef").await.as_str())
    );

    // Promoting somebody who is already promoted changes nothing, so it records nothing.
    let (status, response) = set_instance_admin(&store, "chef", &gast, true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(response["changed"], false);
    assert_eq!(audit(&store).await.len(), 1);

    let (status, response) = set_instance_admin(&store, "chef", &gast, false).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    let entries = audit(&store).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    let demotion = entries
        .iter()
        .find(|e| e.action == "principal.demote")
        .expect("the demotion must be recorded");
    assert_eq!(demotion.target.as_deref(), Some(gast.as_str()));
    assert_eq!(demotion.path, None);
}

#[tokio::test]
async fn promoting_an_account_that_does_not_exist_is_not_a_success() {
    let store = fixture().await;
    let (status, _) = set_instance_admin(&store, "chef", "gibt-es-nicht", true).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(audit(&store).await.is_empty());

    // The same request against an id that does exist, so the 404 cannot be the router's
    // 404 for a route nobody registered.
    let gast = id_of(&store, "gast").await;
    let (status, _) = set_instance_admin(&store, "chef", &gast, true).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn demoting_somebody_who_administers_by_group_says_they_still_do() {
    // The failure this endpoint could most plausibly hide. `chef` administers the
    // instance through Authelia's `admins` group, which great-wiki neither reads from nor
    // writes to here — so removing a promotion row they never had withdraws nothing. A
    // console told only "changed: false" would still be right; one told "done" would have
    // announced a revocation that did not happen.
    let store = fixture().await;
    let chef = id_of(&store, "chef").await;

    let (status, response) = set_instance_admin(&store, "chef", &chef, false).await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], false);
    assert_eq!(
        response["instance_admin"], true,
        "a demotion that withdrew nothing reported that it had"
    );
    assert_eq!(
        get(&store, Some("chef"), "/api/admin/principals").await.0,
        StatusCode::OK
    );
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

#[tokio::test]
async fn the_acl_view_names_what_would_apply_if_this_path_lost_its_own_grants() {
    // Revoking the LAST grant on a path is not a local change. `grants_for_path` returns
    // the rows of the nearest ancestor that has ANY, so removing the final row here makes
    // the ancestor's set apply again — here, and across every page below that has none of
    // its own. The console cannot warn about that without being told what would resume,
    // and it cannot work it out: `inherited_from` names this path itself as soon as this
    // path carries anything.
    let store = fixture().await;
    let lektor = id_of(&store, "lektor").await;

    // While the page has none of its own, what applies IS the ancestor's set, and there
    // is nothing further up.
    let (status, view) = get(
        &store,
        Some("lektor"),
        "/api/admin/acl?path=/raum/unterseite",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["ancestor_source"], "/raum");
    assert_eq!(view["ancestor_grants"].as_array().unwrap().len(), 2);

    // Once it carries its own, `inherited_from` is the page itself — and `ancestor_source`
    // is the only remaining evidence of what a revoke here would bring back.
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

    let (_, view) = get(
        &store,
        Some("lektor"),
        "/api/admin/acl?path=/raum/unterseite",
    )
    .await;
    assert_eq!(view["inherited_from"], "/raum/unterseite");
    assert_eq!(view["ancestor_source"], "/raum");
    assert_eq!(view["ancestor_grants"].as_array().unwrap().len(), 2);

    // A top-level path has nothing above it, and says so rather than naming itself.
    let (_, view) = get(&store, Some("chef"), "/api/admin/acl?path=/raum").await;
    assert_eq!(view["ancestor_source"], Value::Null);
    assert!(view["ancestor_grants"].as_array().unwrap().is_empty());
}

// -------------------------------------------------------------------------------------
// The group-to-baseline mapping: default reach over the WHOLE corpus.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn the_group_baseline_mapping_says_which_groups_reach_everything() {
    // The console's access panel claims to answer "who reaches this page". It cannot do
    // that honestly without this: a group mapped to `admin` reads every `restricted`
    // document in the corpus with no grant anywhere, and no row in the grants table ever
    // shows that. The 0002 migration says the mapping "has to be inspectable in the admin
    // console alongside everything else that decides who sees what" — this is that.
    let store = fixture().await;
    let (status, roles) = get(&store, Some("chef"), "/api/admin/roles").await;
    assert_eq!(status, StatusCode::OK, "{roles}");

    let rows = roles.as_array().expect("a list of mappings");
    let admins: Vec<&str> = rows
        .iter()
        .filter(|row| row["baseline"] == "admin")
        .map(|row| row["group"].as_str().unwrap())
        .collect();
    assert_eq!(admins, vec!["admins"]);

    let internal: Vec<&str> = rows
        .iter()
        .filter(|row| row["baseline"] == "internal")
        .map(|row| row["group"].as_str().unwrap())
        .collect();
    assert_eq!(internal, vec!["users"]);
}

// -------------------------------------------------------------------------------------
// Visibility: how open a page is, and who gets to decide.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn reading_a_page_is_not_being_allowed_to_publish_it() {
    // The one refusal that matters most on this endpoint. `leser` holds `read` on `/raum`
    // and nothing else; `public` puts the page on the open internet. Read is never a way
    // to widen anything.
    let store = fixture().await;

    let (status, response) = post(
        &store,
        Some("leser"),
        "/api/admin/visibility",
        json!({"path": "/raum", "visibility": "public"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
    assert_eq!(visibility_of(&store, "/raum").await, "restricted");
    assert!(audit(&store).await.is_empty());

    // Nor does writing. An editor edits the page; how open the page is, is not the page.
    let gast = id_of(&store, "gast").await;
    store
        .add_grant("/raum", Subject::Principal(gast.clone()), Permission::Write)
        .await
        .unwrap();
    let (status, response) = post(
        &store,
        Some("gast"),
        "/api/admin/visibility",
        json!({"path": "/raum", "visibility": "public"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
    assert_eq!(visibility_of(&store, "/raum").await, "restricted");
}

#[tokio::test]
async fn a_space_admin_changes_the_visibility_of_a_page_in_their_own_subtree() {
    // The same gate as a grant, deliberately: somebody who administers `/raum` can
    // already publish it to the open internet by writing `anyone: read` there, so a
    // stricter gate here would not withhold the power — it would only push the act onto
    // the mechanism this console shows LESS clearly.
    let store = fixture().await;

    let (status, response) = post(
        &store,
        Some("lektor"),
        "/api/admin/visibility",
        json!({"path": "/raum/unterseite", "visibility": "internal"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], true);
    assert_eq!(visibility_of(&store, "/raum/unterseite").await, "internal");

    // And the page above is untouched: visibility is a property of ONE document. Unlike a
    // grant, it does not reach down the tree at all.
    assert_eq!(visibility_of(&store, "/raum").await, "restricted");
}

#[tokio::test]
async fn a_space_admin_cannot_publish_a_page_outside_their_own_subtree() {
    let store = fixture().await;
    page(
        &store,
        None,
        "anderer-raum",
        "Anderer",
        Visibility::Restricted,
    )
    .await;

    let (status, response) = post(
        &store,
        Some("lektor"),
        "/api/admin/visibility",
        json!({"path": "/anderer-raum", "visibility": "public"}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{response}");
    assert_eq!(visibility_of(&store, "/anderer-raum").await, "restricted");
}

#[tokio::test]
async fn publishing_a_page_records_what_it_was_as_well_as_what_it_became() {
    // "Somebody made this public" is worth nothing six months later without "and before
    // that it was restricted". The row is scoped to the page, so the space admin who did
    // it can read it back in their own log.
    let store = fixture().await;
    let chef = id_of(&store, "chef").await;

    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/visibility",
        json!({"path": "/raum", "visibility": "public"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");

    let entries = audit(&store).await;
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0].action, "document.visibility");
    assert_eq!(entries[0].target.as_deref(), Some("/raum"));
    assert_eq!(
        entries[0].path.as_deref(),
        Some("/raum"),
        "publishing one page concerns that page's subtree, not the whole instance"
    );
    assert_eq!(entries[0].principal_id.as_deref(), Some(chef.as_str()));

    let detail: Value = serde_json::from_str(&entries[0].detail).unwrap();
    assert_eq!(detail["from"], "restricted");
    assert_eq!(detail["to"], "public");
}

#[tokio::test]
async fn a_visibility_this_code_does_not_understand_is_refused_rather_than_guessed() {
    // Fail closed. An unrecognised value must never be written, and must never fall back
    // to a default — `Visibility::default()` is `Restricted`, which is the safe direction
    // here, but a handler that silently substitutes a default is one rename away from
    // being the unsafe one.
    let store = fixture().await;

    for value in ["geheim", "", "öffentlich", "admin"] {
        let (status, response) = post(
            &store,
            Some("chef"),
            "/api/admin/visibility",
            json!({"path": "/raum", "visibility": value}),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "`{value}` was accepted: {response}"
        );
    }
    assert_eq!(visibility_of(&store, "/raum").await, "restricted");
    assert!(audit(&store).await.is_empty());
}

#[tokio::test]
async fn a_recognised_visibility_is_canonicalised_rather_than_stored_as_typed() {
    // `"PUBLIC "` IS accepted, and this test exists because the first version of the one
    // above asserted it was not. Refusing it would have meant a second, stricter parse in
    // the handler beside the one `Store::document_for_with_baseline` reads the column back
    // with — and two spellings of "what counts as public" is precisely the shape this
    // codebase keeps warning about. `Visibility::from_str` trims and lowercases; what is
    // stored is always the canonical string, which is what the CHECK constraint and every
    // later read expect.
    let store = fixture().await;

    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/visibility",
        json!({"path": "/raum", "visibility": "PUBLIC "}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(visibility_of(&store, "/raum").await, "public");

    let entries = audit(&store).await;
    assert_eq!(
        serde_json::from_str::<Value>(&entries[0].detail).unwrap()["to"],
        "public",
        "the audit row recorded what was typed rather than what was stored"
    );
}

#[tokio::test]
async fn setting_the_visibility_a_page_already_has_changes_nothing_and_records_nothing() {
    let store = fixture().await;

    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/visibility",
        json!({"path": "/raum", "visibility": "restricted"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(response["changed"], false);
    assert!(
        audit(&store).await.is_empty(),
        "a no-op wrote an audit row, which buries the real changes"
    );
}

#[tokio::test]
async fn changing_the_visibility_of_a_page_that_does_not_exist_is_not_a_success() {
    // A grant may be written on a path nothing lives at — that is deliberate, so access
    // can be prepared before a page arrives. Visibility cannot: there is no row to set.
    // Reporting 200 would tell an administrator they had published something.
    let store = fixture().await;

    let (status, response) = post(
        &store,
        Some("chef"),
        "/api/admin/visibility",
        json!({"path": "/gibt-es-nicht", "visibility": "public"}),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{response}");
    assert!(audit(&store).await.is_empty());
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
    // The password clears the length floor, so the floor alone would let it through and
    // the only thing left that can refuse it is the corpus actually being consulted. It
    // did NOT clear the floor until 2026-08-11, and so proved nothing.
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
                        "password": BREACHED_BUT_LONG
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

// -------------------------------------------------------------------------------------
// The list above promises to be exhaustive. This is what makes that true.
// -------------------------------------------------------------------------------------

/// `mutations()` carries the comment "one list, so a new endpoint cannot be added while
/// quietly escaping the audit-row test". That was a promise, not a mechanism, and on
/// 2026-08-11 it turned out to be false: invitations and view-as had added four mutating
/// admin routes and none of them was in the list. Nothing failed, because nothing was
/// checking.
///
/// So the route table is read from the source and compared against the list. A new
/// mutating route now has to appear in `mutations()` or be named in `AUDIT_EXEMPT` with a
/// reason — and the second is deliberately awkward to write.
const ADMIN_ROUTES: &str = include_str!("../src/routes/admin.rs");
const VIEW_AS_ROUTES: &str = include_str!("../src/view_as.rs");

/// Mutating routes that cannot be driven from a static list, each audited elsewhere.
///
/// Every entry needs a test named in its comment. "It is hard to test here" is a reason to
/// point at the test that does it, not a reason to skip it.
const AUDIT_EXEMPT: &[(&str, &str, &str)] = &[
    (
        "delete",
        "/api/admin/invites/{id}",
        "needs an invite id that only exists after a request, so it cannot be a constant. \
         Audited by tests/invites.rs, and the mutation `invites: revoking one is gated by \
         the scope of the invite itself`.",
    ),
    (
        "post",
        "/api/admin/view-as/exit",
        "writes its row only when a mode is actually active, and every request in the list \
         above goes through a freshly built router, so no mode can be. Audited by \
         tests/view_as.rs, and the mutation `view-as: leaving the mode is recorded too, so \
         the window has a known end`.",
    ),
];

/// Every (method, path) in the admin router that changes something, read from the source.
///
/// Source-level rather than by asking the router, because axum's `Router` does not expose
/// its table. The parse is deliberately dumb: it finds `.route(`, takes the first quoted
/// string as the path, and looks for a mutating verb before the next `.route(`.
fn declared_mutating_routes() -> Vec<(String, String)> {
    let mut found = Vec::new();
    for src in [ADMIN_ROUTES, VIEW_AS_ROUTES] {
        let mut rest = src;
        while let Some(at) = rest.find(".route(") {
            rest = &rest[at + ".route(".len()..];
            let block_end = rest.find(".route(").unwrap_or(rest.len());
            let block = &rest[..block_end];

            let path = route_path(block, src).unwrap_or_else(|| {
                panic!(
                    "a route's path could not be read, so it would have escaped this \
                     check silently — which is exactly the failure the check exists for. \
                     Write the path as a literal, or as a `const NAME: &str = \"…\"` in \
                     the same file:\n{}",
                    block.lines().take(3).collect::<Vec<_>>().join("\n")
                )
            });

            for verb in ["post", "delete", "put", "patch"] {
                if block.contains(&format!("{verb}(")) {
                    found.push((verb.to_string(), path.clone()));
                }
            }
        }
    }
    assert!(
        found.len() > 5,
        "the route parse found {} routes, so it has stopped working and this test is \
         no longer checking anything",
        found.len()
    );
    found
}

/// The path a `.route(` block registers, whether written as a literal or named by a const.
///
/// The const case is not a nicety. `view_as::routes()` registers its exit as
/// `.route(EXIT_PATH, post(exit))`, and an earlier version of this parser looked for a
/// quoted string, found none, and skipped it — so the one route that is deliberately
/// exempt from the view-as refusal was also, silently, exempt from this check. A parser
/// that quietly ignores what it cannot read is worse than no parser at all.
fn route_path(block: &str, src: &str) -> Option<String> {
    let head = block.trim_start();

    if let Some(rest) = head.strip_prefix('"') {
        return rest.find('"').map(|end| rest[..end].to_string());
    }

    let ident: String = head
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if ident.is_empty() {
        return None;
    }
    let declaration = format!("const {ident}: &str = \"");
    let at = src.find(&declaration)?;
    let value = &src[at + declaration.len()..];
    value.find('"').map(|end| value[..end].to_string())
}

/// Does a concrete request URI match a route pattern? `{id}` matches one segment.
fn matches_pattern(pattern: &str, uri: &str) -> bool {
    let uri = uri.split('?').next().unwrap_or(uri);
    let pattern: Vec<&str> = pattern.split('/').collect();
    let actual: Vec<&str> = uri.split('/').collect();
    pattern.len() == actual.len()
        && pattern
            .iter()
            .zip(actual.iter())
            .all(|(p, a)| p.starts_with('{') || p == a)
}

#[test]
fn every_mutating_admin_route_is_covered_by_the_audit_list_or_explicitly_exempt() {
    let listed = mutations("irrelevant-for-matching");

    let mut escaped = Vec::new();
    for (verb, path) in declared_mutating_routes() {
        let exempt = AUDIT_EXEMPT
            .iter()
            .any(|(v, p, _)| *v == verb && *p == path);
        let covered = listed.iter().any(|(method, uri, _)| {
            method.as_str().eq_ignore_ascii_case(&verb) && matches_pattern(&path, uri)
        });
        if !exempt && !covered {
            escaped.push(format!("{} {path}", verb.to_uppercase()));
        }
    }

    assert!(
        escaped.is_empty(),
        "these mutating admin routes are in neither `mutations()` nor `AUDIT_EXEMPT`, so \
         nothing checks that they write an audit row:\n  {}",
        escaped.join("\n  ")
    );
}
