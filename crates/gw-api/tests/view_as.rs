//! "What can this person see?" — the permission engine, run as somebody else (D-M2-17).
//!
//! Every test here goes through `build_router`, never through the middleware or the
//! handlers directly. The property under test is that the substitution and the refusal
//! happen for *every* route including the ones that do not exist, and a layer that is
//! written but never applied would pass any test that called it on its own.
//!
//! The fixture holds three people and five documents, chosen so that the three views are
//! pairwise different. A test that asserts "the admin now sees less" proves nothing if
//! everybody sees the same thing.
//!
//! - `chef` — an `admins` member, so an instance admin by baseline (D-M2-1). Sees all five.
//! - `gast` — a local account granted `read` on `/handbuch`. Sees the public page and the
//!   `/handbuch` subtree, and neither the internal nor the other restricted page.
//! - `niemand` — a local account with nothing at all. Sees the public page only.

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use gw_api::view_as::{Registry, VIEW_AS_COOKIE};
use gw_api::{AppState, Identity};
use gw_auth::{Permission, Subject};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{NewDocument, Store};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceExt;

// -------------------------------------------------------------------------------------
// Fixture.
// -------------------------------------------------------------------------------------

fn body() -> Block {
    serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"hallo"}]}]}"#,
    )
    .unwrap()
}

/// Slugs are given explicitly rather than derived from the title: these paths are asserted
/// against by name, and a change to the slug rules must not quietly move them.
async fn insert(store: &Store, parent: Option<&str>, slug: &str, title: &str, vis: Visibility) {
    store
        .insert_document(&NewDocument {
            parent_path: parent.map(str::to_string),
            doc_type: DocumentType::Page,
            title: title.into(),
            slug: Some(slug.into()),
            language: "de".into(),
            visibility: vis,
            body: body(),
            sort_key: 0,
        })
        .await
        .unwrap();
}

async fn fixture() -> Arc<Store> {
    let store = Store::open("sqlite::memory:").await.unwrap();

    insert(
        &store,
        None,
        "oeffentlich",
        "Öffentlich",
        Visibility::Public,
    )
    .await;
    insert(&store, None, "intern", "Intern", Visibility::Internal).await;
    insert(&store, None, "handbuch", "Handbuch", Visibility::Restricted).await;
    insert(
        &store,
        Some("/handbuch"),
        "onboarding",
        "Onboarding",
        Visibility::Restricted,
    )
    .await;
    insert(&store, None, "geheim", "Geheim", Visibility::Restricted).await;

    store
        .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
        .await
        .unwrap();
    let gast = store
        .create_local_principal("gast", "Gast Konto", None, "$argon2id$fake")
        .await
        .unwrap();
    store
        .create_local_principal("niemand", "Niemand", None, "$argon2id$fake")
        .await
        .unwrap();
    store
        .add_grant(
            "/handbuch",
            Subject::Principal(gast.id.clone()),
            Permission::Read,
        )
        .await
        .unwrap();

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

// -------------------------------------------------------------------------------------
// One browser.
// -------------------------------------------------------------------------------------

/// A router built ONCE, plus the cookie the browser is holding.
///
/// Built once because the mode's server-side record lives in `AppState` — a router rebuilt
/// per request would start with an empty registry and every activation would evaporate,
/// which is exactly the kind of test that passes while proving nothing.
struct Browser {
    state: AppState,
    router: axum::Router,
    /// The `__Host-gw_view_as` value this browser has been given, if any.
    cookie: Option<String>,
}

struct Reply {
    status: StatusCode,
    body: String,
    set_cookie: Vec<String>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or(Value::Null)
    }

    /// The value the server just issued for the view-as cookie, if it issued one.
    fn issued_view_as(&self) -> Option<String> {
        self.set_cookie
            .iter()
            .filter_map(|raw| raw.strip_prefix(&format!("{VIEW_AS_COOKIE}=")))
            .map(|rest| rest.split(';').next().unwrap_or("").to_string())
            .find(|value| !value.is_empty())
    }

    fn clears_view_as(&self) -> bool {
        self.set_cookie
            .iter()
            .any(|raw| raw.starts_with(&format!("{VIEW_AS_COOKIE}=;")))
    }
}

