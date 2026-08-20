#!/usr/bin/env bash
#
# Mutation testing for the code where a wrong answer is a disclosure.
#
# WHY THIS EXISTS
# ---------------
# A passing test suite says the code does something. It does not say the tests would
# notice if it stopped. Every security bug this project has shipped so far was under a
# green suite, and each was found by breaking the code on purpose:
#
#   - `a_mismatched_state_is_refused` passed because the OIDC flow died at a LATER nonce
#     check. Disabling the CSRF defence entirely failed no test.
#   - a forged-session test whose store held no principals, so any token would have failed
#     and the test asserted nothing about forgery.
#   - `an_anonymous_caller_reads_nothing` passed because no grant matched ANY subject, so
#     deleting the authentication check from the audit reader still passed.
#
# All three looked completely reasonable. Nothing but a mutation exposes them, so the
# mutations live here rather than in someone's shell history.
#
# HOW TO READ A RESULT
# --------------------
#   KILLED   the suite noticed. That is the pass condition.
#   SURVIVED the code was broken and every test still passed. Either the mutation is
#            genuinely equivalent — in which case record it below as `equivalent` with the
#            reason — or a test is missing. Assume the second until you have shown the first.
#
# Usage:  scripts/mutate.sh            run every mutation
#         scripts/mutate.sh audit      run those whose description matches "audit"

set -euo pipefail
cd "$(dirname "$0")/.."

# Each mutation is a function call, not a delimited string. The first version of this
# script parsed `file|tests|expectation|expr|description` with IFS='|', and the very
# first permission-engine mutation contained `||` — which split into empty fields and
# fed sed a truncated expression. A separator that can appear in the data is not a
# separator.
#
# `expectation` is `killed` for a mutation the suite must catch, or `equivalent` for one
# it provably cannot because the change has no observable effect. An `equivalent` entry
# must carry its reason in the description — otherwise it is indistinguishable from a gap
# somebody gave up on.
MUTATIONS=()
mutation() { MUTATIONS+=("$1"$'\x1f'"$2"$'\x1f'"$3"$'\x1f'"$4"); }

# --- visibility: the one thing that publishes a page to the internet ----------------
#
# `documents.visibility` had no write path at all until now: the value arrived from
# frontmatter at import, and `seed --update` compares it and REFUSES to change it,
# precisely so a stray `visibility: public` in a bulk file drop cannot publish a page with
# nobody watching. `/api/admin/visibility` is the deliberate single-path alternative, and
# everything that made the refusal worth having applies to it doubled: this wiki is
# internet-facing, `public` means the open internet, and the pages it protects are a
# child's medical records.
#
# The gate is `path_admin` — admin on the page's own path — and the first mutation is the
# one that matters. Swapped for `signed_in` it is not merely weaker, it is no
# authorisation at all: anybody with an account could publish any page. `leser` in the
# fixture holds `read` on `/raum` and nothing else, and `gast` is given `write` in the same
# test, so both the read and the write case are covered by a signed-in caller who must
# still be refused. Without those grants no subject would match anything and the test
# would pass with the gate deleted — the vacuous shape this whole file exists to catch.
#
# `sed` address range rather than a bare substitution: `grant` and `revoke` carry the
# identical gate line, and a global replacement would mutate three endpoints at once and
# prove nothing about any of them.
mutation crates/gw-api/src/routes/admin.rs killed \
  '/pub async fn set_visibility/,/^}$/ s/let actor = path_admin(&state, &jar, &body.path).await?;/let actor = signed_in(\&state, \&jar).await?;/' \
  'visibility: publishing a page needs admin on it — being able to read or write must not widen it'
# Fail closed on a value this code does not understand. The mutation writes the tempting
# version — parse, and fall back to the default — which is safe TODAY only because
# `Visibility::default()` happens to be `Restricted`, and is one enum reordering away from
# publishing a page nobody asked to publish.
mutation crates/gw-api/src/routes/admin.rs killed \
  's/    let Ok(visibility) = Visibility::from_str(&body.visibility) else {/    let visibility = Visibility::from_str(\&body.visibility).unwrap_or_default(); if false {/' \
  'visibility: an unrecognised value is refused rather than quietly defaulted'
# And the record. A visibility change with no audit row is the one administrative act in
# this system that leaves no trace anywhere: `documents` keeps only the current value, and
# `updated_at` is deliberately not touched, so the log is the ONLY place "who published
# this, and what was it before" survives.
mutation crates/gw-store/src/admin.rs killed \
  's/            "document.visibility",/            "dokument.sichtbarkeit",/' \
  'visibility: the change writes an audit row under the name the console reads back'
mutation crates/gw-store/src/admin.rs killed \
  's/&json!({ "from": from, "to": visibility.as_str() }),/\&json!({ "from": visibility.as_str(), "to": visibility.as_str() }),/' \
  'visibility: the row records what the page WAS, not the new value twice'

# --- the audit log: who may read who-did-what -------------------------------------
mutation crates/gw-store/src/audit.rs killed \
  's/can(principal, Action::Admin, Visibility::Restricted/can(principal, Action::Read, Visibility::Restricted/' \
  'audit: reading the log needs admin, not merely read'
mutation crates/gw-store/src/audit.rs killed \
  's/if !principal.is_authenticated() {/if false {/' \
  'audit: the authentication check actually gates the read'
mutation crates/gw-store/src/audit.rs killed \
  's/>= crate::Baseline::Admin/>= crate::Baseline::Public/' \
  'audit: instance-wide entries need the admin baseline'
mutation crates/gw-store/src/audit.rs equivalent \
  's/WHERE path IS NOT NULL ORDER BY/ORDER BY/' \
  "audit: SQL predicate is defence in depth — the loop's own 'else continue' already skips unscoped entries"

# --- the permission engine: one can() decides everything ---------------------------
mutation crates/gw-auth/src/permission.rs killed \
  's/if !principal.is_authenticated() || !principal.active {/if false {/' \
  'permission: authentication and activity gate everything below'
mutation crates/gw-auth/src/permission.rs killed \
  's/if action == Action::Read \&\& visibility == Visibility::Internal {/if visibility == Visibility::Internal {/' \
  'permission: internal reach confers READ only, never write'
mutation crates/gw-auth/src/permission.rs killed \
  's/if action == Action::Read \&\& visibility == Visibility::Public {/if visibility == Visibility::Public {/' \
  'permission: a public document is readable, not writable'
mutation crates/gw-auth/src/permission.rs equivalent \
  's/Subject::Anyone => false, \/\/ already handled above/Subject::Anyone => true,/' \
  'permission: Anyone in the final loop is unreachable-by-value — the early pass already returned true for every Anyone grant that satisfies the action, so the survivors all fail satisfies() anyway'

