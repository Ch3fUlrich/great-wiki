//! Invites: the link that creates an account, and the one transaction that redeems it.
//!
//! This module never sees a token. It is handed the token's SHA-256 and stores that, and
//! every lookup hashes the presented token the same way — the same arrangement
//! [`crate::sessions`] uses, and for the same reason: there is no method here that could
//! accidentally persist a working credential. Hashing itself belongs to the caller, next
//! to the code that generates the token; see `gw_api::auth::session::hash_token`.
//!
//! # The two things that must not go wrong
//!
//! **Single use has to be atomic.** [`Store::accept_invite_audited`] consumes the invite
//! with an UPDATE that carries its own precondition, inside the transaction that creates
//! the account. A SELECT followed by an UPDATE would leave a window in which two accepts
//! both see an unspent invite; the second would then fail on the username's UNIQUE
//! constraint if it failed at all — an error rather than a refusal, and only by luck.
//!
//! **Acceptance is all or nothing.** The account, the grant, the team membership, the
//! session and the audit rows are one transaction. A half-acceptance is the worst outcome
//! an invite can have: an account that can sign in and see nothing, or a grant naming a
//! principal that does not exist. Either everything committed, or the link is still live.

use crate::acl::{permission_column, subject_columns};
use crate::principals::insert_local_principal;
use crate::{Baseline, Store};
use anyhow::Result;
use gw_auth::{can, Action, Permission, Principal, Subject};
use gw_core::Visibility;
use serde::Serialize;
use serde_json::json;
use sqlx::FromRow;
use std::collections::HashMap;

/// How long an invite stays usable (D-M2-21).
///
/// Long enough that a homelab invitation does not expire unnoticed and read as a broken
/// system; short enough that a link in an old message does not stay live for ever.
/// Single-use regardless, so the window only matters until acceptance.
pub const INVITE_TTL_SECONDS: i64 = 60 * 60 * 24 * 30;

/// Where an invite is in its life.
///
/// Derived on every read rather than stored, so it cannot disagree with the timestamps
/// beside it. `Expired` in particular is a fact about the clock, not about anything having
/// run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum InviteState {
    /// Live: it can still be accepted.
    Pending,
    /// Somebody redeemed it. Terminal.
    Accepted,
    /// Withdrawn by an administrator. Terminal.
    Revoked,
    /// It ran out of time. Terminal.
    Expired,
}

/// One invite, as the console sees it.
///
/// There is no token field and there cannot be one: the plaintext is never stored, and the
/// digest is deliberately absent too. A digest in a listing is as good as the token to
/// anybody who can also write to this database, and nothing in the console needs it.
#[derive(Debug, Clone, Serialize)]
pub struct InviteSummary {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    /// The inviter's principal id, or `None` if the row never recorded one.
    pub invited_by: Option<String>,
    /// Their display name, resolved now. `None` when the account is gone — the invite
    /// outlives whoever made it, and saying "somebody" is better than inventing a name.
    pub invited_by_name: Option<String>,
    pub path: Option<String>,
    pub permission: Option<Permission>,
    /// The team's slug, not its id: the slug is what every other API here speaks.
    pub team: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    pub state: InviteState,
    /// The account it produced, once it has produced one.
    pub accepted_principal_id: Option<String>,
}

/// What an invite offers the person holding the link.
///
/// Only ever built for an invite that is actually redeemable, so the page cannot describe
/// something that would then be refused. `token_hash` is carried so the caller can confirm
/// the match in constant time — see `gw_api::auth::invite`.
#[derive(Debug, Clone)]
pub struct InviteOffer {
    pub token_hash: String,
    pub username: String,
    /// Who invited them, for the page to name. `None` when the account is gone.
    pub invited_by_name: Option<String>,
    pub path: Option<String>,
    pub permission: Option<Permission>,
    /// The team's human name, which is what the page shows.
    pub team_name: Option<String>,
    pub expires_at: String,
}

/// An invite to be written. Borrowed, because every field comes straight off a request.
pub struct NewInvite<'a> {
    pub username: &'a str,
    pub email: Option<&'a str>,
    pub path: Option<&'a str>,
    pub permission: Option<Permission>,
    /// The team's slug. Resolved to an id here, so a slug naming no team is a typo the
    /// caller hears about rather than an invite that grants nothing.
    pub team: Option<&'a str>,
    pub ttl_seconds: i64,
}