impl Browser {
    async fn signed_in_as(store: &Arc<Store>, username: &str) -> Self {
        let (principal, _) = store
            .principal_by_username(username)
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("`{username}` must exist in the fixture"));
        let state = AppState::for_test_principal(Arc::clone(store), &principal);
        Self {
            router: gw_api::build_router(state.clone()),
            state,
            cookie: None,
        }
    }

    /// Nobody at all: no session, no development shim.
    fn anonymous(store: &Arc<Store>) -> Self {
        let state = AppState::for_test(Arc::clone(store), None);
        Self {
            router: gw_api::build_router(state.clone()),
            state,
            cookie: None,
        }
    }

    /// A second browser, signed in as somebody else, sharing THIS server's registry.
    ///
    /// One process serves everybody, so a record created here is visible there. That is
    /// what makes "somebody else's view-as cookie" a testable thing rather than a
    /// hypothetical one.
    async fn other(&self, store: &Arc<Store>, username: &str) -> Self {
        let mut other = Self::signed_in_as(store, username).await;
        other.state.view_as = Arc::clone(&self.state.view_as);
        other.router = gw_api::build_router(other.state.clone());
        other
    }

    /// The same, but with substitutions that last `ttl` instead of half an hour.
    ///
    /// The only way to reach the expiry at all. The production deadline is thirty minutes
    /// and no test can outlive it, so before this existed the filter in `Registry::lookup`
    /// could be deleted — turning a bounded window into a mode that never ends — and every
    /// test here still passed. Verified: that mutation survived until this method existed.
    async fn signed_in_briefly(store: &Arc<Store>, username: &str, ttl: Duration) -> Self {
        let mut browser = Self::signed_in_as(store, username).await;
        browser.state.view_as = Arc::new(Registry::with_ttl(ttl));
        browser.router = gw_api::build_router(browser.state.clone());
        browser
    }

    async fn send(&self, method: Method, uri: &str) -> Reply {
        self.send_with(method, uri, self.cookie.as_deref()).await
    }

    /// The same, with the view-as cookie chosen explicitly — a forged one, a stale one, or
    /// none at all.
    async fn send_with(&self, method: Method, uri: &str, cookie: Option<&str>) -> Reply {
        let mut request = Request::builder().method(method).uri(uri);
        if let Some(value) = cookie {
            request = request.header(header::COOKIE, format!("{VIEW_AS_COOKIE}={value}"));
        }
        let response = self
            .router
            .clone()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let set_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap_or_default().to_string())
            .collect();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        Reply {
            status,
            body: String::from_utf8_lossy(&bytes).to_string(),
            set_cookie,
        }
    }

    async fn get(&self, uri: &str) -> Reply {
        self.send(Method::GET, uri).await
    }

    /// Start viewing as `id`, keeping whatever cookie comes back.
    async fn view_as(&mut self, id: &str) -> Reply {
        let reply = self
            .send(Method::POST, &format!("/api/admin/view-as/{id}"))
            .await;
        if let Some(value) = reply.issued_view_as() {
            self.cookie = Some(value);
        }
        reply
    }

    async fn exit(&self) -> Reply {
        self.send(Method::POST, "/api/admin/view-as/exit").await
    }
}

/// Every mutating method, against routes that exist and one that does not.
///
/// `/api/gibt-es-nicht` is the load-bearing one: refusing it proves the block runs BEFORE
/// routing, which is what makes an endpoint written next year safe without anyone
/// remembering this decision.
const MUTATIONS: &[(Method, &str)] = &[
    (Method::POST, "/api/admin/principals"),
    (Method::POST, "/api/admin/teams"),
    (Method::POST, "/api/admin/acl"),
    (Method::DELETE, "/api/admin/acl"),
    (Method::POST, "/auth/logout"),
    (Method::PUT, "/api/documents/handbuch"),
    (Method::PATCH, "/api/tree"),
    (Method::POST, "/api/gibt-es-nicht"),
    (Method::PUT, "/api/gibt-es-nicht"),
    (Method::PATCH, "/api/gibt-es-nicht"),
    (Method::DELETE, "/api/gibt-es-nicht"),
];

