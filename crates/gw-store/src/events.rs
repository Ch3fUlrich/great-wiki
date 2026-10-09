//! The event bus: what happened that a person might want to hear about (ADR 0025).
//!
//! **Emit records candidates; delivery is the check.** A producer names the people an
//! event might concern and writes a row each, with no permission check and no page words —
//! only a kind and ids. Every read here asks again, *at read time, for the reader*, through
//! [`Store::document_for_id_with_baseline`], the crate's one document accessor. A row whose
//! page the reader may no longer read is absent: from the list, from the badge count and
//! from marking-as-read alike (ADR 0022). So a grant removed after the event takes the
//! title out of the inbox with it, which an emit-time check could never do.
//!
//! The title and path in a [`Notification`] come from the accessor's document and nowhere
//! else; the actor's name is only looked up once that check has passed, so a withheld row
//! costs the reader nothing and teaches them nothing.
//!
//! The unread count is the length of the same filtered set, never a SQL `COUNT(*)`: a
//! count taken from the table would disclose how many events concern pages the reader may
//! not see.

use crate::acl::Baseline;
use crate::revisions::byline;
use crate::Store;
use anyhow::Result;
use gw_auth::{can, Action, Principal};
use gw_core::Visibility;
use serde::Serialize;
use sqlx::FromRow;

/// Rows older than this are not read at all (ADR 0025, "Cost").
const HORIZON: &str = "-90 days";

/// What happened. The stored spelling is [`EventKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    CommentReply,
    Mention,
    PageEdited,
    TaskAssigned,
    TaskDue,
    /// Admin kind: also needs the reader to still administer the path.
    InviteAccepted,
    /// Admin kind: also needs the reader to still administer the path.
    GrantChanged,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::CommentReply => "comment_reply",
            EventKind::Mention => "mention",
            EventKind::PageEdited => "page_edited",
            EventKind::TaskAssigned => "task_assigned",
            EventKind::TaskDue => "task_due",
            EventKind::InviteAccepted => "invite_accepted",
            EventKind::GrantChanged => "grant_changed",
        }
    }

    /// `None` for an unknown spelling. Such a row is skipped by readers rather than guessed
    /// at: showing an event of a kind this build does not know would be inventing it.
    pub fn from_stored(s: &str) -> Option<Self> {
        Some(match s {
            "comment_reply" => EventKind::CommentReply,
            "mention" => EventKind::Mention,
            "page_edited" => EventKind::PageEdited,
            "task_assigned" => EventKind::TaskAssigned,
            "task_due" => EventKind::TaskDue,
            "invite_accepted" => EventKind::InviteAccepted,
            "grant_changed" => EventKind::GrantChanged,
            _ => return None,
        })
    }

    fn is_admin(self) -> bool {
        matches!(self, EventKind::InviteAccepted | EventKind::GrantChanged)
    }
}

/// A candidate to record. Ids only; see the module header.
#[derive(Debug, Clone)]
pub struct NewEvent {
    pub kind: EventKind,
    pub recipient: String,
    pub actor: Option<String>,
    pub doc_id: Option<String>,
    /// Only for an admin event about a path with no page.
    pub path: Option<String>,
    /// A comment or task id; opaque here.
    pub subject: Option<String>,
    pub dedupe_key: Option<String>,
}

/// The page an event is about, as the reader may see it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct NotificationPage {
    pub path: String,
    /// `None` for an admin event about a path that has no page.
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: String,
    pub kind: EventKind,
    pub created_at: String,
    pub read: bool,
    pub actor_name: Option<String>,
    pub page: NotificationPage,
    pub subject: Option<String>,
}

#[derive(FromRow)]
struct EventRow {
    id: String,
    kind: String,
    actor: Option<String>,
    doc_id: Option<String>,
    path: Option<String>,
    subject: Option<String>,
    created_at: String,
    read_at: Option<String>,
}

