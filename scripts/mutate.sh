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

# --- reading a page by its id, which is what every aggregate view actually holds --------
#
# `document_for` is keyed by a PATH because a path is what grants hang off. Everything that
# aggregates holds an id instead — a board card knows its `doc_id`, a revision knows its
# `document_id` — and for a while there was no id-keyed accessor at all, so each of them had
# to resolve the id itself and hope it remembered to ask. `document_for_id` is that one
# resolution, and it must stay a way IN to `document_for` rather than a second answer beside
# it: the mutation is the whole difference, and it is one word wide.
#
# It cannot pass vacuously. `a_document_asked_for_by_id_answers_exactly_what_its_path_answers`
# asserts the equivalence in both directions over three callers and two actions, and counts
# the permitted answers, so a fixture that refused everybody — or permitted everybody —
# fails it before the mutation is even made.
#
# The mutation used to swap the accessor for `document_by_path_unchecked`. It cannot any
# more: the by-id resolution now hands back a `DocumentAccess` (0010), which the unchecked
# lookup has no way to build, so that spelling stops compiling rather than leaking. What
# replaces it is the same bypass expressed in the arguments — ask as an ADMIN BASELINE for a
# READ, whatever was actually asked — which is exactly "the id route answers a different
# question from the path route", and which the equivalence test is built to catch.
mutation crates/gw-store/src/acl.rs killed \
  '/pub(crate) async fn document_access_id_with_baseline/,/^    }$/ s@        self.document_access_with_baseline(principal, \&path, action, baseline)@        self.document_access_with_baseline(principal, \&path, Action::Read, Baseline::Admin)@' \
  'documents: reading a page by its id asks exactly what reading it by path asks'
# And the one body every one of those routes ends in. `document_access_with_baseline` is now
# the single place a visibility and a set of grants are turned into a verdict — `document_for`,
# `document_for_id`, `document_access` and `governing_document` are all ways in — so deleting
# its refusal is the whole permission system off in one line. It is here rather than assumed
# because "everything reaches it" is only worth stating if reaching it decides something.
mutation crates/gw-store/src/acl.rs killed \
  's@        if !permits(principal, action, visibility, \&grants, baseline) {@        if false {@' \
  'documents: the one accessor every read goes through actually refuses'

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
# Both of these moved when `Author` grew `id()` and `name()`: the binds in
# `append_revision` are now `author.id()` and `author.name()`, and the choice they used to
# make inline is made inside those two methods. The expressions target the methods, which
# is where the decision actually lives — and the entries were silently STALE until this
# was noticed, which is worth knowing about: a stale expression makes the whole script
# refuse to start, but only on an UNFILTERED run, because the pre-flight staleness check
# skips entries the filter excluded.
mutation crates/gw-store/src/revisions.rs killed \
  's|            Author::Account(principal) => &principal.id,|            Author::Account(principal) => \&principal.username,|' \
  'revisions: the author recorded is the principal, by id — the thing a rename cannot move'
mutation crates/gw-store/src/revisions.rs killed \
  's|            Author::Account(principal) => byline(principal),|            Author::Account(principal) => principal.username.as_str(),|' \
  'revisions: the byline is the display name as it was then, which is what survives deletion'
# Not a disclosure, but the two ways a history can be quietly wrong about ITSELF. The
# timeline and the parent chain are read against each other by diff, restore and blame; if
# they disagree, every one of those answers something else's question.
mutation crates/gw-store/src/revisions.rs killed \
  's/WHERE document_id = ?1 ORDER BY created_at DESC, id DESC/WHERE document_id = ?1 ORDER BY created_at DESC/' \
  "revisions: the timeline breaks ties on the uuid v7 id — datetime('now') is per-second, so two edits in one second are otherwise unordered"
mutation crates/gw-store/src/revisions.rs killed \
  's|sqlx::query_as("SELECT path, current_revision_id FROM documents WHERE id = ?1")|sqlx::query_as("SELECT path, NULL FROM documents WHERE id = ?1")|' \
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

