//! History over HTTP: the timeline of a page, the three diffs between two of its versions,
//! the whole source of one version, and restoring one.
//!
//! # A history is a disclosure surface, and it is treated as one
//!
//! It is tempting to think of a revision list as metadata — ids and timestamps, nothing
//! anybody would mind. It is not. It says the page exists, who works on it, when they were
//! last at it and what they said they were doing; the diff and source endpoints then hand
//! back every word the page has ever held, including the sentence somebody deleted a minute
//! after publishing it. That is *more* than reading the page, not less.
//!
//! So every handler here resolves its document through [`gw_store::Store::document_for`] —
//! the store's one permission-checked accessor, the same one `docs::get_document`,
//! `links::get_backlinks` and `collab::authorise` use — and the store's revision accessors
//! ([`gw_store::Store::revisions_for`], [`gw_store::Store::revision_for`],
//! [`gw_store::Store::restore_revision`]) each ask it again on their own behalf. There is no
//! unfiltered variant reachable from this file: the crate does have one,
//! `revision_unchecked`, and it is deliberately private to `gw-store`.
//!
//! **Reading history follows read; restoring follows write** (D-M3-5). Anybody who may read
//! a page may read how it got that way, and restoring changes what the page says, so it is
//! an edit like any other. The write decision is taken once, inside
//! [`gw_store::Store::restore_revision`] → [`gw_store::Store::publish_revision`]; this file
//! never takes a second one, it only turns a refusal into a status code.
//!
//! **Restoring appends.** It publishes the old content as a NEW revision and rewinds
//! nothing, so the version somebody was unsure about is still there afterwards and the
//! restore is itself undoable. That is the store's property, not this module's; the test
//! `restoring_appends_a_revision_and_never_rewinds_history` in `tests/revisions.rs` is what
//! stops the endpoint growing a "clean up the history" option later.
//!
//! # Two answers, and which questions get which
//!
//! For a **path** — the timeline — an absent page is 404 and a forbidden one is 403, exactly
//! as `/api/documents` and `/api/links/backlinks` split them: collapsing both into 404 hides
//! configuration mistakes, and collapsing both into 403 confirms the existence of every path
//! somebody guesses.
//!
//! For a **revision id** everything unreachable is 404, and the conflation is deliberate. An
//! id is a uuid nobody guesses, so there is no existence to protect; distinguishing "no such
//! revision" from "not yours" would create an oracle for nothing in return. The one
//! exception is restore, which answers 403 when the caller may read the revision but not
//! write the page — there the refusal is about the caller's rights on a page they can
//! already see, and saying so is what tells them to ask for access rather than reload.
//!
//! # DEVIATION from the M3 plan
//!
//! The plan names `GET /api/documents/{id}/revisions`. It is served here as
//! `GET /api/revisions/document/{*path}` instead, for the reason `collab.rs` already
//! records against `POST /api/documents/{id}/publish`: matchit prefers a literal segment
//! over `{*path}`, so `/api/documents/{id}/revisions` shadows the existing
//! `GET /api/documents/{*path}` for any top-level page whose slug happens to be
//! `revisions` — that page would answer this handler's 404 instead of its own content, and
//! the wiki would have a page nobody could read with nothing to say why. Putting the
//! catch-all last, under this module's own prefix, removes the class of bug rather than
//! betting on nobody naming a page that.
//!
//! Keying the timeline by **path** rather than by document id is the second half of the same
//! decision, and it is the one `collab.rs` makes too: the client already has the path (it is
//! in the address bar), a path is resolved by the permission-checked accessor anyway, and an
//! id supplied by the client would have to be turned back into a path to be authorised.

