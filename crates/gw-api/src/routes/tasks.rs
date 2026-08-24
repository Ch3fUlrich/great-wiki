//! Tasks, boards and projects over HTTP: a board to read, a card to create, move or throw
//! away, and the projects those boards belong to.
//!
//! # This module makes no permission decision, and that is the whole design
//!
//! `gw_store::tasks` already decides every one of them, at one choke point: a task is
//! governed by exactly one page — its anchor document, or its project's home page — and
//! `governing_document` resolves that page through [`gw_store::Store::document_for`], the
//! crate's one permission-checked accessor. Seeing a card follows Read on that page,
//! changing one follows Write, and being *given* one follows Read (D-10). All of it is
//! mutation-tested in `gw-store`.
//!
//! So the handlers below do three things and nothing else: turn a request into the store's
//! own vocabulary, turn the store's answer into a status code, and drop the internal
//! identifiers on the way out. A permission check written here would be a second answer that
//! can disagree with the one that actually decides — and the one in the handler is always
//! the one that gets it wrong.
//!
//! # A board is a disclosure surface
//!
//! The design's Security section is explicit about it, and a board is worse than a backlinks
//! panel: a card reveals that a page exists, what it is called and — because a card's title
//! is a copy of the page's own words (D-2) — what somebody wrote on it. D-3 makes a project
//! span pages with different grants *by design*, so the filtering is per document and the
//! natural thing to write ("trust the subtree, it is the project") is exactly the bug.
//!
//! An anchored card names the page it was written on — its path and its title, so a board
//! can link to it — and that name is *itself* the disclosure the filtering is about. It is
//! not looked up here: `gw_store::Task` carries it, resolved from the same document the
//! store's permission-checked accessor answered with. A lookup in this handler would be
//! the second answer the paragraph above rules out, and it would be tempting to make it an
//! unchecked one on the grounds that the card had already been filtered.
//!
//! The way this layer loses that property is not by asking the wrong question but by
//! **adding to the answer**. A total, a count of what was omitted, an id for a card that was
//! filtered out, a status code that differs — each says that something is there. So a board
//! response carries the cards it carries and nothing that could be a number about the rest;
//! `the_board_carries_no_field_that_could_count_what_it_hid` in `tests/tasks.rs` asserts
//! that structurally, on the keys, because a field that cannot exist cannot be wrong later.
//!
//! # One board, in two places, from one query
//!
//! D-12 put a board in **both** places — a global one at `/aufgaben` showing every task the
//! caller may see, and one embedded in each project's home page — and named the cost in the
//! same breath: two places that must agree. They agree because there is only one of them.
//! `GET /api/board` and `GET /api/projects/{id}/board` answer the same [`BoardResponse`] out
//! of the same [`gw_store::Store::board_for`], whose project binding is an `Option`; the
//! project board is that call with a project bound and nothing else. There is no second
//! board query to disagree with the first, and since every card is a disclosure surface,
//! there is no second query to leak from either.
//!
//! `project` is therefore `null` on an unbound board and set on a bound one — one shape, so
//! one component renders both.
//!
//! # Two answers, and which questions get which
//!
//! For a **path** — `POST /api/projects`, `GET /api/tasks/document/…`, `GET /api/board?seite=`
//! — an absent page is 404 and a refused one is 403, exactly as `/api/documents`,
//! `/api/links/backlinks` and `/api/revisions/document` split them. Collapsing both into 404
//! hides configuration mistakes; collapsing both into 403 confirms the existence of every
//! path somebody guesses.
//!
//! For a **project or task id** — `GET /api/projects/{id}/board`, `GET /api/board?projekt=`
//! — everything unreachable is 404 — an id is a uuid nobody guesses, so there is no
//! existence to protect — *except* when the caller may read the governing page but not write
//! it. There the refusal is about their rights on something they can already see, and saying
//! 403 is what sends them to ask for access rather than to check the address. That is
//! `revisions::restore`'s split, and it is taken the same way: the store is asked to do the
//! thing first, and only a refusal asks the second, read-only question. A refused change
//! writes nothing, so asking afterwards is free.
//!
//! **`GET /api/board` takes one of each, so the split lives on the parameter rather than on
//! the route.** `?seite=` is path-keyed and gets 404/403; `?projekt=` is id-keyed and gets
//! 404 for everything. That is not an inconsistency to be tidied away later: the two
//! parameters are asking about two different kinds of thing, and a path is guessable where a
//! uuid is not.
//!
//! # DEVIATION from the plan, and it is the trap this repository has hit twice
//!
//! The obvious shape is `GET /api/documents/{id}/tasks`. It cannot be used: matchit prefers
//! a literal segment over `{*path}`, so that route **shadows** the existing
//! `GET /api/documents/{*path}` for any page whose slug is `tasks` — that page would answer
//! this module's 404 instead of its own content, and the wiki would have a page nobody could
//! read with nothing to say why. `collab.rs` records it against `POST /api/documents/{id}/publish`
//! and `revisions.rs` records it again. Everything here is keyed under its own prefix with
//! the catch-all last, and `a_page_named_after_one_of_these_routes_is_still_readable` is what
//! keeps it that way.
//!
//! # What is deliberately not here
//!
//! - **Creating an *anchored* task.** A checkbox line is a task (D-6) and the record for one
//!   is minted by reconciliation on publish, from the words in the page. An HTTP call that
//!   made an anchored record would put a card on a board saying words the page does not
//!   contain, which is precisely what D-2 forbids. Cards are created on a board; lines are
//!   written in a page.
//! - **Moving a card between projects.** The store supports it and asks both boards for
//!   Write, but it answers `TaskOutcome::Refused` both for "you may not write this card" and
//!   for "an anchored card's project follows its page" (D-3) — and this layer cannot tell
//!   those apart without inventing a second answer to a question the store already answers.
//!   Left out until the store distinguishes them.
//! - **A single-card read.** Every card the caller may see is already on a board or on its
//!   page, and both endpoints return the whole card, so a third way to fetch one adds
//!   surface and answers nothing new.