# --- tasks and boards: a card is a page's words on somebody else's screen --------------
#
# The design's Security section names this as the property most likely to be got wrong by
# an aggregate query written in a hurry, and a board is worse than a backlinks panel: a
# project is DELIBERATELY allowed to span pages with different grants (D-3), so the natural
# thing to write — trust the subtree, it is the project — is exactly the bug. A card's title
# is a copy of its page's own words (D-2), so a leaked card is leaked prose, not merely a
# leaked name.
#
# The first mutation is the one that matters: `governing_document` is the single line every
# task, board and project in the module is authorised through. It used to be swapped for the
# unchecked accessor; since 0010 that call answers a `DocumentAccess`, which the unchecked
# lookup cannot build, so the bypass is written in the arguments instead — authorise every
# card, board and project as an ADMIN BASELINE performing a READ, whatever was asked. Same
# shape, same types, compiles, and it is every leak this section is about at once: a stranger
# sees the closed page's card, and a reader moves it.
mutation crates/gw-store/src/tasks.rs killed \
  's|        self.document_access_with_baseline(principal, \&path, action, baseline)|        self.document_access_with_baseline(principal, \&path, Action::Read, Baseline::Admin)|' \
  'tasks: every card and project is authorised as the caller, for the action asked'
#
# This is also the mutation that stands behind the card's PAGE. An anchored card carries the
# path and the title of the page its line was written on, and both come from the document
# THIS call answered with — never from a second lookup keyed by the card's `doc_id`, which
# is the tempting version and is safe only for as long as the filtering a few lines up stays
# right. Swap the accessor for the unchecked one and the name leaks with the card:
# `a_card_names_its_page_only_to_somebody_who_may_read_that_page` in `gw-api`'s
# `tests/tasks.rs` greps the whole response body for the secret page's path and title, and
# asserts the privileged caller gets both.
mutation crates/gw-store/src/tasks.rs killed \
  's|                        .document_access_with_baseline(principal, \&path, Action::Read, baseline)|                        .document_access_with_baseline(principal, \&path, Action::Read, Baseline::Admin)|' \
  'board: a card names only a page the caller may actually read'
# Asking and then not acting on the answer. The memo now holds the PAGE the accessor
# answered with rather than a boolean about it, so "not acting on it" has to be written as a
# fallback: an unreadable page becomes an empty name and the card is emitted anyway. That is
# not a contrived shape — it is exactly what "the card was already filtered, so I can fill
# the name in myself" produces. `a_board_omits_a_card_whose_page_the_caller_cannot_read`
# asserts that the privileged caller DOES see all three cards, so neither of these can pass
# by the fixture having nothing to hide.
mutation crates/gw-store/src/tasks.rs killed \
  's@            let Some(governed) = known else {@            let governed = known.unwrap_or(Governed { page: TaskPage { path: path.clone(), title: String::new() }, may_write: false }); if false {@' \
  'board: the per-document verdict is acted on rather than merely computed'
# The baseline is hoisted out of the loop because it is a property of the caller. Hoisting
# the WRONG one — anybody's but theirs — is how that optimisation goes wrong, and it reads
# as a tidy constant rather than as a hole.
#
# What it now breaks is subtler than it was, and the test had to be strengthened to keep
# catching it. Before D-12 the loose cards were emitted without a second question — the home
# page had just been authorised — so a borrowed baseline handed a stranger the whole board.
# Now every card goes through the per-document check, so a stranger still gets nothing and
# the leak has moved: the caller is let past the home-page gate and handed the cards they
# happen to be able to read, which is a PARTIAL board where the design says there is none.
# `a_board_is_refused_entirely_to_somebody_who_may_not_read_the_project_home` gained a caller
# with read on a page inside the subtree and nothing on the home page, which is the only
# shape that can tell the two apart.
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn board_for/,/^        let mut out: Vec<Task>/ s|Action::Read, baseline)|Action::Read, Baseline::Admin)|' \
  'board: the home page is read with the CALLERS baseline, never a borrowed one'
# The ordinary prefix bug, in the one place it is a disclosure: `/projektierung` is not
# inside `/projekt`, and a bare `starts_with` would put its cards on somebody else's board.
mutation crates/gw-store/src/tasks.rs killed \
  's|    path == root \|\| path.starts_with(\&format!("{root}/"))|    path.starts_with(root)|' \
  'board: within() is a segment boundary, not a bare prefix'
