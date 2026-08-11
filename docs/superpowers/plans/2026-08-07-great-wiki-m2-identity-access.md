# great-wiki M2 — Identity and Access Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A guest account can be created, put in a team, granted read on one subtree, and
provably cannot reach anything else — through any path.

**Architecture:** Principals (from OIDC or local credentials) belong to teams. ACLs attach
to a document or a space and **inherit down the materialised path** unless a descendant
overrides. One `can(principal, action, resource)` function decides everything; no handler
makes its own decision.

**Tech Stack:** Rust 1.97, sqlx 0.8, `argon2` 0.5, Axum 0.8, SvelteKit.

## Global Constraints

Inherited from [the roadmap](2026-08-07-great-wiki-roadmap.md#global-constraints). The ones
that bite in M2:

- **Fail closed.** Unknown permission → deny. No ACL and not public → deny for anonymous.
- **Filtering happens in the retriever**, never as a post-filter and never in the frontend.
- **great-wiki does not write Authelia's user database** (ADR 0002, ADR context). Local
  accounts are great-wiki's own principals, not homelab SSO accounts.
- **Every task ends green** on `just ci`.

## Decisions locked after M1 (2026-08-07)

**D-M2-1 — Default access follows the Authelia group.** A first sign-in does not grant
anything by itself; what the person can see is derived from the verified `groups` claim:

| Authelia group | Default reach |
|---|---|
| `admins` | Everything, including instance administration |
| `users` | Public plus `internal` |
| anything else, or none | Public only |

This is better than a flat default in both directions: access follows the homelab account
rather than being maintained twice, and "I gave someone an account" never silently becomes
"I gave them the internal wiki". The mapping is **configuration, not code** — a table, so a
new group does not need a release. Local guest accounts have no Authelia groups and
therefore land in the third row until granted explicitly.

**D-M2-2 — Space admins may invite guests, scoped to their own spaces.** Account creation
decentralises to whoever administers an area. The scope restriction is load-bearing: without
it, a space admin could mint a credential reaching the whole instance's public surface and
any `internal` content. An invite may therefore grant **only into spaces the inviter
administers**, and the resulting principal starts with nothing else.

**D-M2-3 — Invites are one-time links, not passwords handed over.** The recipient sets their
own password, so no credential ever passes through chat or email. Links expire and are
single-use.

**D-M2-4 — The admin console ships all four capabilities**, including the hard one:

1. **Who can see what** — for any page or space, every principal and team with read,
   comment, write or admin, showing what is inherited and from where.
2. **Fast grant and revoke**, with confirmation on anything destructive.
3. **A readable audit trail**, written in the same transaction as the change it records, so
   an action cannot succeed unrecorded.
4. **"What can this person see?"** — the whole wiki rendered as another principal would see
   it. This is the one that catches a mistake *before* it matters, and it is genuinely
   harder than the other three: it means running the entire permission engine under a
   substituted principal, on every retrieval path, without ever letting the substitution
   leak into a write. Read-only, explicitly labelled in the interface, and audited.

**D-M2-5 — The OIDC login flow moves into M2.** M1 registered the client in Authelia but
never wrote the application side, so both wiki hostnames currently sit behind a *temporary*
`import authelia` edge gate. That gate blocks four things this milestone must deliver:
public pages readable without signing in, per-person rather than blanket access, real
attribution on revisions and comments, and guest accounts that bypass Authelia entirely.
**Removing the edge gate is an exit criterion of M2**, not a follow-up.

**D-M2-6 — View-as is instance admins only.** Someone who can already see everything reveals
nothing new by using it, so no disclosure is possible. Space-scoped view-as was considered
and rejected: a space admin would see the subject's view of *other* spaces unless the output
were scoped, and that scoping is an easy place to get wrong for a feature whose whole job is
to be trustworthy.

**D-M2-7 — Revocation is immediate, and deactivation ends the session.** Permissions are
read fresh on every request, so removing a grant takes effect on the person's next click
rather than at their next sign-in. Deactivating an account additionally invalidates its
sessions everywhere. The per-request read costs a single indexed query — negligible at this
scale, and the alternative means someone you just removed keeps their access for days.

**D-M2-8 — Write is always an explicit grant.** Being able to read something never implies
being able to change it, for any group including `users`. Consistent with the fail-closed
posture everywhere else, and it means "I gave them access to read the handbook" cannot
become "they can rewrite it".

**D-M2-9 — History visibility is configurable, and defaults to open.** Anyone who can read a
page can, by default, read its full history and timeline — that is what makes "development
of knowledge" visible to readers rather than only to editors. It is a setting, not a
constant, and **spaces carry defaults their documents inherit**, so a space can be created
"history visible to editors only" without setting it per page.

> **The consequence, stated plainly because it will surprise someone eventually.** With
> history open by default, *removing something from a page does not hide it*. An earlier
> revision still holds it, and anyone who can read the page can read that revision. Deleting
> a paragraph is an edit, not a redaction.
>
> Two things follow, and both are M3 requirements rather than nice-to-haves. There must be a
> **purge** operation that removes content from the history itself — audited, admin-only,
> and explicitly distinct from editing. And the interface must **say so at the point of
> editing**: someone pasting a credential and then deleting it needs to learn immediately
> that it is still there, not months later.

## Decisions locked 2026-08-10 (owner)

**D-M2-11 — One sign-in button, both mechanisms behind it.** There is a single *Anmelden*
control. It does not go straight to Authelia; it opens great-wiki's own sign-in page, which
offers the homelab account **and** a guest username/password form. Which mechanism a person
uses is great-wiki's business, not something the reader has to understand before clicking.

*Consequence, and it is not optional:* the guest password form is now reachable from a
public hostname by anyone, where the previous plan had it hidden behind an invite link. **A
publicly reachable password form must ship with rate limiting and a generic failure
message** — per-account and per-IP throttling, and "Anmeldung fehlgeschlagen" whether the
username exists or not, so the form cannot be used to enumerate who has an account. This is
a prerequisite of Task 6b, not a later hardening pass.

*Also:* `/auth/login` currently 302s straight to the identity provider. That becomes a
rendered page, and the redirect moves behind the homelab button.

**D-M2-12 — Invites are a link first, email later.** The console shows a single-use expiring
invite URL once, and the owner sends it by whatever channel they like. No SMTP is wired and
an invite that silently fails to deliver is worse than one you have to paste. Build the
delivery seam so that sending it by mail is a later addition rather than a rewrite: the
invite is created and stored the same way either way, and only the handing-over differs.

**D-M2-13 — Audit entries are kept indefinitely.** The log is a row per administrative
action, not per page view, so it does not grow the way a request log does, and the question
it exists to answer ("when did this change, and who changed it?") is usually asked months
later. Recorded as a deliberate choice rather than an omission: if it is ever revisited, the
reason will be size, and the privacy question about retaining who-viewed-whom should be
reopened at the same time.

**D-M2-14 — The admin console is built entirely on Ark UI.** Not only the primitives that
are hard to hand-write. ADR 0005 chose Ark on paper and nothing has ever used it; a partial
adoption would test the parts that were never in doubt. The console is the right place to
find out what living in it is actually like, because it is small, internal, and the only
thing that breaks if the answer is "no". Every Ark component is restyled through the
existing tokens — a component that cannot be reached by a theme is a component that breaks
the plugin contract.

**D-M2-15 — Ten failures, then five minutes, counted two ways.** Per account *and* per
source address independently; either counter tripping refuses the attempt. Ten is forgiving
enough that somebody fumbling a password manager is not locked out, and five minutes is
long enough that guessing at scale stops being worthwhile. A successful sign-in clears that
account's counter. State lives in SQLite, so a restart is not a reset — an attacker who can
provoke a restart must not be able to clear the evidence with it.

Throttling never applies to the Authelia path. Otherwise somebody guessing at guest
passwords could block homelab sign-in, which turns a login form into a denial-of-service
lever against the whole instance.

**D-M2-16 — Twelve characters, no composition rules, plus a breach check.** Length only:
current NIST guidance is that forced symbols and mixed case push people toward `Passwort1!`
patterns that are shorter and more guessable than a passphrase. The breach check uses Have I
Been Pwned's k-anonymity range API, which receives the first five hex characters of a SHA-1
and never the password or its full hash.

**If the breach service is unreachable the password is allowed, and the fact is written to
the audit log.** That is a deliberate trade rather than an oversight: failing closed would
mean a network outage prevents anyone from ever setting a password, and the length
requirement still applies with the service down. The audit row is what stops the degraded
mode from being invisible.

**D-M2-17 — View-as is blocked at the router, not in each handler.** A persistent banner
names whose view is being shown and offers an exit. Every non-GET request is refused while
the mode is active, *before* reaching any handler — so an endpoint written next year cannot
forget the check. Per-handler checks would be more flexible and would fail open for code
that does not exist yet, which is precisely the class of bug this project keeps finding.

**D-M2-18 — Nobody may deactivate the last instance admin.** The refusal lives in the API,
not only the interface, and counts the remaining admins inside the same transaction as the
deactivation — otherwise two concurrent requests each see one other admin and both succeed.
It applies whoever is asking: deactivating somebody else's account is exactly as capable of
locking everyone out.

Authelia's `admins` group stays the source of truth for real system administrators, and
great-wiki never writes Authelia's user database (ADR 0002). **Authelia groups are not used
for space-level access at all** — that is what teams and path grants are for, and adding
group mappings per space would put reach in two places.

Because great-wiki cannot add anyone to an Authelia group, handing over needs a mechanism of
its own: a **per-account promotion inside great-wiki**, offered as the fallback for exactly
the case it exists for — the last Authelia admin is leaving and somebody has to take over.
It promotes the person chosen and nobody else. A group mapping would have promoted every
other member of that group as a side effect, which is not what "hand over to this person"
means.

The hand-over proposes candidates **sorted by most recent activity**, because the useful
default is somebody who is actually here.

**D-M2-19 — The first grant on a path still replaces what it inherited, and the console
says so before you confirm.** Nearest-ancestor-wins is what makes narrowing a subtree
possible at all, so the rule stays. What was wrong was that it happened invisibly: granting
a team read on a sub-page could drop your own access to it in the same request, with nothing
said. Copying inherited grants down automatically would have made narrowing require a
delete, turning the common case into two steps to avoid a surprise in the rare one.

**D-M2-20 — An invite may carry a direct grant AND a team membership.** Both, because they
answer different questions: "read this one page" and "everything this team can reach". An
invite that could only create an account would leave a person able to sign in and see
nothing, and the gap between the two steps is where somebody gets forgotten. The inviter may
only name paths they administer, enforced against the permission engine server-side rather
than by hiding options in the interface.

**D-M2-21 — Invite links last 30 days.** Single-use regardless, so the window only matters
until acceptance. Long enough that a homelab invitation does not expire unnoticed and read
as a broken system; short enough that a link in an old message does not stay live forever.

## What M2 replaces

M1 left two deliberate stubs, both commented as such. M2 must **replace** them, not sit
beside them — an unfiltered variant left reachable is how a later handler leaks:

- `routes::may_read(identity, visibility)` → the permission engine.
- `Store::tree()` → `Store::tree_for(principal)`.

## File Structure

```
crates/gw-store/migrations/0002_identity.sql   principals, credentials, teams, acl, audit
crates/gw-auth/Cargo.toml                      new crate: the permission engine lives alone
crates/gw-auth/src/lib.rs
crates/gw-auth/src/principal.rs                Principal, PrincipalKind
crates/gw-auth/src/permission.rs               Action, Permission, can()
crates/gw-auth/src/password.rs                 argon2id hashing and verification
crates/gw-store/src/principals.rs              principal + team + membership queries
crates/gw-store/src/acl.rs                     ACL storage and the effective-permission query
crates/gw-api/src/routes/admin.rs              principal, team and ACL management
crates/gw-api/src/routes/tokens.rs             API tokens
web/src/routes/admin/+page.svelte              admin console
```

**Why `gw-auth` is its own crate:** the permission engine is the one piece where a bug is a
disclosure rather than a defect. Isolating it means it can be tested exhaustively without a
web server, and its public surface is small enough to review in one sitting.

---

## Task 1: The permission model

**Files:**
- Create: `crates/gw-auth/Cargo.toml`
- Create: `crates/gw-auth/src/lib.rs`
- Create: `crates/gw-auth/src/principal.rs`
- Create: `crates/gw-auth/src/permission.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `enum PrincipalKind { Oidc, Local }`
  - `struct Principal { id: String, kind: PrincipalKind, username: String, display_name: String, email: Option<String>, groups: Vec<String>, teams: Vec<String>, active: bool }` with `Principal::anonymous()` and `is_authenticated()`
  - `enum Action { Read, Comment, Write, Admin }` with `Action::implies(&self, other: Action) -> bool`
  - `enum Permission { Read, Comment, Write, Admin }` — what a *grant* confers
  - `struct Grant { subject: Subject, permission: Permission }`, `enum Subject { Principal(String), Team(String), Group(String), Anyone, Authenticated }`
  - `fn can(principal: &Principal, action: Action, visibility: Visibility, grants: &[Grant]) -> bool`

- [ ] **Step 1: Write the failing tests**

`crates/gw-auth/src/permission.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::permission::{can, Action, Grant, Permission, Subject};
    use crate::principal::Principal;
    use gw_core::Visibility;

    fn guest() -> Principal {
        Principal::test("guest", &[], &[])
    }
    fn member() -> Principal {
        Principal::test("member", &["users"], &["editors"])
    }
    fn anon() -> Principal {
        Principal::anonymous()
    }

    #[test]
    fn public_documents_are_readable_by_anyone_including_anonymous() {
        assert!(can(&anon(), Action::Read, Visibility::Public, &[]));
    }

    #[test]
    fn public_documents_are_not_writable_without_a_grant() {
        // Public means readable, never editable. Conflating the two is how wikis get defaced.
        assert!(!can(&anon(), Action::Write, Visibility::Public, &[]));
        assert!(!can(&guest(), Action::Write, Visibility::Public, &[]));
    }

    #[test]
    fn internal_documents_need_authentication_but_no_grant() {
        assert!(!can(&anon(), Action::Read, Visibility::Internal, &[]));
        assert!(can(&guest(), Action::Read, Visibility::Internal, &[]));
    }

    #[test]
    fn restricted_documents_need_a_matching_grant() {
        assert!(!can(&guest(), Action::Read, Visibility::Restricted, &[]));
        let grants = [Grant {
            subject: Subject::Principal("guest".into()),
            permission: Permission::Read,
        }];
        assert!(can(&guest(), Action::Read, Visibility::Restricted, &grants));
    }

    #[test]
    fn a_team_grant_reaches_its_members() {
        let grants = [Grant {
            subject: Subject::Team("editors".into()),
            permission: Permission::Write,
        }];
        assert!(can(&member(), Action::Write, Visibility::Restricted, &grants));
        assert!(!can(&guest(), Action::Write, Visibility::Restricted, &grants));
    }

    #[test]
    fn an_oidc_group_grant_reaches_its_members() {
        let grants = [Grant {
            subject: Subject::Group("users".into()),
            permission: Permission::Read,
        }];
        assert!(can(&member(), Action::Read, Visibility::Restricted, &grants));
        assert!(!can(&guest(), Action::Read, Visibility::Restricted, &grants));
    }

    #[test]
    fn stronger_permissions_imply_weaker_actions() {
        let grants = [Grant {
            subject: Subject::Principal("guest".into()),
            permission: Permission::Admin,
        }];
        for action in [Action::Read, Action::Comment, Action::Write, Action::Admin] {
            assert!(can(&guest(), action, Visibility::Restricted, &grants));
        }
    }

    #[test]
    fn weaker_permissions_do_not_imply_stronger_actions() {
        let grants = [Grant {
            subject: Subject::Principal("guest".into()),
            permission: Permission::Read,
        }];
        assert!(can(&guest(), Action::Read, Visibility::Restricted, &grants));
        assert!(!can(&guest(), Action::Comment, Visibility::Restricted, &grants));
        assert!(!can(&guest(), Action::Write, Visibility::Restricted, &grants));
        assert!(!can(&guest(), Action::Admin, Visibility::Restricted, &grants));
    }

    #[test]
    fn anonymous_never_passes_a_grant_gate_even_with_a_matching_name() {
        // A forged identity claiming a group must not pass: authentication is checked
        // BEFORE group membership, because groups are only meaningful once a user is known.
        let mut forged = Principal::anonymous();
        forged.groups = vec!["admins".into()];
        forged.teams = vec!["editors".into()];
        let grants = [
            Grant { subject: Subject::Group("admins".into()), permission: Permission::Admin },
            Grant { subject: Subject::Team("editors".into()), permission: Permission::Admin },
            Grant { subject: Subject::Authenticated, permission: Permission::Admin },
        ];
        assert!(!can(&forged, Action::Read, Visibility::Restricted, &grants));
    }

    #[test]
    fn a_deactivated_principal_is_denied_everything_but_public_reads() {
        let mut suspended = member();
        suspended.active = false;
        let grants = [Grant {
            subject: Subject::Team("editors".into()),
            permission: Permission::Admin,
        }];
        assert!(!can(&suspended, Action::Read, Visibility::Restricted, &grants));
        assert!(!can(&suspended, Action::Read, Visibility::Internal, &[]));
        // Deactivating someone must not make public pages unreadable to them.
        assert!(can(&suspended, Action::Read, Visibility::Public, &[]));
    }

    #[test]
    fn anyone_grants_reach_anonymous_but_only_for_the_granted_permission() {
        let grants = [Grant { subject: Subject::Anyone, permission: Permission::Read }];
        assert!(can(&anon(), Action::Read, Visibility::Restricted, &grants));
        assert!(!can(&anon(), Action::Write, Visibility::Restricted, &grants));
    }
}
```

- [ ] **Step 2: Create the manifest**

`crates/gw-auth/Cargo.toml`:
```toml
[package]
name = "gw-auth"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
gw-core = { path = "../gw-core" }
anyhow = { workspace = true }
argon2 = "0.5"
password-hash = { version = "0.5", features = ["rand_core"] }
rand = "0.8"
serde = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p gw-auth`
Expected: FAIL — `could not find permission in the crate root`.

- [ ] **Step 4: Implement the principal**

`crates/gw-auth/src/principal.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    /// Authenticated by Authelia. Groups come from the verified `groups` claim.
    Oidc,
    /// A great-wiki account for someone with no homelab SSO account. Not an Authelia user
    /// — great-wiki never writes that database.
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub kind: PrincipalKind,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    /// From the OIDC `groups` claim. Empty for local accounts.
    pub groups: Vec<String>,
    /// great-wiki's own teams. Populated by the store on load.
    pub teams: Vec<String>,
    pub active: bool,
}

