//! Topics over HTTP: which topics exist, what is filed under one, and what a page is about.
//!
//! # This module makes no permission decision, and that is the whole design
//!
//! `gw_store::topics` already decides every one of them, and it decides them per document
//! through [`gw_store::Store::document_for`] — the crate's one permission-checked accessor.
//! Seeing a page in a listing follows Read on that page; re-filing one follows Write. All of
//! it is mutation-tested in `gw-store`.
//!
//! So the handlers below do three things and nothing else: turn a request into the store's
//! own vocabulary, turn the store's answer into a status code, and drop the internal
//! identifiers on the way out. A second filter applied to a list here would be a second place
//! for the property to be wrong, and the one in the handler is always the one that gets it
//! wrong.
//!
//! # A topic is a disclosure surface, and its name is half of it
//!
//! D-4 made topics invisible in the graph and named the consequence: a topic page listing its
//! documents is the ONLY way topics are reachable. So this is an aggregate view of exactly
//! the kind the design's Security section is about — every row says a page exists and what it
//! is called — and it carries a second leak a board does not have. **A topic's own name says
//! something.** `Kündigung Mietvertrag`, on nothing but pages one person may read, tells
//! anybody who sees it in an index that such a page exists and roughly what it says.
//!
//! The rule, decided in `docs/decisions/0011-what-a-topic-discloses.md` and implemented in
//! the store: a topic exists, for a given caller, exactly when they may read at least one
//! document under it or under a topic inside it. This layer's job is not to re-apply that but
//! to **not undo it**, and there are exactly two ways it could:
//!
//! * **By adding to the answer.** A total, a count of what was omitted, an id for a topic
//!   that was filtered out — each says that something is there.
//!   `the_index_carries_no_field_that_could_count_what_it_hid` asserts that structurally, on
//!   the keys, because a field that cannot exist cannot be wrong later. `documents` is not of
//!   that family: it is the length of the very list this caller would be handed, computed
//!   from the same filtered set, and it is why `TopicSummary` carries it rather than this
//!   handler counting anything.
//! * **By the status code.** A topic the caller may see no page of answers **404**, the same
//!   as a topic nobody ever typed. A 403 would say it exists, which is precisely the fact
//!   being kept back — and that is a deliberate departure from [`super::docs::get_document`],
//!   which separates the two because a *page* being at a path is not itself the secret.
//!
//! # Its own prefix, not a suffix under `/api/documents`
//!
//! For the reason `crates/gw-api/src/routes/collab.rs` gives: matchit prefers a literal
//! segment over `{*path}`, so `/api/documents/{*path}/topics` would be shadowed by a real
//! page whose slug happens to be `topics`. The catch-all goes last instead, exactly as
//! `/api/collab/{*path}` and `/api/tasks/document/{*path}` already do.
//!
//! # `topics` here, `tags:` in a file
//!
//! The endpoint, the request and the response all say **topic**, which is what D-4 calls the
//! thing. The frontmatter key is `tags:`, which is what the design's data model calls the
//! table and what `gw_core::SeedMeta` reserved before anything read it — and it is the word
//! every other wiki puts in frontmatter, so it is the one somebody guesses. Both are stated
//! rather than reconciled, because renaming either would break something real: the file
//! format, or the vocabulary of the feature.

use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::Principal;
use gw_store::{Topic, TopicListing, TopicOutcome, TopicSummary};
use serde::{Deserialize, Serialize};

/// One topic on the wire.
///
/// This is `gw_store::Topic` verbatim — it already carries no id, for the reason
/// [`super::links::BacklinkView`] drops one: `path` identifies a topic just as uniquely, is
/// what a URL has to spell, and an internal uuid on the wire is a table the client would
/// have to keep. It is redeclared here all the same, because a field added to the store's
/// row type must not appear on the API by itself.
#[derive(Debug, Serialize)]
pub struct TopicView {
    /// The canonical key: `/medizin/darm`. What `/api/topics/tagged/…` takes.
    pub path: String,
    /// The leaf as somebody typed it: `Darm`.
    pub name: String,
    /// The whole ancestry as somebody typed it: `Medizin/Darm`. What a file states.
    pub display_path: String,
}

impl From<Topic> for TopicView {
    fn from(topic: Topic) -> Self {
        Self {
            path: topic.path,
            name: topic.name,
            display_path: topic.display_path,
        }
    }
}

/// A topic in a listing, with how many documents are under it.
///
/// Flattened rather than nested, so a row is one object with four keys and an interface does
/// not have to reach through a wrapper to render a chip.
#[derive(Debug, Serialize)]
pub struct TopicSummaryView {
    #[serde(flatten)]
    pub topic: TopicView,
    /// **Documents the caller may read**, here and in every topic inside this one.
    ///
    /// The length of the list `/api/topics/tagged/{path}` would hand *this* caller, taken
    /// from the same filtered set rather than counted beside it. It says nothing about what
    /// the filter removed — see the module header.
    pub documents: usize,
}