use super::AppState;
use crate::error::ApiError;
use crate::export::{self, FileMeta};
use axum::extract::{Path, Query, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::{Action, Principal};
use gw_core::{
    diff_design, diff_prose, diff_structure, split_frontmatter, Block, DesignChange, ProseChange,
    StructureChange,
};
use gw_store::Revision;
use serde::Serialize;

/// One entry in the timeline.
///
/// Deliberately not `gw_store::Revision` reused directly, for two reasons that pull the same
/// way as [`super::links::BacklinkView`]'s. It carries the **body**, and a page with
/// thirty-four revisions would put thirty-four copies of itself on the wire for a list that
/// shows none of them; and it carries `author_id` and `document_id`, which are internal
/// identifiers with no reason to leave this crate. What a reader needs to recognise a
/// version is who, when, how big and what they said they were doing.
#[derive(Debug, Clone, Serialize)]
pub struct RevisionView {
    pub id: String,
    /// What this was published on top of. `null` for the first revision of a page, which is
    /// how the frontend knows where the chain ends without counting.
    pub parent_id: Option<String>,
    pub summary: Option<String>,
    /// The display name as it was when this was published — never resolved through the
    /// account now, so history stays attributable after somebody leaves (D-M3-4).
    pub author_name: String,
    /// Whether a person wrote this, as against the import that ran with no account. What an
    /// interface must ask before writing "von …" or linking to a profile.
    pub author_is_account: bool,
    pub byte_size: i64,
    pub created_at: String,
}

impl From<&Revision> for RevisionView {
    fn from(revision: &Revision) -> Self {
        Self {
            id: revision.id.clone(),
            parent_id: revision.parent_id.clone(),
            summary: revision.summary.clone(),
            author_name: revision.author_name.clone(),
            author_is_account: revision.author_is_an_account(),
            byte_size: revision.byte_size,
            created_at: revision.created_at.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RevisionsResponse {
    pub revisions: Vec<RevisionView>,
}

/// What changed between two versions, asked three ways.
///
/// All three lists are present on every answer, including when they are empty, and an empty
/// one is a real statement: "the words did not change" is exactly what a reader needs to be
/// told when the design diff below it is full.
#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub from: RevisionView,
    pub to: RevisionView,
    pub prose: Vec<ProseChange>,
    pub structure: Vec<StructureChange>,
    pub design: Vec<DesignChange>,
}

/// One version as a file: the export triple.
///
/// `markdown` and `meta` come from [`crate::export::render_file`], which is the same
/// function `great-wiki export` writes files with — including its round-trip check, so this
/// is the file the exporter would produce or the reason it would refuse to. `design` is the
/// block tree itself, which is what the database actually holds and the only one of the
/// three that can never lose anything.
///
/// **Why this endpoint asks for the page's path as well as the revision's id.** Building the
/// frontmatter needs the document's title, type, visibility, language, slug and sort key,
/// and `gw-store` exposes exactly one permission-checked document accessor —
/// [`gw_store::Store::document_for`], which is keyed by **path**. There is no
/// `document_by_id_for`, and inventing one is `gw-store`'s decision to take, not this
/// crate's. So the caller names the page it is asking about, the path is resolved as this
/// principal like any page read, and the revision must belong to the document that comes
/// back or the answer is 404. Nothing is authorised by the id, in either direction: the path
/// decides what may be read, and the id must agree with it.
///
/// **The metadata is the page's, the body is the revision's.** A revision stores a body and
/// nothing else — title, visibility and slug live on the document and are not versioned —
/// so `meta` describes the page as it is now. Saying so here is cheaper than a reader
/// working it out from a restored page whose title did not change back.
#[derive(Debug, Serialize)]
pub struct SourceResponse {
    pub revision: RevisionView,
    /// The body as markdown, or `null` when this tree cannot be written as markdown
    /// faithfully — an image, an unresolvable internal link, a horizontal rule.
    pub markdown: Option<String>,
    /// Why `markdown` is `null`, in the exporter's own words. `null` when it is not.
    pub problem: Option<String>,
    /// The frontmatter block, as `<slug>.meta.yml` would hold it.
    pub meta: Option<String>,
    /// The block tree, as `<slug>.design.json` would hold it: pretty-printed, and complete.
    pub design: String,
}

/// The answer to a restore: the revision that was just written.
///
/// Named rather than empty so the caller can see the restore in the timeline it is about to
/// reload, and so that "restored" is a thing that happened at a particular id rather than a
/// status code somebody has to trust.
#[derive(Debug, Serialize)]
pub struct RestoreResponse {
    pub revision: RevisionView,
}

/// `?path=/rundgang/tabellen` — which page the revision is expected to belong to.
///
/// See [`SourceResponse`] for why an id alone is not enough.
#[derive(Debug, serde::Deserialize)]
pub struct PathQuery {
    pub path: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        // The catch-all comes LAST in the pattern, which is what keeps it from shadowing
        // anything — see the DEVIATION note at the top of this file.
        .route("/api/revisions/document/{*path}", get(list))
        .route("/api/revisions/{from}/diff/{to}", get(diff))
        .route("/api/revisions/{id}/source", get(source))
        .route("/api/revisions/{id}/restore", post(restore))
}

/// Paths are stored with a leading slash; the route captures without one.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

fn parse_body(revision: &Revision) -> Result<Block, ApiError> {
    serde_json::from_str(&revision.body).map_err(|e| {
        ApiError::Internal(anyhow::anyhow!(
            "revision {} does not hold a block tree: {e}",
            revision.id
        ))
    })
}

/// The history of the page at `path`, newest first.
///
/// Existence before permission, so an absent page is 404 and a forbidden one is 403 — the
/// same split `docs::get_document` and `links::get_backlinks` make, for the same reason.
pub async fn list(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<RevisionsResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&captured);

    if !state
        .store
        .document_exists(&path)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }

    // The one permission-checked accessor. `revisions_for` below asks it again through
    // `may()`, which is not redundancy to be tidied away: this call decides the status code,
    // that one decides what is disclosed, and the property belongs to the store.
    let document = state
        .store
        .document_for(&principal, &path, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    let revisions = state
        .store
        .revisions_for(&principal, &document.id)
        .await
        .map_err(ApiError::Internal)?
        .iter()
        .map(RevisionView::from)
        .collect();

    Ok(Json(RevisionsResponse { revisions }))
}

/// One revision the caller may read, or 404 — see the module comment on why there is no 403.
///
/// Takes the principal rather than resolving one, so a handler that needs two revisions
/// resolves the session once. Resolving it per lookup would read the session table twice for
/// one diff and, worse, would leave two answers where the request should have exactly one.
async fn readable(state: &AppState, principal: &Principal, id: &str) -> Result<Revision, ApiError> {
    state
        .store
        .revision_for(principal, id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::NotFound)
}

/// What changed between two versions, in all three modes.
///
/// Both revisions are fetched through the permission-checked accessor before anything is
/// compared, so a diff can never be computed — let alone returned — across a page the caller
/// may not read.
pub async fn diff(
    State(state): State<AppState>,
    jar: CookieJar,
    Path((from, to)): Path<(String, String)>,
) -> Result<Json<DiffResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let old = readable(&state, &principal, &from).await?;
    let new = readable(&state, &principal, &to).await?;

    // Two pages' histories are not comparable, and letting them be compared would be a way
    // to read one page's content against another's for no purpose anybody has.
    if old.document_id != new.document_id {
        return Err(ApiError::Invalid(
            "those two revisions belong to different documents".into(),
        ));
    }

    let (a, b) = (parse_body(&old)?, parse_body(&new)?);
    Ok(Json(DiffResponse {
        from: RevisionView::from(&old),
        to: RevisionView::from(&new),
        prose: diff_prose(&a, &b),
        structure: diff_structure(&a, &b),
        design: diff_design(&a, &b),
    }))
}

/// One whole version, as the three files an export would write.
pub async fn source(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Query(query): Query<PathQuery>,
) -> Result<Json<SourceResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&query.path);

    if !state
        .store
        .document_exists(&path)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }
    let document = state
        .store
        .document_for(&principal, &path, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    let revision = readable(&state, &principal, &id).await?;
    // The two halves of the request have to be about the same page. Without this, a
    // readable page's metadata would be stapled to another page's body — which is only ever
    // a caller's mistake, but "the file you were shown was two pages" is not an answer a
    // history may give.
    if revision.document_id != document.id {
        return Err(ApiError::NotFound);
    }
    let body = parse_body(&revision)?;

    let meta = FileMeta {
        title: document.title.clone(),
        doc_type: document.doc_type.clone(),
        visibility: document.visibility.clone(),
        language: document.language.clone(),
        sort_key: document.sort_key,
        slug: document.slug.clone(),
    };

    // `render_file` refuses a tree markdown cannot hold rather than writing a lossy file.
    // That refusal is reported here as `problem` instead of failing the request: the design
    // JSON below is complete whatever markdown can do, and a version that cannot be exported
    // is exactly the version somebody most needs to look at.
    let (markdown, meta_yaml, problem) = match export::render_file(&meta, &body) {
        Ok(file) => {
            let (yaml, markdown) = split_frontmatter(&file);
            (Some(markdown.to_string()), yaml.map(str::to_string), None)
        }
        Err(reason) => (None, None, Some(reason)),
    };

    Ok(Json(SourceResponse {
        revision: RevisionView::from(&revision),
        markdown,
        problem,
        meta: meta_yaml,
        // Pretty-printed because this is the tab somebody opens to READ it. A block tree
        // always serialises, so the fallback is unreachable and is written rather than
        // unwrapped anyway: a panic in a handler is a 500 with no body at all.
        design: serde_json::to_string_pretty(&body)
            .unwrap_or_else(|_| "{}\n// dieser Baum ließ sich nicht darstellen".into()),
    }))
}