impl Principal {
    /// The absence of a principal, not a principal with no rights. `is_authenticated`
    /// is false, which is what every gate keys on.
    pub fn anonymous() -> Self {
        Self {
            id: String::new(),
            kind: PrincipalKind::Local,
            username: String::new(),
            display_name: "Anonymous".into(),
            email: None,
            groups: Vec::new(),
            teams: Vec::new(),
            active: true,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        !self.id.is_empty() && !self.username.trim().is_empty()
    }

    #[doc(hidden)]
    pub fn test(username: &str, groups: &[&str], teams: &[&str]) -> Self {
        Self {
            id: format!("test-{username}"),
            kind: PrincipalKind::Local,
            username: username.into(),
            display_name: username.into(),
            email: None,
            groups: groups.iter().map(|s| s.to_string()).collect(),
            teams: teams.iter().map(|s| s.to_string()).collect(),
            active: true,
        }
    }
}
```

- [ ] **Step 5: Implement the permission engine**

`crates/gw-auth/src/permission.rs`, above `mod tests`:
```rust
use crate::principal::Principal;
use gw_core::Visibility;
use serde::{Deserialize, Serialize};

/// What a caller is trying to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Read,
    Comment,
    Write,
    Admin,
}

/// What a grant confers. Ordered, so a stronger grant satisfies a weaker action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Read,
    Comment,
    Write,
    Admin,
}