/// What creating an invite actually did.
#[derive(Debug, Clone)]
pub enum CreateInviteOutcome {
    Created(Box<InviteSummary>),
    /// The username is already an account. Checked here as well as in the API so that a
    /// second caller racing the first gets an answer rather than a UNIQUE violation.
    UsernameTaken,
    /// The slug names no team.
    NoSuchTeam,
    /// Neither a path grant nor a team (D-M2-20). Refused rather than stored: see the
    /// 0007 migration for why.
    NothingGranted,
}

/// What accepting an invite actually did.
#[derive(Debug, Clone)]
pub enum AcceptOutcome {
    /// The account exists, holds what the invite carried, and has a live session.
    Accepted(Box<Principal>),
    /// Unknown, expired, revoked or already spent. ONE variant on purpose: the caller must
    /// not be able to tell those four apart, or the endpoint reports which tokens exist.
    Gone,
    /// Somebody took the username between the invite being written and it being redeemed.
    /// Not a token state, so it is allowed to read differently — only the holder of a
    /// valid token can ever see it.
    UsernameTaken,
}

/// What revoking an invite actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeInviteOutcome {
    Revoked,
    /// The id names no invite.
    NoSuchInvite,
    /// It was already spent, revoked or expired. Reported rather than treated as success,
    /// for the reason every removal in [`crate::admin`] is: an administrator reading "done"
    /// would conclude that a live link has been withdrawn.
    NotPending,
}

/// Read a stored permission. An unrecognised value confers NOTHING and, because a path and
/// a permission come as a pair, makes the whole invite unreadable rather than silently
/// dropping the grant half of it.
fn permission_from(stored: &str) -> Option<Permission> {
    match stored {
        "read" => Some(Permission::Read),
        "comment" => Some(Permission::Comment),
        "write" => Some(Permission::Write),
        "admin" => Some(Permission::Admin),
        _ => None,
    }
}

/// Every column the summary needs, plus the three timestamps the state is derived from.
#[derive(FromRow)]
struct InviteRow {
    id: String,
    username: String,
    email: Option<String>,
    invited_by: Option<String>,
    invited_by_name: Option<String>,
    path: Option<String>,
    permission: Option<String>,
    team: Option<String>,
    created_at: String,
    expires_at: String,
    revoked_at: Option<String>,
    accepted_at: Option<String>,
    accepted_principal_id: Option<String>,
    /// 1 when `expires_at` has already passed. Computed by SQLite against the same clock
    /// the consuming UPDATE uses, rather than compared here against a different one.
    expired: i64,
}

/// Only what redeeming an invite needs, read inside the transaction that redeems it.
///
/// Deliberately NOT [`InviteRow`]: this one carries `team_id` rather than a joined slug,
/// because the membership is written by id, and it carries no derived state at all —
/// nothing here decides whether the invite is live, since the consuming UPDATE below does
/// that atomically and a value read a statement earlier could only disagree with it.
#[derive(FromRow)]
struct RedeemableRow {
    id: String,
    invited_by: Option<String>,
    username: String,
    email: Option<String>,
    path: Option<String>,
    permission: Option<String>,
    team_id: Option<String>,
}

/// The one SELECT list, so a listing and a single read cannot drift into disagreeing about
/// what an invite is.
const INVITE_COLUMNS: &str = "i.id, i.username, i.email, i.invited_by, \
     (SELECT p.display_name FROM principals p WHERE p.id = i.invited_by) AS invited_by_name, \
     i.path, i.permission, \
     (SELECT t.slug FROM teams t WHERE t.id = i.team_id) AS team, \
     i.created_at, i.expires_at, i.revoked_at, i.accepted_at, i.accepted_principal_id, \
     (i.expires_at <= datetime('now')) AS expired";

impl InviteRow {
    fn state(&self) -> InviteState {
        // Accepted first: an invite redeemed last month is still `accepted` today, however
        // long ago its window closed. Revoked next, for the same reason.
        if self.accepted_at.is_some() {
            InviteState::Accepted
        } else if self.revoked_at.is_some() {
            InviteState::Revoked
        } else if self.expired != 0 {
            InviteState::Expired
        } else {
            InviteState::Pending
        }
    }

    fn into_summary(self) -> InviteSummary {
        let state = self.state();
        InviteSummary {
            id: self.id,
            username: self.username,
            email: self.email,
            invited_by: self.invited_by,
            invited_by_name: self.invited_by_name,
            permission: self.permission.as_deref().and_then(permission_from),
            path: self.path,
            team: self.team,
            created_at: self.created_at,
            expires_at: self.expires_at,
            state,
            accepted_principal_id: self.accepted_principal_id,
        }
    }
}