impl Store {
    /// Record that `ev.recipient` might want to hear of something.
    ///
    /// **No permission check, on purpose** (ADR 0025 rule 1): delivery makes it. An event
    /// whose actor is its recipient is dropped — nobody needs telling what they just did.
    /// A repeat of a `(recipient, dedupe_key)` coalesces: the one row is bumped to now,
    /// credited to the latest actor and made unread again.
    pub async fn emit_event(&self, ev: &NewEvent) -> Result<()> {
        if ev.actor.as_deref() == Some(ev.recipient.as_str()) {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO events (id, kind, recipient, actor, doc_id, path, subject, dedupe_key) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT (recipient, dedupe_key) WHERE dedupe_key IS NOT NULL DO UPDATE SET \
             created_at = datetime('now'), actor = excluded.actor, read_at = NULL, \
             digested_at = NULL",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(ev.kind.as_str())
        .bind(&ev.recipient)
        .bind(&ev.actor)
        .bind(&ev.doc_id)
        .bind(&ev.path)
        .bind(&ev.subject)
        .bind(&ev.dedupe_key)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// [`Store::emit_event`] for a producer: a failed emit is logged and swallowed.
    ///
    /// ADR 0025 ("Cost / revisit"): a lost notification is an annoyance, a failed save or
    /// grant because the bus hiccuped is a bug. Every producer goes through here so none of
    /// them can forget — and so the log line is one place to grep.
    pub(crate) async fn emit_logged(&self, ev: &NewEvent) {
        if let Err(err) = self.emit_event(ev).await {
            tracing::warn!(kind = ev.kind.as_str(), error = %err, "event not recorded");
        }
    }

    /// Record an admin event about `path` for every account that administers it.
    ///
    /// Candidates are enumerated by asking [`Store::administers`] of each active account —
    /// the very rule delivery applies (instance admins and the admin baseline, plus Admin
    /// grants on or above the path, directly, by team or by group). That is deliberately
    /// the same function rather than a cheaper look-alike: a second spelling of "who
    /// administers this" would be a second answer. It costs one grants lookup per account,
    /// which is fine at wiki scale and runs only when an invitation is taken or a grant
    /// changes. Delivery still re-asks, so a candidate who stops administering the path
    /// before reading simply never sees the row.
    ///
    /// The row names the page when one lives at `path`, else only the path. Never fails the
    /// caller; see [`Store::emit_logged`].
    pub(crate) async fn emit_admin_event(
        &self,
        kind: EventKind,
        actor: &str,
        path: &str,
        dedupe_key: Option<String>,
    ) {
        if let Err(err) = self
            .try_emit_admin_event(kind, actor, path, dedupe_key)
            .await
        {
            tracing::warn!(kind = kind.as_str(), error = %err, "admin event not recorded");
        }
    }

    async fn try_emit_admin_event(
        &self,
        kind: EventKind,
        actor: &str,
        path: &str,
        dedupe_key: Option<String>,
    ) -> Result<()> {
        let doc_id: Option<String> =
            sqlx::query_scalar("SELECT id FROM documents WHERE path = ?1 AND deleted_at IS NULL")
                .bind(path)
                .fetch_optional(&self.pool)
                .await?;
        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM principals WHERE active = 1")
            .fetch_all(&self.pool)
            .await?;
        for id in ids {
            if id == actor {
                continue;
            }
            let Some((who, _)) = self.principal_by_id(&id).await? else {
                continue;
            };
            let baseline = self.baseline_for(&who).await?;
            if !self.administers(&who, baseline, path).await? {
                continue;
            }
            self.emit_logged(&NewEvent {
                kind,
                recipient: id,
                actor: Some(actor.to_string()),
                doc_id: doc_id.clone(),
                // A page's own row is checked through the page; the path is only for a
                // path with none.
                path: if doc_id.is_some() {
                    None
                } else {
                    Some(path.to_string())
                },
                subject: None,
                dedupe_key: dedupe_key.clone(),
            })
            .await;
        }
        Ok(())
    }

    /// The newest `limit` notifications `principal` may still see.
    pub async fn notifications_for(
        &self,
        principal: &Principal,
        limit: usize,
    ) -> Result<Vec<Notification>> {
        self.visible_events(principal, Some(limit), false).await
    }

    /// The newest `limit` notifications `principal` may still see that no digest has
    /// carried yet. Cut from the same delivery-filtered set as the inbox (never a raw read),
    /// so a withheld event cannot reach an email either. Nobody without an address gets any:
    /// there is nowhere to send them.
    pub async fn digest_for(
        &self,
        principal: &Principal,
        limit: usize,
    ) -> Result<Vec<Notification>> {
        if principal
            .email
            .as_deref()
            .is_none_or(|e| e.trim().is_empty())
        {
            return Ok(Vec::new());
        }
        self.visible_events(principal, Some(limit), true).await
    }

    /// Record that a digest carried these rows. Only the principal's own rows are touched.
    pub async fn mark_digested(&self, principal: &Principal, ids: &[String]) -> Result<()> {
        for id in ids {
            sqlx::query(
                "UPDATE events SET digested_at = COALESCE(digested_at, datetime('now')) \
                 WHERE id = ?1 AND recipient = ?2",
            )
            .bind(id)
            .bind(&principal.id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Active accounts that have an email address: the only possible digest recipients.
    pub async fn digest_recipients(&self) -> Result<Vec<Principal>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM principals WHERE active = 1 AND email IS NOT NULL AND email <> ''",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::new();
        for id in ids {
            if let Some((p, _)) = self.principal_by_id(&id).await? {
                out.push(p);
            }
        }
        Ok(out)
    }

    /// How many unread notifications `principal` may still see: the length of the same
    /// filtered set the list is cut from, so badge and list cannot disagree.
    pub async fn unread_count_for(&self, principal: &Principal) -> Result<usize> {
        Ok(self
            .visible_events(principal, None, false)
            .await?
            .iter()
            .filter(|n| !n.read)
            .count())
    }

    /// Mark one row read. `false` when it is not the principal's or is withheld from them —
    /// the same answer for both, so the id is not an oracle (ADR 0022).
    pub async fn mark_event_read(&self, principal: &Principal, event_id: &str) -> Result<bool> {
        if !principal.is_authenticated() {
            return Ok(false);
        }
        let row: Option<EventRow> = sqlx::query_as(
            "SELECT id, kind, actor, doc_id, path, subject, created_at, read_at FROM events \
             WHERE id = ?1 AND recipient = ?2 AND created_at >= datetime('now', ?3)",
        )
        .bind(event_id)
        .bind(&principal.id)
        .bind(HORIZON)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else { return Ok(false) };
        let baseline = self.baseline_for(principal).await?;
        if self.deliver(principal, baseline, row).await?.is_none() {
            return Ok(false);
        }
        sqlx::query("UPDATE events SET read_at = COALESCE(read_at, datetime('now')) WHERE id = ?1")
            .bind(event_id)
            .execute(&self.pool)
            .await?;
        Ok(true)
    }

    /// Mark everything `principal` can see as read. Withheld rows stay as they are.
    pub async fn mark_all_read(&self, principal: &Principal) -> Result<()> {
        for n in self.visible_events(principal, None, false).await? {
            if !n.read {
                self.mark_event_read(principal, &n.id).await?;
            }
        }
        Ok(())
    }

    /// The one filtered set every read above is cut from. Newest first.
    async fn visible_events(
        &self,
        principal: &Principal,
        limit: Option<usize>,
        undigested_only: bool,
    ) -> Result<Vec<Notification>> {
        if !principal.is_authenticated() {
            return Ok(Vec::new());
        }
        let rows: Vec<EventRow> = sqlx::query_as(
            "SELECT id, kind, actor, doc_id, path, subject, created_at, read_at FROM events \
             WHERE recipient = ?1 AND created_at >= datetime('now', ?2) \
             AND (?3 = 0 OR digested_at IS NULL) \
             ORDER BY created_at DESC, id DESC",
        )
        .bind(&principal.id)
        .bind(HORIZON)
        .bind(undigested_only)
        .fetch_all(&self.pool)
        .await?;
        // Hoisted: the baseline belongs to the reader, not to the row.
        let baseline = self.baseline_for(principal).await?;
        let mut out = Vec::new();
        for row in rows {
            if let Some(n) = self.deliver(principal, baseline, row).await? {
                out.push(n);
                if limit.is_some_and(|l| out.len() >= l) {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// Delivery: ask again, for this reader, now. `None` is withheld.
    async fn deliver(
        &self,
        reader: &Principal,
        baseline: Baseline,
        row: EventRow,
    ) -> Result<Option<Notification>> {
        let Some(kind) = EventKind::from_stored(&row.kind) else {
            return Ok(None);
        };
        let page = match &row.doc_id {
            Some(doc_id) => {
                let Some(doc) = self
                    .document_for_id_with_baseline(reader, doc_id, Action::Read, baseline)
                    .await?
                else {
                    return Ok(None);
                };
                if kind.is_admin() && !self.administers(reader, baseline, &doc.path).await? {
                    return Ok(None);
                }
                NotificationPage {
                    path: doc.path,
                    title: Some(doc.title),
                }
            }
            // A pageless row can only be an admin row about a path; anything else has
            // nothing to be checked against and is withheld.
            None => match &row.path {
                Some(path)
                    if kind.is_admin() && self.administers(reader, baseline, path).await? =>
                {
                    NotificationPage {
                        path: path.clone(),
                        title: None,
                    }
                }
                _ => return Ok(None),
            },
        };
        // Only now, with the page check passed, may the row name a person.
        let actor_name = match &row.actor {
            Some(id) => self
                .principal_by_id(id)
                .await?
                .map(|(who, _)| byline(&who).to_string()),
            None => None,
        };
        Ok(Some(Notification {
            id: row.id,
            kind,
            created_at: row.created_at,
            read: row.read_at.is_some(),
            actor_name,
            page,
            subject: row.subject,
        }))
    }

    /// Whether `who` administers `path`: the admin baseline, or an admin grant. The same
    /// rule the invitation and audit listings apply — `Restricted` makes `can()` consult
    /// grants alone, so a lesser baseline cannot stand in for an admin grant.
    async fn administers(&self, who: &Principal, baseline: Baseline, path: &str) -> Result<bool> {
        if baseline >= Baseline::Admin {
            return Ok(true);
        }
        let grants = self.grants_for_path(path).await?;
        Ok(can(who, Action::Admin, Visibility::Restricted, &grants))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, NewDocument};
    use gw_auth::{Permission, Subject};
    use gw_core::{Block, BlockKind, DocumentType};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    /// A restricted page, so a test that forgets a grant fails closed. Returns (path, id).
    async fn page(store: &Store, title: &str) -> (String, String) {
        let body = Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        };
        let id = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
        let path = store.document_path_unchecked(&id).await.unwrap().unwrap();
        (path, id)
    }

    async fn account(store: &Store, name: &str) -> Principal {
        store
            .create_local_principal(name, name, None, "hash")
            .await
            .unwrap()
    }

    async fn grant(store: &Store, path: &str, who: &Principal, p: Permission) {
        store
            .add_grant(path, Subject::Principal(who.id.clone()), p)
            .await
            .unwrap();
    }

    fn ev(kind: EventKind, to: &Principal, by: Option<&Principal>, doc: Option<&str>) -> NewEvent {
        NewEvent {
            kind,
            recipient: to.id.clone(),
            actor: by.map(|p| p.id.clone()),
            doc_id: doc.map(str::to_string),
            path: None,
            subject: Some("c1".into()),
            dedupe_key: None,
        }
    }

    #[tokio::test]
    async fn an_emitted_event_reaches_a_reader_with_the_page_as_they_see_it() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &path, &b, Permission::Read).await;
        s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        let list = s.notifications_for(&b, 10).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].page.title.as_deref(), Some("Geheim"));
        assert_eq!(list[0].page.path, path);
        assert_eq!(list[0].actor_name.as_deref(), Some("anna"));
        assert!(!list[0].read);
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn a_reader_without_read_access_gets_nothing_and_a_count_of_zero() {
        let s = store().await;
        let (_, id) = page(&s, "Geheim").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        assert!(s.notifications_for(&b, 10).await.unwrap().is_empty());
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn a_grant_removed_after_the_event_makes_it_disappear() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &path, &b, Permission::Read).await;
        s.emit_event(&ev(EventKind::PageEdited, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 1);
        s.remove_grant(&path, &Subject::Principal(b.id.clone()), Permission::Read)
            .await
            .unwrap();
        assert!(s.notifications_for(&b, 10).await.unwrap().is_empty());
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn the_same_dedupe_key_coalesces_and_becomes_unread_again() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let (a, b, c) = (
            account(&s, "anna").await,
            account(&s, "bert").await,
            account(&s, "carl").await,
        );
        grant(&s, &path, &b, Permission::Read).await;
        let mut e = ev(EventKind::PageEdited, &b, Some(&a), Some(&id));
        e.dedupe_key = Some(format!("edit:{id}"));
        s.emit_event(&e).await.unwrap();
        let first = s.notifications_for(&b, 10).await.unwrap();
        assert!(s.mark_event_read(&b, &first[0].id).await.unwrap());
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 0);
        e.actor = Some(c.id.clone());
        s.emit_event(&e).await.unwrap();
        let after = s.notifications_for(&b, 10).await.unwrap();
        assert_eq!(after.len(), 1);
        assert!(!after[0].read);
        assert_eq!(after[0].actor_name.as_deref(), Some("carl"));
    }

    #[tokio::test]
    async fn the_unread_count_is_the_length_of_the_filtered_list() {
        let s = store().await;
        let (p1, d1) = page(&s, "Eins").await;
        let (_, d2) = page(&s, "Zwei").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &p1, &b, Permission::Read).await;
        for d in [&d1, &d2, &d2] {
            s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(d)))
                .await
                .unwrap();
        }
        let list = s.notifications_for(&b, 10).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(s.unread_count_for(&b).await.unwrap(), list.len());
        s.mark_all_read(&b).await.unwrap();
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn marking_a_withheld_event_read_answers_false_and_changes_nothing() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        let (eid,): (String,) = sqlx::query_as("SELECT id FROM events")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert!(!s.mark_event_read(&b, &eid).await.unwrap());
        // Someone else's row is just as absent, even to a reader of the page.
        grant(&s, &path, &a, Permission::Read).await;
        assert!(!s.mark_event_read(&a, &eid).await.unwrap());
        let (read,): (Option<String>,) = sqlx::query_as("SELECT read_at FROM events")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert!(read.is_none());
    }