use super::AppState;
use crate::error::ApiError;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use axum_extra::extract::CookieJar;
use gw_auth::{Action, Principal};
use gw_store::{NewTask, Project, Task, TaskHome, TaskOutcome, TaskPage, TaskStatus, TaskUpdate};
use serde::{Deserialize, Deserializer, Serialize};

// --- what goes on the wire ---------------------------------------------------------------

/// The page a card's line was written on: somewhere to link to, and something to call it.
///
/// Declared here rather than serialising `gw_store::TaskPage` straight out, for the reason
/// [`ProjectView`] is: the wire shape is this crate's to decide, and a field added to the
/// store's type must not appear on the API by itself. The *values* are the store's — that
/// type can only be built from a document its permission-checked accessor answered with.
#[derive(Debug, Clone, Serialize)]
pub struct PageView {
    pub path: String,
    pub title: String,
}

impl From<&TaskPage> for PageView {
    fn from(page: &TaskPage) -> Self {
        Self {
            path: page.path.clone(),
            title: page.title.clone(),
        }
    }
}

/// One card.
///
/// Deliberately not `gw_store::Task` reused directly, for the reason
/// [`super::links::BacklinkView`] gives: that type carries `doc_id`, `block_id` and
/// `project_id`, which are internal identifiers with no reason to leave this crate. A board
/// needs to know whether a card came from a page, not which page's row it is.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub id: String,
    /// The words. For an anchored card these are the page's own (D-2) and the page is where
    /// they are edited; the next publish takes them from the block again.
    pub title: String,
    pub status: TaskStatus,
    /// The principal this card rests on, by id — the same id a change sets it with, so what
    /// is read back is what was written. Resolving it to a display name is left to whoever
    /// needs one: it would be a lookup per card and a name-to-id mapping nothing asked for.
    pub assignee: Option<String>,
    pub due_at: Option<String>,
    pub position: i64,
    /// Whether this card was written as a line in a page, as against created on the board.
    pub anchored: bool,
    /// The page that line was written on, or `null` for a card that lives in no page.
    ///
    /// Never `null` for an anchored card the caller is being shown: a card is on this board
    /// only because the store resolved its page through the accessor, and that document is
    /// where this comes from. "There is a page here you may not see" is not a state this
    /// can express, which is deliberate — it would be the disclosure with the name filed
    /// off.
    pub page: Option<PageView>,
    /// D-8: the page no longer mentions the line that authored this card. Carried rather
    /// than hidden, because a card that looks live but is written nowhere is worse than one
    /// that says so.
    pub detached: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl From<&Task> for TaskView {
    fn from(task: &Task) -> Self {
        Self {
            id: task.id.clone(),
            title: task.title.clone(),
            status: task.status,
            assignee: task.assignee.clone(),
            due_at: task.due_at.clone(),
            position: task.position,
            anchored: task.doc_id.is_some(),
            page: task.page.as_ref().map(PageView::from),
            detached: task.detached,
            created_at: task.created_at.clone(),
            updated_at: task.updated_at.clone(),
        }
    }
}