# The SQL says the same thing one layer down. Provably unobservable while `within` stands,
# and recorded as equivalent rather than as a gap: the prefix in the query is what stops it
# loading every task in the corpus, and `within` is the boundary a reader can check. Either
# alone is correct; they are kept as a pair on purpose, so breaking one changes nothing.
#
# The `?1 IS NULL` in front of it is the global board's filter (D-12) and is kept on both
# sides of the mutation: unbound, the subtree predicate stands down entirely, so removing it
# from the mutated form would be testing something else.
mutation crates/gw-store/src/tasks.rs equivalent \
  's|AND (?1 IS NULL OR d.path = ?1 OR substr(d.path, 1, length(?1) + 1) = ?1 \|\| ./.)|AND (?1 IS NULL OR substr(d.path, 1, length(?1)) = ?1)|' \
  'board: the candidate SQL narrows to the same subtree — defence in depth behind within()'
# Both of these read a document and then decide, in one `match`, whether there is anything
# to return AND what the card names its page. The refusal is the arm that is mutated: with
# it gone the caller is handed the card anyway, which is the leak, and the compiler is happy
# because the other arm's type is all the arm has to produce.
#
# That shape is deliberate and it is why neither of these gates can be *deleted* rather than
# mutated: the value that authorises the read is the value that names the page, so a version
# that skipped the check would have nothing to build the name out of and would not compile.
#
# `task_for`'s arm now yields a TRIPLE — the page the card names, whether the caller may
# change it (0010), and the path its assignee is asked about — so the mutation has to build
# all three out of nothing. That is the honest version of "the card was already filtered": a
# card with no page on it, no verdict behind it and an empty path to ask about, handed to
# somebody who may not read a word of it. `tasks_for_document`'s arm is the same shape one
# type further in, because its answer is now a `Governed` rather than a bare page.
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn task_for/,/^    }$/ s@            None => return Ok(None),@            None => (None, false, String::new()),@' \
  'tasks: reading one task follows the read on the page that governs it'
mutation crates/gw-store/src/tasks.rs killed \
  's@            None => return Ok(Vec::new()),@            None => Governed { page: TaskPage { path: String::new(), title: String::new() }, may_write: false },@' \
  "tasks: a page's own task list follows that page's read"

# --- D-10: who may assign whom, and what a card may say about them ----------------------
#
# Four clauses, and the third is the security-relevant one. Assigning somebody to a task on
# a page they cannot open hands them an obligation they cannot see, and the card's title
# tells them what a page they may not read is called — the board's version of the leak a
# graph edge would be.
#
# Clause 3 is also what decides whether a board may say what its assignee is CALLED, asked
# again when the card is read (ADR 0009). The name is more legible than the id that was
# always there, and more identifying — so it is disclosed to a reader of the page exactly
# while the person named may read that page too, and it comes off again when they cannot.
# The verdict and the name are deliberately one value, which is why several of the
# mutations below stand behind both at once.
mutation crates/gw-store/src/tasks.rs killed \
  's|            .governing_document(principal, \&new.home, Action::Write, baseline)|            .governing_document(principal, \&new.home, Action::Read, baseline)|' \
  'assignment: creating a task needs WRITE on the governing page, not read'
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn update_task/,/^        let mut governing_path/ s|            .governing_document(principal, \&home, Action::Write, baseline)|            .governing_document(principal, \&home, Action::Read, baseline)|' \
  'assignment: changing a task needs WRITE on the governing page, not read'
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn delete_task/,/^    }$/ s|Action::Write, baseline|Action::Read, baseline|' \
  'assignment: deleting a task needs WRITE on the governing page, not read'
# Clause 3 itself. `assignee_named` asks the ASSIGNEE'S own question — their groups, their
# teams, their active flag, their baseline — and this is the line that acts on the answer.
#
# It is also the mutation that stands behind the NAME a card shows. The verdict and the name
# are deliberately one value: `assignee_named` returns what to call the person exactly when
# clause 3 would still permit the assignment, so there is no version of this code in which
# the gate is intact and the name leaks anyway. That is why one mutation covers both, and it
# is checked from both ends — `a_task_may_not_be_assigned_to_somebody_who_may_not_read_its_governing_page`
# fails on the assignment, and `a_card_names_the_person_it_rests_on_only_while_they_may_read_the_page`
# fails on the name a board shows after her read is taken away.
mutation crates/gw-store/src/tasks.rs killed \
  '/async fn assignee_named/,/^    }$/ s|            .is_some();|            .is_some() \|\| true;|' \
  'assignment: a task may not rest on somebody who may not read the page it is on'