impl Store {
    /// Write an invite, recording it in the same transaction (D-M2-4).
    ///
    /// `token_hash` is the SHA-256 of the link the caller will show once. Passing the token
    /// itself would store a working credential, which is the one thing this table exists to
    /// avoid, and there is no overload that accepts one.
    ///
    /// **Nothing about the token reaches the audit row.** What is recorded is who invited
    /// whom, into what, and until when — everything an administrator needs months later,
    /// and nothing that would let a reader of the log redeem the invitation.
    ///
    /// The audit entry is scoped to the invite's path when it has one, so the space admin
    /// who will live with the consequence can see it (D-M2-6). A team-carrying invite has
    /// no subtree, so it is instance-wide — which is consistent with only instance admins
    /// being able to create one at all.
    pub async fn create_invite_audited(
        &self,
        actor: &str,
        token_hash: &str,
        invite: &NewInvite<'_>,
    ) -> Result<CreateInviteOutcome> {
        // D-M2-20, checked before anything is written. The CHECK constraint in 0007 says
        // the same thing and is what actually guarantees it; this is here so the answer is
        // a typed outcome rather than a constraint violation surfacing as a 500.
        if invite.path.is_none() && invite.team.is_none() {
            return Ok(CreateInviteOutcome::NothingGranted);
        }

        let mut tx = self.pool.begin().await?;

        let taken: Option<(String,)> =
            sqlx::query_as("SELECT id FROM principals WHERE username = ?1")
                .bind(invite.username)
                .fetch_optional(&mut *tx)
                .await?;
        if taken.is_some() {
            tx.rollback().await?;
            return Ok(CreateInviteOutcome::UsernameTaken);
        }

        // Looked up rather than left to the foreign key: a slug naming no team is a typo in
        // the team name, and the constraint cannot be told apart from any other failure.
        let team_id = match invite.team {
            Some(slug) => {
                let row: Option<(String,)> = sqlx::query_as("SELECT id FROM teams WHERE slug = ?1")
                    .bind(slug)
                    .fetch_optional(&mut *tx)
                    .await?;
                match row {
                    Some((id,)) => Some(id),
                    None => {
                        tx.rollback().await?;
                        return Ok(CreateInviteOutcome::NoSuchTeam);
                    }
                }
            }
            None => None,
        };

        let id = uuid::Uuid::now_v7().to_string();
        let permission = invite.permission.map(permission_column);
        sqlx::query(
            "INSERT INTO invites \
             (id, token_hash, invited_by, username, email, path, permission, team_id, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, datetime('now', ?9))",
        )
        .bind(&id)
        .bind(token_hash)
        .bind(actor)
        .bind(invite.username)
        .bind(invite.email)
        .bind(invite.path)
        .bind(permission)
        .bind(&team_id)
        .bind(format!("{} seconds", invite.ttl_seconds))
        .execute(&mut *tx)
        .await?;

        Self::record_audit(
            &mut *tx,
            Some(actor),
            "invite.create",
            Some(&id),
            invite.path,
            &json!({
                "username": invite.username,
                "path": invite.path,
                "permission": permission,
                "team": invite.team,
            }),
        )
        .await?;
        tx.commit().await?;

        match self.invite_by_id(&id).await? {
            Some(summary) => Ok(CreateInviteOutcome::Created(Box::new(summary))),
            None => Err(anyhow::anyhow!("invite vanished immediately after insert")),
        }
    }