// -------------------------------------------------------------------------------------
// Who may switch it on.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn a_non_admin_cannot_activate_the_mode() {
    let store = fixture().await;
    let target = id_of(&store, "niemand").await;

    for who in ["gast", "niemand"] {
        let mut browser = Browser::signed_in_as(&store, who).await;
        let reply = browser.view_as(&target).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "`{who}` activated view-as: {}",
            reply.body
        );
        assert!(
            reply.issued_view_as().is_none(),
            "`{who}` was handed a view-as cookie anyway"
        );
    }
}

#[tokio::test]
async fn an_anonymous_caller_cannot_activate_the_mode() {
    let store = fixture().await;
    let target = id_of(&store, "gast").await;

    let mut browser = Browser::anonymous(&store);
    let reply = browser.view_as(&target).await;
    assert_eq!(reply.status, StatusCode::FORBIDDEN, "{}", reply.body);
    assert!(reply.issued_view_as().is_none());
}

#[tokio::test]
async fn a_deactivated_or_unknown_target_cannot_be_viewed_as() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    store.set_principal_active(&gast, false).await.unwrap();

    let mut chef = Browser::signed_in_as(&store, "chef").await;

    let deactivated = chef.view_as(&gast).await;
    assert_eq!(
        deactivated.status,
        StatusCode::CONFLICT,
        "a deactivated account was viewable: {}",
        deactivated.body
    );
    assert!(deactivated.issued_view_as().is_none());

    let unknown = chef.view_as("gibt-es-nicht").await;
    assert_eq!(
        unknown.status,
        StatusCode::NOT_FOUND,
        "an unknown id was viewable: {}",
        unknown.body
    );
    assert!(unknown.issued_view_as().is_none());
}

#[tokio::test]
async fn the_mode_cannot_be_activated_by_a_forged_cookie_or_header() {
    // The naive implementations, both refused: a cookie naming the target outright, and a
    // cookie carrying a value that merely looks like a token. Neither may substitute
    // anybody, and neither may put the interface into the mode.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;

    let chef = Browser::signed_in_as(&store, "chef").await;
    let own_tree = chef.get("/api/tree").await.body;

    for forged in [
        gast.as_str(),
        "gast",
        "true",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let reply = chef.send_with(Method::GET, "/api/tree", Some(forged)).await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(
            reply.body, own_tree,
            "a forged cookie value `{forged}` changed what the admin sees"
        );

        let me = chef.send_with(Method::GET, "/api/me", Some(forged)).await;
        assert_eq!(
            me.json()["view_as"],
            Value::Null,
            "a forged cookie value `{forged}` put the interface into view-as mode"
        );
    }

    // And nothing activates it by header. There is no header for this and there must not
    // be one: a header is trivially forged by anything that can reach the port.
    let response = chef
        .router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/tree")
                .header("x-gw-view-as", &gast)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&bytes), own_tree);
}

#[tokio::test]
async fn somebody_elses_view_as_cookie_does_nothing() {
    // The record is bound to the administrator who created it and re-checked on every
    // request. A copied cookie is not a way to borrow their reach — nor, since `gast`
    // is nobody, a way to borrow anything at all.
    let store = fixture().await;
    let niemand = id_of(&store, "niemand").await;

    let mut chef = Browser::signed_in_as(&store, "chef").await;
    chef.view_as(&niemand).await;
    let stolen = chef.cookie.clone().expect("chef holds a view-as cookie");

    let gast = chef.other(&store, "gast").await;
    let own = gast.get("/api/tree").await.body;
    let with_stolen = gast
        .send_with(Method::GET, "/api/tree", Some(&stolen))
        .await;

    assert_eq!(with_stolen.body, own, "a copied view-as cookie took effect");
    let me = gast.send_with(Method::GET, "/api/me", Some(&stolen)).await;
    assert_eq!(me.json()["view_as"], Value::Null);
}