# The same clause, for an id that names no account at all. The `None` arm is what refuses it;
# naming it after its own id instead is the shape somebody reaches for when a board renders a
# bare uuid and they want SOMETHING on the card.
mutation crates/gw-store/src/tasks.rs killed \
  '/async fn assignee_name_for/,/^    }$/ s|            None => None,|            None => Some(assignee_id.to_string()),|' \
  'assignment: an id that is not an account cannot be given a task'
# The memo is keyed on the PAIR — the account and the page — and the pair is the point. D-3
# makes a project span pages with different grants by design, so a verdict memoised on the
# account alone takes whichever page happened to be looked at first and carries its answer
# across the whole board: somebody who may read the open half of a project gets named on a
# card from the closed half. That is the subtree bug wearing a different hat, and it is
# invisible on any fixture whose assignee can read every page on the board.
#
# `a_board_names_an_assignee_per_page_rather_than_once_for_the_whole_board` is the shape that
# can tell them apart, and it cannot pass vacuously: it asserts the name IS on the card from
# the page she may read, as well as absent from the other.
mutation crates/gw-store/src/tasks.rs killed \
  's|        let key = (assignee_id.to_string(), governing_path.to_string());|        let key = (assignee_id.to_string(), String::new());|' \
  'assignee: the verdict is memoised per person AND page, never per person alone'
# And that the board asks at all. Every other reader of a card resolves the name too, so a
# board that quietly stopped would still look right anywhere a single card is fetched.
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn board_for/,/^        in_board_order/ s|                .assignee_name_for(&mut names, row.assignee.as_deref(), &path)|                .assignee_name_for(\&mut names, None, \&path)|' \
  'board: the board resolves the name of the person each card rests on'
# A change that says nothing about the assignee still comes back naming them, because the
# name is read off the row the change LEFT BEHIND rather than assembled from the change. The
# mutation writes the tempting version — name whatever the change named — which answers
# correctly for every request that sets an assignee and nameless for every one that does not,
# so a `PATCH {"status": …}` on an assigned card silently loses the name it was showing.
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn update_task/,/^    }$/ s|            .assignee_name_for(&mut names, row.assignee.as_deref(), &governing_path)|            .assignee_name_for(\&mut names, effective_assignee.as_deref(), \&governing_path)|' \
  'assignee: a change names the person the card rests on AFTERWARDS, not the one it named'
# Clause 4, and it is a mutation that ADDS a check rather than removing one — which is the
# only way to test a deliberate permission. Making the unassign path ask whether the person
# being removed may still read the page pins a stale name to the card for ever.
mutation crates/gw-store/src/tasks.rs killed \
  's|            Some(chosen) => chosen.clone(),|            Some(chosen) => chosen.clone().or_else(\|\| row.assignee.clone()),|' \
  'assignment: unassigning asks nothing about the person being removed'
# Moving a card changes which page governs it, so both ends are asked — and the name
# already on the card is an assignment onto the destination's page.
mutation crates/gw-store/src/tasks.rs killed \
  's|            None if moved_to.is_some() => row.assignee.clone(),|            None if false => row.assignee.clone(),|' \
  'assignment: a move re-checks the assignee against the board it is going to'
mutation crates/gw-store/src/tasks.rs killed \
  '/if let Some(project_id) = &update.project_id/,/^            governing_path = target_page.document.path;$/ s|                .governing_document(principal, \&target, Action::Write, baseline)|                .governing_document(principal, \&target, Action::Read, baseline)|' \
  'assignment: moving a card needs WRITE on the destination board, not read'
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn create_project/,/^    }$/ s|Action::Write|Action::Read|' \
  'projects: making a page a project home needs WRITE on that page'
mutation crates/gw-store/src/tasks.rs killed \
  '/async fn may_administer_project/,/^    }$/ s|Action::Write, baseline|Action::Read, baseline|' \
  'projects: retagging and deleting a project need WRITE on its home page'