/// Publish an old version again, as a new revision.
///
/// The order here is the whole design. The restore is attempted FIRST, through the store,
/// which is where the single write decision lives; only if it refuses does this handler ask
/// a second, read-only question — and only to choose between 403 and 404. Checking
/// permission here first and then restoring would be two authorisation decisions on one
/// action, and the second one is always the one that gets it wrong.
///
/// A refused restore writes nothing at all, so asking afterwards is free.
///
/// Nothing here re-checks view-as (D-M2-17): `crate::view_as::enforce` refuses every non-GET
/// request while an administrator is viewing as somebody else, and this is a POST. That is
/// the layer that cannot be forgotten, which is why the check is not repeated in the handler
/// where it could be.
pub async fn restore(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<RestoreResponse>, ApiError> {
    let principal = state.principal(&jar).await;

    let restored = state
        .store
        .restore_revision(&principal, &id)
        .await
        .map_err(ApiError::Internal)?;

    let Some(new_id) = restored else {
        // Refused. If the caller can read the revision, the refusal was about writing the
        // page — say so, because "ask for access" and "check the address" send somebody to
        // different places. Otherwise it is the same 404 every unreachable id gets.
        return Err(
            match state
                .store
                .revision_for(&principal, &id)
                .await
                .map_err(ApiError::Internal)?
            {
                Some(_) => ApiError::Forbidden,
                None => ApiError::NotFound,
            },
        );
    };

    let revision = state
        .store
        .revision_for(&principal, &new_id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "the revision just written is not readable by its own author"
            ))
        })?;

    Ok(Json(RestoreResponse {
        revision: RevisionView::from(&revision),
    }))
}