/// One project. `home_doc` is dropped for the same reason a card's `doc_id` is; the path and
/// the title are what an interface links to and shows.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectView {
    pub id: String,
    pub home_path: String,
    pub home_title: String,
    pub tag_id: Option<String>,
    pub created_at: String,
}

impl From<Project> for ProjectView {
    fn from(project: Project) -> Self {
        Self {
            id: project.id,
            home_path: project.home_path,
            home_title: project.home_title,
            tag_id: project.tag_id,
            created_at: project.created_at,
        }
    }
}

/// One of D-9's three columns.
#[derive(Debug, Serialize)]
pub struct ColumnView {
    pub status: TaskStatus,
    pub tasks: Vec<TaskView>,
}

/// A board: the project it belongs to, and its three columns.
///
/// **Two fields, and there is deliberately no third.** Anything that counted — a total, an
/// `omitted`, a "3 cards" badge — would be a number about what the caller was not shown.
///
/// `project` is `null` for the **unbound** global board, which belongs to no project, and
/// for a page that is nobody's home. It is a field that is always present and sometimes
/// empty rather than a field that comes and goes: a key set that varied with the answer
/// would be a shape a client has to branch on, and the structural test that pins these two
/// fields could no longer say what it says.
#[derive(Debug, Serialize)]
pub struct BoardResponse {
    pub project: Option<ProjectView>,
    pub columns: Vec<ColumnView>,
}

#[derive(Debug, Serialize)]
pub struct ProjectsResponse {
    pub projects: Vec<ProjectView>,
}

#[derive(Debug, Serialize)]
pub struct TasksResponse {
    pub tasks: Vec<TaskView>,
}

/// Whether the request changed anything.
///
/// The same field name every other mutating endpoint in this crate uses (see
/// `admin::Changed`), so a client can ask the question generically. It is always `true`
/// here: a delete that changed nothing is a 403 or a 404 rather than a success.
#[derive(Debug, Serialize)]
pub struct Changed {
    pub changed: bool,
}

// --- what comes off it -------------------------------------------------------------------