# --- reconciliation on publish: the detach loop, which loses data under a green suite ---
#
# This one is not a disclosure. It is here because the failure it guards against is silent,
# cumulative and destroys exactly the thing D-8 exists to keep: if a republish mints a
# SECOND id for a line that already has a record, the first record is orphaned, marked
# detached, and the board sheds a card — with the due date and the assignee somebody set on
# it — on every single save. Nothing errors, nothing is logged, and the page still reads
# correctly. The two mutations below are the two ways to arrive there, and the third is the
# way to lose the card outright.
#
# Neither can pass vacuously: the tests they must fail also assert the POSITIVE — that the
# task exists, is attached, and keeps its state — so a reconciliation that did nothing at
# all would fail them too.
#
# Mint into a copy and store the original. This is the one an author is most likely to
# write and least likely to notice, because the FIRST publish is perfect and every later
# one re-mints; a test that publishes once passes forever.
mutation crates/gw-store/src/revisions.rs killed \
  's|    let body_json = if minted {|    let body_json = if false {|' \
  'reconciliation: the body that is stored is the body the ids were minted into'
# Adoption by the words, which is what makes an ID-LESS republish idempotent — a markdown
# import, and `seed --update`, which re-converts the same file on every run and would
# otherwise shed the whole board once per run for as long as anybody kept seeding.
#
# `#` as the delimiter, here and below: these expressions contain Rust closures, and a `|`
# delimiter ends the substitution in the middle of `|&r|`. sed then reports "unknown option
# to `s'" and the entry is refused at startup rather than silently doing nothing — but only
# because the pre-flight check runs every expression before the first test.
mutation crates/gw-store/src/tasks.rs killed \
  's#        if let Some(r) = (0..rows.len()).find(|&r| !taken\[r\] && &rows\[r\].title == text) {#        if let Some(r) = (0..rows.len()).find(|\&r| false \&\& !taken[r] \&\& \&rows[r].title == text) {#' \
  'reconciliation: a line with no id adopts the record for its words rather than minting a second'
# And the ordinary case: a line that CARRIES an id claims the record with that id.
mutation crates/gw-store/src/tasks.rs killed \
  's#        if let Some(r) = rows.iter().position(|row| &row.block_id == id) {#        if let Some(r) = rows.iter().position(|row| false \&\& \&row.block_id == id) {#' \
  'reconciliation: a line carrying an id claims the record with that id'
# D-8, and the tempting wrong answer stated in SQL: a line that has gone is a record
# MARKED, never a record deleted. Deleting discards a due date and an assignee that were
# never the page's to hold in the first place (D-2), and it is unrecoverable — the marker
# is what lets somebody see what happened and put the line back.
mutation crates/gw-store/src/tasks.rs killed \
  's#sqlx::query("UPDATE tasks SET detached = 1, updated_at = datetime(.now.) WHERE id = ?1")#sqlx::query("DELETE FROM tasks WHERE id = ?1")#' \
  'reconciliation: a line that disappears leaves its record detached, not deleted (D-8)'
# The other half of D-2. Reconciliation takes the words and touches nothing else; a pass
# that also wrote the workflow state would undo a card somebody dragged, on the next save,
# from a page that is not the owner of that state. `position` stands in for all four here
# because it is the one a drag writes.
mutation crates/gw-store/src/tasks.rs killed \
  's#"UPDATE tasks SET title = ?2, detached = 0, \\#"UPDATE tasks SET title = ?2, position = 0, detached = 0, \\#' \
  'reconciliation: the page owns the words and the record owns the state (D-2)'
# A checklist copied and pasted in the editor carries the attrs it was copied from, id and
# all. Two blocks quietly sharing one record put one card on the board for two lines, and
# then each edit to either rewrote the other's title.
mutation crates/gw-store/src/tasks.rs killed \
  's#            seen.insert(id).then(|| id.to_string())#            Some(id.to_string())#' \
  'reconciliation: a pasted line carrying an id already in use gets a record of its own'