    #[tokio::test]
    async fn an_event_caused_by_its_recipient_is_not_recorded() {
        let s = store().await;
        let (_, id) = page(&s, "Geheim").await;
        let a = account(&s, "anna").await;
        s.emit_event(&ev(EventKind::PageEdited, &a, Some(&a), Some(&id)))
            .await
            .unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM events")
            .fetch_one(&s.pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn an_admin_event_needs_the_reader_to_still_administer_the_path() {
        let s = store().await;
        let (path, id) = page(&s, "Raum").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &path, &b, Permission::Read).await;
        s.emit_event(&ev(EventKind::GrantChanged, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        // May read the page, does not administer it.
        assert!(s.notifications_for(&b, 10).await.unwrap().is_empty());
        grant(&s, &path, &b, Permission::Admin).await;
        assert_eq!(s.notifications_for(&b, 10).await.unwrap().len(), 1);

        // A pageless row is checked against its path alone.
        let mut e = ev(EventKind::InviteAccepted, &b, Some(&a), None);
        e.path = Some(path.clone());
        s.emit_event(&e).await.unwrap();
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 2);
        s.remove_grant(&path, &Subject::Principal(b.id.clone()), Permission::Admin)
            .await
            .unwrap();
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn rows_older_than_ninety_days_are_ignored() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &path, &b, Permission::Read).await;
        s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        sqlx::query("UPDATE events SET created_at = datetime('now', '-91 days')")
            .execute(&s.pool)
            .await
            .unwrap();
        assert_eq!(s.unread_count_for(&b).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn the_digest_carries_only_visible_undigested_rows_for_an_addressed_reader() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let a = account(&s, "anna").await;
        let b = s
            .create_local_principal("bert", "bert", Some("bert@example.org"), "hash")
            .await
            .unwrap();
        grant(&s, &path, &b, Permission::Read).await;
        s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        let d = s.digest_for(&b, 10).await.unwrap();
        assert_eq!(d.len(), 1);
        // Digested rows are gone from the digest but stay in the inbox.
        s.mark_digested(&b, &[d[0].id.clone()]).await.unwrap();
        assert!(s.digest_for(&b, 10).await.unwrap().is_empty());
        assert_eq!(s.notifications_for(&b, 10).await.unwrap().len(), 1);
        // A coalesced repeat is news again.
        let mut again = ev(EventKind::Mention, &b, Some(&a), Some(&id));
        again.dedupe_key = Some("k".into());
        s.emit_event(&again).await.unwrap();
        let first = s.digest_for(&b, 10).await.unwrap();
        s.mark_digested(&b, &first.iter().map(|n| n.id.clone()).collect::<Vec<_>>())
            .await
            .unwrap();
        s.emit_event(&again).await.unwrap();
        assert_eq!(s.digest_for(&b, 10).await.unwrap().len(), 1);
        // Grant removed: absent.
        s.remove_grant(&path, &Subject::Principal(b.id.clone()), Permission::Read)
            .await
            .unwrap();
        assert!(s.digest_for(&b, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn no_address_no_digest() {
        let s = store().await;
        let (path, id) = page(&s, "Geheim").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &path, &b, Permission::Read).await;
        s.emit_event(&ev(EventKind::Mention, &b, Some(&a), Some(&id)))
            .await
            .unwrap();
        assert!(s.digest_for(&b, 10).await.unwrap().is_empty());
        assert!(s.digest_recipients().await.unwrap().is_empty());
    }
}