impl Permission {
    fn satisfies(self, action: Action) -> bool {
        let level = |p| match p {
            Permission::Read => 0,
            Permission::Comment => 1,
            Permission::Write => 2,
            Permission::Admin => 3,
        };
        let needed = match action {
            Action::Read => 0,
            Action::Comment => 1,
            Action::Write => 2,
            Action::Admin => 3,
        };
        level(self) >= needed
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "lowercase")]
pub enum Subject {
    Principal(String),
    Team(String),
    /// An OIDC group from the verified `groups` claim.
    Group(String),
    /// Everyone, including anonymous callers. Used by public share links.
    Anyone,
    /// Any signed-in principal.
    Authenticated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub subject: Subject,
    pub permission: Permission,
}

/// The single authorisation decision in the system.
///
/// Every handler, every retriever and every export path calls this. Nothing else decides.
///
/// The ordering below is load-bearing. Authentication is checked BEFORE group and team
/// membership, because those lists are only meaningful once the caller is known — an
/// anonymous request carrying a forged group must fail, and the test
/// `anonymous_never_passes_a_grant_gate_even_with_a_matching_name` proves it.
pub fn can(
    principal: &Principal,
    action: Action,
    visibility: Visibility,
    grants: &[Grant],
) -> bool {
    // Public reads are the one thing that survives everything, including deactivation:
    // suspending an account must not make the public site disappear for that person.
    if action == Action::Read && visibility == Visibility::Public {
        return true;
    }

    // An `Anyone` grant is the only way an anonymous caller gets past this point. It is
    // how a public share link works, and it confers exactly what it says.
    let anyone = grants
        .iter()
        .filter(|g| g.subject == Subject::Anyone)
        .any(|g| g.permission.satisfies(action));
    if anyone {
        return true;
    }

    if !principal.is_authenticated() || !principal.active {
        return false;
    }

    if action == Action::Read && visibility == Visibility::Internal {
        return true;
    }

    grants.iter().any(|grant| {
        let matches = match &grant.subject {
            Subject::Principal(id) => *id == principal.id || *id == principal.username,
            Subject::Team(t) => principal.teams.iter().any(|mine| mine == t),
            Subject::Group(g) => principal.groups.iter().any(|mine| mine == g),
            Subject::Authenticated => true,
            Subject::Anyone => false, // already handled above
        };
        matches && grant.permission.satisfies(action)
    })
}
```

`crates/gw-auth/src/lib.rs`:
```rust
//! Identity and authorisation for great-wiki.
//!
//! Isolated in its own crate because a bug here is a disclosure rather than a defect:
//! the surface stays small enough to review in one sitting, and every rule is testable
//! without a web server or a database.

