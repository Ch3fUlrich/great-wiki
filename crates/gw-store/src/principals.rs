use crate::Store;
use anyhow::Result;
use gw_auth::{Principal, PrincipalKind};
use serde_json::json;
use sqlx::FromRow;

/// The one definition of the merge key, and the only thing `Oma@Example.de ` and
/// `oma@example.de` have in common.
///
/// `None` means "this address cannot be matched", and that is a first-class answer rather
/// than a failure: the address is still stored exactly as it was typed, it is still shown
/// in the console, and it simply never merges. Guessing at what somebody meant is how two
/// people become one account.
///
/// The rule, in order, and every step of it is a decision:
///
/// 1. **Trim ASCII whitespace.** A pasted address routinely carries a trailing space.
/// 2. **Refuse anything that is not printable ASCII**, `!`–`~`. This is the deliberate
///    answer to internationalised addresses, and it is a refusal rather than an attempt:
///    folding `exämple.de` to its punycode would make it equal to a DIFFERENT
///    registration, and `оma@…` with a Cyrillic о renders identically to `oma@…` in every
///    font a person reads. Either one, silently folded, hands one person's grants to
///    whoever can register the other. So a non-ASCII address is kept, displayed, and
///    never used as a key — the person is invited under an ASCII address, or the owner
///    grants them access by hand. If IDN ever has to be supported it needs UTS-46 plus a
///    confusable check, which is a decision of its own and not a patch to this function.
/// 3. **Require exactly one `@`**, with a non-empty local part and a non-empty domain.
/// 4. **Lowercase both sides.** RFC 5321 makes the local part case-SENSITIVE, and this
///    deliberately ignores that: no mail provider a family uses distinguishes `Oma` from
///    `oma`, the owner's decision names `Oma@Example.de` and `oma@example.de ` as one
///    person, and treating them as two would leave exactly the duplicate this exists to
///    remove. The cost is that a provider which really does distinguish them would let
///    two of its accounts claim one identity here — noted rather than handled, because
///    the alternative fails for every real user to defend against none.
pub fn canonical_email(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    if !trimmed.bytes().all(|b| (0x21..=0x7e).contains(&b)) || trimmed.is_empty() {
        return None;
    }
    let (local, domain) = trimmed.split_once('@')?;
    if local.is_empty() || domain.is_empty() || domain.contains('@') {
        return None;
    }
    Some(format!(
        "{}@{}",
        local.to_ascii_lowercase(),
        domain.to_ascii_lowercase()
    ))
}

#[derive(FromRow)]
struct PrincipalRow {
    id: String,
    kind: String,
    username: String,
    oidc_username: Option<String>,
    display_name: String,
    email: Option<String>,
    groups: String,
    active: i64,
}

impl PrincipalRow {
    fn into_principal(self, teams: Vec<String>) -> Principal {
        Principal {
            id: self.id,
            // Fail closed on an unrecognised value: `local` is the kind with no OIDC
            // groups, so a corrupted row loses reach rather than gaining it.
            kind: if self.kind == "oidc" {
                PrincipalKind::Oidc
            } else {
                PrincipalKind::Local
            },
            username: self.username,
            oidc_username: self.oidc_username,
            display_name: self.display_name,
            email: self.email,
            groups: serde_json::from_str(&self.groups).unwrap_or_default(),
            teams,
            active: self.active != 0,
        }
    }
}

/// Write a local principal and its credential. Returns the new id.
///
/// Takes a connection rather than the pool so it can be part of a larger transaction —
/// [`crate::admin`] creates an account and its audit row together. One implementation for
/// both callers, because the credential row is the easy half to forget from a second one,
/// and an account with no credential cannot sign in at all.
pub(crate) async fn insert_local_principal(
    conn: &mut sqlx::SqliteConnection,
    username: &str,
    display_name: &str,
    email: Option<&str>,
    password_hash: &str,
) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();

    // The merge key is derived HERE rather than by each caller, so that every local
    // account — invited, or typed in under »Person anlegen« — arrives with one, and no
    // path can produce an account that is invisible to the merge by forgetting a field.
    sqlx::query(
        "INSERT INTO principals (id, kind, username, display_name, email, email_canonical) \
         VALUES (?1, 'local', ?2, ?3, ?4, ?5)",
    )
    .bind(&id)
    .bind(username)
    .bind(display_name)
    .bind(email)
    .bind(email.and_then(canonical_email))
    .execute(&mut *conn)
    .await?;

    sqlx::query("INSERT INTO credentials (principal_id, password_hash) VALUES (?1, ?2)")
        .bind(&id)
        .bind(password_hash)
        .execute(&mut *conn)
        .await?;

    Ok(id)
}

/// Set the active flag, deleting every session the principal holds when deactivating
/// (D-M2-7). Returns whether a principal row was actually matched.
///
/// Same reason as [`insert_local_principal`] for taking a connection: the flag, the
/// sessions and the audit row belong to one transaction. Two statements would leave a
/// window in which the account is inactive but its cookies still resolve.
pub(crate) async fn apply_active(
    conn: &mut sqlx::SqliteConnection,
    id: &str,
    active: bool,
) -> Result<bool> {
    let result = sqlx::query("UPDATE principals SET active = ?2 WHERE id = ?1")
        .bind(id)
        .bind(i64::from(active))
        .execute(&mut *conn)
        .await?;

    if !active {
        sqlx::query("DELETE FROM sessions WHERE principal_id = ?1")
            .bind(id)
            .execute(&mut *conn)
            .await?;
    }

    Ok(result.rows_affected() > 0)
}

/// Add or remove the per-account promotion (0006). Returns whether a row actually moved.
///
/// Takes a connection for the same reason [`apply_active`] does: the row, the check that
/// an administrator is left, and the audit entry belong to one transaction. The boolean is
/// what tells "already promoted" (nothing to do) apart from "promoted just now", so that
/// only the second is recorded.
pub(crate) async fn apply_instance_admin(
    conn: &mut sqlx::SqliteConnection,
    id: &str,
    admin: bool,
    granted_by: &str,
) -> Result<bool> {
    let result = if admin {
        sqlx::query(
            "INSERT OR IGNORE INTO instance_admins (principal_id, granted_by) VALUES (?1, ?2)",
        )
        .bind(id)
        .bind(granted_by)
        .execute(&mut *conn)
        .await?
    } else {
        sqlx::query("DELETE FROM instance_admins WHERE principal_id = ?1")
            .bind(id)
            .execute(&mut *conn)
            .await?
    };
    Ok(result.rows_affected() > 0)
}

