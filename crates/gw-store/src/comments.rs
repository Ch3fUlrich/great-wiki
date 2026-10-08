//! Comments, governed by their page (ADR 0025).
//!
//! A comment belongs to exactly one page, and **Read on that page is the only check**: a
//! reader of a page may read and write its comments. Every function here asks the crate's
//! one accessor, [`Store::document_for`] / [`Store::document_for_id`], and a page the
//! caller may not read answers exactly as an absent page does (`None`). Nothing is counted
//! or listed in SQL across pages, and the author's name is looked up only after the page
//! check has passed. There is deliberately no delete here (ADR 0025).

use crate::events::{EventKind, NewEvent};
use crate::revisions::byline;
use crate::Store;
use anyhow::{bail, Result};
use gw_auth::{Action, Principal};
use serde::Serialize;
use sqlx::FromRow;

pub const MAX_BODY_CHARS: usize = 8000;
pub const MAX_QUOTE_CHARS: usize = 200;

/// An anchored comment's position: opaque Yjs relative positions plus a short quote.
#[derive(Debug, Clone)]
pub struct CommentAnchor {
    pub start: Vec<u8>,
    pub end: Vec<u8>,
    pub quote: String,
}

#[derive(Debug, Clone)]
pub struct NewComment {
    pub body: String,
    pub parent_id: Option<String>,
    pub anchor: Option<CommentAnchor>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Comment {
    pub id: String,
    pub doc_id: String,
    pub parent_id: Option<String>,
    pub author_id: Option<String>,
    pub author_name: String,
    pub body: String,
    pub anchor_start: Option<Vec<u8>>,
    pub anchor_end: Option<Vec<u8>>,
    pub anchor_quote: Option<String>,
    pub orphaned: bool,
    pub resolved_at: Option<String>,
    pub resolved_by: Option<String>,
    pub created_at: String,
}

#[derive(FromRow)]
struct CommentRow {
    id: String,
    doc_id: String,
    parent_id: Option<String>,
    author_id: Option<String>,
    author_name: String,
    body: String,
    anchor_start: Option<Vec<u8>>,
    anchor_end: Option<Vec<u8>>,
    anchor_quote: Option<String>,
    orphaned: i64,
    resolved_at: Option<String>,
    resolved_by: Option<String>,
    created_at: String,
}

const COLUMNS: &str = "id, doc_id, parent_id, author_id, author_name, body, anchor_start, \
                       anchor_end, anchor_quote, orphaned, resolved_at, resolved_by, created_at";

impl From<CommentRow> for Comment {
    fn from(r: CommentRow) -> Self {
        Comment {
            id: r.id,
            doc_id: r.doc_id,
            parent_id: r.parent_id,
            author_id: r.author_id,
            author_name: r.author_name,
            body: r.body,
            anchor_start: r.anchor_start,
            anchor_end: r.anchor_end,
            anchor_quote: r.anchor_quote,
            orphaned: r.orphaned != 0,
            resolved_at: r.resolved_at,
            resolved_by: r.resolved_by,
            created_at: r.created_at,
        }
    }
}

/// The distinct `@username` tokens in `body`, in order of appearance. An `@` counts at the
/// start of a word only (so an e-mail address is not a mention); the name is
/// `[A-Za-z0-9._-]+` with trailing dots dropped (sentence punctuation).
pub fn mentions(body: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let ok = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    let mut out: Vec<String> = Vec::new();
    for (i, &c) in chars.iter().enumerate() {
        if c != '@' || (i > 0 && (ok(chars[i - 1]) || chars[i - 1] == '@')) {
            continue;
        }
        let name: String = chars[i + 1..].iter().take_while(|c| ok(**c)).collect();
        let name = name.trim_end_matches('.');
        if !name.is_empty() && !out.iter().any(|n| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

impl Store {
    /// `path_or_id`: a path (starts with `/`) or a document id. `None` = withheld or absent.
    async fn readable_page(
        &self,
        principal: &Principal,
        path_or_id: &str,
    ) -> Result<Option<crate::StoredDocument>> {
        if !principal.is_authenticated() {
            return Ok(None);
        }
        if path_or_id.starts_with('/') {
            self.document_for(principal, path_or_id, Action::Read).await
        } else {
            self.document_for_id(principal, path_or_id, Action::Read)
                .await
        }
    }

    /// Add a comment to a page the principal may **read** (not write). `Ok(None)` when the
    /// page is withheld or absent, or `parent_id` is not a top-level comment of that page.
    /// A bad body is an error.
    pub async fn create_comment(
        &self,
        principal: &Principal,
        path_or_id: &str,
        new: NewComment,
    ) -> Result<Option<Comment>> {
        let len = new.body.chars().count();
        if new.body.trim().is_empty() || len > MAX_BODY_CHARS {
            bail!("a comment must be 1 to {MAX_BODY_CHARS} characters");
        }
        if let Some(a) = &new.anchor {
            if a.quote.chars().count() > MAX_QUOTE_CHARS {
                bail!("an anchor quote is at most {MAX_QUOTE_CHARS} characters");
            }
        }
        let Some(doc) = self.readable_page(principal, path_or_id).await? else {
            return Ok(None);
        };
        let mut parent_author: Option<String> = None;
        if let Some(pid) = &new.parent_id {
            let parent: Option<(Option<String>, Option<String>)> = sqlx::query_as(
                "SELECT author_id, parent_id FROM comments WHERE id = ?1 AND doc_id = ?2",
            )
            .bind(pid)
            .bind(&doc.id)
            .fetch_optional(&self.pool)
            .await?;
            match parent {
                Some((author, None)) => parent_author = author,
                _ => return Ok(None),
            }
        }
        let id = uuid::Uuid::now_v7().to_string();
        let name = byline(principal).to_string();
        let (start, end, quote) = match new.anchor {
            Some(a) => (Some(a.start), Some(a.end), Some(a.quote)),
            None => (None, None, None),
        };
        sqlx::query(
            "INSERT INTO comments (id, doc_id, parent_id, author_id, author_name, body, \
             anchor_start, anchor_end, anchor_quote) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(&id)
        .bind(&doc.id)
        .bind(&new.parent_id)
        .bind(&principal.id)
        .bind(&name)
        .bind(&new.body)
        .bind(start)
        .bind(end)
        .bind(quote)
        .execute(&self.pool)
        .await?;

        // Candidates only; delivery checks (ADR 0024). No body text goes in an event.
        if let Some(to) = parent_author {
            self.emit_logged(&NewEvent {
                kind: EventKind::CommentReply,
                recipient: to,
                actor: Some(principal.id.clone()),
                doc_id: Some(doc.id.clone()),
                path: None,
                subject: Some(id.clone()),
                dedupe_key: Some(format!("comment:{id}")),
            })
            .await;
        }
        for username in mentions(&new.body) {
            match self.principal_by_username(&username).await {
                Ok(Some((who, _))) if who.active => {
                    self.emit_logged(&NewEvent {
                        kind: EventKind::Mention,
                        recipient: who.id,
                        actor: Some(principal.id.clone()),
                        doc_id: Some(doc.id.clone()),
                        path: None,
                        subject: Some(id.clone()),
                        dedupe_key: None,
                    })
                    .await;
                }
                Ok(_) => {}
                Err(err) => tracing::warn!(error = %err, "mention lookup failed"),
            }
        }

        let row: CommentRow =
            sqlx::query_as(&format!("SELECT {COLUMNS} FROM comments WHERE id = ?1"))
                .bind(&id)
                .fetch_one(&self.pool)
                .await?;
        Ok(Some(row.into()))
    }

    /// The page's comments in created order, or `None` when the page is withheld or absent
    /// (the same answer for both, ADR 0022). Threading is the caller's. Author names are
    /// resolved only after the page check.
    pub async fn comments_for_document(
        &self,
        principal: &Principal,
        path_or_id: &str,
    ) -> Result<Option<Vec<Comment>>> {
        let Some(doc) = self.readable_page(principal, path_or_id).await? else {
            return Ok(None);
        };
        let rows: Vec<CommentRow> = sqlx::query_as(&format!(
            "SELECT {COLUMNS} FROM comments WHERE doc_id = ?1 ORDER BY created_at, id"
        ))
        .bind(&doc.id)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let mut c: Comment = row.into();
            if let Some(aid) = &c.author_id {
                if let Some((who, _)) = self.principal_by_id(aid).await? {
                    c.author_name = byline(&who).to_string();
                }
            }
            out.push(c);
        }
        Ok(Some(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, NewDocument};
    use gw_auth::{Permission, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Visibility};

    #[test]
    fn mentions_are_distinct_word_start_tokens() {
        assert_eq!(mentions("hi @anna, and @bert.x. @anna"), ["anna", "bert.x"]);
        assert_eq!(mentions("@a_b-c"), ["a_b-c"]);
        assert!(mentions("mail me@example.com or @ alone").is_empty());
        assert!(mentions("@@x").is_empty());
        assert_eq!(mentions("(@x) \n@y"), ["x", "y"]);
    }

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

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

    fn new(body: &str, parent: Option<&str>) -> NewComment {
        NewComment {
            body: body.into(),
            parent_id: parent.map(str::to_string),
            anchor: None,
        }
    }

    #[tokio::test]
    async fn a_reader_can_comment_and_see_the_thread() {
        let s = store().await;
        let (path, id) = page(&s, "Raum").await;
        let a = account(&s, "anna").await;
        grant(&s, &path, &a, Permission::Read).await;
        let c = s
            .create_comment(&a, &path, new("Tippfehler", None))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.author_name, "anna");
        let list = s.comments_for_document(&a, &id).await.unwrap().unwrap();
        assert_eq!(list, vec![c]);
    }

    #[tokio::test]
    async fn without_read_access_create_and_list_answer_as_for_a_missing_page() {
        let s = store().await;
        let (path, _) = page(&s, "Raum").await;
        let a = account(&s, "anna").await;
        assert!(s
            .create_comment(&a, &path, new("x", None))
            .await
            .unwrap()
            .is_none());
        assert!(s.comments_for_document(&a, &path).await.unwrap().is_none());
        assert!(s
            .comments_for_document(&a, "/gibt-es-nicht")
            .await
            .unwrap()
            .is_none());
        assert!(s
            .create_comment(&a, "/gibt-es-nicht", new("x", None))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_parent_from_another_page_or_a_reply_is_rejected() {
        let s = store().await;
        let (p1, _) = page(&s, "Eins").await;
        let (p2, _) = page(&s, "Zwei").await;
        let a = account(&s, "anna").await;
        grant(&s, &p1, &a, Permission::Read).await;
        grant(&s, &p2, &a, Permission::Read).await;
        let top = s
            .create_comment(&a, &p1, new("oben", None))
            .await
            .unwrap()
            .unwrap();
        assert!(s
            .create_comment(&a, &p2, new("x", Some(&top.id)))
            .await
            .unwrap()
            .is_none());
        let reply = s
            .create_comment(&a, &p1, new("r", Some(&top.id)))
            .await
            .unwrap()
            .unwrap();
        assert!(s
            .create_comment(&a, &p1, new("rr", Some(&reply.id)))
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_reply_reaches_the_parent_author_until_their_grant_is_gone() {
        let s = store().await;
        let (path, _) = page(&s, "Raum").await;
        let (a, b) = (account(&s, "anna").await, account(&s, "bert").await);
        grant(&s, &path, &a, Permission::Read).await;
        grant(&s, &path, &b, Permission::Read).await;
        let top = s
            .create_comment(&a, &path, new("frage", None))
            .await
            .unwrap()
            .unwrap();
        s.create_comment(&b, &path, new("antwort", Some(&top.id)))
            .await
            .unwrap()
            .unwrap();
        let n = s.notifications_for(&a, 10).await.unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, EventKind::CommentReply);
        assert_eq!(n[0].actor_name.as_deref(), Some("bert"));
        s.remove_grant(&path, &Subject::Principal(a.id.clone()), Permission::Read)
            .await
            .unwrap();
        assert!(s.notifications_for(&a, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_mention_without_read_access_is_invisible_and_one_with_it_arrives() {
        let s = store().await;
        let (path, _) = page(&s, "Raum").await;
        let (a, b, c) = (
            account(&s, "anna").await,
            account(&s, "bert").await,
            account(&s, "carl").await,
        );
        grant(&s, &path, &a, Permission::Read).await;
        grant(&s, &path, &b, Permission::Read).await;
        s.create_comment(&a, &path, new("@bert @carl @niemand", None))
            .await
            .unwrap()
            .unwrap();
        assert!(s.notifications_for(&c, 10).await.unwrap().is_empty());
        let n = s.notifications_for(&b, 10).await.unwrap();
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].kind, EventKind::Mention);
    }

    #[tokio::test]
    async fn the_body_length_is_validated() {
        let s = store().await;
        let (path, _) = page(&s, "Raum").await;
        let a = account(&s, "anna").await;
        grant(&s, &path, &a, Permission::Read).await;
        assert!(s.create_comment(&a, &path, new("  ", None)).await.is_err());
        assert!(s
            .create_comment(&a, &path, new(&"x".repeat(8001), None))
            .await
            .is_err());
        assert!(s
            .create_comment(&a, &path, new(&"x".repeat(8000), None))
            .await
            .is_ok());
    }
}
