//! Comments over HTTP: the threads on a page, and a way to add to them.
//!
//! # This module makes no permission decision
//!
//! `gw_store::comments` is the decision (ADR 0025): **Read on the page is the only check**, so
//! a reader with a read-only grant may comment, and a page the caller may not read answers as
//! an absent page does. The handlers turn a request into `comments_for_document` /
//! `create_comment`, turn the answer into a status code and drop internal identifiers on the
//! way out. A check written here would be a second answer that can disagree with the first.
//!
//! # One 404 for "withheld" and "absent"
//!
//! Both leave through [`withheld_or_absent`] (ADR 0022), so a signed-in caller without a grant
//! gets identical bytes for either, on `GET` and on `POST`. An anonymous `GET` is the same
//! 404 (the store treats an anonymous caller as able to read no comment); an anonymous `POST`
//! is 401, a statement about the caller and the same for every path, so it tells nothing about
//! any page.
//!
//! # A bad parent is 400, not 404
//!
//! The store reports `None` for a withheld page *and* for a `parent_id` that is not a
//! top-level comment of this page (another page's comment, a reply, or no such id). Once the
//! caller is known to be able to read the page, the second reading is the only one left, so it
//! is a 400: the page is theirs to know about and the request is what is wrong. A caller who
//! cannot read the page never reaches that branch.
//!
//! # Nothing can be deleted or edited
//!
//! ADR 0025 gives authors no delete and no edit of a body, so there is deliberately no
//! `DELETE` or `PUT` route here: the method answers 405 and `tests/comments.rs` pins it.
//!
//! # No field counts what was hidden
//!
//! The list is `{ "threads": [...] }` and nothing else: no total, no count.

use super::docs::withheld_or_absent;
use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use gw_auth::Action;
use gw_store::comments::{Comment, CommentAnchor, NewComment, MAX_BODY_CHARS, MAX_QUOTE_CHARS};
use serde::{Deserialize, Serialize};

/// An anchor on the wire: the two Yjs relative positions as standard base64, and the quote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorView {
    pub start: String,
    pub end: String,
    pub quote: String,
}

/// One comment. The document id, author id and resolver id stay inside.
#[derive(Debug, Clone, Serialize)]
pub struct CommentView {
    pub id: String,
    pub parent_id: Option<String>,
    pub author_name: String,
    pub body: String,
    pub anchor: Option<AnchorView>,
    pub orphaned: bool,
    pub resolved_at: Option<String>,
    pub created_at: String,
}

impl From<&Comment> for CommentView {
    fn from(c: &Comment) -> Self {
        let anchor = match (&c.anchor_start, &c.anchor_end) {
            (Some(s), Some(e)) => Some(AnchorView {
                start: STANDARD.encode(s),
                end: STANDARD.encode(e),
                quote: c.anchor_quote.clone().unwrap_or_default(),
            }),
            _ => None,
        };
        Self {
            id: c.id.clone(),
            parent_id: c.parent_id.clone(),
            author_name: c.author_name.clone(),
            body: c.body.clone(),
            anchor,
            orphaned: c.orphaned,
            resolved_at: c.resolved_at.clone(),
            created_at: c.created_at.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ThreadView {
    #[serde(flatten)]
    pub comment: CommentView,
    pub replies: Vec<CommentView>,
}

/// Deliberately a single field: see the module header.
#[derive(Debug, Serialize)]
pub struct ListResponse {
    pub threads: Vec<ThreadView>,
}

#[derive(Debug, Deserialize)]
pub struct NewCommentBody {
    pub body: String,
    pub parent_id: Option<String>,
    pub anchor: Option<AnchorView>,
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/comments/document/{*path}", get(list).post(create))
}

fn full(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
) -> Result<Json<ListResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let full = full(&path);
    if !principal.is_authenticated() {
        return Err(ApiError::NotFound);
    }
    let Some(found) = state
        .store
        .comments_for_document(&principal, &full)
        .await
        .map_err(ApiError::Internal)?
    else {
        return Err(withheld_or_absent(&state, &principal, &full).await);
    };
    // Created order, so a parent always precedes its replies.
    let mut threads: Vec<ThreadView> = Vec::new();
    for c in found.iter().filter(|c| c.parent_id.is_none()) {
        threads.push(ThreadView {
            comment: c.into(),
            replies: found
                .iter()
                .filter(|r| r.parent_id.as_deref() == Some(c.id.as_str()))
                .map(CommentView::from)
                .collect(),
        });
    }
    Ok(Json(ListResponse { threads }))
}

fn decode_anchor(a: AnchorView) -> Result<CommentAnchor, ApiError> {
    let bad = || ApiError::Invalid("the anchor is not a valid pair of positions".into());
    let start = STANDARD.decode(&a.start).map_err(|_| bad())?;
    let end = STANDARD.decode(&a.end).map_err(|_| bad())?;
    // Decodes or not; whether the passage can be placed is judged later, and an anchor that
    // decodes but cannot be judged is accepted (it is assumed to survive).
    let (start, end) = gw_collab::anchor::anchor_from_relative(&start, &end).ok_or_else(bad)?;
    if a.quote.chars().count() > MAX_QUOTE_CHARS {
        return Err(ApiError::Invalid(format!(
            "an anchor quote is at most {MAX_QUOTE_CHARS} characters"
        )));
    }
    Ok(CommentAnchor {
        start,
        end,
        quote: a.quote,
    })
}

async fn create(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(path): Path<String>,
    Json(request): Json<NewCommentBody>,
) -> Result<(StatusCode, Json<CommentView>), ApiError> {
    let principal = state.principal(&jar).await;
    if !principal.is_authenticated() {
        return Err(ApiError::Unauthorized);
    }
    let full = full(&path);

    // Validation that depends on the request alone comes AFTER the page check below, so a
    // caller who cannot read the page gets the 404 whatever they sent.
    let readable = state
        .store
        .document_for(&principal, &full, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .is_some();
    if !readable {
        return Err(withheld_or_absent(&state, &principal, &full).await);
    }
    let len = request.body.chars().count();
    if request.body.trim().is_empty() || len > MAX_BODY_CHARS {
        return Err(ApiError::Invalid(format!(
            "a comment must be 1 to {MAX_BODY_CHARS} characters"
        )));
    }
    let anchor = request.anchor.map(decode_anchor).transpose()?;
    let created = state
        .store
        .create_comment(
            &principal,
            &full,
            NewComment {
                body: request.body,
                parent_id: request.parent_id,
                anchor,
            },
        )
        .await
        .map_err(ApiError::Internal)?;
    match created {
        Some(comment) => Ok((StatusCode::CREATED, Json(CommentView::from(&comment)))),
        // The page was readable a moment ago, so the store's `None` is the parent.
        None => Err(ApiError::Invalid(
            "parent_id is not a top-level comment on this page".into(),
        )),
    }
}