# --- the task API: the same conflations, kept intact on the way out --------------------
#
# `gw-api` holds NO unfiltered accessor for a task, a board or a project — `gw-store`
# exposes none — so a handler here cannot leak a card by asking the wrong question. What it
# CAN do is un-conflate an answer the store deliberately conflated, or open a door the store
# would have closed a moment later, and those are the four below.
#
# None can pass vacuously: `a_board_discloses_no_card_whose_page_the_caller_may_not_read`
# asserts that the privileged caller sees all three cards, and every test named here asserts
# a POSITIVE outcome beside the refusal it is about.
#
# The first is the one that matters, and it is the only real permission gate in the file.
# `create_project` looks for a duplicate home page BEFORE it inserts, so that a second
# project is a 409 rather than a UNIQUE violation surfacing as a 500. That check runs after
# the caller has been authorised, deliberately: with the gate weakened to Read, somebody who
# may only READ a page is told "that page is already the home of a project" — a fact about a
# board they may not touch, and one a 403 does not give them.
mutation crates/gw-api/src/routes/tasks.rs killed \
  '/pub async fn create_project/,/^}$/ s@Action::Write@Action::Read@' \
  'project api: the door asks for WRITE, so a reader is refused before the conflict check speaks'
# Fail closed on a value this code does not understand (D-9). The mutation writes the
# tempting version — parse, and fall back to the default — which is not merely lax: it
# silently REOPENS a task somebody marked done, because `TaskStatus::default()` is `Offen`.
# The schema CHECK would never see the bad value, so nothing anywhere would say so.
mutation crates/gw-api/src/routes/tasks.rs killed \
  's@TaskStatus::from_stored(&composed).ok_or_else@Some(TaskStatus::from_stored(\&composed).unwrap_or_default()).ok_or_else@' \
  'task api: an unrecognised status is refused, never quietly defaulted to Offen'
# Existence before permission, for a PATH. Collapsing the refusal into 404 hides a
# configuration mistake behind a status code that says "you spelled it wrong" — the split
# `/api/documents`, `/api/links/backlinks` and `/api/revisions/document` all make.
mutation crates/gw-api/src/routes/tasks.rs killed \
  '/pub async fn document_tasks/,/^}$/ s@.ok_or(ApiError::Forbidden)?@.ok_or(ApiError::NotFound)?@' \
  "task api: a page's task list answers 404 for a page that is not there, 403 for one refused"
# And the opposite rule for an ID, which is where somebody copying the pattern above goes
# wrong. A project id is a uuid nobody guesses, so there is no existence to protect and
# everything unreachable is 404; a 403 would be an answer about a board the caller cannot
# see. `an_unreachable_project_answers_exactly_what_a_missing_one_answers` compares the two
# replies byte for byte, so the two branches cannot drift apart later either.
mutation crates/gw-api/src/routes/tasks.rs killed \
  '/pub async fn board/,/^}$/ s@.ok_or(ApiError::NotFound)?@.ok_or(ApiError::Forbidden)?@' \
  'board api: an id is not a path — an unreachable board is 404, never the 403 a path gets'

# --- the GLOBAL board (D-12): the same query with nothing bound ------------------------
#
# D-12 put a board in two places and named the cost in the same breath: two places that must
# agree. They agree by being ONE query with a filter — `board_for`'s project binding is an
# `Option` and the project board is that call with a project bound — so there is no second
# retrieval path here to mutate. That is the point: the mutations that stand behind the
# global board are the ones already recorded above, because the global board runs the same
# lines. In particular `board: a card names only a page the caller may actually read` and
# `board: the per-document verdict is acted on rather than merely computed` now guard the
# widest aggregate in the system as well as the narrowest, and
# `an_unbound_board_carries_every_card_the_caller_may_see_and_no_other` fails on its own for
# either of them — verified by hand, with the mutation applied and that one test run alone.
#
# What IS new is the filter, and each of the three below is a way of writing it that filters
# nothing. None can pass vacuously: the tests they must fail assert the positive too — which
# cards ARE on each board, and that the two places answer byte for byte.
#
# The first is the one that matters, and it SURVIVED when it was first written — which is the
# only reason the test it now needs exists.
#
# The loose half of the query is bound by project id, and a loose card has no page of its own:
# its `project_id` is the ONLY thing that can say which board it belongs on. Unbinding it
# looked equivalent, because `within` filters a loose card too whenever its project's home
# page lies outside the bound subtree — which was true of every fixture in the suite. It stops
# being true the moment a project is homed on a page INSIDE another project's subtree, which
# D-3 makes ordinary: the inner project's loose cards then land on the outer board.
# `a_project_homed_inside_another_keeps_its_loose_cards_to_itself` is that shape, and it fails
# on its own under this mutation.
mutation crates/gw-store/src/tasks.rs killed \
  's|AND (?2 IS NULL OR t.project_id = ?2)|AND (?2 IS NULL OR t.project_id IS NOT NULL)|' \
  'global board: the project binding filters the loose cards, which nothing else can'