# --- the admin API: who may administer what ----------------------------------------
#
# The first of these is the one that matters. `can()` answers an `Anyone` grant BEFORE it
# checks whether the caller is signed in — that is what a public share link is — so on a
# path carrying `anyone: admin` the engine alone would hand the ACL editor to an
# anonymous request. The gates check authentication first, and
# `an_anonymous_caller_is_refused_even_where_anyone_holds_admin` is what proves it: without
# that grant in the fixture, no subject would match anything and the test would pass with
# the check deleted.
mutation crates/gw-api/src/routes/admin.rs killed \
  's/if !principal.is_authenticated() || !principal.active {/if false {/' \
  'admin: the gates check authentication before any grant is consulted'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/if baseline >= Baseline::Admin {/if baseline >= Baseline::Public {/' \
  'admin: instance-wide operations need the admin baseline, not merely an account'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^        >= Baseline::Admin$/        >= Baseline::Public/' \
  'admin: the instance-admin bypass on a path is not a bypass for everybody'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^    if can(&principal, Action::Admin, Visibility::Restricted, &grants) {/    if can(\&principal, Action::Read, Visibility::Restricted, \&grants) {/' \
  'admin: administering a space needs admin on it, not merely read'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^        if can(&principal, Action::Admin, Visibility::Restricted, &grants) {/        if can(\&principal, Action::Read, Visibility::Restricted, \&grants) {/' \
  'admin: reading the audit log needs admin on a path, not merely read'

# --- audited mutations: the record and the change stand or fall together ------------
mutation crates/gw-store/src/admin.rs killed \
  's/if !written {/if false {/' \
  'admin store: a membership that changed nothing is not recorded as a change'
mutation crates/gw-store/src/admin.rs killed \
  's/if existing.is_some() {/if false {/' \
  'admin store: a grant that was already there is not recorded as a new one'
mutation crates/gw-store/src/admin.rs killed \
  's/if !removed {/if false {/' \
  'admin store: a removal that removed nothing is not reported or recorded as done'
# Revocation is the half of access control that is never exercised by using the system, so
# it is the half that can be broken without anybody noticing: everything keeps working, and
# what breaks is somebody keeping reach they were meant to lose. The M2 exit criterion rests
# on it — "provably cannot reach anything else" is only proven if leaving the team takes the
# reach with it. Killed by `milestone_m2`, which is in gw-api rather than gw-store, so the
# probe misses it and the full suite is what catches it. That is the probe working as
# designed, not a gap.
mutation crates/gw-store/src/admin.rs killed \
  's/            sqlx::query("DELETE FROM team_members WHERE team_id = ?1 AND principal_id = ?2")/            sqlx::query("SELECT 1 FROM team_members WHERE team_id = ?1 AND principal_id = ?2")/' \
  'admin store: leaving a team actually deletes the membership, rather than reporting that it did'

# --- the administrative interlock: the instance always keeps one administrator -------
#
# The failure this guards is not a disclosure but a lock-out, and it is permanent: an
# administrator who deactivates the last active administrator — themselves included —
# leaves nobody able to reach the console, and great-wiki cannot promote anybody in
# Authelia to fix it (ADR 0002). There is no in-app recovery, so the refusal is the whole
# defence and it must be impossible to break quietly.
#
# The first two mutations are the interlock itself. The third and fourth are the two ways
# the COUNT can be wrong while the refusal still looks implemented: counting a deactivated
# account, and reading a promotion as if it applied to everybody rather than to the one
# principal it names.
mutation crates/gw-store/src/admin.rs killed \
  's/if !active \&\& !leaves_an_administrator(\&mut tx).await? {/if false {/' \
  'admin store interlock: deactivating the last active administrator is refused'
mutation crates/gw-store/src/admin.rs killed \
  's/if !admin \&\& !leaves_an_administrator(\&mut tx).await? {/if false {/' \
  'admin store interlock: demoting the last administrator is refused by the same rule'
mutation crates/gw-store/src/acl.rs killed \
  's/SELECT id, groups FROM principals WHERE active = 1/SELECT id, groups FROM principals/' \
  'acl interlock: the floor counts administrators who can still sign in'
mutation crates/gw-store/src/acl.rs killed \
  's/SELECT 1 FROM instance_admins WHERE principal_id = ?1/SELECT 1 FROM instance_admins/' \
  'acl interlock: a per-account promotion promotes ONE principal, never everybody'
mutation crates/gw-store/src/acl.rs killed \
  's/    if promoted(\&mut \*conn, principal_id).await? {/    if false {/' \
  'acl interlock: the per-account promotion confers the admin baseline with no groups'
mutation crates/gw-store/src/admin.rs killed \
  's/if !apply_instance_admin(\&mut tx, id, admin, actor).await? {/if false {/' \
  'admin store interlock: a promotion that changed nothing is not recorded as a change'
mutation crates/gw-store/src/principals.rs killed \
  's/ORDER BY last_active_at DESC, p.username ASC/ORDER BY last_active_at ASC, p.username ASC/' \
  'interlock candidates: the successors who have actually been here lately come first'
mutation crates/gw-store/src/principals.rs killed \
  's/^            if crate::acl::baseline_on(\&mut conn, \&id, \&groups).await? >= crate::Baseline::Admin {$/            if false {/' \
  'interlock candidates: somebody who already administers the instance is not a candidate'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/^        instance_admin,$/        instance_admin: body.admin,/' \
  'admin interlock: "still an administrator" is asked of baseline_for, not assumed from the request'

# --- invites: a link that creates an account -----------------------------------------
#
# An invite is a CREDENTIAL, and an unusual one: it is handed to somebody who has no
# account yet, over a channel nobody controls, and redeeming it both creates a principal
# and grants it access. Four things have to hold, and each is a mutation below.
#
#   1. SCOPE. D-M2-2 — only into spaces the inviter administers, and a team only for an
#      instance admin, because a team's reach is bounded by no path at all.
#   2. SINGLE USE. One link, one account, even when two accepts arrive together.
#   3. ONE ANSWER FOR FOUR STATES. Unknown, expired, revoked and spent must be
#      indistinguishable, or the endpoint reports which tokens exist. The three state
#      mutations below each make one of them readable as live, which is what a
#      distinguisher looks like from the outside.
#   4. THE LINK IS NEVER STORED. Only its SHA-256, exactly as for a session.
mutation crates/gw-api/src/routes/admin.rs killed \
  's/        (Some(path), None) => path_admin(&state, &jar, path).await?,/        (Some(_path), None) => signed_in(\&state, \&jar).await?,/' \
  'invites: a path invite may only be written by somebody who administers that path'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/        (_, Some(_)) => instance_admin(&state, &jar).await?,/        (_, Some(_)) => signed_in(\&state, \&jar).await?,/' \
  'invites: a team invite is instance admins only — a team reaches beyond any one space'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/        Some(path) => path_admin(&state, &jar, path).await,/        Some(_path) => signed_in(\&state, \&jar).await,/' \
  'invites: revoking one is gated by the scope of the invite itself'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/    let principal = signed_in(&state, &jar).await?;/    let principal = state.principal(\&jar).await;/' \
  'invites: listing them establishes there is a caller before the retriever filters'
mutation crates/gw-api/src/routes/admin.rs killed \
  's/            &hash_token(&token),/            \&token,/' \
  'invites: the store is handed the digest, never the link'
mutation crates/gw-store/src/invites.rs killed \
  's/             WHERE id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL/             WHERE id = ?1 AND revoked_at IS NULL/' \
  'invites: consumption is single-use, and atomically so'
mutation crates/gw-store/src/invites.rs killed \
  's/        if self.accepted_at.is_some() {/        if false {/' \
  'invites: a spent invite does not read as live'
mutation crates/gw-store/src/invites.rs killed \
  's/        } else if self.revoked_at.is_some() {/        } else if false {/' \
  'invites: a revoked invite does not read as live'
mutation crates/gw-store/src/invites.rs killed \
  's/        } else if self.expired != 0 {/        } else if false {/' \
  'invites: an expired invite does not read as live'
mutation crates/gw-store/src/invites.rs killed \
  's/        if row.state() != InviteState::Pending {/        if false {/' \
  'invites: only a pending invite is offered to the person holding the link'
mutation crates/gw-store/src/invites.rs killed \
  's/        if self.baseline_for(principal).await? >= Baseline::Admin {/        if true {/' \
  'invites: the listing is filtered per path, not handed over wholesale'
mutation crates/gw-store/src/invites.rs killed \
  's/        if !principal.is_authenticated() || !principal.active {/        if false {/' \
  'invites: the listing refuses an anonymous caller before any row is read'
mutation crates/gw-store/src/invites.rs killed \
  's/            let subject = Subject::Principal(principal_id.clone());/            let subject = Subject::Principal("niemand".to_string());/' \
  'invites: the grant an acceptance writes names the account it just created'
mutation crates/gw-store/src/invites.rs killed \
  's/INSERT OR IGNORE INTO team_members (team_id, principal_id) VALUES (?1, ?2)/INSERT OR IGNORE INTO team_members (team_id, principal_id) VALUES (?2, ?1)/' \
  'invites: a team-carrying invite really joins the team'
mutation crates/gw-api/src/auth/invite.rs killed \
  's/            &hash_token(&session),/            \&session,/' \
  'invites: the session an acceptance issues is stored as a digest, never as the cookie'
mutation crates/gw-api/src/auth/invite.rs killed \
  "s/            '<' => out.push_str(\"&lt;\"),/            '<' => out.push('<'),/" \
  'invites: the page escapes names chosen by somebody else'
mutation crates/gw-api/src/auth/invite.rs equivalent \
  's/        Ok(Some(offer)) if constant_time_eq(&offer.token_hash, &presented) => Some(offer),/        Ok(Some(offer)) => Some(offer),/' \
  'invites: the constant-time confirmation is unreachable-by-value — the SELECT already matched on the digest, so a row that came back cannot carry a different one; it is there so no code path in this file compares a secret with ==, and so that a future non-indexed lookup cannot become a timing oracle by omission'

# --- the password policy, reached through the invite page as well as the console ------
#
# Both halves of D-M2-16, and the second one is the reason this pair is here at all.
#
# It was killable by exactly one test for a while, and the history is the point: the admin
# API's own breached-password test submitted "password" — EIGHT characters against a floor
# of twelve — so it was refused by the length check and passed whether or not the corpus
# was ever consulted, while its comment claimed the opposite. Its stub was wrong in a
# second way that hid the first: it answered with the digest of "password" whatever was
# asked, so it reported nothing else as breached and lengthening the password alone would
# have turned the test red rather than honest. Both were fixed on 2026-08-11, so two tests
# now kill the corpus mutation — `a_breached_password_is_refused_and_creates_nothing` in
# tests/invites.rs and `an_administrator_cannot_create_an_account_with_a_breached_password`
# in tests/admin.rs. If either stops killing it, that test has gone vacuous again.
mutation crates/gw-auth/src/password.rs killed \
  's/    validate_password_strength(plain)?;/    let _ = validate_password_strength(plain);/' \
  'password policy: the length floor refuses a password somebody is SETTING'
mutation crates/gw-auth/src/password.rs killed \
  's/        Ok(times) => Err(PasswordError::Breached { times }),/        Ok(_times) => Ok(BreachCheck::Clean),/' \
  'password policy: a password the corpus knows is refused, not merely looked up'

# --- view-as: the permission engine, run as somebody else (D-M2-17) -------------------
#
# This mode deliberately makes a request answer as a DIFFERENT principal, so every defence
# below is holding back something the code is otherwise built to do. Three of them are the
# ones the decision names, and each has a shape that looks fine while doing nothing:
#
#   1. The refusal is a LAYER, applied after every route so it wraps the 404 fallback too.
#      Deleted, the suite must notice via a path that does not exist — a per-handler check
#      would pass every test written today and fail open for every handler written later.
#   2. The SUBSTITUTION must actually replace the principal. Skipped, the administrator
#      sees their own documents while the banner says otherwise, which is the one outcome
#      worse than the feature not existing: it answers "what can this person see?" with
#      somebody else's answer.
#   3. The cookie must NAME a server-side record rather than carry the target. The fourth
#      mutation writes the naive design — `view_as=<principal id>` — and it must die,
#      because that design is a privilege escalation anything on the network can perform.
#
# The rest guard the ways the mode can quietly outlive its authority: a record that is not
# checked against the caller, a viewer who has been demoted since, a target who has been
# deactivated since.
mutation crates/gw-api/src/view_as.rs killed \
  's/    if method == Method::GET || method == Method::HEAD || is_exit(method, request.uri().path())/    if true/' \
  'view-as: the non-GET block actually refuses mutating requests, before routing'
mutation crates/gw-api/src/routes/mod.rs killed \
  's/            Some(active) => (active.principal, source),/            Some(_) => (real, source),/' \
  'view-as: the substitution actually replaces the principal the engine runs as'
mutation crates/gw-api/src/view_as.rs killed \
  's/    if !real.is_authenticated() || !real.active || real.id != record.viewer_id {/    if false {/' \
  'view-as: the record is bound to the administrator who created it'
mutation crates/gw-api/src/view_as.rs killed \
  's|    let record = state.view_as.lookup(jar.get(VIEW_AS_COOKIE)?.value())?;|    let record = Record { viewer_id: real.id.clone(), target_id: jar.get(VIEW_AS_COOKIE)?.value().to_string(), expires_at: Instant::now() + Duration::from_secs(60) };|' \
  'view-as: the cookie names a server-side record rather than carrying the target itself'
mutation crates/gw-api/src/view_as.rs killed \
  's/^        Ok(baseline) if baseline >= Baseline::Admin$/        Ok(baseline) if baseline >= Baseline::Public/' \
  'view-as: substituting needs the admin baseline on EVERY request, not only at the start'
mutation crates/gw-api/src/view_as.rs killed \
  's/            let usable = still_admin \&\& stored.active;/            let usable = true;/' \
  'view-as: a substitution that cannot be completed falls back to nobody, never to the viewer'
mutation crates/gw-api/src/view_as.rs killed \
  's/    if !target.active {/    if false {/' \
  'view-as: a deactivated account cannot be viewed as'
mutation crates/gw-api/src/view_as.rs killed \
  's/    method == Method::POST \&\& path == EXIT_PATH/    method == Method::POST \&\& path.starts_with(EXIT_PATH)/' \
  'view-as: the exit exemption is one exact path, not a prefix that covers what is mounted under it later'
mutation crates/gw-api/src/view_as.rs killed \
  's/            "view-as.start",/            "view-as.begonnen",/' \
  'view-as: activation writes an audit row at all'
mutation crates/gw-api/src/view_as.rs killed \
  's/                "viewer_id": actor.id,/                "viewer_id": target.id,/' \
  'view-as: the audit row names the ADMINISTRATOR as well as the person being viewed as'
mutation crates/gw-api/src/view_as.rs killed \
  's/            "view-as.stop",/            "view-as.beendet",/' \
  'view-as: leaving the mode is recorded too, so the window has a known end'
# This one SURVIVED when the mode was first written, and it is the reason `Registry` carries
# its lifetime as a field rather than reading the constant. The deadline was thirty minutes,
# no test could outlive it, and so deleting the filter — turning a bounded window into a
# substitution that never ends — broke nothing. It is also what a `view-as.start` row with
# no matching stop row rests on: "until the deadline at the latest" is only true if there
# is one. Killed by `a_substitution_does_not_outlive_its_deadline`, which is the only test
# that constructs a registry it can outlive.
mutation crates/gw-api/src/view_as.rs killed \
  's/            .filter(|record| record.expires_at > Instant::now())//' \
  'view-as: a substitution stops resolving once its deadline has passed'

# --- default reach by visibility ---------------------------------------------------
mutation crates/gw-store/src/acl.rs killed \
  's/Visibility::Internal => baseline >= Baseline::Internal,/Visibility::Internal => true,/' \
  'acl: internal documents are not readable by everyone'
mutation crates/gw-store/src/acl.rs killed \
  's/Visibility::Restricted => baseline >= Baseline::Admin,/Visibility::Restricted => true,/' \
  'acl: restricted documents are not readable by everyone'

# --- revisions: the append-only history under every page -----------------------------
#
# Two different kinds of wrong answer live here, and both are below.
#
# The first is an ordinary disclosure. A revision body IS page content — it is the page as
# it was last Tuesday — so `revisions_for` and `revision_for` are retrievers in the sense
# architecture rule 2 means, and handing one to somebody who cannot read the page is the
# same leak as handing them the page. Both gates go through `document_for`, so what these
# mutations really check is that the call is still there and still asks for the right
# action.
#
# The second is subtler and has no equivalent elsewhere in this file: a revision is a
# RECORD OF WHO. The byline is what a reader trusts to answer "who wrote this", and three
# things have to hold for that trust to be warranted — the author is the authenticated
# principal and not a name a caller supplied; the id recorded is the principal's id; and
# the name is the display name as it was at the time, which is what makes attribution
# survive the account being deleted (D-M3-4). Each is one mutation.
#
# The anonymous mutation needs the same fixture trick the audit log needed. `can()` answers
# an `Anyone` grant BEFORE it looks at authentication — that is what a public share link is
# — so `an_anonymous_caller_cannot_publish_even_where_anyone_may_write` puts `anyone: write`
# on the path deliberately. Without that grant no subject would match, the publish would be
# refused by the permission check, and the test would pass with the authentication check
# deleted: the right assertion for the wrong reason, which is the failure this whole file
# exists to catch.
mutation crates/gw-store/src/revisions.rs killed \
  's/        if !self.may(author, document_id, Action::Write).await? {/        if !self.may(author, document_id, Action::Read).await? {/' \
  'revisions: publishing needs WRITE on the page, never merely read (D-M2-8)'
mutation crates/gw-store/src/revisions.rs killed \
  's/        if !author.is_authenticated() || !author.active {/        if false {/' \
  'revisions: an edit is attributed to a signed-in account, established before any grant is consulted'
mutation crates/gw-store/src/revisions.rs killed \
  's/        if !self.may(principal, document_id, Action::Read).await? {/        if false {/' \
  "revisions: a page's history is handed only to somebody who may read the page (D-M3-5)"
mutation crates/gw-store/src/revisions.rs killed \
  's/        if !readable {/        if false {/' \
  'revisions: one revision body is gated by the same read as the page it belongs to'
mutation crates/gw-store/src/revisions.rs killed \
  's/        .bind(&author.id)/        .bind(\&author.username)/' \
  'revisions: the author recorded is the principal, by id — the thing a rename cannot move'
mutation crates/gw-store/src/revisions.rs killed \
  's/        .bind(byline(author))/        .bind(author.username.as_str())/' \
  'revisions: the byline is the display name as it was then, which is what survives deletion'
# Not a disclosure, but the two ways a history can be quietly wrong about ITSELF. The
# timeline and the parent chain are read against each other by diff, restore and blame; if
# they disagree, every one of those answers something else's question.
mutation crates/gw-store/src/revisions.rs killed \
  's/WHERE document_id = ?1 ORDER BY created_at DESC, id DESC/WHERE document_id = ?1 ORDER BY created_at DESC/' \
  "revisions: the timeline breaks ties on the uuid v7 id — datetime('now') is per-second, so two edits in one second are otherwise unordered"
mutation crates/gw-store/src/revisions.rs killed \
  's/sqlx::query_scalar("SELECT current_revision_id FROM documents WHERE id = ?1")/sqlx::query_scalar("SELECT NULL FROM documents WHERE id = ?1")/' \
  'revisions: parent_id is the revision the document actually pointed at, not NULL for everything'
# The schema half of append-only. `BEFORE UPDATE ON revisions` is NOT the line to mutate —
# pointing the trigger at another table makes the migration itself fail, because the table
# is created further down, and 109 tests then die of "no such table". That kills the entry
# while proving nothing about the defence. Emptying the trigger's body leaves the migration
# valid and the trigger firing, and exactly one test notices.
mutation crates/gw-store/migrations/0008_revisions.sql killed \
  "s/    SELECT RAISE(ABORT, 'revisions are append-only: publish a new one instead');/    SELECT 1;/" \
  'revisions: the append-only trigger actually refuses an UPDATE, rather than merely existing'

# --- crdt state: what is being typed, as against what has been published --------------
#
# The same two kinds of wrong answer as revisions, plus one that is neither.
#
# The DISCLOSURE half is easy to miss in review precisely because the value is a `Vec<u8>`.
# It is the page: `CollabDoc::from_state(&bytes)?.to_block().plain_text()` is the whole
# distance from those bytes back to the text, so `crdt_state_for` is a retriever in the
# sense architecture rule 2 means, and it goes through the same `document_for` everything
# else does.
#
# The WRITE half needed its test rewritten before it could be trusted, and the history is
# the point. `a_sweep_writes_nothing_once_the_last_writer_may_no_longer_write` originally
# revoked the writer's grant outright — which also takes away her READ, because the fixture
# page is restricted. It therefore passed with the store's check weakened from `Write` to
# `Read`: the right assertion for the wrong reason, asserting "she cannot reach this page"
# while claiming to assert "writing is an explicit grant" (D-M2-8). It now DEMOTES her to
# `read`, so only the action distinguishes the two outcomes.
#
# The third is the fork, and it has no analogue anywhere else in this file because it is
# not an access-control failure at all — it is silent corruption. `CollabDoc::from_block`
# *creates* content, so a room seeded from the page body and then handed its own stored
# CRDT state holds every word of the page twice, under two client ids, and a CRDT keeps
# both for ever: the duplicates are not a conflict to resolve, they are two legitimate
# insertions. Nothing errors, nothing logs, the page simply says everything twice.
mutation crates/gw-store/src/crdt.rs killed \
  's/        if !self.may(principal, document_id, Action::Read).await? {/        if false {/' \
  'crdt state: the live text of a page is handed only to somebody who may read the page'
mutation crates/gw-store/src/crdt.rs killed \
  's/        if !self.may(principal, document_id, Action::Write).await? {/        if !self.may(principal, document_id, Action::Read).await? {/' \
  'crdt state: storing it needs WRITE on the page, never merely read (D-M2-8)'
mutation crates/gw-store/src/revisions.rs killed \
  's/        if restored.is_some() {/        if false {/' \
  'crdt state: a restore discards the live state it was reaching past, or it is invisible to editors'
mutation crates/gw-api/src/routes/collab.rs killed \
  's/            .crdt_state_for(principal, &document.id)/            .crdt_state_for(principal, "")/' \
  'crdt state: a room is rebuilt from the stored state, not re-seeded from the published body'
mutation crates/gw-api/src/routes/collab.rs killed \
  's/                    .join(&document.id, &empty_document())/                    .join(\&document.id, \&serde_json::from_str(\&document.body).unwrap_or_else(|_: serde_json::Error| empty_document()))/' \
  'crdt state: a room loaded from stored state is NOT also seeded from the body — that forks the document'
mutation crates/gw-api/src/routes/collab.rs killed \
  's/    if !state.collab.differs(document_id, &encoded) {/    if false {/' \
  'crdt state: an editing session in which nothing was typed writes no row, once per sweep for ever'

# --- links: the graph, and who is allowed to see an edge of it ------------------------
#
# A backlinks panel is an aggregate view, and an aggregate view is where filtering gets
# forgotten — the page is protected, the list of pages pointing at it is not, and the list
# names them. So the first mutation swaps the permission-checked accessor for the unchecked
# one, which is the exact mistake: same shape, same types, compiles, and every candidate
# comes back. `a_backlink_to_a_page_the_caller_cannot_read_is_not_listed` also asserts that
# the privileged caller DOES see the backlink, so it cannot pass by having no link to hide.
mutation crates/gw-store/src/links.rs killed \
  's/self.document_for(principal, &path, Action::Read)/self.document_by_path_unchecked(\&path)/' \
  'links: a backlink names only a page the caller may actually read'
# The second is about the word "replace". Extraction runs on every publish, so an INSERT
# that does not first clear what was there turns every edit into another copy of the graph,
# and a link somebody DELETED from the page stays an edge for ever — visible as a backlink
# on a page that is no longer pointed at.
mutation crates/gw-store/src/links.rs killed \
  's/    sqlx::query("DELETE FROM links WHERE from_doc = ?1")/    sqlx::query("SELECT 1 FROM links WHERE from_doc = ?1")/' \
  'links: republishing replaces this page edges rather than accumulating them'

# `gw-store` cannot name its own origin (it is a library; the hostname is the application's
# configuration, see `Store::with_public_origin`), so an absolute URL is only ever internal
# when it matches the CONFIGURED origin exactly. Deleting the comparison turns "matches" into
# "an origin is configured at all" — any absolute URL becomes an edge the moment one is, which
# is a much bigger door than the one this feature was meant to open. Only a test that
# configures an origin and then presents a MISMATCHED absolute URL can catch it: every
# existing test either configures no origin (so this branch never runs) or matches exactly
# (so removing the comparison changes nothing observable) — the mismatched entries inside
# `an_absolute_url_at_the_configured_origin_is_internal` are what is actually load-bearing.
mutation crates/gw-store/src/links.rs killed \
  's/if parsed.origin() != public_origin.origin() {/if false {/' \
  'links: an absolute URL is internal only when its origin matches the configured one exactly'

# --- the graph: an edge names TWO pages, so it discloses two ---------------------------
#
# The whole graph is one aggregate view over every document at once, which makes it the
# worst place in the application for the filter to be one character wrong. These two are the
# two ways it can be:
#
# `&&` to `||` is the character. It keeps every edge with at least ONE readable end, so a
# public page linking to a restricted one draws a line to it — and a line to a page is a
# statement that the page is there, whether or not the far end carries a label.
mutation crates/gw-store/src/links.rs killed \
  's/readable.contains_key(from) \&\& readable.contains_key(to)/readable.contains_key(from) || readable.contains_key(to)/' \
  'graph: an edge needs BOTH ends readable — one is a disclosure'
# And the same swap the backlinks mutation above makes, because the conjunction being right
# proves nothing if the thing being conjoined never asked a permission question. Same shape,
# same types, compiles: every candidate comes back readable and the whole corpus is drawn.
# `an_edge_needs_both_ends_readable` asserts that the privileged caller DOES see the edge, so
# neither of these can pass by the fixture having nothing to hide.
mutation crates/gw-store/src/links.rs killed \
  's/                .document_for_with_baseline(principal, path, Action::Read, baseline)/                .document_by_path_unchecked(path)/' \
  'graph: a node names only a page the caller may actually read'

# --- crash recovery ------------------------------------------------------------------
#
# A trap does not survive SIGKILL, and a killed run leaves the mutated file in place.
# That happened: an interrupted run left `can()` with the read-only check deleted, which
# makes every public document world-writable, and it sat in the working tree looking like
# ordinary uncommitted work. `cargo test` caught it only because a test happened to cover
# it — nothing about the file said "this is a mutation".
#
# So the backup lives at a known path, and a marker records what is currently mutated.
# Any later run restores it first. The marker is the durable part; the trap is now only
# an optimisation for the ordinary case.
#
# NOTHING ELSE MAY WRITE TO THE TREE WHILE THIS RUNS — and the marker does not save you
# from that one. This script restores by copying the backup over the file; a `cargo fmt`
# started by somebody else reads the file while it is MUTATED and writes it back after the
# restore, reinstating the mutation under a clean marker and an exit code of zero. That
# happened, across six files at once, and it left three security rules disabled in the
# working tree looking like ordinary uncommitted work: the admin API's authentication
# checks, the interlock below, and `can()`'s read-only check on `internal` — which makes
# every internal document world-writable. The run reported KILLED for all of them.
#
# The tell is `ERROR ... the mutation changed nothing` on entries that passed yesterday:
# the sed found no target because the file was already mutated. If you see one, do not
# trust the summary — check `git diff` for one-line changes you did not make before
# running anything else.
#
# This script never runs `cargo fmt`, and must not be made to: formatting a deliberately
# mutated file is precisely the mechanism above, with this script as the other party.
readonly BACKUP_DIR=".mutate-backups"
readonly MARKER="$BACKUP_DIR/in-progress"
# The tree as it was before anything was touched. A hash would tell you something changed;
# a copy tells you that AND puts it back, which is the difference between a harness that
# reports a hole and one that closes it.
readonly PRISTINE_DIR="$BACKUP_DIR/pristine"
readonly MANIFEST="$BACKUP_DIR/pristine.sha256"
# What the REST of the tree looked like. `verify_tree` guards the files a mutation writes
# to; nothing guarded the files the suite merely READS, and a verdict can be flipped by
# either. See `note_drift` for the run where that happened.
readonly TREE_STATE="$BACKUP_DIR/tree-state"

mkdir -p "$BACKUP_DIR"

# One name per path, used for both the rotating backup and the pristine copy, so the two
# can never disagree about which file they hold.
mangle() { echo "$1" | tr '/' '_'; }

if [ -f "$MARKER" ]; then
  echo "WARNING: a previous mutation run did not finish. Restoring before continuing."
  while IFS=$'\t' read -r mfile mbackup; do
    [ -n "$mfile" ] || continue
    if [ -f "$mbackup" ]; then
      cp "$mbackup" "$mfile"
      echo "  restored $mfile"
    else
      echo "  ERROR: $mfile was mutated but its backup $mbackup is gone."
      echo "         Check it by hand — `git diff $mfile` — before trusting this tree."
      exit 2
    fi
  done < "$MARKER"
  rm -f "$MARKER"
  echo
fi

filter="${1:-}"
killed=0 survived=0 unexpected=0 drifted=0
drifted_at=()

# Did anything ELSE in the repository change while that mutation was being scored?
#
# `verify_tree` answers a narrower question — are the files this run MUTATES still what
# they were — and it answers it well. It cannot see the other half. A mutation is scored
# KILLED because a test failed, and a test can fail because somebody edited a FIXTURE the
# suite reads. Nothing in this file touches such a file, so nothing checks it.
#
# That is not hypothetical. On 2026-08-12, with several agents working in this repository
# at once, `content-example/rundgang/tabellen.md` was edited between the preflight and the
# invites mutations. `crates/gw-api/tests/seed.rs` reads that directory, it failed, and the
# entry recorded as `equivalent` was reported as "the suite killed it — the note is wrong".
# Re-running it alone on a quiet tree gave `(equiv)` immediately. The verdict was false and
# nothing in the output said so; the note it accused had been right all along.
#
# This WARNS and does not fail. Voiding a four-minute run because somebody saved a Svelte
# file would be a check that cries wolf, and this file has already learned where those end
# up. What it does instead is name the mutation whose window the change landed in, so the
# reader knows which line not to believe — and then re-baselines, so one edit is attributed
# to one verdict rather than to every verdict after it.
note_drift() {
  local description="$1" now
  [ -f "$TREE_STATE" ] || return 0
  # If git cannot answer, say nothing. It failed for a second the first time this ran,
  # because another agent's `git` held `.git/index.lock`, and the empty output that came
  # back was compared as though every file in the repository had vanished — fifteen lines
  # of alarm about a tree that was fine. An answer a command could not give is not evidence.
  now="$(git status --porcelain=v1 2>/dev/null)" || return 0
  [ "$now" = "$(cat "$TREE_STATE")" ] && return 0
  echo "           WARNING: the repository changed while this was being scored. If a test"
  echo "                    reads what changed, this verdict means nothing. Re-run it alone."
  diff <(cat "$TREE_STATE") <(echo "$now") | grep '^[<>]' | sed 's/^/                    /'
  printf '%s' "$now" > "$TREE_STATE"
  drifted=$((drifted + 1))
  drifted_at+=("$description")
}

# HOW LONG THIS IS ALLOWED TO TAKE
# --------------------------------
# A gate too slow to run stops being run. This one got there: eighteen mutations, a whole
# `cargo test --workspace` each, and argon2id at Authelia's parameters inside that suite —
# over ten minutes, and it timed out rather than finishing. Two things fixed it, and only
# the second one is in this file.
#
# The first was the suite itself: the hashing cost is now a value on `AppState`, so tests
# hash at `HashingCost::CHEAP_FOR_TESTS` while the server hashes at Authelia's, and the two
# tests that are genuinely about cost ask for the production parameters by name. The suite
# went from 58 s to under 7 s, and this script inherits all of it.
#
# The second is the probe below. Most mutations are caught by the tests of the crate they
# were made in, and building *that crate's* tests is a third of the cost of building the
# workspace's. So each mutation gets one cheap, targeted run first.
#
# WHY A PROBE CANNOT CHANGE A VERDICT
# -----------------------------------
# It is not a filter and it never decides that something was NOT caught:
#
#   - KILLED is only ever recorded because a real test in real code actually failed. A
#     probe that fails has already proved the mutation is caught; running the rest of the
#     suite could add nothing but time.
#   - SURVIVED is only ever recorded after `cargo test --workspace`, unfiltered, has passed
#     in full — exactly the command this script used before, unchanged. A probe that passes
#     decides nothing at all; it falls through.
#
# So a wrong or stale probe costs a couple of seconds and cannot cost correctness. Prove it
# rather than believe it: `MUTATE_NO_PROBE=1 ./scripts/mutate.sh` skips the probe entirely,
# and the two outputs must be identical line for line.
#
# (Note `cargo test --workspace <filter>` is NOT how to narrow anything here: it silently
# runs ZERO tests — every binary reports "0 passed; N filtered out" — which once scored
# every mutation as survived. Per-crate `-p` invocations are the form that works.)
#
# WHY THIS IS NOT PARALLEL
# ------------------------
# The obvious next step is several mutations at once, each with its own
# `CARGO_TARGET_DIR`. It was measured and rejected: this workspace's debug target
# directory is over ten gigabytes and the disk has about four free — one extra worker does
# not fit, let alone four, and a run that fills the disk breaks every other build on the
# machine rather than only this one. Separate target directories would not make it safe in
# any case: the race that has actually bitten this script is two processes writing the same
# SOURCE file, which no amount of artefact separation separates.
probe_for() {
  case "$1" in
    crates/gw-store/*) echo "-p gw-store --lib" ;;
    crates/gw-auth/*) echo "-p gw-auth --lib" ;;
    crates/gw-core/*) echo "-p gw-core --lib" ;;
    # gw-api's route gates are covered by an integration test rather than by unit tests,
    # and `-p gw-api` on its own would build all six integration binaries — nearly the
    # whole workspace. This names the one that covers the admin routes. If that stops
    # being true the probe simply stops firing.
    crates/gw-api/src/routes/admin.rs) echo "-p gw-api --test admin --test invites" ;;
    # The invite page needs both: the escaping lives in this crate's unit tests and the
    # flow in the integration binary, and a probe that ran only one of them would fall
    # through to the whole workspace for half the invite mutations.
    crates/gw-api/src/auth/invite.rs) echo "-p gw-api --lib --test invites" ;;
    # Same argument for view-as. `routes/mod.rs` is named as well because the one mutation
    # made there — skipping the substitution — is a view-as defence living in the file that
    # resolves the principal; every test that notices it is in this binary.
    crates/gw-api/src/view_as.rs) echo "-p gw-api --test view_as" ;;
    crates/gw-api/src/routes/mod.rs) echo "-p gw-api --test view_as" ;;
    *) echo "" ;;
  esac
}

# One `cargo test` invocation, classified. `test result: FAILED` is checked BEFORE the
# error pattern because cargo prints `error: test failed, to rerun pass ...` for an
# ordinary failing test, and reading that as "does not compile" would turn every kill into
# a mis-recording.
run_tests() {
  local out
  out="$(cargo test "$@" 2>&1 || true)"
  if grep -q "test result: FAILED" <<<"$out"; then
    echo killed
  elif grep -qE "^error(\[|:)" <<<"$out"; then
    echo uncompilable
  else
    echo passed
  fi
}

# Put the file back, and PROVE it went back. A restore that silently failed would leave
# broken security code in the tree with a green summary printed over it.
restore() {
  local file="$1" backup="$2"
  cp "$backup" "$file"
  if ! cmp -s "$file" "$backup"; then
    echo "FATAL: could not restore $file from $backup. The tree is left mutated."
    exit 2
  fi
  rm -f "$MARKER"
  trap - EXIT INT TERM HUP
}

# Every file the selected mutations will touch, once each.
selected_files() {
  local entry f d
  for entry in "${MUTATIONS[@]}"; do
    IFS=$'\x1f' read -r f _ _ d <<<"$entry"
    [ -z "$filter" ] || [[ "$d" == *"$filter"* ]] || continue
    echo "$f"
  done | sort -u
}

# Is the tree we are handing back the tree we were given?
#
# `restore` proving the copy landed is NOT enough, because the damage arrives AFTER it. A
# concurrent `cargo fmt` reads a file while it is mutated and writes its buffer back once
# the restore has already happened and the marker has already been cleared — so the
# mutation is reinstated under a green summary and an exit code of zero. That is not a
# story: it happened twice, and the second time it left `can()` treating every internal
# document as writable, in a working tree that looked like ordinary uncommitted work.
#
# So the run is only over when every file it touched is byte-for-byte what it was. This is
# called after every restore, which localises the damage to one mutation's window, and
# again at the end, which is what makes the exit code mean something.
verify_tree() {
  local file pristine damaged=()
  while IFS= read -r file; do
    pristine="$PRISTINE_DIR/$(mangle "$file")"
    cmp -s "$file" "$pristine" || damaged+=("$file")
  done < <(selected_files)
  [ ${#damaged[@]} -eq 0 ] && return 0

  echo
  echo "FATAL: the working tree was modified underneath this run."
  echo
  echo "  These files no longer match what they were before it started:"
  for file in "${damaged[@]}"; do
    echo "    $file"
  done
  echo
  echo "  Every result printed above is void. The usual cause is another process writing"
  echo "  to the tree — a \`cargo fmt\` that read a file while it was MUTATED and wrote it"
  echo "  back after the restore, which reinstates the mutation silently. Whatever the"
  echo "  cause, a security rule may be disabled in your working tree right now."
  echo
  for file in "${damaged[@]}"; do
    cp "$PRISTINE_DIR/$(mangle "$file")" "$file"
    echo "  restored $file to its pre-run contents"
  done
  echo
  echo "  Check \`git diff\` before running anything else, then re-run this with nothing"
  echo "  else touching the repository."
  rm -f "$MARKER"
  exit 3
}

# Everything that has to be true before a single mutation is worth making.
preflight() {
  local file entry expr d stale=() missing=()

  # 1. Nobody else may be building or formatting this tree. Separate target directories do
  #    not help: the race is on the SOURCE file, not on the artefacts. `cargo fmt` is the
  #    dangerous one, but any cargo invocation can pull in a formatter or a fix-up, and a
  #    second `cargo test` would also fight this one for the target directory lock and make
  #    every timing here meaningless.
  #    `-x` matches the process NAME, not the command line. `-f` looks tempting and is
  #    wrong: it matches any shell, editor or tmux server whose arguments merely mention
  #    cargo, and a check that cries wolf is a check somebody switches off.
  local others
  others="$(pgrep -a -x 'cargo|rustc|rustdoc|rustfmt|cargo-clippy|cargo-fmt' 2>/dev/null || true)"
  if [ -n "$others" ] && [ -z "${MUTATE_ALLOW_CONCURRENT_CARGO:-}" ]; then
    echo "REFUSING TO START: another cargo or rustc is running against this machine."
    echo
    echo "$others" | sed 's/^/    /'
    echo
    echo "  This script rewrites security-critical source files in place and restores them"
    echo "  afterwards. Anything else holding those files open — a \`cargo fmt\` above all —"
    echo "  can write its copy back AFTER the restore and reinstate a mutation under a"
    echo "  green summary. Wait for the other build to finish."
    echo
    echo "  If that listing is a false positive (an editor's language server, say), re-run"
    echo "  with MUTATE_ALLOW_CONCURRENT_CARGO=1 — and then read the final integrity check"
    echo "  rather than the summary."
    exit 2
  fi

  # 2. Every target has to exist, and every sed expression has to still find something. A
  #    mutation whose target text is gone is either an entry that has rotted or — the
  #    reason this is checked up front rather than discovered halfway through — a file that
  #    is ALREADY mutated from an earlier interrupted or sabotaged run.
  for entry in "${MUTATIONS[@]}"; do
    IFS=$'\x1f' read -r file _ expr d <<<"$entry"
    [ -z "$filter" ] || [[ "$d" == *"$filter"* ]] || continue
    if [ ! -f "$file" ]; then
      missing+=("$file")
    elif sed "$expr" "$file" | cmp -s - "$file"; then
      stale+=("$d")
    fi
  done

  if [ ${#missing[@]} -gt 0 ]; then
    echo "REFUSING TO START: a mutation names a file that is not there."
    printf '    %s\n' "${missing[@]}"
    exit 2
  fi

  if [ ${#stale[@]} -gt 0 ]; then
    echo "WARNING: ${#stale[@]} mutation(s) will find nothing to change:"
    printf '    %s\n' "${stale[@]}"
    echo
    echo "  Either the code they target has moved, or the file is already mutated. Check"
    echo "  \`git diff\` for one-line changes you did not make before trusting this run."
    echo "  They are reported as errors below and the exit code will be non-zero."
    echo
  fi

  # 3. The suite must pass BEFORE anything is broken. This one is not hygiene: a mutation
  #    is scored KILLED because a test failed, so a suite that is ALREADY failing scores
  #    every mutation as killed no matter what — the harness would report a perfect run
  #    while proving nothing at all. That is the exact failure this script exists to catch,
  #    committed by the script itself.
  echo "checking the suite passes before anything is mutated..."
  case "$(run_tests --workspace)" in
    passed) ;;
    *)
      echo "REFUSING TO START: \`cargo test --workspace\` does not pass on the unmutated tree."
      echo
      echo "  Every mutation would be scored KILLED by the failure that is already there,"
      echo "  and the run would report a clean sweep having tested nothing. Fix the suite"
      echo "  first."
      exit 2
      ;;
  esac

  # 4. The tree as handed to us, kept for comparison and for putting things back.
  mkdir -p "$PRISTINE_DIR"
  rm -f "$MANIFEST"
  while IFS= read -r file; do
    cp "$file" "$PRISTINE_DIR/$(mangle "$file")"
  done < <(selected_files)
  if command -v sha256sum >/dev/null 2>&1; then
    # For the record and for a human reading afterwards. `verify_tree` compares against the
    # copies rather than these digests: a copy detects the same change and can also undo it.
    # shellcheck disable=SC2046
    sha256sum $(selected_files) > "$MANIFEST" 2>/dev/null || true
  fi

  # 5. And everything else, as a baseline for `note_drift`. Taken AFTER the preflight suite
  #    run, so it describes the tree the first mutation is actually scored against. If git
  #    cannot answer, the file is removed rather than left empty: `note_drift` skips a
  #    missing baseline, and would read an empty one as "everything has changed".
  git status --porcelain=v1 > "$TREE_STATE" 2>/dev/null || rm -f "$TREE_STATE"
}

preflight

for entry in "${MUTATIONS[@]}"; do
  IFS=$'\x1f' read -r file expectation expr description <<<"$entry"
  [ -z "$filter" ] || [[ "$description" == *"$filter"* ]] || continue

  backup="$BACKUP_DIR/$(echo "$file" | tr '/' '_')"
  cp "$file" "$backup"
  printf '%s\t%s\n' "$file" "$backup" > "$MARKER"
  # Covers the ordinary interruptions. SIGKILL is covered by the marker above.
  # shellcheck disable=SC2064
  trap "cp '$backup' '$file'; rm -f '$MARKER'" EXIT INT TERM HUP

  sed -i "$expr" "$file"
  if cmp -s "$file" "$backup"; then
    echo "  ERROR    $description"
    echo "           the mutation changed nothing — the code it targets has moved or been"
    echo "           rewritten, so this entry is testing an assumption that no longer holds."
    unexpected=$((unexpected + 1))
    restore "$file" "$backup"
    continue
  fi

  started=$SECONDS
  outcome=""

  # Cheap and targeted, and only ever able to end this early by finding a real failure.
  probe="$(probe_for "$file")"
  if [ -z "${MUTATE_NO_PROBE:-}" ] && [ -n "$probe" ]; then
    # shellcheck disable=SC2086
    case "$(run_tests $probe)" in
      killed) outcome=killed ;;
      uncompilable) outcome=uncompilable ;;
    esac
  fi

  # The whole suite, unfiltered — the only thing allowed to conclude that a mutation was
  # NOT caught. A mutation killed by a test in another crate is still killed, so nothing
  # here is narrowed by crate, by name, or by anything else.
  if [ -z "$outcome" ]; then
    outcome="$(run_tests --workspace)"
    [ "$outcome" = passed ] && outcome=survived
  fi

  restore "$file" "$backup"
  # Immediately, so that damage is attributed to the window it landed in rather than
  # discovered at the end with twenty-six other mutations to disentangle it from.
  verify_tree
  elapsed=$((SECONDS - started))

  case "$outcome:$expectation" in
    uncompilable:*)
      echo "  ERROR    $description"
      echo "           the mutated code does not compile, so it exercises no test at all."
      unexpected=$((unexpected + 1)) ;;
    killed:killed)
      echo "  KILLED   [${elapsed}s] $description"; killed=$((killed + 1)) ;;
    survived:equivalent)
      echo "  (equiv)  [${elapsed}s] $description"; killed=$((killed + 1)) ;;
    survived:killed)
      echo "  SURVIVED $description"
      echo "           The code was broken and every test still passed. Add a test, or"
      echo "           demonstrate the mutation is equivalent and record it as such."
      survived=$((survived + 1)) ;;
    killed:equivalent)
      echo "  ERROR    $description"
      echo "           recorded as equivalent but the suite killed it — the note is wrong."
      unexpected=$((unexpected + 1)) ;;
  esac
  # After the verdict, so the warning sits directly under the line it casts doubt on.
  note_drift "$description"
done

# The last word, and it is about the tree rather than about the mutations: a summary
# printed over a working tree with a security rule disabled in it is worse than no summary.
verify_tree

echo
# The wall clock is part of the result. This gate is only useful if it is actually run,
# and the last time it stopped being run it was because it had quietly grown past ten
# minutes — which nothing in its own output said.
printf 'mutation testing: %dm %02ds total\n' $((SECONDS / 60)) $((SECONDS % 60))
if [ "$drifted" -gt 0 ]; then
  echo "mutation testing: $drifted verdict(s) were scored while the repository was being"
  echo "                  changed underneath them, and are not evidence of anything:"
  printf '                    %s\n' "${drifted_at[@]}"
  echo "                  Re-run each one alone — \`scripts/mutate.sh <words from it>\` —"
  echo "                  with nothing else writing to the tree."
fi
if [ "$survived" -gt 0 ] || [ "$unexpected" -gt 0 ]; then
  echo "mutation testing: $killed as expected, $survived survived, $unexpected mis-recorded"
  exit 1
fi
echo "mutation testing: $killed mutations, all as expected"