/// A field that can be **absent**, `null`, or a value — three states, which `Option<T>`
/// alone cannot hold on the wire.
///
/// `serde` maps a missing field and an explicit `null` onto the same `None`, so without this
/// a change could not say "clear the due date" as against "leave it alone", and every change
/// of a status would silently unassign the card. [`gw_store::TaskUpdate`] draws exactly the
/// same distinction, for the reason its own comment gives: unassigning is deliberately
/// permitted to somebody the assignment itself would not have been (D-10, clause 4).
fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// What the global board is bound to, off the query string. Nothing, a project, or the page
/// a project is homed on — never two at once.
///
/// German on the wire because these are the words the interface uses: `/aufgaben` is the
/// board and `?projekt=`/`?seite=` are what a person filtering it would type into an address
/// bar. The Rust below stays English, exactly as [`TaskStatus`] does.
#[derive(Debug, Deserialize)]
pub struct BoardQuery {
    #[serde(default)]
    pub projekt: Option<String>,
    /// A page's path. This exists so the board embedded in a project's home page is ONE
    /// request from a page loader that knows only the path it is rendering — without it,
    /// every page would have to fetch the project listing first to find out whether it is a
    /// home page, and an ordinary page would pay for a board it does not have.
    #[serde(default)]
    pub seite: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct NewProject {
    pub home_path: String,
    #[serde(default)]
    pub tag_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectChange {
    /// The tag that pulls in documents from elsewhere (D-3), or `null` for none. Absent is
    /// not "leave it alone" but "you named nothing" — a project has one changeable field, so
    /// a change that omits it is a request with no content.
    #[serde(default, deserialize_with = "present")]
    pub tag_id: Option<Option<String>>,
}

/// A new card on a board.
///
/// `status` arrives as a string rather than as [`TaskStatus`] so that an unknown one is this
/// crate's own 400 — see [`status_from_wire`].
#[derive(Debug, Deserialize)]
pub struct NewCard {
    pub project_id: String,
    pub title: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    pub due_at: Option<String>,
    #[serde(default)]
    pub position: Option<i64>,
}

/// A change to a card. Every field absent means "leave it alone"; `null` on the two that
/// take one means "clear it".
#[derive(Debug, Deserialize)]
pub struct CardChange {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, deserialize_with = "present")]
    pub assignee: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    pub due_at: Option<Option<String>>,
    #[serde(default)]
    pub position: Option<i64>,
}

// --- the refusals that are not permission problems ---------------------------------------

/// D-10, clause 3, refused. Not 403: the caller may do this, and would be allowed to a
/// moment after granting that person read — so it is 409 and it names the way out, exactly
/// as the last-administrator interlock in `admin.rs` does.
///
/// It names no person. Telling somebody who may write the page that the name they picked
/// cannot read it is one fact, and it is the least that can be said while still letting them
/// tell "you may not touch this card" from "the person you picked cannot see it". Saying
/// *who* would be a second one, about somebody who is not part of this request.
const ASSIGNEE_CANNOT_READ: &str =
    "the person you named may not read the page this task is on, so the task cannot rest \
     on them; give them read there first, or leave it unassigned";

/// A page can be the home of only one project — that is what makes "which project is this
/// page the home of" a question with one answer. Checked before the insert so the answer is
/// 409 rather than a UNIQUE violation surfacing as a 500, the same pre-check
/// `admin::create_principal` makes; the constraint in `0010_tasks.sql` is still what
/// actually guarantees it.
const HOME_TAKEN: &str =
    "that page is already the home of a project; open that board, or pick another page";

const NOTHING_TO_CHANGE: &str = "name at least one field to change";

/// Two bindings is a request with two answers. Preferring one of them quietly would hand the
/// caller a board they did not ask for, and — because the two are authorised by different
/// rules — possibly one they were refused under the other.
const ONE_BINDING: &str =
    "name a project or a page, not both: projekt=<id> for a project's board, seite=<pfad> \
     for the board of the project that page is the home of";

/// An untitled card cannot be told from any other untitled card, and nothing but a deletion
/// would ever remove it. The store refuses the same thing one layer down for the same reason
/// — an empty checkbox line is not yet a task.
const NO_WORDS: &str = "a task needs words; title must not be empty";

/// D-9's three, read off the wire.
///
/// Parsed here rather than by `serde` so that an unknown status is a 400 in this crate's own
/// error shape, saying what the three are. Left to the `Json` extractor it would be a 422
/// with a plain-text body about a rejected enum variant, which is neither. The schema's
/// CHECK constraint is what actually guarantees the set; this is what stops a typo reaching
/// it as a 500.
///
/// The list in the message is built from [`TaskStatus::ALL`], so it cannot drift from the
/// set that is actually accepted.
fn status_from_wire(value: &str) -> Result<TaskStatus, ApiError> {
    // NFC, for the one character that matters. `Läuft` composed (U+00E4) and decomposed
    // (`a` + U+0308) look identical and are different bytes, and SQLite compares text byte
    // for byte — so a client sending the decomposed form, which is what a Mac produces,
    // would be refused a status it spelled correctly and shown an error naming what looks
    // like the same word. Only the three names below survive this, so widening what is
    // accepted cannot widen what is stored.
    let composed = value.replace("a\u{308}", "ä");
    TaskStatus::from_stored(&composed).ok_or_else(|| {
        let names: Vec<&str> = TaskStatus::ALL.iter().map(|s| s.as_str()).collect();
        ApiError::Invalid(format!("a task's status is one of {}", names.join(", ")))
    })
}