impl From<TopicSummary> for TopicSummaryView {
    fn from(summary: TopicSummary) -> Self {
        Self {
            topic: summary.topic.into(),
            documents: summary.documents,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct TopicsResponse {
    pub topics: Vec<TopicSummaryView>,
}

/// One page under a topic: somewhere to go, and something to call it.
#[derive(Debug, Serialize)]
pub struct TopicDocumentView {
    pub path: String,
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct TopicPageResponse {
    pub topic: TopicView,
    /// Every page the caller may read under this topic **or any topic inside it**.
    pub documents: Vec<TopicDocumentView>,
    /// The topics directly inside this one that the caller may see.
    pub children: Vec<TopicSummaryView>,
}

impl From<TopicListing> for TopicPageResponse {
    fn from(listing: TopicListing) -> Self {
        Self {
            topic: listing.topic.into(),
            documents: listing
                .documents
                .into_iter()
                .map(|document| TopicDocumentView {
                    path: document.path,
                    title: document.title,
                })
                .collect(),
            children: listing.children.into_iter().map(Into::into).collect(),
        }
    }
}

/// What one page is about.
#[derive(Debug, Serialize)]
pub struct DocumentTopicsResponse {
    pub topics: Vec<TopicView>,
}

/// The whole set a page is to be filed under.
///
/// Whole, not a change: `PUT`, because "these are the topics" is what a frontmatter line says
/// and what a file drop has to be able to mean. A `PATCH` that added one would make a topic
/// impossible to remove by editing the file that put it there, and would need a second verb
/// for removing — two ways to say one thing.
#[derive(Debug, Deserialize)]
pub struct SetTopics {
    /// Topics as somebody types them — `Medizin/Darm`, or `/medizin/darm`. An empty list
    /// files the page under nothing, which is how a page is un-filed.
    pub topics: Vec<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/topics", get(list_topics))
        // Two literal segments beside each other, each followed by its own catch-all, so
        // matchit never has to choose between them — see the module header.
        .route("/api/topics/tagged/{*path}", get(topic_page))
        .route(
            "/api/topics/document/{*path}",
            get(document_topics).put(set_document_topics),
        )
}

/// Paths are stored with a leading slash; a route captures without one.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// Every topic the caller may see — the index, and the list an interface offers so people
/// reuse a topic rather than re-inventing it.
///
/// **No 403 and no 404, and both absences are deliberate.** This endpoint asks about no
/// particular topic, so there is no name whose existence a status code could confirm: every
/// caller gets 200 and the index they are entitled to, which for somebody entitled to nothing
/// is `{"topics": []}`. That is the same answer a wiki with no topics at all gives, and the
/// conflation is the point. `super::links::get_graph` makes the same choice for the same
/// reason.
pub async fn list_topics(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<TopicsResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let topics = state
        .store
        .topics_for(&principal)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(Into::into)
        .collect();
    Ok(Json(TopicsResponse { topics }))
}

/// One topic: the pages under it and the topics inside it.
///
/// **404 for a topic the caller may see no page of, and for one nobody ever typed alike.**
/// The store answers `None` to both, deliberately, and this handler does not try to tell them
/// apart: a 403 would say the topic exists, and a topic's name is exactly what is being kept
/// back. See ADR 0011.
pub async fn topic_page(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<TopicPageResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    state
        .store
        .topic_for(&principal, &full_path(&captured))
        .await
        .map_err(ApiError::Internal)?
        .map(|listing| Json(listing.into()))
        .ok_or(ApiError::NotFound)
}

/// What one page is about.
///
/// Existence before permission, exactly as `super::docs::get_document` and
/// `super::links::get_backlinks` do it: an absent path is 404 and a forbidden one is 403.
/// That is the right way round *here* — unlike [`topic_page`] above — because the fact in
/// question is a page's, and a page's presence at a path is not what its grants are hiding;
/// collapsing the two would either hide a typo behind "you spelled it wrong" or confirm the
/// existence of every path somebody guesses.
pub async fn document_topics(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<DocumentTopicsResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);
    let topics = read_topics(&state, &principal, &path).await?;
    Ok(Json(DocumentTopicsResponse { topics }))
}

/// Replace what a page is about. Needs **Write** on it.
///
/// The order is the store's decision first: `set_document_topics` is attempted, and only if
/// it refuses does this handler ask a second, read-only question — and only to choose between
/// 403 and 404. Asking first and writing second would be two authorisation decisions on one
/// action, and the second is always the one that gets it wrong. A refused write changes
/// nothing, so asking afterwards is free.
///
/// Nothing here re-checks view-as (D-M2-17): `crate::view_as::enforce` refuses every non-GET
/// request while an administrator is viewing as somebody else, and this is a PUT.
pub async fn set_document_topics(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
    Json(body): Json<SetTopics>,
) -> Result<Json<DocumentTopicsResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    match state
        .store
        .set_document_topics(&principal, &path, &body.topics)
        .await
        .map_err(ApiError::Internal)?
    {
        TopicOutcome::Done(topics) => Ok(Json(DocumentTopicsResponse {
            topics: topics.into_iter().map(Into::into).collect(),
        })),
        // The reason names what was rejected and why, because a refusal nobody can act on is
        // not a refusal — the same standard `super::tasks` holds a bad status to.
        TopicOutcome::Rejected(reason) => Err(ApiError::Invalid(reason)),
        // 404 for an absent page, 403 for one this caller may not write. The store conflates
        // them; this is where the two are told apart, and it costs one read that has already
        // been decided.
        TopicOutcome::Refused => {
            if !state
                .store
                .document_exists(&path)
                .await
                .map_err(ApiError::Internal)?
            {
                return Err(ApiError::NotFound);
            }
            Err(ApiError::Forbidden)
        }
    }
}

/// A page's topics, or the status code that says why not.
async fn read_topics(
    state: &AppState,
    principal: &Principal,
    path: &str,
) -> Result<Vec<TopicView>, ApiError> {
    if !state
        .store
        .document_exists(path)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }
    // `document_topics_for` puts the page through the accessor itself and answers `None`
    // when it refuses; asking `document_for` here as well would be a second decision about
    // the same page. The existence check above is not that — it decides a status code, not
    // an outcome.
    state
        .store
        .document_topics_for(principal, path)
        .await
        .map_err(ApiError::Internal)?
        .map(|topics| topics.into_iter().map(Into::into).collect())
        .ok_or(ApiError::Forbidden)
}