// -------------------------------------------------------------------------------------
// Read-only, at the router.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn every_mutating_method_is_refused_while_the_mode_is_active() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;

    // The control run FIRST, and it is not decoration. Every assertion below is "403",
    // and an admin who is refused these anyway would satisfy it without the mode existing.
    for (method, uri) in MUTATIONS {
        let reply = chef.send(method.clone(), uri).await;
        assert_ne!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{method} {uri} is 403 for an administrator anyway, so the assertion below \
             would hold whether or not view-as blocks anything"
        );
    }

    chef.view_as(&gast).await;

    for (method, uri) in MUTATIONS {
        let reply = chef.send(method.clone(), uri).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{method} {uri} was not refused while viewing as somebody else"
        );
    }
}

#[tokio::test]
async fn the_exit_is_the_only_non_get_that_passes() {
    // The exemption is one method on one exact path. A prefix, or a second method, would
    // be a hole that grows on its own.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;
    chef.view_as(&gast).await;

    for (method, uri) in [
        (Method::PUT, "/api/admin/view-as/exit"),
        (Method::DELETE, "/api/admin/view-as/exit"),
        (Method::POST, "/api/admin/view-as/exit/"),
        (Method::POST, "/api/admin/view-as/exit/x"),
        // Switching target without leaving first: still a non-GET, still refused.
        (Method::POST, "/api/admin/view-as/anything"),
    ] {
        let reply = chef.send(method.clone(), uri).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{method} {uri} passed the block"
        );
    }

    assert_eq!(chef.exit().await.status, StatusCode::SEE_OTHER);
}

// -------------------------------------------------------------------------------------
// Exactly their view, and never more.
// -------------------------------------------------------------------------------------

/// Every read a reader interface makes. Asserted as a set, so a path added to the fixture
/// without being listed here is a gap somebody can see rather than one they cannot.
const READS: &[&str] = &[
    "/api/tree",
    "/api/documents/oeffentlich",
    "/api/documents/intern",
    "/api/documents/handbuch",
    "/api/documents/handbuch/onboarding",
    "/api/documents/geheim",
    "/api/gibt-es-nicht",
];

#[tokio::test]
async fn the_tree_and_the_documents_are_exactly_what_the_target_sees() {
    let store = fixture().await;

    for who in ["gast", "niemand"] {
        let target = id_of(&store, who).await;
        let mut chef = Browser::signed_in_as(&store, "chef").await;
        let alone = Browser::signed_in_as(&store, who).await;
        let chef_alone = Browser::signed_in_as(&store, "chef").await;

        chef.view_as(&target).await;

        let mut differed = false;
        for uri in READS {
            let substituted = chef.get(uri).await;
            let direct = alone.get(uri).await;
            assert_eq!(
                (substituted.status, substituted.body.as_str()),
                (direct.status, direct.body.as_str()),
                "viewing as `{who}`, {uri} did not answer what `{who}` is answered"
            );

            // ... and the comparison means something only because the admin's own answer
            // is different for at least one of these. Without this the test would pass
            // against an implementation that substitutes nobody at all.
            let own = chef_alone.get(uri).await;
            differed |= (own.status, own.body.as_str()) != (direct.status, direct.body.as_str());
        }
        assert!(
            differed,
            "`chef` and `{who}` see the same thing everywhere, so the equality above \
             proves nothing"
        );
    }
}

#[tokio::test]
async fn viewing_as_somebody_reaches_nothing_they_cannot() {
    // The escalation question, asked of the endpoints an administrator — and only an
    // administrator — may read. Being able to see them again the moment the mode ends is
    // the other half: the substitution narrows, it does not revoke.
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;

    const ADMIN_ONLY: &[&str] = &[
        "/api/admin/principals",
        "/api/admin/teams",
        "/api/admin/acl",
        "/api/admin/audit",
        "/api/admin/admins/candidates",
        "/api/documents/geheim",
        "/api/documents/intern",
    ];

    for uri in ADMIN_ONLY {
        assert_eq!(
            chef.get(uri).await.status,
            StatusCode::OK,
            "{uri} must be readable by the administrator before the mode starts, or the \
             refusal below says nothing"
        );
    }

    chef.view_as(&gast).await;

    for uri in ADMIN_ONLY {
        assert_eq!(
            chef.get(uri).await.status,
            StatusCode::FORBIDDEN,
            "{uri} was still reachable while viewing as somebody who may not read it"
        );
    }
}