# What a card SAYS about its page, in the one place the unified query could get it wrong. A
# loose card names no page; naming the one that governs it would claim a line exists on a
# page that never held one, and the page named is real, readable and wrong.
mutation crates/gw-store/src/tasks.rs killed \
  's|                (anchored == 1).then_some(governed.page),|                Some(governed.page),|' \
  'global board: a loose card names no page, and an anchored one names the page it is on'
# `?seite=` answers the board of the project homed AT that path. Matching any project instead
# is the shape of the bug — `find` on a predicate that is true too often — and on a page that
# is nobody's home it turns an empty board into somebody else's, on every ordinary page a
# loader renders.
mutation crates/gw-api/src/routes/tasks.rs killed \
  's@.find(|project| project.home_path == path)@.find(|project| true \|\| project.home_path == path)@' \
  'global board: seite= answers the board of the project homed at THAT path, or none'
# The two status-code rules, on one endpoint. `?seite=` is a path and `?projekt=` is an id,
# so they answer differently on purpose — which makes this the likeliest place in the crate
# for somebody to copy the wrong one of the two branches sitting next to each other.
mutation crates/gw-api/src/routes/tasks.rs killed \
  '/pub async fn global_board/,/^}$/ s@.ok_or(ApiError::NotFound)?@.ok_or(ApiError::Forbidden)?@' \
  'global board: projekt= is an id — unreachable is 404, never the 403 a path gets'
mutation crates/gw-api/src/routes/tasks.rs killed \
  '/pub async fn global_board/,/^}$/ s@.ok_or(ApiError::Forbidden)?@.ok_or(ApiError::NotFound)?@' \
  'global board: seite= is a path — 404 for a page that is not there, 403 for one refused'
# Existence before permission, for the path binding. With the check gone an absent page falls
# through to the accessor and is refused as 403, which says nothing exists at a path where
# nothing does — the configuration mistake this split exists to keep visible.
mutation crates/gw-api/src/routes/tasks.rs killed \
  '/pub async fn global_board/,/^}$/ s@            if !state@            if false \&\& !state@' \
  'global board: seite= asks whether the page is there before it asks who may read it'

# --- may_write: the bit an interface offers a control on (0010) -------------------------
#
# Nothing on the wire used to say whether the caller may WRITE a page, so every control that
# needs write was offered to whoever was signed in and the true answer arrived as a refusal
# afterwards. The fix is one boolean — and the whole of its correctness is that it is the
# SAME `permits()` verdict the write itself goes through, taken from the same visibility and
# the same grants, one action further along. A separately computed "can I write this" is a
# second answer, and a second answer can disagree with the one that decides: the interface
# then either offers somebody a thing that is refused, or hides a control from somebody
# entitled to it — and nobody reports the second.
#
# Which is why the mutations below are not about a value being wrong. They are about the bit
# being derived from something OTHER than the verdict, which is what every plausible wrong
# version of this code does. `the_write_bit_agrees_with_what_a_write_actually_does`
# (gw-store) and `may_write_on_the_wire_agrees_with_what_a_write_actually_does` (gw-api) are
# what fail: neither compares the boolean against a written-down expectation — each asks for
# the bit and then PERFORMS the write, for four callers refused for four different reasons.
# Neither can pass vacuously either; both assert that the four callers did not all answer the
# same way, which would make the agreement a constant.
mutation crates/gw-store/src/acl.rs killed \
  's|        let may_write = permits(principal, Action::Write, visibility, \&grants, baseline);|        let may_write = permits(principal, Action::Read, visibility, \&grants, baseline);|' \
  'may_write: the bit is the WRITE verdict, not the read that just permitted the answer'