/// What a sign-in through Authelia turned out to be.
///
/// Five outcomes and not a `Result<Principal>`, because three of them are decisions the
/// caller has to log differently and two of them are refusals that are not errors: a
/// refused sign-in is a well-formed request that is not allowed to become an identity,
/// and collapsing it into an `Err` is how it would end up reported as an outage.
#[derive(Debug)]
pub enum OidcSignIn {
    /// Already known by this Authelia handle. Every sign-in after the first.
    Recognised(Principal),
    /// First sign-in: a principal of their own, with whatever `group_roles` confers.
    Created(Principal),
    /// The verified address matched an account that already existed here. `username` is
    /// the handle that account keeps — the other half of the pair, for the log.
    Merged {
        principal: Box<Principal>,
        username: String,
    },
    /// Refused: the Authelia handle is another principal's `username`, and no verified
    /// address pointed at anybody. Merging on a bare name collision is a takeover.
    UsernameHeldByAnother,
    /// Refused: more than one live account carries that address, so which person this is
    /// cannot be answered. Fail closed and let the owner resolve it under »Personen«.
    AmbiguousEmail,
}

/// One address, and every account that carries it.
///
/// Only ever built for an address more than one account carries — see
/// [`Store::merge_candidates`], which is a listing and never a merge.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MergeCandidate {
    pub email: String,
    pub accounts: Vec<MergeCandidateAccount>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MergeCandidateAccount {
    pub id: String,
    pub username: String,
    pub kind: PrincipalKind,
    /// True when this account already answers to Authelia as well as to a password, so
    /// the owner can see that half the pair is done.
    pub already_merged: bool,
    pub active: bool,
}