#[tokio::test]
async fn exiting_restores_the_admins_own_view_on_the_very_next_request() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;

    let own_tree = chef.get("/api/tree").await.body;
    chef.view_as(&gast).await;
    assert_ne!(chef.get("/api/tree").await.body, own_tree);

    let exit = chef.exit().await;
    assert_eq!(exit.status, StatusCode::SEE_OTHER);
    assert!(exit.clears_view_as(), "the exit did not clear the cookie");

    // The stale cookie is deliberately still attached. Clearing it in the browser is not
    // the defence — the server-side record being gone is.
    assert_eq!(chef.get("/api/tree").await.body, own_tree);
    assert_eq!(chef.get("/api/me").await.json()["view_as"], Value::Null);
    assert_eq!(
        chef.get("/api/admin/principals").await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn me_reports_the_mode_and_both_identities() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;

    assert_eq!(chef.get("/api/me").await.json()["view_as"], Value::Null);

    chef.view_as(&gast).await;
    let me = chef.get("/api/me").await.json();

    // The identity the interface renders as is the TARGET's, because that is what the
    // permission engine ran as.
    assert_eq!(me["username"], "gast");
    assert_eq!(me["view_as"]["target"]["username"], "gast");
    assert_eq!(me["view_as"]["target"]["display_name"], "Gast Konto");
    assert_eq!(me["view_as"]["target"]["id"], gast);
    // And the administrator is named too, or the banner cannot say whose session this
    // really is.
    assert_eq!(me["view_as"]["viewer"]["username"], "chef");
    assert_eq!(me["view_as"]["viewer"]["display_name"], "Chef");
}

// -------------------------------------------------------------------------------------
// Audited.
// -------------------------------------------------------------------------------------

#[tokio::test]
async fn activation_writes_an_audit_row_naming_both_identities() {
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let chef_id = id_of(&store, "chef").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;

    chef.view_as(&gast).await;

    let (chef_principal, _) = store.principal_by_username("chef").await.unwrap().unwrap();
    let entries = store.audit_for(&chef_principal, 500).await.unwrap().entries;
    let started = entries
        .iter()
        .find(|e| e.action == "view-as.start")
        .expect("activating view-as writes an audit row");

    assert_eq!(started.principal_id.as_deref(), Some(chef_id.as_str()));
    assert_eq!(started.target.as_deref(), Some(gast.as_str()));
    let detail: Value = serde_json::from_str(&started.detail).unwrap();
    assert_eq!(detail["viewer"], "chef");
    assert_eq!(detail["viewer_id"], chef_id);
    assert_eq!(detail["target"], "gast");
    assert_eq!(detail["target_id"], gast);

    // The window is bounded by the start row itself, so a missing stop row means "until
    // the deadline at the latest" rather than "possibly for ever".
    assert!(detail["ttl_seconds"].as_i64().unwrap_or_default() > 0);

    chef.exit().await;
    let entries = store.audit_for(&chef_principal, 500).await.unwrap().entries;
    let stopped = entries
        .iter()
        .find(|e| e.action == "view-as.stop")
        .expect("leaving view-as writes an audit row");
    let detail: Value = serde_json::from_str(&stopped.detail).unwrap();
    assert_eq!(detail["viewer_id"], chef_id);
    assert_eq!(detail["target_id"], gast);
}