/// A blank string is not a value. `""` for an assignee or a due date is a client that sent
/// an empty form field, and storing it would put a card in a state nothing can explain — the
/// same trim-and-drop `admin::create_principal` applies to an email.
fn some_text(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", patch(set_tag).delete(remove_project))
        .route("/api/projects/{id}/board", get(board))
        // The global board (D-12). A literal two-segment path, so it shadows nothing: the
        // only catch-all under `/api` is `/api/documents/{*path}`, and a page whose slug is
        // `board` is still served from there.
        .route("/api/board", get(global_board))
        .route("/api/tasks", post(create_task))
        .route("/api/tasks/{id}", patch(change_task).delete(remove_task))
        // The catch-all comes LAST in the pattern, which is what keeps it from shadowing
        // anything — see the DEVIATION note at the top of this file. `{id}` above matches
        // one segment and this matches three or more, so matchit never has to choose.
        .route("/api/tasks/document/{*path}", get(document_tasks))
}

/// Paths are stored with a leading slash; a route captures without one, and a body may carry
/// either.
fn full_path(captured: &str) -> String {
    format!("/{}", captured.trim_start_matches('/'))
}

/// Which refusal a task id earns: 403 if the caller may read the card, 404 otherwise.
///
/// Asked only after the store has already refused, so nothing was written and this costs a
/// read. It is the same read the store just made — this call chooses the status code, that
/// one decided what happens — and never a second rule.
async fn task_refusal(state: &AppState, principal: &Principal, id: &str) -> ApiError {
    match state.store.task_for(principal, id).await {
        Ok(Some(_)) => ApiError::Forbidden,
        Ok(None) => ApiError::NotFound,
        Err(error) => ApiError::Internal(error),
    }
}

/// The same, for a project id.
async fn project_refusal(state: &AppState, principal: &Principal, id: &str) -> ApiError {
    match state.store.project_for(principal, id).await {
        Ok(Some(_)) => ApiError::Forbidden,
        Ok(None) => ApiError::NotFound,
        Err(error) => ApiError::Internal(error),
    }
}

// --- projects -----------------------------------------------------------------------------

/// Every project whose home page the caller may read.
///
/// **No 403 and no 404, and both absences are deliberate** — the same reasoning
/// `links::get_graph` gives. This endpoint asks about no particular page, so there is no
/// existence a status code could confirm: every caller gets 200 and the projects they are
/// entitled to, which for somebody entitled to none is `{"projects": []}`. That is also what
/// an instance with no projects at all answers, and the conflation is the point.
pub async fn list_projects(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<Json<ProjectsResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    let projects = state
        .store
        .projects_for(&principal)
        .await
        .map_err(ApiError::Internal)?
        .into_iter()
        .map(ProjectView::from)
        .collect();
    Ok(Json(ProjectsResponse { projects }))
}

/// Make the page at `home_path` the home of a project (D-3).
///
/// Keyed by path, so existence comes before permission: 404 for a page that is not there,
/// 403 for one the caller may not write. `Store::create_project` asks the same accessor
/// again on its own behalf and is the call that actually decides.
pub async fn create_project(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<NewProject>,
) -> Result<(StatusCode, Json<ProjectView>), ApiError> {
    let principal = state.principal(&jar).await;
    let path = full_path(&body.home_path);

    if !state
        .store
        .document_exists(&path)
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(ApiError::NotFound);
    }
    // Authorised before the conflict is looked for, so that somebody who may not write the
    // page never learns from a 409 whether it is already a project's home.
    let home = state
        .store
        .document_for(&principal, &path, Action::Write)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    if state
        .store
        .projects_for(&principal)
        .await
        .map_err(ApiError::Internal)?
        .iter()
        .any(|project| project.home_path == home.path)
    {
        return Err(ApiError::Conflict(HOME_TAKEN.into()));
    }

    let project = state
        .store
        .create_project(&principal, &path, some_text(body.tag_id).as_deref())
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;
    Ok((StatusCode::CREATED, Json(project.into())))
}