    /// One invite by id, whatever state it is in. Used to decide who may revoke it.
    pub async fn invite_by_id(&self, id: &str) -> Result<Option<InviteSummary>> {
        let row: Option<InviteRow> = sqlx::query_as(&format!(
            "SELECT {INVITE_COLUMNS} FROM invites i WHERE i.id = ?1"
        ))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(InviteRow::into_summary))
    }

    /// What the link offers, or `None` if it offers nothing.
    ///
    /// `None` covers unknown, expired, revoked and already-spent without distinction —
    /// deliberately, because the caller renders one answer for all four and a second return
    /// value here would be an invitation to render four.
    ///
    /// A row whose stored permission this code does not recognise also answers `None`. It
    /// cannot happen through the CHECK constraint; if it ever does, refusing the whole
    /// invite is the closed choice, where dropping the grant half of it would silently
    /// create an account with less access than the page promised.
    pub async fn invite_offer(&self, token_hash: &str) -> Result<Option<InviteOffer>> {
        let row: Option<InviteRow> = sqlx::query_as(&format!(
            "SELECT {INVITE_COLUMNS} FROM invites i WHERE i.token_hash = ?1",
        ))
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else { return Ok(None) };
        if row.state() != InviteState::Pending {
            return Ok(None);
        }

        let permission = match &row.permission {
            Some(stored) => match permission_from(stored) {
                Some(permission) => Some(permission),
                None => return Ok(None),
            },
            None => None,
        };

        let team_name: Option<String> = match &row.team {
            Some(slug) => sqlx::query_as::<_, (String,)>("SELECT name FROM teams WHERE slug = ?1")
                .bind(slug)
                .fetch_optional(&self.pool)
                .await?
                .map(|(name,)| name),
            None => None,
        };

        Ok(Some(InviteOffer {
            token_hash: token_hash.to_string(),
            username: row.username,
            invited_by_name: row.invited_by_name,
            path: row.path,
            permission,
            team_name,
            expires_at: row.expires_at,
        }))
    }

    /// Redeem an invite: create the account, apply what the invite carried, sign them in,
    /// and record all of it. One transaction.
    ///
    /// `password_hash` is already hashed and has already been through the full policy —
    /// this crate never sees a plaintext password, and hashing without the policy is how a
    /// breached password gets in. `session_token_hash` is the SHA-256 of the cookie the
    /// browser will hold, exactly as [`Store::create_session`] expects.
    ///
    /// The session is created **inside** this transaction rather than after it. The
    /// alternative leaves a window in which an account exists that nobody has been signed
    /// in to, which is the "made an account and lost the person" failure in miniature.
    pub async fn accept_invite_audited(
        &self,
        token_hash: &str,
        display_name: &str,
        password_hash: &str,
        session_token_hash: &str,
        session_ttl_seconds: i64,
    ) -> Result<AcceptOutcome> {
        let mut tx = self.pool.begin().await?;

        let row: Option<RedeemableRow> = sqlx::query_as(
            "SELECT id, invited_by, username, email, path, permission, team_id \
             FROM invites WHERE token_hash = ?1",
        )
        .bind(token_hash)
        .fetch_optional(&mut *tx)
        .await?;

        let Some(RedeemableRow {
            id,
            invited_by,
            username,
            email,
            path,
            permission,
            team_id,
        }) = row
        else {
            tx.rollback().await?;
            return Ok(AcceptOutcome::Gone);
        };

        // CONSUME FIRST, and in one statement carrying its own precondition. This is what
        // makes the invite single-use against two simultaneous accepts: the second
        // transaction cannot start its own write until this one has committed or rolled
        // back, and it then matches zero rows. Reading the state and updating afterwards
        // would leave both accepts believing the invite was unspent.
        let consumed = sqlx::query(
            "UPDATE invites SET accepted_at = datetime('now') \
             WHERE id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL \
             AND expires_at > datetime('now')",
        )
        .bind(&id)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !consumed {
            tx.rollback().await?;
            return Ok(AcceptOutcome::Gone);
        }

        // A permission this code does not recognise refuses the whole invite rather than
        // creating an account with less access than the page promised.
        let permission = match &permission {
            Some(stored) => match permission_from(stored) {
                Some(permission) => Some(permission),
                None => {
                    tx.rollback().await?;
                    return Ok(AcceptOutcome::Gone);
                }
            },
            None => None,
        };

        // Checked rather than left to the UNIQUE constraint, so that "somebody took this
        // name while the invitation was outstanding" is an answer the page can give rather
        // than a 500. The constraint is still what actually guarantees it.
        let taken: Option<(String,)> =
            sqlx::query_as("SELECT id FROM principals WHERE username = ?1")
                .bind(&username)
                .fetch_optional(&mut *tx)
                .await?;
        if taken.is_some() {
            tx.rollback().await?;
            return Ok(AcceptOutcome::UsernameTaken);
        }

        let principal_id = insert_local_principal(
            &mut tx,
            &username,
            display_name,
            email.as_deref(),
            password_hash,
        )
        .await?;
        sqlx::query("UPDATE invites SET accepted_principal_id = ?2 WHERE id = ?1")
            .bind(&id)
            .bind(&principal_id)
            .execute(&mut *tx)
            .await?;

        // The direct grant, written exactly as `POST /api/admin/acl` writes one. Recorded
        // as `acl.grant` and scoped to the path for the same reason: whoever administers
        // that subtree needs to see who gained access to it, and an entry with a different
        // verb would not appear where they look.
        if let (Some(path), Some(permission)) = (&path, permission) {
            let subject = Subject::Principal(principal_id.clone());
            let (kind, subject_id) = subject_columns(&subject);
            let perm = permission_column(permission);
            sqlx::query(
                "INSERT INTO acl (id, path, subject_kind, subject_id, permission) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(path)
            .bind(kind)
            .bind(&subject_id)
            .bind(perm)
            .execute(&mut *tx)
            .await?;

            Self::record_audit(
                &mut *tx,
                // The inviter, because they are who decided it. The person accepting chose
                // a password, not a permission.
                invited_by.as_deref(),
                "acl.grant",
                Some(path),
                Some(path),
                &json!({ "subject_kind": kind, "subject_id": subject_id, "permission": perm,
                         "via_invite": id }),
            )
            .await?;
        }

        if let Some(team_id) = &team_id {
            sqlx::query(
                "INSERT OR IGNORE INTO team_members (team_id, principal_id) VALUES (?1, ?2)",
            )
            .bind(team_id)
            .bind(&principal_id)
            .execute(&mut *tx)
            .await?;

            let slug: Option<(String,)> = sqlx::query_as("SELECT slug FROM teams WHERE id = ?1")
                .bind(team_id)
                .fetch_optional(&mut *tx)
                .await?;
            Self::record_audit(
                &mut *tx,
                invited_by.as_deref(),
                "team.member.add",
                slug.as_ref().map(|(slug,)| slug.as_str()),
                None,
                &json!({ "principal_id": principal_id, "via_invite": id }),
            )
            .await?;
        }

        // The same table, the same digest and the same TTL the sign-in form uses; see
        // `crate::sessions`. Written here rather than through `create_session` only because
        // that one takes the pool, and this has to be part of the transaction above.
        sqlx::query(
            "INSERT INTO sessions (token_hash, principal_id, expires_at) \
             VALUES (?1, ?2, datetime('now', ?3))",
        )
        .bind(session_token_hash)
        .bind(&principal_id)
        .bind(format!("{session_ttl_seconds} seconds"))
        .execute(&mut *tx)
        .await?;

        // Instance-wide, exactly as `create_local_principal_audited` records it: an account
        // belongs to no subtree. `via` is what tells an auditor months later that nobody
        // typed this account in by hand.
        Self::record_audit(
            &mut *tx,
            invited_by.as_deref(),
            "principal.create",
            Some(&principal_id),
            None,
            &json!({ "username": username, "kind": "local", "via": "invite", "invite": id }),
        )
        .await?;

        // And the acceptance itself, by the person who did it, where the space admin looks.
        Self::record_audit(
            &mut *tx,
            Some(&principal_id),
            "invite.accept",
            Some(&id),
            path.as_deref(),
            &json!({ "username": username, "invited_by": invited_by }),
        )
        .await?;

        tx.commit().await?;

        self.principal_by_id(&principal_id)
            .await?
            .map(|(principal, _)| AcceptOutcome::Accepted(Box::new(principal)))
            .ok_or_else(|| anyhow::anyhow!("principal vanished immediately after insert"))
    }

    /// Withdraw an invite that has not been redeemed.
    ///
    /// Only a pending one can be revoked. Reporting success for an invite that was already
    /// spent would tell an administrator they had closed a door that is not merely open but
    /// already walked through — and the account it created is a separate thing to deal with.
    pub async fn revoke_invite_audited(
        &self,
        actor: &str,
        id: &str,
    ) -> Result<RevokeInviteOutcome> {
        let mut tx = self.pool.begin().await?;

        let row: Option<(Option<String>,)> =
            sqlx::query_as("SELECT path FROM invites WHERE id = ?1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some((path,)) = row else {
            tx.rollback().await?;
            return Ok(RevokeInviteOutcome::NoSuchInvite);
        };

        let revoked = sqlx::query(
            "UPDATE invites SET revoked_at = datetime('now'), revoked_by = ?2 \
             WHERE id = ?1 AND accepted_at IS NULL AND revoked_at IS NULL \
             AND expires_at > datetime('now')",
        )
        .bind(id)
        .bind(actor)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            > 0;
        if !revoked {
            tx.rollback().await?;
            return Ok(RevokeInviteOutcome::NotPending);
        }

        Self::record_audit(
            &mut *tx,
            Some(actor),
            "invite.revoke",
            Some(id),
            path.as_deref(),
            &json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(RevokeInviteOutcome::Revoked)
    }

    /// The invites `principal` is entitled to see, newest first.
    ///
    /// A retrieval path, so the filtering happens **here** and not in a caller. An instance
    /// admin sees everything; anybody else sees only invites carrying a path they hold
    /// `admin` on. An invite with no path is instance-wide — which is consistent, because
    /// only an instance admin can create one: a team's reach is not bounded by any subtree.
    ///
    /// Permission is evaluated per distinct path rather than by prefix, for the reason
    /// [`Store::audit_for`] gives: grants do not union up the tree, so holding `admin` at
    /// `/a` says nothing about `/a/b` once `/a/b` carries its own.
    pub async fn invites_for(&self, principal: &Principal) -> Result<Vec<InviteSummary>> {
        // Refused before any row is read. `can()` would reach the same answer, but not
        // reading is a stronger guarantee than not returning.
        if !principal.is_authenticated() || !principal.active {
            return Ok(Vec::new());
        }

        let rows: Vec<InviteRow> = sqlx::query_as(&format!(
            "SELECT {INVITE_COLUMNS} FROM invites i ORDER BY i.created_at DESC, i.id DESC",
        ))
        .fetch_all(&self.pool)
        .await?;
        let all: Vec<InviteSummary> = rows.into_iter().map(InviteRow::into_summary).collect();

        if self.baseline_for(principal).await? >= Baseline::Admin {
            return Ok(all);
        }

        let mut allowed: HashMap<String, bool> = HashMap::new();
        let mut out = Vec::new();
        for invite in all {
            let Some(path) = invite.path.clone() else {
                continue;
            };
            let permitted = match allowed.get(&path) {
                Some(known) => *known,
                None => {
                    let grants = self.grants_for_path(&path).await?;
                    // `Restricted` is how this crate asks "is there a grant for this
                    // caller?" without keeping a second copy of the subject-matching
                    // rules. A baseline must not stand in for an admin grant: reading who
                    // has been invited into a space is not something `internal` confers.
                    let verdict = can(principal, Action::Admin, Visibility::Restricted, &grants);
                    allowed.insert(path, verdict);
                    verdict
                }
            };
            if permitted {
                out.push(invite);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AcceptOutcome, CreateInviteOutcome, InviteState, NewInvite, RevokeInviteOutcome,
        INVITE_TTL_SECONDS,
    };
    use crate::Store;
    use gw_auth::{Permission, Principal, Subject};

    async fn store() -> Store {
        let store = Store::open("sqlite::memory:").await.unwrap();
        store.create_team("redaktion", "Redaktion").await.unwrap();
        store
    }

    fn to_raum(username: &str) -> NewInvite<'_> {
        NewInvite {
            username,
            email: None,
            path: Some("/raum"),
            permission: Some(Permission::Read),
            team: None,
            ttl_seconds: INVITE_TTL_SECONDS,
        }
    }

    async fn created(store: &Store, hash: &str, invite: NewInvite<'_>) -> super::InviteSummary {
        match store
            .create_invite_audited("chef", hash, &invite)
            .await
            .unwrap()
        {
            CreateInviteOutcome::Created(summary) => *summary,
            other => panic!("expected the invite to be written, got {other:?}"),
        }
    }

    /// The audit log as an instance admin sees it: everything.
    async fn actions(store: &Store) -> Vec<String> {
        let admin = Principal::test("chef", &["admins"], &[]);
        store
            .audit_for(&admin, 100)
            .await
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.action)
            .collect()
    }

    #[tokio::test]
    async fn the_stored_value_is_the_digest_it_was_given_and_never_a_token() {
        // The same property `only_the_digest_is_stored_never_the_token` pins for sessions.
        // This module is expressed entirely in digests, so there is no method that COULD
        // persist a plaintext — and this is what keeps that true.
        let store = store().await;
        created(&store, "digest-of-geheim", to_raum("gast")).await;

        let (stored,): (String,) = sqlx::query_as("SELECT token_hash FROM invites")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(stored, "digest-of-geheim");
        assert_ne!(stored, "geheim");
    }

    #[tokio::test]
    async fn an_invite_that_carries_nothing_is_refused_and_writes_no_row() {
        // D-M2-20. The CHECK constraint in 0007 says the same thing; this is the typed
        // answer, so the API can report it as a bad request rather than a 500.
        let store = store().await;
        let outcome = store
            .create_invite_audited(
                "chef",
                "digest",
                &NewInvite {
                    username: "niemand",
                    email: None,
                    path: None,
                    permission: None,
                    team: None,
                    ttl_seconds: INVITE_TTL_SECONDS,
                },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CreateInviteOutcome::NothingGranted));

        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM invites")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(count, 0);
        assert!(actions(&store).await.is_empty());
    }

    #[tokio::test]
    async fn a_slug_naming_no_team_is_refused_rather_than_granting_nothing() {
        let store = store().await;
        let outcome = store
            .create_invite_audited(
                "chef",
                "digest",
                &NewInvite {
                    username: "gast",
                    email: None,
                    path: None,
                    permission: None,
                    team: Some("tippfehler"),
                    ttl_seconds: INVITE_TTL_SECONDS,
                },
            )
            .await
            .unwrap();
        assert!(matches!(outcome, CreateInviteOutcome::NoSuchTeam));
        assert!(actions(&store).await.is_empty());
    }

    #[tokio::test]
    async fn an_expired_invite_offers_nothing_although_the_row_is_still_there() {
        // Expiry is a fact about the clock, enforced on the read, exactly as a session's
        // is. Nothing has to have swept anything.
        let store = store().await;
        let summary = created(
            &store,
            "digest",
            NewInvite {
                ttl_seconds: -60,
                ..to_raum("gast")
            },
        )
        .await;
        assert_eq!(summary.state, InviteState::Expired);
        assert!(store.invite_offer("digest").await.unwrap().is_none());
        assert!(store.invite_by_id(&summary.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_permission_this_code_does_not_recognise_refuses_the_whole_invite() {
        // A row written by a future version, or by hand. Dropping the grant half of it
        // would silently create an account with less access than the page promised, so the
        // closed answer is to refuse the invitation altogether.
        let store = store().await;
        let summary = created(&store, "digest", to_raum("gast")).await;
        for statement in [
            "PRAGMA ignore_check_constraints = ON",
            "UPDATE invites SET permission = 'zukunft'",
            "PRAGMA ignore_check_constraints = OFF",
        ] {
            sqlx::query(statement).execute(&store.pool).await.unwrap();
        }

        assert!(store.invite_offer("digest").await.unwrap().is_none());
        assert!(matches!(
            store
                .accept_invite_audited("digest", "Gast", "$argon2id$fake", "sitzung", 60)
                .await
                .unwrap(),
            AcceptOutcome::Gone
        ));
        assert!(store.principal_by_username("gast").await.unwrap().is_none());
        assert!(!actions(&store).await.contains(&"invite.accept".to_string()));
        let _ = summary;
    }

    #[tokio::test]
    async fn accepting_writes_the_account_the_grant_the_session_and_the_record_together() {
        let store = store().await;
        created(&store, "digest", to_raum("gast")).await;

        let AcceptOutcome::Accepted(principal) = store
            .accept_invite_audited("digest", "Gast", "$argon2id$fake", "sitzung", 3600)
            .await
            .unwrap()
        else {
            panic!("the invite must be redeemable");
        };
        assert_eq!(principal.username, "gast");
        assert_eq!(principal.display_name, "Gast");

        let grants = store.grants_defined_at("/raum").await.unwrap();
        assert_eq!(grants.len(), 1);
        assert_eq!(grants[0].subject, Subject::Principal(principal.id.clone()));
        assert_eq!(grants[0].permission, Permission::Read);
        assert_eq!(store.session_count_for(&principal.id).await.unwrap(), 1);

        let recorded = actions(&store).await;
        for action in [
            "invite.create",
            "principal.create",
            "acl.grant",
            "invite.accept",
        ] {
            assert!(recorded.contains(&action.to_string()), "{recorded:?}");
        }
    }

    #[tokio::test]
    async fn a_revoked_invite_cannot_be_revoked_or_accepted_again() {
        let store = store().await;
        let summary = created(&store, "digest", to_raum("gast")).await;

        assert_eq!(
            store
                .revoke_invite_audited("chef", &summary.id)
                .await
                .unwrap(),
            RevokeInviteOutcome::Revoked
        );
        assert_eq!(
            store
                .revoke_invite_audited("chef", &summary.id)
                .await
                .unwrap(),
            RevokeInviteOutcome::NotPending,
            "revoking twice must not read as two withdrawals"
        );
        assert_eq!(
            store
                .revoke_invite_audited("chef", "niemand")
                .await
                .unwrap(),
            RevokeInviteOutcome::NoSuchInvite
        );
        assert!(matches!(
            store
                .accept_invite_audited("digest", "Gast", "$argon2id$fake", "sitzung", 60)
                .await
                .unwrap(),
            AcceptOutcome::Gone
        ));
        assert!(store.principal_by_username("gast").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn a_username_taken_while_the_invitation_was_outstanding_is_reported_not_crashed() {
        let store = store().await;
        created(&store, "digest", to_raum("gast")).await;
        store
            .create_local_principal("gast", "Jemand Anderes", None, "x")
            .await
            .unwrap();

        assert!(matches!(
            store
                .accept_invite_audited("digest", "Gast", "$argon2id$fake", "sitzung", 60)
                .await
                .unwrap(),
            AcceptOutcome::UsernameTaken
        ));
        // And the invite is NOT spent: the person still has an invitation, it just needs a
        // different name choosing for it.
        assert!(store.invite_offer("digest").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_space_admin_sees_only_the_invites_into_the_space_they_administer() {
        let store = store().await;
        let lektor = Principal::test("lektor", &[], &[]);
        store
            .add_grant(
                "/raum",
                Subject::Principal(lektor.id.clone()),
                Permission::Admin,
            )
            .await
            .unwrap();

        created(&store, "eins", to_raum("gast-raum")).await;
        created(
            &store,
            "zwei",
            NewInvite {
                path: Some("/anderer-raum"),
                ..to_raum("gast-anderswo")
            },
        )
        .await;
        created(
            &store,
            "drei",
            NewInvite {
                path: None,
                permission: None,
                team: Some("redaktion"),
                ..to_raum("gast-team")
            },
        )
        .await;

        let seen: Vec<String> = store
            .invites_for(&lektor)
            .await
            .unwrap()
            .into_iter()
            .map(|invite| invite.username)
            .collect();
        assert_eq!(seen, vec!["gast-raum"], "{seen:?}");

        let everything: Vec<String> = store
            .invites_for(&Principal::test("chef", &["admins"], &[]))
            .await
            .unwrap()
            .into_iter()
            .map(|invite| invite.username)
            .collect();
        assert_eq!(everything.len(), 3, "{everything:?}");

        // And an anonymous caller reads nothing, even though `/raum` would answer an
        // `Anyone` grant if there were one.
        assert!(store
            .invites_for(&Principal::anonymous())
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn an_anonymous_caller_lists_no_invites_even_where_anyone_holds_admin() {
        // The `Anyone` grant is what makes this test mean anything. Without it no subject
        // would match, so the assertion would hold whether or not authentication is
        // checked at all — the exact shape that once let a mutation delete the audit
        // reader's authentication check and still pass every test.
        //
        // `Anyone` is the one subject an unauthenticated caller can match, and `can()`
        // answers it BEFORE it looks at whether the caller is signed in, so with this
        // grant in place the only thing standing between an anonymous request and the
        // list of who has been invited is the check being tested here.
        let store = store().await;
        store
            .add_grant("/raum", Subject::Anyone, Permission::Admin)
            .await
            .unwrap();
        created(&store, "eins", to_raum("gast")).await;

        assert!(
            store
                .invites_for(&Principal::anonymous())
                .await
                .unwrap()
                .is_empty(),
            "an anonymous caller read the invite list"
        );

        // Proof that the fixture really is one where the check is the only obstacle: a
        // signed-in nobody DOES see it through that same grant.
        assert_eq!(
            store
                .invites_for(&Principal::test("irgendwer", &[], &[]))
                .await
                .unwrap()
                .len(),
            1
        );

        // And a suspended account does not, although `can()` on its own would say yes:
        // the `Anyone` branch runs before the activity check, so this is the list's own
        // refusal and not the engine's.
        let mut suspended = Principal::test("gesperrt", &[], &[]);
        suspended.active = false;
        assert!(store.invites_for(&suspended).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_summary_never_carries_the_token_or_its_digest() {
        let store = store().await;
        let summary = created(&store, "digest-of-geheim", to_raum("gast")).await;
        let rendered = serde_json::to_string(&summary).unwrap();
        assert!(!rendered.contains("geheim"), "{rendered}");
        assert!(rendered.contains("gast"), "{rendered}");
    }
}