pub mod password;
pub mod permission;
pub mod principal;

pub use permission::{can, Action, Grant, Permission, Subject};
pub use principal::{Principal, PrincipalKind};
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p gw-auth`
Expected: `test result: ok. 11 passed`.

- [ ] **Step 7: Lint, changelog and commit**

Add under `### Added`:
```markdown
- `gw-auth`: the permission engine as its own crate. One `can()` function decides every
  authorisation in the system, checking authentication before group or team membership so
  a forged group on an anonymous request cannot pass.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-auth CHANGELOG.md
git commit -m "feat(auth): permission engine with fail-closed authorisation"
```

---

## Task 2: Local account credentials

**Files:**
- Create: `crates/gw-auth/src/password.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `fn hash_password(plain: &str) -> anyhow::Result<String>`
  - `fn verify_password(plain: &str, hash: &str) -> bool`
  - `fn validate_password_strength(plain: &str) -> Result<(), PasswordError>`

**Parameters match Authelia's** (m=65536, t=3, p=4, argon2id) so the two systems have the
same cost profile and a local account is no weaker than an SSO one.

- [ ] **Step 1: Write the failing tests**

`crates/gw-auth/src/password.rs`:
```rust
#[cfg(test)]
mod tests {
    use crate::password::{hash_password, validate_password_strength, verify_password};

    #[test]
    fn a_hash_verifies_against_its_own_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(verify_password("correct horse battery staple", &hash));
    }

    #[test]
    fn a_hash_rejects_a_different_password() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(!verify_password("Correct horse battery staple", &hash));
        assert!(!verify_password("", &hash));
    }

    #[test]
    fn hashes_are_salted_so_the_same_password_hashes_differently() {
        let a = hash_password("same password").unwrap();
        let b = hash_password("same password").unwrap();
        assert_ne!(a, b, "identical hashes mean the salt is missing");
        assert!(verify_password("same password", &a));
        assert!(verify_password("same password", &b));
    }

    #[test]
    fn the_hash_declares_argon2id_with_authelia_parameters() {
        let hash = hash_password("x").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=65536,t=3,p=4$"), "got {hash}");
    }

    #[test]
    fn a_malformed_hash_verifies_to_false_rather_than_panicking() {
        // A corrupted row must deny access, not take the process down.
        assert!(!verify_password("x", "not-a-hash"));
        assert!(!verify_password("x", ""));
    }

    #[test]
    fn short_passwords_are_rejected() {
        assert!(validate_password_strength("short").is_err());
        assert!(validate_password_strength("a-perfectly-fine-passphrase").is_ok());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gw-auth password`
Expected: FAIL — `could not find password in the crate root`.

- [ ] **Step 3: Implement**

`crates/gw-auth/src/password.rs`, above `mod tests`:
```rust
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use thiserror::Error;

/// Deliberately identical to Authelia's configured parameters, so a local account is no
/// cheaper to attack than a homelab SSO account.
const MEMORY_KIB: u32 = 65536;
const ITERATIONS: u32 = 3;
const PARALLELISM: u32 = 4;
const MIN_LENGTH: usize = 12;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PasswordError {
    #[error("password must be at least {MIN_LENGTH} characters")]
    TooShort,
}

fn argon2() -> Result<Argon2<'static>> {
    let params = Params::new(MEMORY_KIB, ITERATIONS, PARALLELISM, None)
        .map_err(|e| anyhow!("invalid argon2 parameters: {e}"))?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

pub fn validate_password_strength(plain: &str) -> Result<(), PasswordError> {
    // Length only. Composition rules push people toward "Passw0rd!" and are worse than
    // a length floor; strength beyond this is the user's judgement.
    if plain.chars().count() < MIN_LENGTH {
        return Err(PasswordError::TooShort);
    }
    Ok(())
}

pub fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    Ok(argon2()?
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow!("hashing failed: {e}"))?
        .to_string())
}