/// Point a project at a different tag, or at none. Needs Write on its home page.
///
/// The home page itself is deliberately not changeable, and that is the store's decision,
/// not this endpoint's omission: a project *is* its home subtree (D-3), so re-homing one is
/// a different project, and doing it silently would move every anchored card on the board at
/// once.
pub async fn set_tag(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<ProjectChange>,
) -> Result<Json<ProjectView>, ApiError> {
    let principal = state.principal(&jar).await;
    let Some(tag) = body.tag_id else {
        return Err(ApiError::Invalid(NOTHING_TO_CHANGE.into()));
    };
    let tag = some_text(tag);

    if !state
        .store
        .set_project_tag(&principal, &id, tag.as_deref())
        .await
        .map_err(ApiError::Internal)?
    {
        return Err(project_refusal(&state, &principal, &id).await);
    }

    // Read back through the permission-checked accessor rather than assembled from what was
    // sent: what the caller is shown is what the store holds.
    state
        .store
        .project_for(&principal, &id)
        .await
        .map_err(ApiError::Internal)?
        .map(|project| Json(project.into()))
        .ok_or_else(|| {
            ApiError::Internal(anyhow::anyhow!(
                "the project just changed is not readable by the caller who changed it"
            ))
        })
}

/// Delete a project. Its standalone cards go with it, by the foreign key in
/// `0010_tasks.sql`; tasks anchored to pages in the home subtree are governed by their own
/// pages and are not touched.
pub async fn remove_project(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<Changed>, ApiError> {
    let principal = state.principal(&jar).await;
    if state
        .store
        .delete_project(&principal, &id)
        .await
        .map_err(ApiError::Internal)?
    {
        Ok(Json(Changed { changed: true }))
    } else {
        Err(project_refusal(&state, &principal, &id).await)
    }
}

/// D-9's three columns, always all three, built from [`TaskStatus::ALL`] so they cannot drift
/// from the set the store stores.
///
/// `board_for` already returns board order — column, then position, then id — so a column is
/// a filter over that list and keeps the order it was given.
///
/// Shared by both boards deliberately: D-12's two places have to agree, and a second copy of
/// this loop is a second thing to keep in step. It takes the cards it is given and asks
/// nothing about them, which is what keeps every permission decision in `gw-store`.
fn columns_of(tasks: &[Task]) -> Vec<ColumnView> {
    TaskStatus::ALL
        .into_iter()
        .map(|status| ColumnView {
            status,
            tasks: tasks
                .iter()
                .filter(|task| task.status == status)
                .map(TaskView::from)
                .collect(),
        })
        .collect()
}

/// One project's board: its standalone cards, plus the tasks written into the pages of its
/// home subtree, filtered per document (D-3).
///
/// The filtering is `Store::board_for`'s and is not repeated here — a second filter in the
/// handler would be a second place for the property to be wrong, and it is mutation-tested
/// in `gw-store`. What this handler adds is the shape.
///
/// `project_for` decides the status code and `board_for` decides what is disclosed. Both ask
/// the same accessor the same question; neither is redundant.
pub async fn board(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<BoardResponse>, ApiError> {
    let principal = state.principal(&jar).await;
    // 404 for "no such project" and "not for you" alike. An id is a uuid nobody guesses, so
    // there is no existence to protect, and a 403 here would say the project is there.
    let project = state
        .store
        .project_for(&principal, &id)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;

    let tasks = state
        .store
        .board_for(&principal, Some(&id))
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(BoardResponse {
        project: Some(project.into()),
        columns: columns_of(&tasks),
    }))
}

/// The global board (D-12): every task the caller may see, optionally bound to one project.
///
/// **This is the project board with the binding taken off**, not a second board. The one
/// call below is the one the project board makes, and the binding is a filter it already
/// takes — see the module header on why D-12 required exactly that. So nothing here decides
/// what is disclosed; what it decides is which of the two settled status-code rules applies,
/// because the two parameters ask about two different kinds of thing:
///
/// - `?projekt=<id>` — an id, so everything unreachable is 404 and a refusal is
///   indistinguishable from a project that is not there.
/// - `?seite=<pfad>` — a path, so an absent page is 404 and a refused one is 403, exactly as
///   a plain `GET` of that page already answers. It is resolved to the project homed there
///   through `projects_for`, which is the permission-filtered listing — the same one
///   `create_project` asks about a duplicate home — so "which project is this page the home
///   of" has one answer in this crate rather than a second, board-shaped one. A page that is
///   nobody's home is an **empty** board, never the unbound one: a page loader asks this
///   about every page it renders, and falling through would put every task in the wiki on an
///   ordinary page.
/// - neither — 200 and whatever the caller is entitled to, which for somebody entitled to
///   nothing is an empty board. No 403 and no 404, for the reason [`list_projects`] gives:
///   the request asks about no particular thing, so there is no existence a status code
///   could confirm.
pub async fn global_board(
    State(state): State<AppState>,
    jar: CookieJar,
    Query(query): Query<BoardQuery>,
) -> Result<Json<BoardResponse>, ApiError> {
    let principal = state.principal(&jar).await;

    let project = match (query.projekt.as_deref(), query.seite.as_deref()) {
        (Some(_), Some(_)) => return Err(ApiError::Invalid(ONE_BINDING.into())),
        (Some(id), None) => Some(
            state
                .store
                .project_for(&principal, id)
                .await
                .map_err(ApiError::Internal)?
                .ok_or(ApiError::NotFound)?,
        ),
        (None, Some(captured)) => {
            let path = full_path(captured);
            if !state
                .store
                .document_exists(&path)
                .await
                .map_err(ApiError::Internal)?
            {
                return Err(ApiError::NotFound);
            }
            // Authorised on the page itself before anything is said about a project homed
            // there. Otherwise "is this page a project's home" is a question anybody could
            // ask of any page — and the answer names the project, its tag and when it was
            // made.
            state
                .store
                .document_for(&principal, &path, Action::Read)
                .await
                .map_err(ApiError::Internal)?
                .ok_or(ApiError::Forbidden)?;

            let homed = state
                .store
                .projects_for(&principal)
                .await
                .map_err(ApiError::Internal)?
                .into_iter()
                .find(|project| project.home_path == path);
            let Some(project) = homed else {
                return Ok(Json(BoardResponse {
                    project: None,
                    columns: columns_of(&[]),
                }));
            };
            Some(project)
        }
        (None, None) => None,
    };

    let tasks = state
        .store
        .board_for(&principal, project.as_ref().map(|p| p.id.as_str()))
        .await
        .map_err(ApiError::Internal)?;

    Ok(Json(BoardResponse {
        project: project.map(ProjectView::from),
        columns: columns_of(&tasks),
    }))
}

// --- tasks --------------------------------------------------------------------------------

/// Create a standalone card on a project's board (D-1).
///
/// Standalone only — see the module comment on why an anchored task is not creatable here.
pub async fn create_task(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(body): Json<NewCard>,
) -> Result<(StatusCode, Json<TaskView>), ApiError> {
    let principal = state.principal(&jar).await;

    let title = body.title.trim();
    if title.is_empty() {
        return Err(ApiError::Invalid(NO_WORDS.into()));
    }
    let status = match &body.status {
        Some(wanted) => status_from_wire(wanted)?,
        None => TaskStatus::default(),
    };

    let new = NewTask {
        home: TaskHome::Standalone {
            project_id: body.project_id.clone(),
        },
        title: title.to_string(),
        status,
        assignee: some_text(body.assignee),
        due_at: some_text(body.due_at),
        position: body.position.unwrap_or(0),
    };

    match state
        .store
        .create_task(&principal, &new)
        .await
        .map_err(ApiError::Internal)?
    {
        TaskOutcome::Done(task) => Ok((StatusCode::CREATED, Json(TaskView::from(&*task)))),
        TaskOutcome::AssigneeMayNotRead => Err(ApiError::Conflict(ASSIGNEE_CANNOT_READ.into())),
        TaskOutcome::Refused => Err(project_refusal(&state, &principal, &body.project_id).await),
    }
}

/// Change a card: its column, its position in it, who it rests on, when it is due, and — for
/// a card created on the board — its words.
///
/// **Nothing here writes a page or files a revision** (D-2). That is not a promise this
/// handler keeps by being careful; it is what the store's task accessors do, and
/// `moving_a_card_changes_no_page_and_files_no_revision` in `tests/tasks.rs` is what stops
/// it changing.
///
/// `title` is accepted for every card, and for an **anchored** one the page owns the words:
/// the next publish takes the title from the block again (D-2). Refusing it here instead
/// would be this layer deciding something the store already decides, and the two could then
/// disagree about a card that is visibly on a page.
pub async fn change_task(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
    Json(body): Json<CardChange>,
) -> Result<Json<TaskView>, ApiError> {
    let principal = state.principal(&jar).await;

    let title = match &body.title {
        Some(words) => {
            let words = words.trim();
            if words.is_empty() {
                return Err(ApiError::Invalid(NO_WORDS.into()));
            }
            Some(words.to_string())
        }
        None => None,
    };

    let update = TaskUpdate {
        title,
        status: body.status.as_deref().map(status_from_wire).transpose()?,
        assignee: body.assignee.map(some_text),
        due_at: body.due_at.map(some_text),
        position: body.position,
        // Not on the wire — see the module comment.
        project_id: None,
    };

    // A change that names nothing is not a no-op worth reporting as success: it would touch
    // `updated_at` and nothing else, and tell the caller their change landed.
    if update.title.is_none()
        && update.status.is_none()
        && update.assignee.is_none()
        && update.due_at.is_none()
        && update.position.is_none()
    {
        return Err(ApiError::Invalid(NOTHING_TO_CHANGE.into()));
    }

    match state
        .store
        .update_task(&principal, &id, &update)
        .await
        .map_err(ApiError::Internal)?
    {
        TaskOutcome::Done(task) => Ok(Json(TaskView::from(&*task))),
        TaskOutcome::AssigneeMayNotRead => Err(ApiError::Conflict(ASSIGNEE_CANNOT_READ.into())),
        TaskOutcome::Refused => Err(task_refusal(&state, &principal, &id).await),
    }
}

/// Throw a card away. The deliberate act; D-8's `detached` is what happens when a line
/// merely disappears from a page.
pub async fn remove_task(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(id): Path<String>,
) -> Result<Json<Changed>, ApiError> {
    let principal = state.principal(&jar).await;
    if state
        .store
        .delete_task(&principal, &id)
        .await
        .map_err(ApiError::Internal)?
    {
        Ok(Json(Changed { changed: true }))
    } else {
        Err(task_refusal(&state, &principal, &id).await)
    }
}

/// The cards written into one page — D-2's other half, so a page can render its checkboxes
/// from the records rather than from the words.
///
/// Keyed by path, and resolved through `document_for` exactly as `links::get_backlinks`
/// resolves the id `backlinks_for` takes: the client already has the path, and an id it
/// supplied would have to be turned back into a path to be authorised anyway.
pub async fn document_tasks(
    State(state): State<AppState>,
    jar: CookieJar,
    Path(captured): Path<String>,
) -> Result<Json<TasksResponse>, ApiError> {
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
    let document = state
        .store
        .document_for(&principal, &path, Action::Read)
        .await
        .map_err(ApiError::Internal)?
        .ok_or(ApiError::Forbidden)?;

    let tasks = state
        .store
        .tasks_for_document(&principal, &document.id)
        .await
        .map_err(ApiError::Internal)?
        .iter()
        .map(TaskView::from)
        .collect();

    Ok(Json(TasksResponse { tasks }))
}