#[tokio::test]
async fn the_substitution_does_not_outlive_the_authority_that_created_it() {
    // D-M2-7 applied to this mode: reach is re-read on every request, so a demotion takes
    // effect on the next one rather than at the next sign-in.
    //
    // What it drops to is the point. Once the record belongs to this caller the mode is
    // ON, and the only remaining question is whether the substitution can be completed —
    // so a demoted viewer becomes NOBODY rather than reverting to their own session. There
    // is deliberately no branch in `active_for` that resolves to the viewer, because that
    // branch is how "the substitution failed" turns into "here are your own documents".
    let store = fixture().await;
    let gast = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_as(&store, "chef").await;
    let gast_tree = Browser::signed_in_as(&store, "gast")
        .await
        .get("/api/tree")
        .await
        .body;
    let public_only = Browser::anonymous(&store).get("/api/tree").await.body;
    assert_ne!(
        gast_tree, public_only,
        "the fixture makes these two the same"
    );

    chef.view_as(&gast).await;
    assert_ne!(chef.get("/api/me").await.json()["view_as"], Value::Null);

    // `chef` administers by group. Taking the mapping away is how that reach ends.
    store
        .set_group_role("admins", gw_store::Baseline::Public)
        .await
        .unwrap();

    let me = chef.get("/api/me").await.json();
    // Not `chef` again: the request runs as nobody. Were the mode simply switched off, the
    // caller would be signed in as `chef` here, which is what this assertion separates.
    assert_eq!(
        me["authenticated"], false,
        "a demoted viewer went back to their own session instead of losing the mode"
    );
    assert_eq!(me["username"], Value::Null);
    // The banner still names it, or there would be no way to understand the empty page —
    // and no exit offered on it.
    assert_ne!(me["view_as"], Value::Null);
    assert_eq!(chef.get("/api/tree").await.body, public_only);

    // And leaving still works, from inside a mode nobody may any longer start.
    assert_eq!(chef.exit().await.status, StatusCode::SEE_OTHER);
    let me = chef.get("/api/me").await.json();
    assert_eq!(me["username"], "chef");
    assert_eq!(me["view_as"], Value::Null);
}

#[tokio::test]
async fn the_development_shim_is_not_a_way_in() {
    // `AppState::for_test_principal` goes through `GW_DEV_IDENTITY`, and the shim resolves
    // a NAME to a stored principal. Nothing about it may look like an activated mode.
    let store = fixture().await;
    let browser = Browser {
        state: AppState::for_test(Arc::clone(&store), Some(Identity::dev("gast", &[]))),
        router: gw_api::build_router(AppState::for_test(
            Arc::clone(&store),
            Some(Identity::dev("gast", &[])),
        )),
        cookie: None,
    };
    let _ = &browser.state;
    assert_eq!(browser.get("/api/me").await.json()["view_as"], Value::Null);
}

/// The window really closes: an expired substitution stops resolving, and the very next
/// request is the administrator's own view again.
///
/// This is the one property the file could not express until `Registry::with_ttl` existed.
/// The deadline is what makes a `view-as.start` row with no matching stop row mean "until
/// the deadline at the latest" rather than "possibly for ever", so a deleted expiry filter
/// would quietly retract the guarantee the audit log is written to give.
///
/// It costs a real half-second of wall clock, which is the price of asserting something
/// about time. `tokio::time::pause()` cannot help: the record holds a `std::time::Instant`,
/// which tokio's clock does not move.
#[tokio::test]
async fn a_substitution_does_not_outlive_its_deadline() {
    let store = fixture().await;
    let gast_id = id_of(&store, "gast").await;
    let mut chef = Browser::signed_in_briefly(&store, "chef", Duration::from_millis(400)).await;

    let own_tree = chef.get("/api/tree").await.body;
    assert_eq!(chef.view_as(&gast_id).await.status, StatusCode::OK);

    // The mode has to be REAL before its ending can prove anything. Without this pair the
    // assertions below would pass just as well against a substitution that never took
    // effect at all — which is the shape of vacuous test this project keeps finding.
    assert_ne!(
        chef.get("/api/tree").await.body,
        own_tree,
        "the substitution never took effect, so its expiry proves nothing"
    );
    assert_ne!(
        chef.get("/api/me").await.json()["view_as"],
        Value::Null,
        "the mode was never reported as active, so its expiry proves nothing"
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert_eq!(
        chef.get("/api/tree").await.body,
        own_tree,
        "an expired substitution still resolved — the mode outlived its deadline"
    );
    assert_eq!(
        chef.get("/api/me").await.json()["view_as"],
        Value::Null,
        "/api/me reported a mode whose deadline had passed"
    );
}