/// Verify a password. Returns false for a malformed stored hash rather than erroring —
/// a corrupted row must deny access, not take the process down.
pub fn verify_password(plain: &str, hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(hash) else {
        return false;
    };
    let Ok(hasher) = argon2() else {
        return false;
    };
    hasher.verify_password(plain.as_bytes(), &parsed).is_ok()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gw-auth`
Expected: `test result: ok. 17 passed`.

- [ ] **Step 5: Lint, changelog and commit**

Add under `### Added`:
```markdown
- Local account credentials using argon2id with Authelia's parameters (m=65536, t=3,
  p=4), so a guest account is no cheaper to attack than a homelab SSO account. A
  malformed stored hash denies access rather than panicking.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-auth CHANGELOG.md
git commit -m "feat(auth): argon2id credentials for local accounts"
```

---

## Task 3: Identity storage and effective permissions

**Files:**
- Create: `crates/gw-store/migrations/0002_identity.sql`
- Create: `crates/gw-store/src/principals.rs`
- Create: `crates/gw-store/src/acl.rs`
- Modify: `crates/gw-store/src/lib.rs`
- Modify: `crates/gw-store/src/documents.rs`
- Modify: `crates/gw-store/Cargo.toml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_auth::{Principal, PrincipalKind, Grant, Subject, Permission, Action, can}`.
- Produces:
  - `async fn Store::upsert_oidc_principal(&self, username, display_name, email, groups) -> Result<Principal>`
  - `async fn Store::create_local_principal(&self, username, display_name, email, password_hash) -> Result<Principal>`
  - `async fn Store::principal_by_username(&self, username) -> Result<Option<(Principal, Option<String>)>>` — the second element is the password hash
  - `async fn Store::set_principal_active(&self, id, active) -> Result<()>`
  - `async fn Store::create_team(&self, slug, name) -> Result<String>`
  - `async fn Store::add_team_member(&self, team_slug, principal_id) -> Result<()>`
  - `async fn Store::grants_for_path(&self, path) -> Result<Vec<Grant>>` — walks ancestors
  - `async fn Store::tree_for(&self, principal) -> Result<Vec<TreeNode>>` — **replaces `tree()`**
  - `async fn Store::document_for(&self, principal, path, action) -> Result<Option<StoredDocument>>`

- [ ] **Step 1: Write the migration**

`crates/gw-store/migrations/0002_identity.sql`:
```sql
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
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/gw-store/src/lib.rs` tests module:
```rust
    use gw_auth::{Action, Permission, Principal, Subject};

    async fn seeded() -> Store {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store.insert_document(&new_doc(None, "Handbuch", Visibility::Restricted)).await.unwrap();
        store
            .insert_document(&new_doc(Some("/handbuch"), "Onboarding", Visibility::Restricted))
            .await
            .unwrap();
        store.insert_document(&new_doc(None, "Öffentlich", Visibility::Public)).await.unwrap();
        store
    }

    #[tokio::test]
    async fn an_oidc_principal_is_created_on_first_login_and_updated_after() {
        let store = Store::open("sqlite::memory:").await.unwrap();
        let first = store
            .upsert_oidc_principal("sergej", "Sergej", None, &["users".into()])
            .await
            .unwrap();
        let second = store
            .upsert_oidc_principal("sergej", "Sergej Maul", None, &["users".into(), "admins".into()])
            .await
            .unwrap();

        assert_eq!(first.id, second.id, "the same user must not create a second principal");
        assert_eq!(second.display_name, "Sergej Maul");
        // Groups are refreshed from the verified claim: losing a group must take effect.
        assert_eq!(second.groups, vec!["users", "admins"]);
    }

    #[tokio::test]
    async fn grants_inherit_down_the_tree() {
        let store = seeded().await;
        store
            .add_grant("/handbuch", Subject::Team("editors".into()), Permission::Read)
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
            .add_grant("/handbuch", Subject::Team("editors".into()), Permission::Admin)
            .await
            .unwrap();
        store
            .add_grant("/handbuch/onboarding", Subject::Team("editors".into()), Permission::Read)
            .await
            .unwrap();

        let grants = store.grants_for_path("/handbuch/onboarding").await.unwrap();
        assert_eq!(grants.len(), 1, "the nearest ancestor with grants wins outright");
        assert_eq!(grants[0].permission, Permission::Read);
    }

    #[tokio::test]
    async fn the_tree_hides_documents_the_principal_cannot_read() {
        let store = seeded().await;
        let guest = Principal::test("guest", &[], &[]);

        let tree = store.tree_for(&guest).await.unwrap();
        let titles: Vec<&str> = tree.iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, vec!["Öffentlich"], "restricted branches must not appear at all");
    }

    #[tokio::test]
    async fn a_granted_team_member_sees_the_restricted_branch() {
        let store = seeded().await;
        store
            .add_grant("/handbuch", Subject::Team("editors".into()), Permission::Read)
            .await
            .unwrap();
        let editor = Principal::test("ed", &[], &["editors"]);

        let tree = store.tree_for(&editor).await.unwrap();
        let handbuch = tree.iter().find(|n| n.path == "/handbuch").expect("branch should appear");
        assert_eq!(handbuch.children.len(), 1, "the granted subtree comes with it");
    }

    #[tokio::test]
    async fn document_for_enforces_the_action_not_just_readability() {
        let store = seeded().await;
        store
            .add_grant("/handbuch", Subject::Team("editors".into()), Permission::Read)
            .await
            .unwrap();
        let editor = Principal::test("ed", &[], &["editors"]);

        assert!(store.document_for(&editor, "/handbuch", Action::Read).await.unwrap().is_some());
        assert!(store.document_for(&editor, "/handbuch", Action::Write).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_deactivated_principal_loses_granted_access_immediately() {
        let store = seeded().await;
        store
            .add_grant("/handbuch", Subject::Team("editors".into()), Permission::Admin)
            .await
            .unwrap();
        let mut editor = Principal::test("ed", &[], &["editors"]);
        editor.active = false;

        assert!(store.document_for(&editor, "/handbuch", Action::Read).await.unwrap().is_none());
        assert!(store.tree_for(&editor).await.unwrap().iter().all(|n| n.path != "/handbuch"));
    }
```

- [ ] **Step 3: Add the dependency and run the tests**

Add to `crates/gw-store/Cargo.toml` `[dependencies]`: `gw-auth = { path = "../gw-auth" }`.

Run: `cargo test -p gw-store`
Expected: FAIL — `no method named upsert_oidc_principal`.

- [ ] **Step 4: Implement principals and teams**

`crates/gw-store/src/principals.rs`:
```rust
use crate::Store;
use anyhow::Result;
use gw_auth::{Principal, PrincipalKind};
use sqlx::FromRow;

#[derive(FromRow)]
struct PrincipalRow {
    id: String,
    kind: String,
    username: String,
    display_name: String,
    email: Option<String>,
    groups: String,
    active: i64,
}

impl PrincipalRow {
    fn into_principal(self, teams: Vec<String>) -> Principal {
        Principal {
            id: self.id,
            kind: if self.kind == "oidc" { PrincipalKind::Oidc } else { PrincipalKind::Local },
            username: self.username,
            display_name: self.display_name,
            email: self.email,
            groups: serde_json::from_str(&self.groups).unwrap_or_default(),
            teams,
            active: self.active != 0,
        }
    }
}

impl Store {
    async fn teams_of(&self, principal_id: &str) -> Result<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT t.slug FROM teams t
             JOIN team_members m ON m.team_id = t.id
             WHERE m.principal_id = ?1 ORDER BY t.slug",
        )
        .bind(principal_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    /// Create on first login, refresh on every subsequent one.
    ///
    /// Groups are REPLACED, not merged: they mirror the verified `groups` claim, so
    /// losing a group in Authelia must take effect here at the next login. Merging would
    /// make removal impossible.
    pub async fn upsert_oidc_principal(
        &self,
        username: &str,
        display_name: &str,
        email: Option<&str>,
        groups: &[String],
    ) -> Result<Principal> {
        let groups_json = serde_json::to_string(groups)?;
        let id = uuid::Uuid::now_v7().to_string();

        sqlx::query(
            r#"
            INSERT INTO principals (id, kind, username, display_name, email, groups, last_seen_at)
            VALUES (?1, 'oidc', ?2, ?3, ?4, ?5, datetime('now'))
            ON CONFLICT (username) DO UPDATE SET
                display_name = excluded.display_name,
                email        = excluded.email,
                groups       = excluded.groups,
                last_seen_at = datetime('now')
            "#,
        )
        .bind(&id)
        .bind(username)
        .bind(display_name)
        .bind(email)
        .bind(&groups_json)
        .execute(&self.pool)
        .await?;

        self.principal_by_username(username)
            .await?
            .map(|(p, _)| p)
            .ok_or_else(|| anyhow::anyhow!("principal vanished immediately after upsert"))
    }

    pub async fn create_local_principal(
        &self,
        username: &str,
        display_name: &str,
        email: Option<&str>,
        password_hash: &str,
    ) -> Result<Principal> {
        let id = uuid::Uuid::now_v7().to_string();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO principals (id, kind, username, display_name, email) \
             VALUES (?1, 'local', ?2, ?3, ?4)",
        )
        .bind(&id)
        .bind(username)
        .bind(display_name)
        .bind(email)
        .execute(&mut *tx)
        .await?;

        sqlx::query("INSERT INTO credentials (principal_id, password_hash) VALUES (?1, ?2)")
            .bind(&id)
            .bind(password_hash)
            .execute(&mut *tx)
            .await?;

        tx.commit().await?;
        Ok(self
            .principal_by_username(username)
            .await?
            .map(|(p, _)| p)
            .expect("just inserted"))
    }

    /// Returns the principal and, for local accounts, its password hash.
    pub async fn principal_by_username(
        &self,
        username: &str,
    ) -> Result<Option<(Principal, Option<String>)>> {
        let row: Option<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, display_name, email, groups, active \
             FROM principals WHERE username = ?1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else { return Ok(None) };
        let hash: Option<(String,)> =
            sqlx::query_as("SELECT password_hash FROM credentials WHERE principal_id = ?1")
                .bind(&row.id)
                .fetch_optional(&self.pool)
                .await?;
        let teams = self.teams_of(&row.id).await?;
        Ok(Some((row.into_principal(teams), hash.map(|(h,)| h))))
    }

    pub async fn set_principal_active(&self, id: &str, active: bool) -> Result<()> {
        sqlx::query("UPDATE principals SET active = ?2 WHERE id = ?1")
            .bind(id)
            .bind(i64::from(active))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn create_team(&self, slug: &str, name: &str) -> Result<String> {
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO teams (id, slug, name) VALUES (?1, ?2, ?3)")
            .bind(&id)
            .bind(slug)
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn add_team_member(&self, team_slug: &str, principal_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO team_members (team_id, principal_id) \
             SELECT id, ?2 FROM teams WHERE slug = ?1",
        )
        .bind(team_slug)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
```

- [ ] **Step 5: Implement ACL resolution and the permission-scoped queries**

`crates/gw-store/src/acl.rs`:
```rust
use crate::{Store, StoredDocument, TreeNode};
use anyhow::Result;
use gw_auth::{can, Action, Grant, Permission, Principal, Subject};
use gw_core::Visibility;
use sqlx::FromRow;
use std::str::FromStr;

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
    Some(Grant { subject, permission })
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
        let Some(doc) = self.document_by_path(path).await? else {
            return Ok(None);
        };
        let visibility = Visibility::from_str(&doc.visibility).unwrap_or_default();
        let grants = self.grants_for_path(path).await?;
        Ok(can(principal, action, visibility, &grants).then_some(doc))
    }

    /// The navigation tree, filtered to what `principal` may read.
    ///
    /// This REPLACES `Store::tree()`, which M1 left unfiltered. A restricted title in the
    /// navigation is a disclosure even when the body is protected.
    pub async fn tree_for(&self, principal: &Principal) -> Result<Vec<TreeNode>> {
        let all = self.tree().await?;
        self.filter_tree(all, principal).await
    }

    async fn filter_tree(
        &self,
        nodes: Vec<TreeNode>,
        principal: &Principal,
    ) -> Result<Vec<TreeNode>> {
        let mut out = Vec::new();
        for mut node in nodes {
            let visibility = Visibility::from_str(&node.visibility).unwrap_or_default();
            let grants = self.grants_for_path(&node.path).await?;
            if !can(principal, Action::Read, visibility, &grants) {
                // Skipping the whole branch is correct: a child cannot be more visible
                // than a parent the caller cannot see, or the tree would leak structure.
                continue;
            }
            let children = std::mem::take(&mut node.children);
            node.children = Box::pin(self.filter_tree(children, principal)).await?;
            out.push(node);
        }
        Ok(out)
    }
}
```

Change `Store::tree` in `documents.rs` from `pub` to `pub(crate)` so no caller outside the
store can obtain an unfiltered tree, and add `pub mod acl; pub mod principals;` to
`lib.rs`.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p gw-store`
Expected: `test result: ok. 13 passed`.

- [ ] **Step 7: Lint, changelog and commit**

Add under `### Added`:
```markdown
- Identity storage: principals (OIDC or local), teams and memberships, path-scoped ACLs
  that inherit down the document tree, and an audit log.
- Permission-scoped queries. `Store::tree()` is now crate-private and replaced by
  `tree_for(principal)`, so no caller outside the store can obtain an unfiltered tree.
  The nearest ancestor with grants wins outright rather than grants unioning upward,
  which is what makes it possible to narrow access on a subtree.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-store CHANGELOG.md
git commit -m "feat(store): principals, teams, inheriting ACLs and permission-scoped queries"
```

---

## Task 4: Wiring the API to the permission engine

**Files:**
- Modify: `crates/gw-api/src/routes/mod.rs`
- Modify: `crates/gw-api/src/routes/tree.rs`
- Modify: `crates/gw-api/src/routes/docs.rs`
- Modify: `crates/gw-api/src/auth/oidc.rs`
- Modify: `crates/gw-api/tests/api.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `Store::{tree_for, document_for, upsert_oidc_principal}`.
- Produces: `AppState::principal(&CookieJar) -> Principal`; `may_read` **deleted**.

- [ ] **Step 1: Extend the integration tests**

Append to `crates/gw-api/tests/api.rs`:
```rust
#[tokio::test]
async fn a_granted_guest_reaches_only_the_granted_subtree() {
    let store = seed_with_acl().await;

    // Granted: readable.
    assert_eq!(get_as(&store, "guest", "/api/documents/handbuch").await, StatusCode::OK);
    // Not granted: refused, even though the same principal is authenticated.
    assert_eq!(get_as(&store, "guest", "/api/documents/geheim").await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn the_tree_endpoint_omits_branches_a_guest_cannot_read() {
    let store = seed_with_acl().await;
    let body = get_body_as(&store, "guest", "/api/tree").await;
    assert!(body.contains("Handbuch"));
    assert!(!body.contains("Geheim"), "an ungranted title must not appear in navigation");
}

#[tokio::test]
async fn a_deactivated_principal_is_refused_everything_but_public() {
    let store = seed_with_acl().await;
    deactivate(&store, "guest").await;
    assert_eq!(get_as(&store, "guest", "/api/documents/handbuch").await, StatusCode::FORBIDDEN);
    assert_eq!(get_as(&store, "guest", "/api/documents/oeffentlich").await, StatusCode::OK);
}
```

Implement the helpers `seed_with_acl`, `get_as`, `get_body_as` and `deactivate` alongside
the existing `seed`/`app`/`get` helpers, constructing the principal through
`AppState::for_test_principal(store, principal)`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p gw-api --test api`
Expected: FAIL — `no function or associated item named for_test_principal`.

- [ ] **Step 3: Replace `may_read` with the engine**

In `crates/gw-api/src/routes/mod.rs`: **delete** `pub fn may_read`, replace
`AppState::identity` with:
```rust
    /// The calling principal.
    ///
    /// Session cookie first, then the development shim, then anonymous. The shim can only
    /// be set on a loopback bind — `config::validate` refuses to start otherwise.
    pub async fn principal(&self, jar: &CookieJar) -> Principal {
        if let Some(cookie) = jar.get(crate::auth::SESSION_COOKIE) {
            if let Some(session) = self.sessions.get(cookie.value()) {
                // Re-read from the store on every request so deactivating an account or
                // changing a team takes effect immediately, not at the next login.
                if let Ok(Some((p, _))) =
                    self.store.principal_by_username(&session.username).await
                {
                    return p;
                }
            }
        }
        if let Some(dev) = &self.dev_principal {
            return dev.clone();
        }
        Principal::anonymous()
    }
```

Rewrite the handlers to call `store.tree_for(&principal)` and
`store.document_for(&principal, &full, Action::Read)`, mapping `None` to 404 when the
document does not exist and 403 when it exists but is not permitted:
```rust
pub async fn get_document(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
) -> Result<Json<StoredDocument>, ApiError> {
    let principal = state.principal(&jar).await;
    let full = format!("/{}", path.trim_start_matches('/'));

    // Existence is checked first so an absent path is 404 and a forbidden one is 403.
    // Collapsing both to 404 would hide configuration mistakes; collapsing both to 403
    // would confirm the existence of every path someone guesses.
    if state.store.document_by_path_unchecked(&full).await
        .map_err(ApiError::Internal)?.is_none()
    {
        return Err(ApiError::NotFound);
    }

    state
        .store
        .document_for(&principal, &full, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .map(Json)
        .ok_or(ApiError::Forbidden)
}
```

In `auth/oidc.rs`, after verifying the id token, call
`state.store.upsert_oidc_principal(&username, &display_name, email.as_deref(), &groups)` and
store only the username in the session.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p gw-api`
Expected: all green, including the three new cases.

- [ ] **Step 5: Lint, changelog and commit**

Add under `### Changed`:
```markdown
- API authorisation now goes through the permission engine. The M1 `may_read` stub is
  deleted rather than deprecated, and principals are re-read from the store on every
  request so deactivating an account takes effect immediately rather than at next login.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates/gw-api CHANGELOG.md
git commit -m "feat(api): route authorisation through the permission engine"
```

---

## Task 5: Admin console and API tokens

**Files:**
- Create: `crates/gw-api/src/routes/admin.rs`
- Create: `crates/gw-api/src/routes/tokens.rs`
- Create: `crates/gw-store/migrations/0003_tokens.sql`
- Create: `web/src/routes/admin/+page.server.ts`
- Create: `web/src/routes/admin/+page.svelte`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Produces:
  - `GET/POST /api/admin/principals`, `POST /api/admin/principals/{id}/active`
  - `GET/POST /api/admin/teams`, `POST /api/admin/teams/{slug}/members`
  - `GET/POST/DELETE /api/admin/acl`
  - `GET /api/admin/audit`
  - `POST /api/tokens` → the token, shown **once**; `GET /api/tokens`; `DELETE /api/tokens/{id}`
  - Every admin route requires `Action::Admin` on the affected path, or membership of the
    `admins` OIDC group for instance-wide operations.

- [ ] **Step 1: Write the migration**

`crates/gw-store/migrations/0003_tokens.sql`:
```sql
CREATE TABLE api_tokens (
    id           TEXT PRIMARY KEY,
    principal_id TEXT NOT NULL REFERENCES principals(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- SHA-256 of the token. The plaintext is shown once at creation and never stored,
    -- so a database disclosure does not hand over working credentials.
    token_hash   TEXT NOT NULL UNIQUE,
    -- Highest action this token may perform, regardless of the principal's own rights.
    max_action   TEXT NOT NULL DEFAULT 'read'
                 CHECK (max_action IN ('read','comment','write','admin')),
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at TEXT,
    expires_at   TEXT
);

CREATE INDEX api_tokens_principal ON api_tokens (principal_id);
```

- [ ] **Step 2: Write the failing tests**

Append to `crates/gw-api/tests/api.rs`:
```rust
#[tokio::test]
async fn a_non_admin_cannot_reach_the_admin_api() {
    let store = seed_with_acl().await;
    assert_eq!(get_as(&store, "guest", "/api/admin/principals").await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_admin_can_list_principals() {
    let store = seed_with_acl().await;
    assert_eq!(get_as(&store, "sergej", "/api/admin/principals").await, StatusCode::OK);
}

#[tokio::test]
async fn a_token_never_exceeds_its_max_action() {
    // A read-scoped token held by an admin must not be able to write. The token's ceiling
    // is the point of scoping it.
    let store = seed_with_acl().await;
    let token = create_token(&store, "sergej", "read").await;
    assert_eq!(post_with_token(&store, &token, "/api/admin/teams").await, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn an_expired_token_is_rejected() {
    let store = seed_with_acl().await;
    let token = create_expired_token(&store, "sergej").await;
    assert_eq!(
        get_with_token(&store, &token, "/api/admin/principals").await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn admin_mutations_are_written_to_the_audit_log() {
    let store = seed_with_acl().await;
    create_team_as(&store, "sergej", "reviewers").await;
    let entries = store.recent_audit(10).await.unwrap();
    assert!(entries.iter().any(|e| e.action == "team.create" && e.target.as_deref() == Some("reviewers")));
}
```

- [ ] **Step 3: Run the tests to verify they fail, then implement**

Run: `cargo test -p gw-api --test api`
Expected: FAIL — the admin routes do not exist.

Implement `admin.rs` with an `require_admin` extractor that calls
`can(&principal, Action::Admin, ...)` and returns `ApiError::Forbidden` otherwise; every
mutating handler writes an `audit_log` row in the same transaction as its change, so an
action cannot succeed without being recorded. Implement `tokens.rs` with creation returning
`{"token": "gw_<64 hex chars>"}` exactly once, storage of only the SHA-256, and an
authentication path that rejects an expired or unknown token with 401 and clamps the
effective action to `max_action`.

- [ ] **Step 4: Build the admin console**

`web/src/routes/admin/+page.svelte`: three panels — principals (list, create local account,
activate/deactivate), teams (list, create, add and remove members), and access (grants per
path). Each destructive action confirms. Tables carry `<caption>`, `scope` on headers and
`aria-sort` on sortable columns.

Include a prominent link to `https://accounts.ohje.ooguy.com` with the text *"Homelab SSO
accounts are managed in the accounts app"* — great-wiki manages its own principals, teams
and permissions, never Authelia's user database (ADR 0002).

- [ ] **Step 5: Run the gate, changelog and commit**

Run: `just ci`
Expected: green.

Add under `### Added`:
```markdown
- Admin console and API: manage principals, local guest accounts, teams, memberships and
  path-scoped grants, with every mutation written to the audit log in the same
  transaction as the change it records.
- API tokens with a per-token action ceiling and an expiry. Only the SHA-256 is stored,
  and the plaintext is shown exactly once.
```

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
git add crates web CHANGELOG.md
git commit -m "feat(admin): principal, team and access management with audited mutations"
```

---

---

## Task 6: OIDC login, and removing the edge gate

**Files:** `crates/gw-api/src/auth/{mod,oidc,session}.rs`, `crates/gw-store/migrations/0004_sessions.sql`,
`web/src/routes/auth/`, and `Server/server/network/opnsense/caddy.d/10-services.conf`.

**Produces:** `GET /auth/login`, `GET /auth/callback`, `POST /auth/logout`, `GET /api/me`;
sessions persisted in SQLite; `AppState::principal` reading the session cookie.

Authorization-code flow with PKCE against Authelia. The client `great-wiki` is already
registered with both redirect URIs, `two_factor`, and the `groups` scope. The client secret
is in the Server repo's secret store as `server__coding__great-wiki__.env`.

**On callback:** verify the id token, read `preferred_username` and the `groups` claim,
`upsert_oidc_principal`, and apply the D-M2-1 mapping. **Groups are replaced on every login,
never merged** — losing a group in Authelia must take effect here at the next sign-in, and
merging would make removal impossible.

**Sessions go in SQLite, not memory** — M1's plan deferred this, but attribution (D-M2-5)
means a restart must not silently reattribute in-flight work to nobody.

**The last step is deleting the two `import authelia` lines** from the wiki site blocks and
reloading Caddy. Before doing so, prove in this order: a published page is readable
anonymously; a restricted page is not; signing in yields the right groups at `/api/me`; a
local guest account signs in *without* Authelia. If any fails, the gate stays.

---

## Task 6b: One sign-in page, and the rate limiting it now requires

**Produces:** a rendered `GET /auth/login` offering both mechanisms, `POST /auth/local`,
and per-account plus per-IP throttling.

Follows from D-M2-11. `/auth/login` currently 302s straight to Authelia; it becomes a page
with a homelab button (which performs today's redirect) and a guest username/password form.

**This task exists because that form is publicly reachable.** The earlier plan hid guest
login behind an invite link, so nothing advertised that password authentication existed.
One visible button is better for the person signing in and strictly worse for exposure, and
the throttling is the price of the trade rather than a later hardening pass.

Tests, each of which must fail before it passes:

- A wrong password and an unknown username produce the **same** response, in the same
  time envelope — otherwise the form enumerates who has an account.
- N failures for one username lock that account's attempts regardless of source address,
  so a distributed attempt on one account is still throttled.
- N failures from one address are throttled across usernames, so a spray across many
  accounts is too.
- Throttling never applies to the Authelia path: a homelab sign-in must not be blockable
  by somebody else guessing at guest passwords.
- A successful sign-in clears the counter for that account.


## Task 7: Invites and space-scoped account creation

**Produces:** `invite` table (token hash, scope, inviter, expiry, single-use),
`POST /api/admin/invites`, `GET /auth/invite/{token}`, `POST /auth/invite/{token}/accept`.

An invite may grant **only into spaces the inviter administers** (D-M2-2) — enforced
server-side against the permission engine, not by hiding options in the interface. Tokens
are stored hashed, single-use, and expiring; accepting one creates a local principal whose
password the recipient chooses (D-M2-3).

Tests: an invite scoped to a space the inviter does *not* administer is rejected; a used
token cannot be reused; an expired token is refused; the created principal can reach the
invited space and nothing else.

---

## Task 8: "What can this person see?"

**Produces:** `GET /api/admin/view-as/{principal}/tree` and a console view.

Runs the real permission engine under a substituted principal. Three properties, each
tested:

- **Read-only.** The substitution must be impossible to carry into a write — a mutating
  request while viewing-as is refused outright, not silently performed as the real user.
- **No escalation.** Viewing as someone must never reveal more than they can see, even to
  an admin who could see it anyway by other means; the output is exactly their view.
- **Audited.** Every activation is written to the audit log with both identities.

The interface must state whose view is shown, persistently and unmissably.

## Milestone exit criteria

Checked 2026-08-11. A box is ticked only where a test proves it; where the proof is a
test rather than a live system, the test is named, because "passes in CI" and "works on
the box" are different claims and this list has to say which one is being made.

- [x] `just ci` passes. — 422 Rust tests, 92 web tests, fmt, clippy, `npm run check`, the
      production build and the secret scan, all green. 59 mutations, all as expected.
- [x] A first-time Authelia sign-in gets exactly the reach its group implies — `admins`
      everything, `users` public plus internal, anyone else public only. — `gw-store`
      `acl.rs` unit tests. **Against the real Authelia this has not been run**; see below.
- [x] A space admin can invite a guest into their own space and **cannot** scope an invite
      to a space they do not administer. — `tests/invites.rs`, including the separate rule
      that attaching a *team* needs an instance admin.
- [x] A guest signs in **without Authelia** and reaches only what they were granted. —
      `tests/local_login.rs` and `tests/milestone_m2.rs`.
- [x] Viewing-as another principal shows exactly their view, refuses writes, and is
      audited. — `tests/view_as.rs`, 15 tests; the read tests assert *equality* with what
      the target sees directly, and the refusal is tested against routes that do not exist.
- [~] **The `import authelia` lines are gone from both wiki site blocks** — done, in
      `Server/server/network/opnsense/caddy.d/10-services.conf` — but "a published page is
      readable anonymously, and a restricted one is not" is **unverified in production**:
      `wiki.ohje.ooguy.com` still points at `cloud.vm:8100`, which serves nothing.
- [x] A local guest account can be created in the admin console, added to a team, granted
      read on one subtree, and signs in. — `tests/milestone_m2.rs`, through the HTTP API
      only.
- [x] That guest sees **only** the granted subtree plus public pages, in the tree and by
      direct URL. — same test, both doors, and it ends by removing the membership to show
      the reach goes with it.
- [x] Deactivating the guest takes effect on the next request, not the next login. —
      `tests/api.rs::a_deactivated_principal_is_refused_everything_but_public`.
- [ ] A read-scoped API token cannot perform a write, even for an admin. — **deferred to
      M14 by owner decision (2026-08-10).** Not implemented, not attempted.
- [x] `grep -rn "may_read" crates/` returns nothing — the M1 stub is gone, not shadowed.
- [x] Every admin mutation appears in `/api/admin/audit`. — and as of 2026-08-11 the list
      that checks this is itself checked: `every_mutating_admin_route_is_covered_by_the_-
      audit_list_or_explicitly_exempt` reads the route table out of the source, because the
      hand-maintained list had silently missed three new endpoints.

## Self-Review

**Spec coverage.** Implements spec §4 (identity, access), §6.2 (one authorisation
function, deny by default) and the "create and remove users, edit groups, teams"
requirement, resolved per ADR 0002 as great-wiki owning principals, teams and permissions
while homelab SSO accounts stay in the accounts app.

**Placeholders.** Tasks 1–4 carry complete code. Task 5's admin console is specified by
its panels, behaviours and accessibility requirements rather than full Svelte, because it
is presentation over interfaces fully defined in Tasks 3 and 4 — the tests that gate it are
given in full.

**Type consistency.** `Action` and `Permission` are separate types on purpose: what a
caller attempts versus what a grant confers. `Subject` variants map one-to-one to the
`subject_kind` CHECK constraint. `Principal` field names match between `gw-auth`,
`PrincipalRow::into_principal` and the API's JSON. `can()` has one definition and one call
path per query.

**The invariant this milestone exists to establish:** after M2, no code outside `gw-store`
can obtain an unfiltered document or tree. `Store::tree` is `pub(crate)`; every public
accessor takes a `Principal`. Later milestones inherit that rather than re-deriving it.