# The same line, asked with a reach that is not the caller's. This one is EQUIVALENT and is
# recorded rather than dropped, because the reason it is equivalent is a rule worth pinning:
# D-M2-8 says no baseline confers write, so `permits` returns on the grant alone for any
# action but Read and the baseline argument cannot change this line's answer. If that ever
# stops being true the script will report this entry as mis-recorded, which is exactly the
# alarm wanted — a baseline that silently began conferring write would otherwise turn this
# bit into an offer nobody can accept.
mutation crates/gw-store/src/acl.rs equivalent \
  's|        let may_write = permits(principal, Action::Write, visibility, \&grants, baseline);|        let may_write = permits(principal, Action::Write, visibility, \&grants, Baseline::Admin);|' \
  "may_write: the baseline cannot widen it — D-M2-8 means no baseline confers write, so this argument is dead for Action::Write"
# A board of forty cards is the surface where "just say yes" is most tempting, because the
# person writing it is looking at their own board and every card on it is theirs to move.
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn board_for/,/^        in_board_order/ s|                governed.may_write,|                true,|' \
  'may_write: a board says WHICH cards may be moved, not that all of them may'
# And the other half: a card created ON a board names no page at all, so an implementation
# that hung the verdict off the page the card NAMES answers false for somebody who may move
# it perfectly well. `a_card_that_names_no_page_still_says_whether_it_may_be_moved` is the
# shape that can tell the two apart; the ordinary fixture cannot, because its anchored cards
# have both.
mutation crates/gw-store/src/tasks.rs killed \
  '/pub async fn board_for/,/^        in_board_order/ s|                governed.may_write,|                anchored == 1 \&\& governed.may_write,|' \
  'may_write: a card that names no page still says whether it may be moved'
# A project listing is the same claim one object up, and it is what »Neues Projekt« and the
# delete control are offered on.
mutation crates/gw-store/src/tasks.rs killed \
  's|            may_write: home.may_write,|            may_write: true,|' \
  'may_write: a project says which rows may be changed, not that every listed row may'
# The two places the wire could invent it instead of carrying it. The module header of
# `routes/tasks.rs` says this layer takes no permission decision of its own; these are what
# make that a property rather than a promise.
mutation crates/gw-api/src/routes/tasks.rs killed \
  's|            may_write: task.may_write,|            may_write: true,|' \
  'may_write: a card on the wire carries the store verdict, not one the handler made up'
mutation crates/gw-api/src/routes/docs.rs killed \
  's|                may_write: access.may_write,|                may_write: true,|' \
  'may_write: a page read carries the verdict the accessor gave, not a constant'

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
# DO NOT WRAP THIS IN `timeout`. The trap restores the file and does NOT exit — bash resumes
# the interrupted command — so a SIGTERM arriving mid-mutation un-mutates the code UNDER the
# test that is scoring it. That test then passes and the entry is recorded SURVIVED, while
# the run carries on past its own deadline as if nothing had happened. Nothing in the output
# says so: `verify_tree` is satisfied, because the file was put back to exactly what it
# should be, and `note_drift` sees a clean tree for the same reason. It happened on
# 2026-08-24 — `timeout 595`, fired eleven seconds into `tasks: reading one task follows the
# read on the page that governs it`, which re-ran KILLED alone a minute later. The tell is a
# lone SURVIVED near the end of a run whose reported wall clock EXCEEDS the timeout it was
# given. If you need a deadline, run this in the background and read the summary when it
# lands.
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
  # `|| true`, and it is load-bearing rather than tidy. `diff` exits 1 when it finds a
  # difference — which is the only reason this line runs at all — and under `set -euo
  # pipefail` that status is the pipeline's, so the warning KILLED the run: five verdicts,
  # one drift warning, exit 1, and no summary at all. The comment above this function says
  # in as many words that this warns and does not fail; for one run it did the opposite of
  # what it documented, and the tell was an exit code with nothing after it. Found on
  # 2026-08-24, with another agent saving Svelte files while the suite ran — the exact
  # scenario the paragraph above was written for.
  diff <(cat "$TREE_STATE") <(echo "$now") | grep '^[<>]' | sed 's/^/                    /' || true
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
    # The task, board and project endpoints. One integration binary covers all of them,
    # including the two disclosure tests, so the probe is exact rather than a whole-crate
    # build of six binaries.
    crates/gw-api/src/routes/tasks.rs) echo "-p gw-api --test tasks" ;;
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