/// Somebody who could be promoted to administer the instance.
///
/// Carries where the account comes from and what it already holds, because the person
/// choosing a successor is choosing under time pressure — usually because they are about
/// to lose their own access — and a list of bare usernames is how the wrong `m.schmidt`
/// gets the instance.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AdminCandidate {
    pub id: String,
    pub username: String,
    pub display_name: String,
    /// `oidc` for an Authelia account, `local` for a great-wiki guest account.
    pub kind: PrincipalKind,
    /// The verified Authelia groups, so `admins` next to a name is visible.
    pub groups: Vec<String>,
    /// When this account was last seen doing something, or `None` if never — see
    /// [`Store::admin_candidates`] for what that is derived from.
    pub last_active_at: Option<String>,
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
    /// The plain form of [`Store::upsert_oidc_identity`], with no merge key: it is what
    /// the development shim and the tests use, and it is the exact behaviour an
    /// unverified address gets. A refusal becomes an error here rather than an outcome,
    /// because a caller that passes no merge key has nothing to do about one.
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
        match self
            .upsert_oidc_identity(username, display_name, email, None, groups)
            .await?
        {
            OidcSignIn::Recognised(p) | OidcSignIn::Created(p) => Ok(p),
            OidcSignIn::Merged { principal, .. } => Ok(*principal),
            refused => Err(anyhow::anyhow!("the sign-in was refused: {refused:?}")),
        }
    }

    /// Sign somebody in through Authelia, merging them onto the account they already have
    /// here when a VERIFIED address says it is the same person.
    ///
    /// `merge_key` is [`canonical_email`] of the address Authelia asserted as
    /// `email_verified: true`, and nothing else may ever be passed here — not the `email`
    /// claim on its own, and certainly not an address somebody typed. `None` is the
    /// old behaviour in full: a principal of this account's own, with no access.
    ///
    /// The order is deliberate and each step is the refusal the one below it would
    /// otherwise become:
    ///
    /// 1. **Known by this Authelia handle** → refresh and return. This is every sign-in
    ///    after the first, merged or not.
    /// 2. **A verified address that exactly one live, un-merged account carries** → merge
    ///    onto it and record both halves. The account keeps its id, so every grant, every
    ///    revision and every attachment it already owns is simply still there.
    /// 3. **Two accounts carry it** → refused. "Which of these is this person" has no safe
    ///    guess, and guessing hands one of them somebody else's access.
    /// 4. **Nobody carries it** → a new principal, exactly as before.
    /// 5. **…unless the Authelia handle is already somebody else's `username`** → refused.
    ///
    /// What a merge does NOT do is widen anything. The account keeps its own direct
    /// grants; what it gains is the Authelia groups, which reach only as far as the
    /// `group_roles` table already says they do — the same union [`crate::acl::baseline_on`]
    /// computes for every mirrored account today. There is no third source.
    pub async fn upsert_oidc_identity(
        &self,
        oidc_username: &str,
        display_name: &str,
        email: Option<&str>,
        merge_key: Option<&str>,
        groups: &[String],
    ) -> Result<OidcSignIn> {
        let groups_json = serde_json::to_string(groups)?;
        let canonical = email.and_then(canonical_email);
        let mut tx = self.pool.begin().await?;

        // 1. Somebody this Authelia account has signed in as before.
        let known: Option<(String, String)> =
            sqlx::query_as("SELECT id, kind FROM principals WHERE oidc_username = ?1")
                .bind(oidc_username)
                .fetch_optional(&mut *tx)
                .await?;
        if let Some((id, kind)) = known {
            // Authelia is authoritative for the groups of anybody it signs in, always.
            // It is authoritative for the DISPLAY NAME only of an account it is the sole
            // source of: a merged account's name was typed by the person themselves on
            // the acceptance page, and having it silently replaced by whatever the
            // homelab directory holds is a rename nobody asked for.
            if kind == "oidc" {
                sqlx::query(
                    "UPDATE principals SET display_name = ?2, email = ?3, email_canonical = ?4, \
                     groups = ?5, last_seen_at = datetime('now') WHERE id = ?1",
                )
                .bind(&id)
                .bind(display_name)
                .bind(email)
                .bind(&canonical)
                .bind(&groups_json)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(
                    "UPDATE principals SET groups = ?2, last_seen_at = datetime('now') \
                     WHERE id = ?1",
                )
                .bind(&id)
                .bind(&groups_json)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            return Ok(OidcSignIn::Recognised(self.must_read(&id).await?));
        }

        // 2-3. A verified address, and who already answers to it.
        if let Some(key) = merge_key {
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT id, username FROM principals \
                 WHERE email_canonical = ?1 AND oidc_username IS NULL AND active = 1",
            )
            .bind(key)
            .fetch_all(&mut *tx)
            .await?;

            match rows.len() {
                // A deactivated account is deliberately NOT a candidate, and neither is
                // one already answering to a different Authelia handle. Both are excluded
                // by the WHERE above rather than refused here, so this sign-in falls
                // through to an account of its own — "this person is gone" must not be
                // undone by them signing in the other way.
                0 => {}
                1 => {
                    let (id, username) = rows.into_iter().next().expect("one row");
                    sqlx::query(
                        "UPDATE principals SET oidc_username = ?2, groups = ?3, \
                         last_seen_at = datetime('now') WHERE id = ?1",
                    )
                    .bind(&id)
                    .bind(oidc_username)
                    .bind(&groups_json)
                    .execute(&mut *tx)
                    .await?;

                    // Instance-wide, and it names BOTH halves: an auditor reading this a
                    // year later has to be able to see that `erika.mueller` and `oma`
                    // became one row, and on the strength of which address.
                    Self::record_audit(
                        &mut *tx,
                        Some(&id),
                        "identity.merge",
                        Some(&id),
                        None,
                        &json!({
                            "username": username,
                            "oidc_username": oidc_username,
                            "email": key,
                            "groups": groups,
                        }),
                    )
                    .await?;
                    tx.commit().await?;
                    return Ok(OidcSignIn::Merged {
                        principal: Box::new(self.must_read(&id).await?),
                        username,
                    });
                }
                _ => {
                    tx.rollback().await?;
                    return Ok(OidcSignIn::AmbiguousEmail);
                }
            }
        }

        // 5. Before creating anything: the handle must not already be a person here.
        //
        // This is the hole the old `ON CONFLICT (username) DO UPDATE` left open. An
        // Authelia account called `oma` used to take over the local `oma` outright — its
        // grants, its revisions, its history — with no address involved and nothing
        // logged. Refused now, and the operator reads why in the sign-in log.
        let taken: Option<(String,)> =
            sqlx::query_as("SELECT id FROM principals WHERE username = ?1")
                .bind(oidc_username)
                .fetch_optional(&mut *tx)
                .await?;
        if taken.is_some() {
            tx.rollback().await?;
            return Ok(OidcSignIn::UsernameHeldByAnother);
        }

        // 4. A principal of their own, mirrored from the verified claims.
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO principals \
             (id, kind, username, oidc_username, display_name, email, email_canonical, groups, \
              last_seen_at) \
             VALUES (?1, 'oidc', ?2, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        )
        .bind(&id)
        .bind(oidc_username)
        .bind(display_name)
        .bind(email)
        .bind(&canonical)
        .bind(&groups_json)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(OidcSignIn::Created(self.must_read(&id).await?))
    }

    /// A principal that has just been written in this process. Its absence is a bug here,
    /// not a caller's mistake, so it is an error rather than an `Option` every call site
    /// would have to invent an answer for.
    async fn must_read(&self, id: &str) -> Result<Principal> {
        self.principal_by_id(id)
            .await?
            .map(|(p, _)| p)
            .ok_or_else(|| anyhow::anyhow!("principal {id} vanished immediately after writing it"))
    }

    /// Give every row that predates the merge its key, without inventing one.
    ///
    /// Run from [`Store::open`] after the migrations, because the rule that derives the
    /// key is [`canonical_email`] and there is exactly one of it — see the comment in
    /// `0014_identity_merge.sql` for why it is not a second implementation in SQL.
    /// Idempotent: an address that produces no key stays NULL and is looked at again next
    /// start, over a table with tens of rows.
    pub(crate) async fn backfill_email_keys(&self) -> Result<u64> {
        let mut written = 0;
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, email FROM principals WHERE email IS NOT NULL AND email_canonical IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        for (id, email) in rows {
            if let Some(key) = canonical_email(&email) {
                sqlx::query("UPDATE principals SET email_canonical = ?2 WHERE id = ?1")
                    .bind(&id)
                    .bind(&key)
                    .execute(&self.pool)
                    .await?;
                written += 1;
            }
        }

        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, email FROM invites WHERE email IS NOT NULL AND email_canonical IS NULL",
        )
        .fetch_all(&self.pool)
        .await?;
        for (id, email) in rows {
            if let Some(key) = canonical_email(&email) {
                sqlx::query("UPDATE invites SET email_canonical = ?2 WHERE id = ?1")
                    .bind(&id)
                    .bind(&key)
                    .execute(&self.pool)
                    .await?;
                written += 1;
            }
        }
        Ok(written)
    }

    /// Accounts that would merge if the owner said so — and nothing else.
    ///
    /// **This reads. It never writes.** Merging what is already in a database is the
    /// owner's decision and not a migration's: two accounts sharing an address might be
    /// one person with two credentials, or might be a shared family address that two
    /// people genuinely use, and the second case merged is one person silently holding
    /// the other's grants. So there is a listing, the listing is all there is, and
    /// resolving a pair is done by hand under »Personen«.
    ///
    /// A group is only interesting when it has more than one account in it; an address
    /// nobody shares needs no decision.
    pub async fn merge_candidates(&self) -> Result<Vec<MergeCandidate>> {
        let rows: Vec<(String, String, String, String, Option<String>, i64)> = sqlx::query_as(
            "SELECT email_canonical, id, kind, username, oidc_username, active \
             FROM principals WHERE email_canonical IS NOT NULL \
             ORDER BY email_canonical, username",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out: Vec<MergeCandidate> = Vec::new();
        for (email, id, kind, username, oidc_username, active) in rows {
            let account = MergeCandidateAccount {
                id,
                username,
                kind: if kind == "oidc" {
                    PrincipalKind::Oidc
                } else {
                    PrincipalKind::Local
                },
                already_merged: oidc_username.is_some() && kind != "oidc",
                active: active != 0,
            };
            match out.last_mut() {
                Some(group) if group.email == email => group.accounts.push(account),
                _ => out.push(MergeCandidate {
                    email,
                    accounts: vec![account],
                }),
            }
        }
        out.retain(|group| group.accounts.len() > 1);
        Ok(out)
    }

    pub async fn create_local_principal(
        &self,
        username: &str,
        display_name: &str,
        email: Option<&str>,
        password_hash: &str,
    ) -> Result<Principal> {
        let mut tx = self.pool.begin().await?;
        insert_local_principal(&mut tx, username, display_name, email, password_hash).await?;
        tx.commit().await?;

        self.principal_by_username(username)
            .await?
            .map(|(p, _)| p)
            .ok_or_else(|| anyhow::anyhow!("principal vanished immediately after insert"))
    }

    /// Returns the principal and, for local accounts, its password hash.
    pub async fn principal_by_username(
        &self,
        username: &str,
    ) -> Result<Option<(Principal, Option<String>)>> {
        let row: Option<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, oidc_username, display_name, email, groups, active \
             FROM principals WHERE username = ?1",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?;

        self.hydrate(row).await
    }

    /// The same, by primary key. What a session resolves through: a session names an id,
    /// and the principal behind it is read fresh on every request (D-M2-7).
    ///
    /// Keyed by id rather than by username on purpose — a username is a display handle and
    /// could in principle be changed, whereas the id is what every grant and membership
    /// already points at.
    pub async fn principal_by_id(&self, id: &str) -> Result<Option<(Principal, Option<String>)>> {
        let row: Option<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, oidc_username, display_name, email, groups, active \
             FROM principals WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        self.hydrate(row).await
    }

    /// Attach the credential and the team memberships a bare row does not carry.
    ///
    /// One implementation for both lookups, so `principal_by_id` cannot drift into
    /// returning a principal with no teams while `principal_by_username` returns one with
    /// them — a difference that would show up as a permission bug, not as a data bug.
    async fn hydrate(
        &self,
        row: Option<PrincipalRow>,
    ) -> Result<Option<(Principal, Option<String>)>> {
        let Some(row) = row else { return Ok(None) };
        let hash: Option<(String,)> =
            sqlx::query_as("SELECT password_hash FROM credentials WHERE principal_id = ?1")
                .bind(&row.id)
                .fetch_optional(&self.pool)
                .await?;
        let teams = self.teams_of(&row.id).await?;
        Ok(Some((row.into_principal(teams), hash.map(|(h,)| h))))
    }

    /// Activate or deactivate an account.
    ///
    /// Deactivating additionally **deletes every session that principal holds**, in the
    /// SAME transaction as the flag (D-M2-7). Two statements would leave a window in which
    /// the account is inactive but its cookies still resolve; one transaction leaves none.
    ///
    /// Reactivating deliberately does not restore anything — sessions are not recoverable,
    /// and the person signs in again. Restoring them would mean keeping deleted rows,
    /// which is the opposite of what "invalidate everywhere" promises.
    pub async fn set_principal_active(&self, id: &str, active: bool) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        apply_active(&mut tx, id, active).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Promote or demote an account, with no actor and no record.
    ///
    /// The plain sibling of [`Store::set_instance_admin_audited`], for seeding and for
    /// tests. It does NOT enforce the floor of one administrator — the audited variant
    /// does, in the transaction that also writes the audit row, which is the only place
    /// the count can be trusted. Nothing reachable from the API may call this.
    pub async fn set_instance_admin(&self, id: &str, admin: bool) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let changed = apply_instance_admin(&mut tx, id, admin, "system").await?;
        tx.commit().await?;
        Ok(changed)
    }

    /// Who could be promoted to administer the instance, most recently active first.
    ///
    /// **Activity is derived, because nothing records it directly.** `principals` does
    /// carry a `last_seen_at`, but only [`Store::upsert_oidc_principal`] ever writes it —
    /// it is NULL for every local guest account, which is precisely the population this
    /// list exists to choose from. So the answer here is the more recent of:
    ///
    ///   * the newest session the account holds (`sessions.created_at`), which is the last
    ///     time it signed in and still has a live session; and
    ///   * the newest audit entry the account is the ACTOR of (`audit_log.at`), which is
    ///     the last time it did something administrative.
    ///
    /// Both are approximations and the comment is here so nobody mistakes them for more:
    /// signing out or being deactivated deletes the sessions, and expired ones are swept,
    /// so an account that was busy last year and has since signed out reads as never seen.
    /// It is enough for the job — sorting the plausible successors above the dormant ones
    /// — and it invents nothing.
    ///
    /// The list excludes anyone who ALREADY holds the admin baseline, by promotion or by
    /// Authelia group: a candidate is somebody whose promotion would add an administrator,
    /// and offering the ones who would add none is how a handover ends with nobody in
    /// charge. Deactivated accounts are excluded for the same reason.
    pub async fn admin_candidates(&self) -> Result<Vec<AdminCandidate>> {
        // The empty string is what "never" sorts as, and it sorts BELOW every timestamp
        // in the `datetime('now')` format, so accounts with no evidence land at the
        // bottom rather than at the top. Ties break on username so the order is stable.
        let rows: Vec<(String, String, String, String, String, String)> = sqlx::query_as(
            "SELECT p.id, p.kind, p.username, p.display_name, p.groups,
                    MAX(
                      COALESCE((SELECT MAX(s.created_at) FROM sessions s
                                WHERE s.principal_id = p.id), ''),
                      COALESCE((SELECT MAX(a.at) FROM audit_log a
                                WHERE a.principal_id = p.id), '')
                    ) AS last_active_at
             FROM principals p
             WHERE p.active = 1
             ORDER BY last_active_at DESC, p.username ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut conn = self.pool.acquire().await?;
        let mut out = Vec::new();
        for (id, kind, username, display_name, groups, last_active_at) in rows {
            let groups: Vec<String> = serde_json::from_str(&groups).unwrap_or_default();
            if crate::acl::baseline_on(&mut conn, &id, &groups).await? >= crate::Baseline::Admin {
                continue;
            }
            out.push(AdminCandidate {
                id,
                username,
                display_name,
                // Fail closed on an unrecognised value, exactly as `PrincipalRow` does.
                kind: if kind == "oidc" {
                    PrincipalKind::Oidc
                } else {
                    PrincipalKind::Local
                },
                groups,
                last_active_at: (!last_active_at.is_empty()).then_some(last_active_at),
            });
        }
        Ok(out)
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

    /// Put somebody in a team. Returns whether a row was actually written.
    ///
    /// The boolean matters because of how this is expressed: the insert selects the team
    /// id by slug, so a slug that names no team inserts NO ROWS and reports success. An
    /// administrator would see "added to team" and the person would have gained nothing —
    /// the failure mode being that a typo in a team name silently withholds access
    /// instead of announcing itself.
    pub async fn add_team_member(&self, team_slug: &str, principal_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "INSERT OR IGNORE INTO team_members (team_id, principal_id) \
             SELECT id, ?2 FROM teams WHERE slug = ?1",
        )
        .bind(team_slug)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

/// A team and who is in it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TeamSummary {
    pub slug: String,
    pub name: String,
    /// Principal ids. Resolving them to names is the caller's job, so this stays one
    /// query regardless of how many teams there are.
    pub members: Vec<String>,
}

impl Store {
    /// Everyone great-wiki knows about, active and deactivated alike.
    ///
    /// Deactivated accounts are included deliberately: an administrator needs to see that
    /// an account exists and is suspended. Hiding it would make "this person cannot sign
    /// in" indistinguishable from "this person was never here", and the second invites
    /// creating a duplicate.
    pub async fn list_principals(&self) -> Result<Vec<Principal>> {
        let rows: Vec<PrincipalRow> = sqlx::query_as(
            "SELECT id, kind, username, oidc_username, display_name, email, groups, active \
             FROM principals ORDER BY username",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let teams = self.teams_of(&row.id).await?;
            out.push(row.into_principal(teams));
        }
        Ok(out)
    }

    pub async fn list_teams(&self) -> Result<Vec<TeamSummary>> {
        let teams: Vec<(String, String, String)> =
            sqlx::query_as("SELECT id, slug, name FROM teams ORDER BY slug")
                .fetch_all(&self.pool)
                .await?;

        let mut out = Vec::with_capacity(teams.len());
        for (id, slug, name) in teams {
            let members: Vec<(String,)> =
                sqlx::query_as("SELECT principal_id FROM team_members WHERE team_id = ?1")
                    .bind(&id)
                    .fetch_all(&self.pool)
                    .await?;
            out.push(TeamSummary {
                slug,
                name,
                members: members.into_iter().map(|(m,)| m).collect(),
            });
        }
        Ok(out)
    }

    /// Remove somebody from a team. Returns whether a row was actually removed.
    ///
    /// Same reason [`Store::remove_grant`] returns a boolean: a removal that matched
    /// nothing must not report success to an administrator who is about to conclude that
    /// access has been withdrawn.
    pub async fn remove_team_member(&self, team_slug: &str, principal_id: &str) -> Result<bool> {
        let result = sqlx::query(
            "DELETE FROM team_members WHERE principal_id = ?2 \
             AND team_id = (SELECT id FROM teams WHERE slug = ?1)",
        )
        .bind(team_slug)
        .bind(principal_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod admin_tests {
    use crate::Store;
    use gw_auth::{Permission, Subject};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    #[tokio::test]
    async fn adding_to_a_team_that_does_not_exist_reports_failure() {
        // The insert selects the team id by slug, so a slug naming no team writes no
        // rows. Before this returned a boolean it reported success, and a typo in a team
        // name silently withheld access instead of announcing itself.
        let store = store().await;
        let p = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap()
            .id;

        assert!(
            !store.add_team_member("tippfehler", &p).await.unwrap(),
            "a nonexistent team reported a successful add"
        );

        store.create_team("redaktion", "Redaktion").await.unwrap();
        assert!(store.add_team_member("redaktion", &p).await.unwrap());
    }

    #[tokio::test]
    async fn removing_a_membership_that_is_not_there_reports_failure() {
        let store = store().await;
        let p = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap()
            .id;
        store.create_team("redaktion", "Redaktion").await.unwrap();

        assert!(!store.remove_team_member("redaktion", &p).await.unwrap());
        store.add_team_member("redaktion", &p).await.unwrap();
        assert!(store.remove_team_member("redaktion", &p).await.unwrap());
        assert!(!store.remove_team_member("redaktion", &p).await.unwrap());
    }

    #[tokio::test]
    async fn revoking_an_inherited_grant_from_the_child_reports_failure() {
        // The grant lives on the ancestor. A console offering a revoke control here would
        // be claiming an effect it cannot have — the boolean is what lets it refuse.
        let store = store().await;
        store
            .add_grant("/raum", Subject::Anyone, Permission::Read)
            .await
            .unwrap();

        assert!(
            !store
                .remove_grant("/raum/unterseite", &Subject::Anyone, Permission::Read)
                .await
                .unwrap(),
            "revoking an inherited grant from the child claimed success"
        );
        assert!(store
            .remove_grant("/raum", &Subject::Anyone, Permission::Read)
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn effective_grants_name_the_ancestor_they_come_from() {
        let store = store().await;
        store
            .add_grant("/raum", Subject::Anyone, Permission::Read)
            .await
            .unwrap();

        let (source, grants) = store.effective_grants("/raum/tief/tiefer").await.unwrap();
        assert_eq!(source.as_deref(), Some("/raum"));
        assert_eq!(grants.len(), 1);

        // Nothing anywhere above: reach is whatever the baseline confers, and the console
        // must say so rather than implying an empty grant list was configured here.
        let (none, empty) = store.effective_grants("/woanders").await.unwrap();
        assert_eq!(none, None);
        assert!(empty.is_empty());
    }

    /// Rewrite a stored timestamp.
    ///
    /// `datetime('now')` has one-second resolution, so a test that created two sessions in
    /// a row would compare two identical timestamps and pass whatever the ORDER BY said.
    /// Backdating is the only way to assert an ORDER without sleeping through a real
    /// second, and it is available here because the pool is crate-private — which is also
    /// why this test lives in the store rather than against the HTTP API.
    async fn backdate(store: &Store, table: &str, column: &str, id: &str, at: &str) {
        let affected = sqlx::query(&format!(
            "UPDATE {table} SET {column} = ?2 WHERE principal_id = ?1"
        ))
        .bind(id)
        .bind(at)
        .execute(&store.pool)
        .await
        .unwrap()
        .rows_affected();
        assert!(affected > 0, "nothing to backdate in {table}");
    }

    #[tokio::test]
    async fn candidates_come_back_most_recently_active_first() {
        // Activity is derived from two sources and neither is authoritative on its own, so
        // this pins that BOTH are consulted and that the more recent of them wins.
        let store = store().await;
        let mut ids = std::collections::HashMap::new();
        for username in ["sitzung-neu", "sitzung-alt", "protokoll", "nie-gesehen"] {
            let id = store
                .create_local_principal(username, username, None, "x")
                .await
                .unwrap()
                .id;
            ids.insert(username, id);
        }

        // Two accounts whose only evidence is a session, one recent and one ancient.
        for (username, at) in [
            ("sitzung-neu", "2026-08-01 09:00:00"),
            ("sitzung-alt", "2019-01-01 09:00:00"),
        ] {
            store
                .create_session(&ids[username], username, crate::SESSION_TTL_SECONDS)
                .await
                .unwrap();
            backdate(&store, "sessions", "created_at", &ids[username], at).await;
        }

        // One whose only evidence is something they did, dated between the two — so an
        // implementation that consulted sessions alone would sort it last, and one that
        // consulted the audit log alone would lose the other two.
        store
            .record_audit_entry(
                Some(&ids["protokoll"]),
                "acl.grant",
                Some("/raum"),
                Some("/raum"),
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        backdate(
            &store,
            "audit_log",
            "at",
            &ids["protokoll"],
            "2023-05-05 09:00:00",
        )
        .await;

        let names: Vec<String> = store
            .admin_candidates()
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.username)
            .collect();
        assert_eq!(
            names,
            vec!["sitzung-neu", "protokoll", "sitzung-alt", "nie-gesehen"],
            "candidates are not ordered by the most recent of session and audit activity"
        );
    }

    #[tokio::test]
    async fn the_more_recent_of_a_session_and_an_audit_entry_wins() {
        // The MAX across the two sources, rather than a preference for one of them.
        let store = store().await;
        let alt = store
            .create_local_principal("alt", "Alt", None, "x")
            .await
            .unwrap()
            .id;
        let beides = store
            .create_local_principal("beides", "Beides", None, "x")
            .await
            .unwrap()
            .id;

        // `beides` has an ancient session and a recent audit entry; `alt` has a session
        // in between. Taking the session alone would put `alt` first.
        store
            .create_session(&beides, "beides", crate::SESSION_TTL_SECONDS)
            .await
            .unwrap();
        backdate(
            &store,
            "sessions",
            "created_at",
            &beides,
            "2019-01-01 09:00:00",
        )
        .await;
        store
            .record_audit_entry(
                Some(&beides),
                "team.create",
                None,
                None,
                &serde_json::json!({}),
            )
            .await
            .unwrap();
        backdate(&store, "audit_log", "at", &beides, "2026-08-01 09:00:00").await;

        store
            .create_session(&alt, "alt", crate::SESSION_TTL_SECONDS)
            .await
            .unwrap();
        backdate(
            &store,
            "sessions",
            "created_at",
            &alt,
            "2022-01-01 09:00:00",
        )
        .await;

        let candidates = store.admin_candidates().await.unwrap();
        let names: Vec<&str> = candidates.iter().map(|c| c.username.as_str()).collect();
        assert_eq!(names, vec!["beides", "alt"]);
        assert_eq!(
            candidates[0].last_active_at.as_deref(),
            Some("2026-08-01 09:00:00")
        );
    }

    #[tokio::test]
    async fn candidates_exclude_administrators_and_deactivated_accounts() {
        // A candidate is somebody whose promotion would ADD an administrator. Offering the
        // ones who would add none is how a handover ends with nobody in charge.
        let store = store().await;
        let by_group = store
            .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
            .await
            .unwrap();
        let promoted = store
            .create_local_principal("befoerdert", "Befördert", None, "x")
            .await
            .unwrap();
        let gesperrt = store
            .create_local_principal("gesperrt", "Gesperrt", None, "x")
            .await
            .unwrap();
        let gast = store
            .create_local_principal("gast", "Gast", None, "x")
            .await
            .unwrap();
        let kollege = store
            .upsert_oidc_principal("kollege", "Kollege", None, &["users".into()])
            .await
            .unwrap();

        store.set_instance_admin(&promoted.id, true).await.unwrap();
        store
            .set_principal_active(&gesperrt.id, false)
            .await
            .unwrap();

        let candidates = store.admin_candidates().await.unwrap();
        let names: Vec<&str> = candidates.iter().map(|c| c.username.as_str()).collect();
        assert_eq!(names, vec!["gast", "kollege"], "{candidates:?}");
        assert!(!names.contains(&by_group.username.as_str()));

        // The console needs the source and the groups to tell two people apart.
        let kollege_row = candidates
            .iter()
            .find(|c| c.id == kollege.id)
            .expect("listed");
        assert_eq!(kollege_row.kind, gw_auth::PrincipalKind::Oidc);
        assert_eq!(kollege_row.groups, vec!["users".to_string()]);
        assert_eq!(kollege_row.display_name, "Kollege");
        let gast_row = candidates.iter().find(|c| c.id == gast.id).expect("listed");
        assert_eq!(gast_row.kind, gw_auth::PrincipalKind::Local);
        assert!(gast_row.groups.is_empty());
        assert_eq!(
            gast_row.last_active_at, None,
            "an account with no session and no audit entry has no derived activity"
        );
    }

    #[tokio::test]
    async fn deactivated_accounts_are_still_listed() {
        // Hiding them would make "cannot sign in" indistinguishable from "never existed",
        // and the second invites creating a duplicate account.
        let store = store().await;
        let p = store
            .create_local_principal("gesperrt", "Gesperrt", None, "x")
            .await
            .unwrap()
            .id;
        store.set_principal_active(&p, false).await.unwrap();

        let all = store.list_principals().await.unwrap();
        let found = all.iter().find(|x| x.id == p).expect("not listed");
        assert!(!found.active);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::{canonical_email, OidcSignIn};
    use crate::Store;
    use gw_auth::{Action, Permission, Subject};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    /// A sign-in that carries a verified address, as `gw_api::auth::oidc` would hand it
    /// over once the token has been checked.
    async fn verified_sign_in(
        store: &Store,
        username: &str,
        display_name: &str,
        email: &str,
        groups: &[&str],
    ) -> OidcSignIn {
        let groups: Vec<String> = groups.iter().map(|g| (*g).to_string()).collect();
        let key = canonical_email(email);
        store
            .upsert_oidc_identity(username, display_name, Some(email), key.as_deref(), &groups)
            .await
            .unwrap()
    }

    // ---------------------------------------------------------------------------------
    // The key itself.
    // ---------------------------------------------------------------------------------

    #[test]
    fn case_and_surrounding_space_are_one_address() {
        let expected = Some("oma@example.de".to_string());
        assert_eq!(canonical_email("oma@example.de"), expected);
        assert_eq!(canonical_email("Oma@Example.de"), expected);
        assert_eq!(canonical_email("oma@example.de "), expected);
        assert_eq!(canonical_email("\t OMA@EXAMPLE.DE\r\n"), expected);
    }

    #[test]
    fn an_address_this_code_cannot_match_has_no_key_at_all() {
        // Every one of these is stored as typed and simply never merges. Guessing at what
        // somebody meant is how two people become one.
        for raw in [
            "",
            "   ",
            "oma",                    // no domain
            "@example.de",            // no local part
            "oma@",                   // no domain
            "oma@@example.de",        // two separators
            "a@b@c.de",               // two separators
            "oma @example.de",        // an interior space
            "oma@exämple.de",         // IDN: never folded to punycode here
            "омa@example.de",         // a homograph local part
            "oma@example.de\u{200b}", // a zero-width space somebody pasted
        ] {
            assert_eq!(canonical_email(raw), None, "{raw:?} produced a merge key");
        }
    }

    #[test]
    fn a_homograph_domain_is_not_the_domain_it_imitates() {
        // The reason IDN is refused rather than punycoded: `exämple.de` and `example.de`
        // are different registrations, and folding them here would hand one person's
        // grants to whoever can register the other.
        assert_ne!(
            canonical_email("oma@exämple.de"),
            canonical_email("oma@example.de")
        );
    }

    // ---------------------------------------------------------------------------------
    // The merge.
    // ---------------------------------------------------------------------------------

    #[tokio::test]
    async fn a_verified_address_signs_in_as_the_account_that_was_invited() {
        let store = store().await;
        let oma = store
            .create_local_principal("oma", "Oma Erika", Some("Oma@Example.de"), "hash")
            .await
            .unwrap();
        store
            .add_grant(
                "/rezepte",
                Subject::Principal(oma.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        let outcome = verified_sign_in(
            &store,
            "erika.mueller",
            "Erika Müller",
            " oma@example.de",
            &["users"],
        )
        .await;

        let OidcSignIn::Merged { principal, .. } = outcome else {
            panic!("did not merge: {outcome:?}");
        };
        assert_eq!(principal.id, oma.id, "a second principal was made");
        assert_eq!(
            principal.username, "oma",
            "the handle she chose was overwritten"
        );
        assert_eq!(
            principal.display_name, "Oma Erika",
            "the name she typed was overwritten by Authelia's"
        );
        assert_eq!(principal.groups, vec!["users".to_string()]);
        assert_eq!(store.list_principals().await.unwrap().len(), 1);

        // Signing in a second time is recognition, not a second merge.
        let again = verified_sign_in(
            &store,
            "erika.mueller",
            "Erika Müller",
            "oma@example.de",
            &["users"],
        )
        .await;
        assert!(
            matches!(&again, OidcSignIn::Recognised(p) if p.id == oma.id),
            "{again:?}"
        );
        assert_eq!(store.list_principals().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn without_a_verified_address_nothing_merges() {
        // The merge key is the email Authelia asserts as verified. `None` here is what an
        // unverified or absent claim produces, and it must land on the old behaviour: a
        // separate principal with no access.
        let store = store().await;
        let oma = store
            .create_local_principal("oma", "Oma Erika", Some("oma@example.de"), "hash")
            .await
            .unwrap();

        let outcome = store
            .upsert_oidc_identity(
                "erika.mueller",
                "Erika Müller",
                Some("oma@example.de"),
                None,
                &["users".to_string()],
            )
            .await
            .unwrap();

        let OidcSignIn::Created(made) = outcome else {
            panic!("an unverified address was merged: {outcome:?}");
        };
        assert_ne!(made.id, oma.id);
        assert_eq!(store.list_principals().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn merging_reaches_exactly_the_address_it_matched_and_no_other() {
        // The disclosure test. An attacker who controls an Authelia account with a
        // VERIFIED address must not reach a different address's grants.
        let store = store().await;
        let owner = store
            .create_local_principal("sergej", "Sergej", Some("sergej@example.de"), "hash")
            .await
            .unwrap();
        store
            .add_grant(
                "/privat",
                Subject::Principal(owner.id.clone()),
                Permission::Admin,
            )
            .await
            .unwrap();

        let outcome = verified_sign_in(
            &store,
            "angreifer",
            "Angreifer",
            "angreifer@example.de",
            &["users"],
        )
        .await;
        let OidcSignIn::Created(made) = outcome else {
            panic!("an unrelated address merged: {outcome:?}");
        };
        assert_ne!(made.id, owner.id);

        let attacker = store.principal_by_id(&made.id).await.unwrap().unwrap().0;
        assert!(
            store
                .document_access(&attacker, "/privat", Action::Read)
                .await
                .unwrap()
                .is_none(),
            "the attacker reached the owner's page"
        );
    }

    #[tokio::test]
    async fn an_account_already_merged_cannot_be_merged_into_again() {
        let store = store().await;
        store
            .create_local_principal("oma", "Oma Erika", Some("oma@example.de"), "hash")
            .await
            .unwrap();
        let first = verified_sign_in(&store, "erika", "Erika", "oma@example.de", &["users"]).await;
        assert!(matches!(first, OidcSignIn::Merged { .. }), "{first:?}");

        // A second Authelia account asserting the same verified address. One principal
        // already answers to it, so this one gets a principal of its own instead.
        let second =
            verified_sign_in(&store, "zweite", "Zweite", "oma@example.de", &["admins"]).await;
        assert!(matches!(second, OidcSignIn::Created(_)), "{second:?}");
        assert_eq!(store.list_principals().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn an_authelia_handle_that_is_somebody_else_s_username_is_refused() {
        // Before the merge existed this was an UPSERT on `username`: an Authelia account
        // called `oma` silently took over the local `oma`, with no address checked at all.
        let store = store().await;
        store
            .create_local_principal("oma", "Oma Erika", Some("oma@example.de"), "hash")
            .await
            .unwrap();

        let outcome = store
            .upsert_oidc_identity("oma", "Wer Auch Immer", Some("fremd@example.de"), None, &[])
            .await
            .unwrap();
        assert!(
            matches!(outcome, OidcSignIn::UsernameHeldByAnother),
            "{outcome:?}"
        );

        let oma = store.principal_by_username("oma").await.unwrap().unwrap().0;
        assert_eq!(oma.display_name, "Oma Erika");
        assert_eq!(oma.email.as_deref(), Some("oma@example.de"));
    }

    #[tokio::test]
    async fn two_accounts_carrying_one_address_refuse_to_merge() {
        // Fail closed: "which of these two is this person?" has no safe guess.
        let store = store().await;
        store
            .create_local_principal("eins", "Eins", Some("doppelt@example.de"), "hash")
            .await
            .unwrap();
        store
            .create_local_principal("zwei", "Zwei", Some("Doppelt@example.DE"), "hash")
            .await
            .unwrap();

        let outcome = verified_sign_in(&store, "wer", "Wer", "doppelt@example.de", &[]).await;
        assert!(matches!(outcome, OidcSignIn::AmbiguousEmail), "{outcome:?}");
        assert_eq!(store.list_principals().await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn a_deactivated_account_is_not_merged_back_into_life() {
        let store = store().await;
        let gone = store
            .create_local_principal("weg", "Weg", Some("weg@example.de"), "hash")
            .await
            .unwrap();
        store.set_principal_active(&gone.id, false).await.unwrap();

        let outcome = verified_sign_in(&store, "weg.extern", "Weg", "weg@example.de", &[]).await;
        let OidcSignIn::Created(made) = outcome else {
            panic!("a deactivated account was reanimated: {outcome:?}");
        };
        assert_ne!(made.id, gone.id);
    }

    #[tokio::test]
    async fn the_merge_is_recorded_naming_both_halves() {
        let store = store().await;
        store
            .create_local_principal("oma", "Oma Erika", Some("oma@example.de"), "hash")
            .await
            .unwrap();
        verified_sign_in(
            &store,
            "erika.mueller",
            "Erika",
            "oma@example.de",
            &["users"],
        )
        .await;

        let admin = store
            .upsert_oidc_principal("chef", "Chef", None, &["admins".into()])
            .await
            .unwrap();
        let page = store.audit_for(&admin, 50).await.unwrap();
        let merge = page
            .entries
            .iter()
            .find(|e| e.action == "identity.merge")
            .expect("no identity.merge entry");
        let detail: serde_json::Value = serde_json::from_str(&merge.detail).unwrap();
        assert_eq!(detail["username"], serde_json::json!("oma"));
        assert_eq!(detail["oidc_username"], serde_json::json!("erika.mueller"));
        assert_eq!(detail["email"], serde_json::json!("oma@example.de"));
    }

    #[tokio::test]
    async fn deactivating_a_merged_account_ends_both_ways_in_at_once() {
        // One principal, so D-M2-7 covers both credentials. Before the merge these were
        // two rows and deactivating one left the other signing in.
        let store = store().await;
        let oma = store
            .create_local_principal("oma", "Oma Erika", Some("oma@example.de"), "hash")
            .await
            .unwrap();
        verified_sign_in(
            &store,
            "erika.mueller",
            "Erika",
            "oma@example.de",
            &["users"],
        )
        .await;

        store
            .create_session(&oma.id, "hash-lokal", 3600)
            .await
            .unwrap();
        store
            .create_session(&oma.id, "hash-authelia", 3600)
            .await
            .unwrap();
        assert_eq!(store.session_count_for(&oma.id).await.unwrap(), 2);

        store.set_principal_active(&oma.id, false).await.unwrap();
        assert_eq!(store.session_count_for(&oma.id).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn signing_out_one_way_leaves_the_other_way_signed_in() {
        // Decided and pinned: "abmelden" ends the session presented, not every session the
        // principal holds. Ending all of them is deactivation, which is a different act.
        let store = store().await;
        let oma = store
            .create_local_principal("oma", "Oma Erika", Some("oma@example.de"), "hash")
            .await
            .unwrap();
        verified_sign_in(
            &store,
            "erika.mueller",
            "Erika",
            "oma@example.de",
            &["users"],
        )
        .await;
        store
            .create_session(&oma.id, "hash-lokal", 3600)
            .await
            .unwrap();
        store
            .create_session(&oma.id, "hash-authelia", 3600)
            .await
            .unwrap();

        assert!(store.delete_session("hash-lokal").await.unwrap());
        assert!(store
            .principal_for_session("hash-authelia")
            .await
            .unwrap()
            .is_some());
        assert!(store
            .principal_for_session("hash-lokal")
            .await
            .unwrap()
            .is_none());
    }

    // ---------------------------------------------------------------------------------
    // What the owner is shown before anything is merged retroactively.
    // ---------------------------------------------------------------------------------

    #[tokio::test]
    async fn the_candidate_listing_pairs_accounts_and_merges_nothing() {
        let store = store().await;
        let oma = store
            .create_local_principal("oma", "Oma Erika", Some("Oma@Example.de"), "hash")
            .await
            .unwrap();
        let mirrored = store
            .upsert_oidc_principal(
                "erika.mueller",
                "Erika Müller",
                Some("oma@example.de "),
                &[],
            )
            .await
            .unwrap();
        // Nobody to pair with.
        store
            .create_local_principal("allein", "Allein", Some("allein@example.de"), "hash")
            .await
            .unwrap();

        let candidates = store.merge_candidates().await.unwrap();
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(candidates[0].email, "oma@example.de");
        let ids: Vec<&str> = candidates[0]
            .accounts
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert!(ids.contains(&oma.id.as_str()) && ids.contains(&mirrored.id.as_str()));

        // Read-only: both principals are still there, unchanged.
        assert_eq!(store.list_principals().await.unwrap().len(), 3);
    }
}
