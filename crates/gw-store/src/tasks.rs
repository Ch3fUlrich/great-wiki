//! Tasks, boards and projects: the records behind a to-do written in a page and a card
//! dragged on a board (D-1, D-2, D-9, D-10).
//!
//! **One rule runs through every function here: a task is governed by exactly one page.**
//! For an anchored task that page is the document the line was written in; for a standalone
//! card it is its project's home page (D-3). The schema makes any other shape
//! unrepresentable — see `migrations/0010_tasks.sql` — so this module never has to ask what
//! to do about a task with two governing pages or none.
//!
//! Everything else follows from that one rule:
//!
//! - **Who may see a card** is whoever may Read its governing page, decided by
//!   [`Store::document_for`] — the crate's one permission-checked document accessor — and
//!   never by a second answer written into the SQL. The design's Security section is
//!   explicit that a board is a disclosure surface: a card reveals that a page exists and
//!   what it is called, which is the whole of what a restricted title was hiding. Because
//!   of D-3 the question is asked **per document**, not per subtree: a project's home
//!   subtree narrows *which* candidates are considered and decides nothing, exactly as
//!   `root` does in [`Store::graph_for`].
//! - **What a card may say about its page** is decided by the same call and no other. An
//!   anchored card carries [`TaskPage`] — the path and title of the page its line was
//!   written on — and that value can only be built from a [`StoredDocument`], which is what
//!   the accessor hands back. So there is no way to name a page without having asked
//!   whether the caller may read it, and the tempting shortcut (the card was filtered
//!   already, so look its page up by `doc_id` unchecked) is not merely discouraged here,
//!   it has nothing to construct.
//! - **Who may change a card**, including who may set its assignee, is whoever may Write
//!   that same page.
//! - **Who may be assigned** is anybody who may Read it. This is the answer to D-10's open
//!   question, and [`Store::create_task`] states it in full.
//!
//! Reconciling a page's task blocks against these rows on publish — minting a task for a
//! new checkbox line and marking one [`Task::detached`] when its line disappears (D-6, D-8)
//! — is [`reconcile_tasks`], at the bottom of this file. It is the only thing that writes
//! `block_id` or `detached`, and it runs inside the transaction that writes the revision
//! the blocks were read out of.

use crate::acl::Baseline;
use crate::{Store, StoredDocument};
use anyhow::{bail, Result};
use gw_auth::{Action, Principal};
use gw_core::{Block, BlockKind};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};

/// D-9's fixed columns. The same three on every board, built in.
///
/// The stored spelling is German and the variant name is not, for one reason: `Läuft` is a
/// legal Rust identifier but a non-ASCII one, and a `ä` that can be typed two ways (U+00E4,
/// or `a` + U+0308) is a poor thing to key a match on. [`TaskStatus::as_str`] holds the one
/// spelling, `migrations/0010_tasks.sql` holds the CHECK constraint listing the same three,
/// and `every_status_the_rust_enum_knows_is_accepted_by_the_schema` is what stops the two
/// drifting apart.
///
/// The derived `Ord` is the BOARD's order — Offen, then Läuft, then Fertig — and reads sort
/// by it. Sorting on the stored strings instead would order them alphabetically (Fertig,
/// Läuft, Offen), which is not a column order anybody asked for.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub enum TaskStatus {
    /// Not started. The default a new task lands in.
    #[default]
    Offen,
    /// In progress.
    #[serde(rename = "Läuft")]
    Laeuft,
    /// Done.
    Fertig,
}

impl TaskStatus {
    /// Every status there is, in board order. Small and closed, per D-9, so a query never
    /// has to handle a status set it could not know in advance.
    pub const ALL: [TaskStatus; 3] = [TaskStatus::Offen, TaskStatus::Laeuft, TaskStatus::Fertig];

    /// The stored spelling. Must match `0010_tasks.sql`'s CHECK constraint exactly.
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Offen => "Offen",
            TaskStatus::Laeuft => "Läuft",
            TaskStatus::Fertig => "Fertig",
        }
    }

    /// Read a stored value, or `None` if it is not one of the three.
    ///
    /// Deliberately not `unwrap_or_default()` at the call site, which is the shape
    /// [`crate::Baseline::from_stored`] uses: a baseline has a fail-CLOSED direction to
    /// fall back to, and a status does not — guessing `Offen` for an unrecognised value
    /// would silently reopen a finished task, and guessing `Fertig` would silently close an
    /// open one. The CHECK constraint makes the case unreachable; if it is ever reached,
    /// [`row_to_task`] says so instead of picking a column.
    pub fn from_stored(value: &str) -> Option<Self> {
        TaskStatus::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// Where a task lives, and therefore which page governs it.
///
/// An enum and not two `Option`s, so that "anchored to a page AND filed under a project"
/// and "neither" cannot be written in Rust any more than they can be stored — see the
/// `CHECK ((doc_id IS NULL) <> (project_id IS NULL))` in `0010_tasks.sql`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskHome {
    /// A line somebody wrote in a page (D-1). `block_id` is the uuid the task block carries
    /// in its `attrs`; it is `None` until reconciliation exists to mint one.
    Anchored {
        doc_id: String,
        block_id: Option<String>,
    },
    /// A card on a board that belongs to no page (D-1). Its project's home page governs it.
    Standalone { project_id: String },
}

/// The page an anchored card's line was written on, as a board needs to name it: somewhere
/// to link to, and something to call it.
///
/// **Constructed only from a [`StoredDocument`] that has been through the permission-checked
/// accessor**, which is why this carries no document id and cannot be built from one. That
/// is the whole point of the type. The tempting alternative — a card is already filtered, so
/// look its page up unchecked — is safe only for as long as the filtering somewhere else
/// stays right, and it makes the safety of one query a property of another.
///
/// The path and the title are resolved when the board is read rather than copied onto the
/// record when the card is made, for D-5's reason: renaming or moving a page must not leave
/// a board saying what it used to be called.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskPage {
    pub path: String,
    pub title: String,
}

impl From<&StoredDocument> for TaskPage {
    fn from(doc: &StoredDocument) -> Self {
        Self {
            path: doc.path.clone(),
            title: doc.title.clone(),
        }
    }
}

/// The page a card names, given the document that governs it.
///
/// `None` for a **standalone** card, and that is not a gap: no page holds it — that is what
/// standalone means — and naming its project's home would say a line exists on a page that
/// never held one. `anchored` and `page` therefore agree by construction rather than by
/// two separate decisions that could disagree.
fn page_of(home: &TaskHome, governing: &StoredDocument) -> Option<TaskPage> {
    match home {
        TaskHome::Anchored { .. } => Some(TaskPage::from(governing)),
        TaskHome::Standalone { .. } => None,
    }
}

/// A task record. The workflow state lives here; for an anchored task the words live in the
/// page (D-2) and `title` is the copy the board renders.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Task {
    pub id: String,
    pub doc_id: Option<String>,
    pub block_id: Option<String>,
    pub title: String,
    pub status: TaskStatus,
    /// The principal id this task rests on, or `None`. See [`Store::create_task`] for who
    /// may put a name here.
    pub assignee: Option<String>,
    pub due_at: Option<String>,
    pub project_id: Option<String>,
    pub position: i64,
    /// D-8: the page no longer mentions the line that authored this task. The card stays on
    /// the board, marked, rather than vanishing with a due date somebody set.
    pub detached: bool,
    /// The page this card's line was written on, or `None` for a card that was created on
    /// a board and lives in no page. Always the page that was just authorised — see
    /// [`TaskPage`], which cannot be built from anything else.
    pub page: Option<TaskPage>,
    pub created_at: String,
    pub updated_at: String,
}

/// A project: a home page, and later a tag that pulls in documents from elsewhere (D-3).
///
/// `home_path` and `home_title` are on this struct because a `Project` is only ever handed
/// back after its home page has been through [`Store::document_for`] — they are that
/// document's own fields, not a second lookup that skipped the check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    pub id: String,
    pub home_doc: String,
    pub home_path: String,
    pub home_title: String,
    pub tag_id: Option<String>,
    pub created_at: String,
}

/// What to create. `home` decides which page must be writable for this to be allowed.
#[derive(Debug, Clone)]
pub struct NewTask {
    pub home: TaskHome,
    pub title: String,
    pub status: TaskStatus,
    pub assignee: Option<String>,
    pub due_at: Option<String>,
    pub position: i64,
}

/// What to change. Every field absent means "leave it alone".
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub title: Option<String>,
    pub status: Option<TaskStatus>,
    /// Absent leaves the assignee alone; `Some(None)` **unassigns**; `Some(Some(id))`
    /// assigns. The three cases are genuinely different — see [`Store::update_task`], where
    /// unassigning is deliberately permitted to somebody the assignment itself would not
    /// have been.
    pub assignee: Option<Option<String>>,
    pub due_at: Option<Option<String>>,
    pub position: Option<i64>,
    /// Move a standalone card to another board. Refused for an anchored task: D-3 already
    /// decides which project its page belongs to, and a second answer stored on the card
    /// would let the two disagree about something visibly on a page.
    pub project_id: Option<String>,
}

/// The answer to a create or an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskOutcome {
    /// Boxed for the reason [`crate::AcceptOutcome::Accepted`] is: a `Task` is an order of
    /// magnitude larger than the two refusals, and an enum sized to its biggest variant is
    /// paid for by every call that returns one of the small ones.
    Done(Box<Task>),
    /// No such task, or the caller may not Write its governing page — one answer for both,
    /// exactly as [`Store::document_for`] returns `None` for "absent" and "not permitted"
    /// alike. This layer does not decide whether existence may be revealed.
    Refused,
    /// The caller may Write the governing page, but the principal they named may not Read
    /// it — so the assignment is refused (see [`Store::create_task`]).
    ///
    /// A distinct answer, and the cost is worth stating: it tells somebody who may write
    /// the page one fact about somebody else's access to it. The alternative is worse. A
    /// bare `Refused` gives a writer no way to tell "you may not touch this card" from "the
    /// person you picked cannot see it", and quietly dropping the name would leave them
    /// believing they had assigned it.
    AssigneeMayNotRead,
}

/// The columns of `tasks`, in the order [`TaskRow`] declares them. One spelling, so a
/// second query cannot drift into selecting them in a different order — the same trick
/// `REVISION_COLUMNS` plays in [`crate::revisions`].
const TASK_COLUMNS: &str = "id, doc_id, block_id, title, status, assignee, due_at, \
                            project_id, position, detached, created_at, updated_at";

#[derive(FromRow)]
struct TaskRow {
    id: String,
    doc_id: Option<String>,
    block_id: Option<String>,
    title: String,
    status: String,
    assignee: Option<String>,
    due_at: Option<String>,
    project_id: Option<String>,
    position: i64,
    detached: i64,
    created_at: String,
    updated_at: String,
}

/// Where this row lives, read back out of the two columns the CHECK constraint keeps
/// exclusive.
///
/// An error rather than a guess when neither or both are set. That state cannot be stored —
/// `CHECK ((doc_id IS NULL) <> (project_id IS NULL))` — so reaching this arm means the
/// constraint is gone, and the safe reading of "this row has no governing page" is not to
/// invent one.
fn home_of(row: &TaskRow) -> Result<TaskHome> {
    match (&row.doc_id, &row.project_id) {
        (Some(doc_id), None) => Ok(TaskHome::Anchored {
            doc_id: doc_id.clone(),
            block_id: row.block_id.clone(),
        }),
        (None, Some(project_id)) => Ok(TaskHome::Standalone {
            project_id: project_id.clone(),
        }),
        _ => bail!(
            "task {} is anchored to {:?} and filed under {:?}: it has no single governing page",
            row.id,
            row.doc_id,
            row.project_id
        ),
    }
}

/// A row, plus the page it names.
///
/// `page` is a parameter rather than something this function looks up, and that is the
/// design: a `TaskRow` carries a `doc_id`, so a lookup here would be one line away and
/// would be unchecked. Every caller therefore has to have asked the accessor already, and
/// hands in what it answered.
fn row_to_task(row: TaskRow, page: Option<TaskPage>) -> Result<Task> {
    let Some(status) = TaskStatus::from_stored(&row.status) else {
        bail!(
            "task {} carries the status {:?}, which is not one of D-9's three",
            row.id,
            row.status
        );
    };
    Ok(Task {
        id: row.id,
        doc_id: row.doc_id,
        block_id: row.block_id,
        title: row.title,
        status,
        assignee: row.assignee,
        due_at: row.due_at,
        project_id: row.project_id,
        position: row.position,
        detached: row.detached != 0,
        page,
        created_at: row.created_at,
        updated_at: row.updated_at,
    })
}

/// Board order: column first, then the author's arrangement within it, then the id.
///
/// The id breaks the tie because `position` is not unique — two cards can collide, and a
/// board that reshuffles between two identical requests looks broken. Same reasoning as the
/// revisions timeline, which breaks its tie on a uuid v7 for the same reason.
fn in_board_order(tasks: &mut [Task]) {
    tasks.sort_by(|a, b| {
        a.status
            .cmp(&b.status)
            .then(a.position.cmp(&b.position))
            .then(a.id.cmp(&b.id))
    });
}

/// Whether `path` is inside the subtree at `root`, on a SEGMENT boundary.
///
/// The root itself is in its own subtree. `/darmspiegelung` is not inside `/darm`; a bare
/// prefix match would pull it in, which is the ordinary prefix bug. This narrows a view and
/// decides nothing — see [`Store::board_for`], where every survivor still goes through
/// [`Store::document_for`].
fn within(root: &str, path: &str) -> bool {
    path == root || path.starts_with(&format!("{root}/"))
}

impl Store {
    /// The document that governs a task, if `principal` may `action` it.
    ///
    /// This is the whole permission model of this module in one function: resolve the
    /// governing page, then hand it to [`Store::document_for_with_baseline`] — the crate's
    /// one authorisation path — and let it answer. Nothing here decides anything itself,
    /// and no caller in this module is allowed to skip it.
    ///
    /// `None` covers "the page is gone", "the project is gone" and "not permitted" alike.
    async fn governing_document(
        &self,
        principal: &Principal,
        home: &TaskHome,
        action: Action,
        baseline: Baseline,
    ) -> Result<Option<StoredDocument>> {
        let Some(path) = self.governing_path(home).await? else {
            return Ok(None);
        };
        self.document_for_with_baseline(principal, &path, action, baseline)
            .await
    }

    /// The PATH of a task's governing page, with no permission check at all.
    ///
    /// Crate-private and unexported. It answers "which page decides?", never "may you", and
    /// its one caller is [`Store::governing_document`], which puts the path straight into
    /// `document_for`.
    ///
    /// The anchored arm is [`Store::document_path_unchecked`] rather than the same SELECT
    /// written again: "which path does this id name" has one answer in this crate, and two
    /// copies of it are two places for it to drift. The standalone arm is a different
    /// question — which page is this *project* homed on — and stays a JOIN.
    async fn governing_path(&self, home: &TaskHome) -> Result<Option<String>> {
        Ok(match home {
            TaskHome::Anchored { doc_id, .. } => self.document_path_unchecked(doc_id).await?,
            TaskHome::Standalone { project_id } => {
                sqlx::query_scalar(
                    "SELECT d.path FROM projects p JOIN documents d ON d.id = p.home_doc \
                 WHERE p.id = ?1",
                )
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?
            }
        })
    }

    /// Whether `assignee_id` may be given a task governed by the page at `governing_path`.
    ///
    /// **This asks the assignee's own question, not the caller's.** It loads that
    /// principal — with their groups, their teams and their active flag — and puts THEM
    /// through [`Store::document_for`], which resolves their baseline rather than reusing
    /// the one the caller was authorised with. Reusing the caller's would answer "could I
    /// read it", which is the question that has already been answered and not the one being
    /// asked.
    ///
    /// An id with no principal row is `false`: an obligation cannot rest on somebody who
    /// cannot sign in. (The foreign key on `tasks.assignee` refuses it too; this is what
    /// turns a constraint violation into an answer.)
    async fn may_be_assigned(&self, assignee_id: &str, governing_path: &str) -> Result<bool> {
        let Some((assignee, _)) = self.principal_by_id(assignee_id).await? else {
            return Ok(false);
        };
        Ok(self
            .document_for(&assignee, governing_path, Action::Read)
            .await?
            .is_some())
    }

    /// A task row with NO permission check whatsoever.
    ///
    /// Crate-private and named so the danger is unmissable, exactly as
    /// [`Store::document_by_path_unchecked`] is. Authorising a task means knowing which page
    /// governs it, and that is on the row — so the row is read first and refused afterwards.
    async fn task_row_unchecked(&self, task_id: &str) -> Result<Option<TaskRow>> {
        Ok(
            sqlx::query_as::<_, TaskRow>(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"
            ))
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await?,
        )
    }
}

// --- tasks ------------------------------------------------------------------------------

impl Store {
    /// Create a task. **This is where D-10's open question is answered.**
    ///
    /// The design left "who may assign whom" open and required the plan to state a rule and
    /// a test for it rather than leaving assignment ungoverned. The rule, in full:
    ///
    /// 1. A task's **governing page** is its anchor document when it is anchored, and its
    ///    project's home page when it is standalone.
    /// 2. You may create or modify a task, **including setting its assignee**, if you may
    ///    **Write** the governing page. Nothing else confers it — not reading the page, not
    ///    being the assignee, not owning the project.
    /// 3. You may only assign a task to a principal who may **Read** the governing page.
    ///    Assigning somebody to a task on a page they cannot open would create an
    ///    obligation they cannot see, and the card's title would tell them what a page they
    ///    may not read is called. Refused, as [`TaskOutcome::AssigneeMayNotRead`].
    /// 4. **Unassigning** is allowed to anybody who may Write the governing page, with no
    ///    question asked about the person being removed — see [`Store::update_task`]. A
    ///    stale assignee must be clearable even after that person has lost their read, or
    ///    losing access would pin somebody's name to a card forever.
    ///
    /// `Ok(TaskOutcome::Refused)` covers a governing page that is absent, in the trash, or
    /// not writable by the caller — the same conflation [`Store::document_for`] makes.
    pub async fn create_task(&self, principal: &Principal, new: &NewTask) -> Result<TaskOutcome> {
        let baseline = self.baseline_for(principal).await?;
        let Some(governing) = self
            .governing_document(principal, &new.home, Action::Write, baseline)
            .await?
        else {
            return Ok(TaskOutcome::Refused);
        };

        if let Some(assignee) = &new.assignee {
            if !self.may_be_assigned(assignee, &governing.path).await? {
                return Ok(TaskOutcome::AssigneeMayNotRead);
            }
        }

        let (doc_id, block_id, project_id) = match &new.home {
            TaskHome::Anchored { doc_id, block_id } => {
                (Some(doc_id.clone()), block_id.clone(), None)
            }
            TaskHome::Standalone { project_id } => (None, None, Some(project_id.clone())),
        };

        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO tasks (id, doc_id, block_id, title, status, assignee, due_at, \
             project_id, position) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(&id)
        .bind(&doc_id)
        .bind(&block_id)
        .bind(&new.title)
        .bind(new.status.as_str())
        .bind(&new.assignee)
        .bind(&new.due_at)
        .bind(&project_id)
        .bind(new.position)
        .execute(&self.pool)
        .await?;

        let Some(row) = self.task_row_unchecked(&id).await? else {
            bail!("task {id} vanished immediately after being inserted");
        };
        // The page named on the card is the document that just authorised the write, not a
        // second lookup by the `doc_id` two lines above.
        let page = page_of(&new.home, &governing);
        Ok(TaskOutcome::Done(Box::new(row_to_task(row, page)?)))
    }

    /// One task, if the caller may Read its governing page.
    ///
    /// `None` for "no such task" and for "not for you" alike. A card's title is the page's
    /// words (D-2), so reading one is reading the page.
    pub async fn task_for(&self, principal: &Principal, task_id: &str) -> Result<Option<Task>> {
        let Some(row) = self.task_row_unchecked(task_id).await? else {
            return Ok(None);
        };
        let home = home_of(&row)?;
        let baseline = self.baseline_for(principal).await?;
        // What authorises the read is what names the page: one answer, and no way to reach
        // the second line without having acted on the first.
        let page = match self
            .governing_document(principal, &home, Action::Read, baseline)
            .await?
        {
            Some(governing) => page_of(&home, &governing),
            None => return Ok(None),
        };
        Ok(Some(row_to_task(row, page)?))
    }

    /// Change a task. Rule 2 of [`Store::create_task`] decides whether it happens at all.
    ///
    /// Two of the clauses are visible in the shape of this function rather than in a
    /// comment somewhere:
    ///
    /// - **Unassigning asks nothing about the person being removed.** `Some(None)` skips
    ///   the [`Store::may_be_assigned`] check entirely, which is rule 4: somebody who has
    ///   lost their read must still be removable from the card, or the assignment outlives
    ///   the access it was granted under.
    /// - **Moving a card to another board changes its governing page**, so BOTH ends are
    ///   asked for Write — otherwise a card could be walked out of a project you may write
    ///   into one you may not, and the schema would be perfectly happy about it. For the
    ///   same reason the assignee is re-checked against the DESTINATION when a move carries
    ///   one along: an assignment that was legitimate on the old board is exactly the
    ///   obligation rule 3 forbids if the new board's page is closed to that person.
    pub async fn update_task(
        &self,
        principal: &Principal,
        task_id: &str,
        update: &TaskUpdate,
    ) -> Result<TaskOutcome> {
        let Some(row) = self.task_row_unchecked(task_id).await? else {
            return Ok(TaskOutcome::Refused);
        };
        let home = home_of(&row)?;
        let baseline = self.baseline_for(principal).await?;
        let Some(governing) = self
            .governing_document(principal, &home, Action::Write, baseline)
            .await?
        else {
            return Ok(TaskOutcome::Refused);
        };

        // Named from the document that authorised the change. A move cannot change it: only
        // a standalone card moves, and a standalone card names no page either side of one.
        let page = page_of(&home, &governing);
        // Where the card will be governed from once this update lands.
        let mut governing_path = governing.path;
        let mut moved_to = None;
        if let Some(project_id) = &update.project_id {
            if !matches!(home, TaskHome::Standalone { .. }) {
                // An anchored task's project follows its page (D-3). Refusing rather than
                // ignoring: silently dropping the field would report success for a move
                // that did not happen.
                return Ok(TaskOutcome::Refused);
            }
            let target = TaskHome::Standalone {
                project_id: project_id.clone(),
            };
            let Some(target_page) = self
                .governing_document(principal, &target, Action::Write, baseline)
                .await?
            else {
                return Ok(TaskOutcome::Refused);
            };
            governing_path = target_page.path;
            moved_to = Some(project_id.clone());
        }

        // Rule 3, against the page the card will be governed by afterwards. `None` — leave
        // the assignee alone — still has to be checked when the card MOVES, because the
        // name already on it is an assignment onto the destination's page.
        let effective_assignee = match &update.assignee {
            Some(chosen) => chosen.clone(),
            None if moved_to.is_some() => row.assignee.clone(),
            None => None,
        };
        if let Some(assignee) = &effective_assignee {
            if !self.may_be_assigned(assignee, &governing_path).await? {
                return Ok(TaskOutcome::AssigneeMayNotRead);
            }
        }

        // COALESCE(?, column) is not usable here: it cannot express "set this to NULL", and
        // the assignee has to be clearable. Each field is therefore bound twice — once as
        // the flag saying whether it was supplied, once as the value — which keeps this one
        // statement rather than a builder that assembles SQL from strings.
        sqlx::query(
            "UPDATE tasks SET \
               title      = CASE WHEN ?2  THEN ?3  ELSE title      END, \
               status     = CASE WHEN ?4  THEN ?5  ELSE status     END, \
               assignee   = CASE WHEN ?6  THEN ?7  ELSE assignee   END, \
               due_at     = CASE WHEN ?8  THEN ?9  ELSE due_at     END, \
               position   = CASE WHEN ?10 THEN ?11 ELSE position   END, \
               project_id = CASE WHEN ?12 THEN ?13 ELSE project_id END, \
               updated_at = datetime('now') \
             WHERE id = ?1",
        )
        .bind(task_id)
        .bind(update.title.is_some())
        .bind(&update.title)
        .bind(update.status.is_some())
        .bind(update.status.map(TaskStatus::as_str))
        .bind(update.assignee.is_some())
        .bind(update.assignee.clone().flatten())
        .bind(update.due_at.is_some())
        .bind(update.due_at.clone().flatten())
        .bind(update.position.is_some())
        .bind(update.position)
        .bind(moved_to.is_some())
        .bind(&moved_to)
        .execute(&self.pool)
        .await?;

        let Some(row) = self.task_row_unchecked(task_id).await? else {
            bail!("task {task_id} vanished immediately after being updated");
        };
        Ok(TaskOutcome::Done(Box::new(row_to_task(row, page)?)))
    }

    /// Delete a task. `false` for "no such task" and for "not permitted" alike.
    ///
    /// Needs Write on the governing page, the same bar as every other change. This is the
    /// deliberate act; D-8's `detached` is what happens when a line merely disappears.
    pub async fn delete_task(&self, principal: &Principal, task_id: &str) -> Result<bool> {
        let Some(row) = self.task_row_unchecked(task_id).await? else {
            return Ok(false);
        };
        let home = home_of(&row)?;
        let baseline = self.baseline_for(principal).await?;
        if self
            .governing_document(principal, &home, Action::Write, baseline)
            .await?
            .is_none()
        {
            return Ok(false);
        }
        let done = sqlx::query("DELETE FROM tasks WHERE id = ?1")
            .bind(task_id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// The tasks written into one page, filtered by the caller's Read on that page.
    ///
    /// Every task here shares one governing page — the document itself — so the question is
    /// asked once and the rows follow. An empty list is the answer both for "no tasks" and
    /// for "not for you", the same closed conflation the rest of this crate makes.
    ///
    /// [`Store::document_for_id`] rather than [`Store::may`], because the answer is needed
    /// twice: once to decide whether there is anything to return, and once to name the page
    /// on every card. A boolean would have thrown away the document and left the name to be
    /// fetched again, unchecked, from the id right there in the signature.
    pub async fn tasks_for_document(
        &self,
        principal: &Principal,
        document_id: &str,
    ) -> Result<Vec<Task>> {
        let page = match self
            .document_for_id(principal, document_id, Action::Read)
            .await?
        {
            Some(doc) => TaskPage::from(&doc),
            None => return Ok(Vec::new()),
        };
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE doc_id = ?1"
        ))
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?;

        // Every row here is anchored to `document_id` — that is what the WHERE says — so
        // they all name the one page that was just authorised.
        let mut out = rows
            .into_iter()
            .map(|row| row_to_task(row, Some(page.clone())))
            .collect::<Result<Vec<_>>>()?;
        in_board_order(&mut out);
        Ok(out)
    }

    /// One project's board: its standalone cards, plus the tasks written into the pages of
    /// its home subtree (D-3), filtered per document.
    ///
    /// **The filtering is the point, and it is per DOCUMENT.** The subtree prefix below
    /// narrows which candidates are considered — it is cheap and it discloses nothing —
    /// and then every survivor goes through [`Store::document_for_with_baseline`], which is
    /// what actually decides. Never the other way round: D-3 makes membership per document,
    /// so a project spanning pages with different grants is normal rather than exceptional,
    /// and a path prefix cannot express who may read what. A card whose page the caller may
    /// not read is omitted **entirely** — not shown as "a card you may not open", and not
    /// counted, either of which would say that the page exists.
    ///
    /// Nothing at all is returned to somebody who may not read the project's home page, and
    /// an empty board is the answer for "no such project" too.
    ///
    /// The verdict is memoised per path and the baseline is hoisted out of the loop, for the
    /// reason [`Store::graph_for`] gives: the baseline is a property of the caller, not of
    /// the document, and forty cards on one page must not be forty authorisations.
    pub async fn board_for(&self, principal: &Principal, project_id: &str) -> Result<Vec<Task>> {
        let baseline = self.baseline_for(principal).await?;
        let home = TaskHome::Standalone {
            project_id: project_id.to_string(),
        };
        let Some(home_page) = self
            .governing_document(principal, &home, Action::Read, baseline)
            .await?
        else {
            return Ok(Vec::new());
        };

        // The standalone cards. Their governing page is the home page, which the caller has
        // just been authorised for, so no second question arises for these — and they name
        // no page, because no page holds them.
        let mut out: Vec<Task> = sqlx::query_as::<_, TaskRow>(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1"
        ))
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|row| row_to_task(row, None))
        .collect::<Result<Vec<_>>>()?;

        // The anchored ones, with the path of the page each is written in. A JOIN, so that
        // forty cards across ten pages is one round trip rather than forty — and it
        // discloses nothing, because nothing selected here is returned until the loop below
        // has put its path through the accessor.
        //
        // `substr(...) = ?1 || '/'` rather than `LIKE ?1 || '/%'`: LIKE would read a `%` or
        // a `_` in a path as a wildcard, and the boundary has to be a segment anyway so that
        // `/darmspiegelung` is outside `/darm`.
        let anchored: Vec<(String, String)> = sqlx::query_as(
            "SELECT t.id, d.path FROM tasks t JOIN documents d ON d.id = t.doc_id \
             WHERE d.deleted_at IS NULL \
               AND (d.path = ?1 OR substr(d.path, 1, length(?1) + 1) = ?1 || '/')",
        )
        .bind(&home_page.path)
        .fetch_all(&self.pool)
        .await?;

        // The memo holds the PAGE the accessor answered with, not a boolean about it. The
        // verdict and the name are then one value: a card is emitted exactly when its page
        // came back, and what it is called is that page's own title. Storing a boolean and
        // fetching the name afterwards would be the same code with an unchecked lookup in
        // the middle, correct only for as long as the loop above stays right.
        let mut pages: HashMap<String, Option<TaskPage>> = HashMap::new();
        for (task_id, path) in anchored {
            // Defence in depth against the SQL above: the prefix narrows, `within` is the
            // same boundary stated where a human can read it, and neither decides.
            if !within(&home_page.path, &path) {
                continue;
            }
            let known = match pages.get(&path) {
                Some(known) => known.clone(),
                None => {
                    let known = self
                        .document_for_with_baseline(principal, &path, Action::Read, baseline)
                        .await?
                        .map(|doc| TaskPage::from(&doc));
                    pages.insert(path.clone(), known.clone());
                    known
                }
            };
            let Some(page) = known else {
                continue;
            };
            let Some(row) = self.task_row_unchecked(&task_id).await? else {
                continue;
            };
            out.push(row_to_task(row, Some(page))?);
        }

        in_board_order(&mut out);
        Ok(out)
    }
}

// --- projects ---------------------------------------------------------------------------

impl Store {
    /// Make the page at `home_path` the home of a new project (D-3).
    ///
    /// Needs Write on that page — the same bar as writing a task on it, and for the same
    /// reason: a project is a claim made about a page and the subtree below it. `None` for
    /// "no such page" and "not permitted" alike.
    ///
    /// A page can be the home of only one project; a second attempt on the same page is an
    /// error rather than a silent second project, because the `UNIQUE` constraint is what
    /// makes "which project is this page the home of" a question with one answer.
    pub async fn create_project(
        &self,
        principal: &Principal,
        home_path: &str,
        tag_id: Option<&str>,
    ) -> Result<Option<Project>> {
        let Some(home) = self
            .document_for(principal, home_path, Action::Write)
            .await?
        else {
            return Ok(None);
        };
        let id = uuid::Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO projects (id, home_doc, tag_id) VALUES (?1, ?2, ?3)")
            .bind(&id)
            .bind(&home.id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        self.project_for(principal, &id).await
    }

    /// One project, if the caller may Read its home page.
    pub async fn project_for(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<Option<Project>> {
        let baseline = self.baseline_for(principal).await?;
        self.project_with_baseline(principal, project_id, baseline)
            .await
    }

    /// [`Store::project_for`] with the caller's baseline already resolved, so a listing
    /// pays for it once. Same hoist, same reason, as `document_for_with_baseline`.
    async fn project_with_baseline(
        &self,
        principal: &Principal,
        project_id: &str,
        baseline: Baseline,
    ) -> Result<Option<Project>> {
        let row: Option<(String, String, Option<String>, String)> =
            sqlx::query_as("SELECT id, home_doc, tag_id, created_at FROM projects WHERE id = ?1")
                .bind(project_id)
                .fetch_optional(&self.pool)
                .await?;
        let Some((id, home_doc, tag_id, created_at)) = row else {
            return Ok(None);
        };
        // Through the accessor, always: the home page's title is the project's name as far
        // as anybody reading a listing is concerned.
        let home = TaskHome::Anchored {
            doc_id: home_doc.clone(),
            block_id: None,
        };
        let Some(page) = self
            .governing_document(principal, &home, Action::Read, baseline)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(Project {
            id,
            home_doc,
            home_path: page.path,
            home_title: page.title,
            tag_id,
            created_at,
        }))
    }

    /// Every project whose home page the caller may Read, home page first in path order.
    ///
    /// A project the caller may not reach is omitted entirely, for the reason the design's
    /// Security section gives about every aggregate view: a listing that named it would say
    /// the page exists and what it is called.
    pub async fn projects_for(&self, principal: &Principal) -> Result<Vec<Project>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT p.id FROM projects p JOIN documents d ON d.id = p.home_doc \
             WHERE d.deleted_at IS NULL ORDER BY d.path",
        )
        .fetch_all(&self.pool)
        .await?;

        let baseline = self.baseline_for(principal).await?;
        let mut out = Vec::new();
        for id in ids {
            if let Some(project) = self.project_with_baseline(principal, &id, baseline).await? {
                out.push(project);
            }
        }
        Ok(out)
    }

    /// Point a project at a different tag, or at none. Needs Write on the home page.
    ///
    /// The home page itself is deliberately not changeable: a project IS its home subtree
    /// (D-3), so re-homing one is not an edit, it is a different project — and doing it
    /// silently would move every anchored card on the board at once.
    pub async fn set_project_tag(
        &self,
        principal: &Principal,
        project_id: &str,
        tag_id: Option<&str>,
    ) -> Result<bool> {
        if !self.may_administer_project(principal, project_id).await? {
            return Ok(false);
        }
        let done = sqlx::query("UPDATE projects SET tag_id = ?2 WHERE id = ?1")
            .bind(project_id)
            .bind(tag_id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Delete a project. Needs Write on the home page.
    ///
    /// **Its standalone cards go with it**, by the foreign key in `0010_tasks.sql`: a card
    /// with no page of its own belongs to its project and to nothing else, and leaving it
    /// behind would leave a row no permission check could ever answer for. Tasks anchored to
    /// pages in the home subtree are NOT affected — they are governed by their own pages and
    /// were never rows of this project.
    pub async fn delete_project(&self, principal: &Principal, project_id: &str) -> Result<bool> {
        if !self.may_administer_project(principal, project_id).await? {
            return Ok(false);
        }
        let done = sqlx::query("DELETE FROM projects WHERE id = ?1")
            .bind(project_id)
            .execute(&self.pool)
            .await?;
        Ok(done.rows_affected() > 0)
    }

    /// Whether the caller may Write the project's home page — the one gate both project
    /// mutations use, so the two cannot drift into different bars.
    async fn may_administer_project(
        &self,
        principal: &Principal,
        project_id: &str,
    ) -> Result<bool> {
        let baseline = self.baseline_for(principal).await?;
        let home = TaskHome::Standalone {
            project_id: project_id.to_string(),
        };
        Ok(self
            .governing_document(principal, &home, Action::Write, baseline)
            .await?
            .is_some())
    }
}

// --- reconciliation on publish ----------------------------------------------------------

/// The `attrs` key a task block carries its identity under — the uuid that ties the line
/// somebody typed to the row that holds its due date.
///
/// `gw_core::markdown::convert` deliberately mints none: it is a pure function of its
/// input, and `gw_api::export::render_file` re-imports its own output and compares it
/// against the stored document, so a random id would fail that comparison on every export
/// forever. `export`'s `TASK_ITEM_ATTRS` reduces a `taskItem` to `checked` alone for the
/// same reason. So this key is written in exactly one place: here, on publish.
const BLOCK_ID: &str = "id";

/// One checklist line, paired with the `attrs` map its id is written into.
struct TaskLine<'a> {
    attrs: &'a mut Map<String, Value>,
    /// The words. `tasks.title` is a copy of this and of nothing else — D-2: the page owns
    /// the words.
    text: String,
    /// Whether the box is ticked. This decides a NEW record's status and nothing else; see
    /// [`reconcile_tasks`] for why it must never be read for an existing one.
    checked: bool,
}

/// The words a card shows for this line: its **first paragraph**, not its whole subtree.
///
/// A `taskItem` holds block content rather than bare text, deliberately, so that a line can
/// grow a second paragraph or a nested checklist without changing kind. Taking the item's
/// whole [`Block::plain_text`] would therefore fold a nested checklist's lines into their
/// parent's title — "Reise buchen Flug Hotel" for three separate to-dos, two of which have
/// cards of their own saying the same words again.
fn task_text(item: &Block) -> String {
    item.content
        .iter()
        .find(|child| child.kind == BlockKind::Paragraph)
        .map(Block::plain_text)
        .unwrap_or_else(|| item.plain_text())
        .trim()
        .to_string()
}

/// Every checklist line in the tree, in document order, skipping the ones with no words.
///
/// **An empty checkbox line is not yet a task.** Pressing Enter in a checklist makes an
/// empty item, and the next autosave would otherwise put a nameless card on somebody's
/// board — one that cannot be told from any other nameless card, and that nothing but a
/// deletion will ever remove. So an empty line gets no id and no record. It is not
/// *claimed*, either, which means clearing a line's words detaches its record like any
/// other disappearance; typing words back re-attaches it, because the block kept its id
/// through the edit. That self-healing is the reason this is safe to do at all.
///
/// The walk descends INTO an empty item all the same: a checklist nested under one is
/// still a checklist, and its lines are still to-dos.
fn collect_task_lines<'a>(block: &'a mut Block, out: &mut Vec<TaskLine<'a>>) {
    let line = (block.kind == BlockKind::TaskItem).then(|| (task_text(block), is_checked(block)));

    // Destructured so that `attrs` and `content` are two disjoint borrows: this function
    // both stores a mutable handle on the item's own attrs and keeps walking its children,
    // and a `&mut Block` cannot do both.
    let Block { attrs, content, .. } = block;
    if let Some((text, checked)) = line {
        if !text.is_empty() {
            out.push(TaskLine {
                attrs,
                text,
                checked,
            });
        }
    }
    for child in content.iter_mut() {
        collect_task_lines(child, out);
    }
}

fn is_checked(item: &Block) -> bool {
    item.attrs
        .get("checked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Write `id` onto a block, reporting whether that changed anything.
///
/// The answer is what tells [`crate::revisions::append_revision`] whether the tree it was
/// handed is still the tree it should store.
fn stamp(attrs: &mut Map<String, Value>, id: &str) -> bool {
    if attrs.get(BLOCK_ID).and_then(Value::as_str) == Some(id) {
        return false;
    }
    attrs.insert(BLOCK_ID.into(), Value::String(id.to_string()));
    true
}

/// A task row this document's blocks own.
#[derive(FromRow)]
struct AnchoredRow {
    id: String,
    block_id: String,
    title: String,
    detached: i64,
}

/// Reconcile a document's checklist lines against the `tasks` table, on publish.
///
/// **D-2 is the whole rule: the page owns the words, the record owns the state.** The text
/// is taken from the block on every pass; `status`, `assignee`, `due_at` and `position` are
/// never written for a record that already exists. Nothing here writes to a page — the one
/// thing it changes about the tree is the id it mints into a line that has none, which is
/// identity rather than content — and no board operation files a revision.
///
/// Returns whether the tree was changed, so that the caller stores **the tree that was
/// minted into**. Storing the original instead would re-mint on every publish, and the
/// detach loop below is what that costs.
///
/// # Permission
///
/// **This function asks nobody anything, and that is deliberate.** It is unreachable except
/// through [`crate::revisions::append_revision`], which is itself reachable only from
/// [`Store::publish_revision`] — which refuses a caller who may not **Write** the document
/// (and refuses an anonymous one outright) before a transaction is even opened — and from
/// [`Store::create_document`], which is the importer's path and states in its own doc
/// comment why it answers no permission question. So reconciliation inherits publishing's
/// authorisation exactly, and a second check here would be a second rule to keep in step
/// with the first. `a_reader_cannot_cause_a_task_to_be_created` is what holds that claim up.
///
/// The rows it may touch are exactly the rows carrying a `block_id`: the ones it or an
/// earlier pass authored. An anchored task created through [`Store::create_task`] with no
/// block behind it is left entirely alone — no page ever mentioned a line for it, so a line
/// disappearing cannot be said about it, and detaching it would be reconciliation asserting
/// something about a task it did not write.
///
/// # Identity, in three passes
///
/// 1. **A line carrying an id claims the row with that id.** This is the ordinary case and
///    the one that makes republishing idempotent.
/// 2. **A line carrying none adopts an unclaimed row with the same words.** Without this,
///    the two paths that produce an id-less body — a markdown import, and any republish of
///    a body that never went through an editor — would mint a fresh id every time, orphan
///    the previous record and shed a card, with its due date and its assignee, on every
///    save. `seed --update` republishes converted markdown on every run, so that loop would
///    run for as long as anybody kept seeding. Adoption is by title because that is the only
///    thing an id-less line has: two lines reading the same words are genuinely
///    interchangeable, and which of their records each adopts is decided in board order so
///    that at least it is the same answer every time.
/// 3. **Anything left mints a fresh uuid**, which is stamped onto the block.
///
/// A block id that appears twice in one document — a checklist copied and pasted in the
/// editor carries the attrs it was copied from — is treated as an id on the first line and
/// as no id at all on the rest, so pasting a line makes a second task rather than two
/// blocks quietly sharing one record's due date.
///
/// # Detaching, and what happens when a line comes back
///
/// A row nothing claimed is marked [`Task::detached`] (D-8) and **not** deleted: deleting
/// would silently discard a due date and an assignee somebody set on a board. Retyping a
/// line therefore produces a new task and leaves the old one visibly detached, because a
/// retyped line carries no id.
///
/// **A detached record whose block comes back re-attaches, keeping its state.** Both
/// answers are defensible and this one is chosen, for a reason that is not symmetric: the
/// block id is the identity, so the same id returning *is* the same line returning. That
/// happens on an editor undo and on [`Store::restore_revision`], which republishes an older
/// body — one that carries the ids this function minted into it. The alternative would
/// double every card on a page whose revision was restored, leave the original detached
/// with the due date on it, and store two rows with one `(doc_id, block_id)` between them,
/// which the next pass could not tell apart. Re-attaching also makes the empty-line rule
/// above self-healing.
pub(crate) async fn reconcile_tasks(
    conn: &mut sqlx::SqliteConnection,
    document_id: &str,
    body: &mut Block,
) -> Result<bool> {
    let mut lines: Vec<TaskLine> = Vec::new();
    collect_task_lines(body, &mut lines);

    // `block_id IS NOT NULL`: see the permission note above — these are the rows this
    // function authored. Ordered so that adoption is deterministic and prefers a record
    // still attached to the page over one that has fallen off it.
    let rows: Vec<AnchoredRow> = sqlx::query_as(
        "SELECT id, block_id, title, detached FROM tasks \
         WHERE doc_id = ?1 AND block_id IS NOT NULL \
         ORDER BY detached, position, id",
    )
    .bind(document_id)
    .fetch_all(&mut *conn)
    .await?;

    // The id each line effectively carries, with a repeat inside one document counting as
    // none. Resolved up front so the matching below borrows nothing from `lines`, which is
    // written to afterwards.
    let mut seen: HashSet<&str> = HashSet::new();
    let carried: Vec<Option<String>> = lines
        .iter()
        .map(|line| {
            let id = line.attrs.get(BLOCK_ID).and_then(Value::as_str)?;
            seen.insert(id).then(|| id.to_string())
        })
        .collect();

    let mut row_for_line: Vec<Option<usize>> = vec![None; lines.len()];
    let mut taken: Vec<bool> = vec![false; rows.len()];

    // Pass 1: exact identity.
    for (i, id) in carried.iter().enumerate() {
        let Some(id) = id else { continue };
        if let Some(r) = rows.iter().position(|row| &row.block_id == id) {
            if !taken[r] {
                taken[r] = true;
                row_for_line[i] = Some(r);
            }
        }
    }

    // Pass 2: an id-less line adopts a record with the same words. A line that DOES carry
    // an id and matched nothing is not offered adoption — it names a record, and the honest
    // answer to "that record is not here" is a new one under the id it names, not somebody
    // else's card that happens to read the same.
    for (i, id) in carried.iter().enumerate() {
        if id.is_some() {
            continue;
        }
        let text = &lines[i].text;
        if let Some(r) = (0..rows.len()).find(|&r| !taken[r] && &rows[r].title == text) {
            taken[r] = true;
            row_for_line[i] = Some(r);
        }
    }

    let mut changed = false;
    for (i, line) in lines.iter_mut().enumerate() {
        match row_for_line[i] {
            Some(r) => {
                let row = &rows[r];
                changed |= stamp(line.attrs, &row.block_id);
                // Only when something actually differs, so that publishing an unchanged
                // page does not walk `updated_at` forward on every card in it. `detached`
                // is cleared here and nowhere else: this is a line that came back.
                if row.title != line.text || row.detached != 0 {
                    sqlx::query(
                        "UPDATE tasks SET title = ?2, detached = 0, \
                         updated_at = datetime('now') WHERE id = ?1",
                    )
                    .bind(&row.id)
                    .bind(&line.text)
                    .execute(&mut *conn)
                    .await?;
                }
            }
            None => {
                let block_id = match &carried[i] {
                    Some(id) => id.clone(),
                    None => uuid::Uuid::now_v7().to_string(),
                };
                changed |= stamp(line.attrs, &block_id);
                // `checked` is read HERE and only here. A new record has no state of its
                // own yet, and the tick is the only thing the page can say about it — D-7
                // adopts existing checkbox lines as they were written, and once the page
                // renders its checkbox from the record (D-2) a ticked line that arrived as
                // Offen would visibly untick itself. For a record that already exists the
                // tick is a stale copy and reading it would undo a card somebody dragged,
                // which is exactly the disagreement D-2 exists to prevent.
                //
                // `position` is the line's place in the page, so a page's to-dos land on
                // the board in the order they were written. It is a starting value and
                // nothing more: an existing record's position is the board's, never the
                // page's.
                let status = if line.checked {
                    TaskStatus::Fertig
                } else {
                    TaskStatus::Offen
                };
                sqlx::query(
                    "INSERT INTO tasks (id, doc_id, block_id, title, status, position) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(document_id)
                .bind(&block_id)
                .bind(&line.text)
                .bind(status.as_str())
                .bind(i as i64)
                .execute(&mut *conn)
                .await?;
            }
        }
    }

    // D-8. Marked, never deleted.
    for (r, row) in rows.iter().enumerate() {
        if taken[r] || row.detached != 0 {
            continue;
        }
        sqlx::query("UPDATE tasks SET detached = 1, updated_at = datetime('now') WHERE id = ?1")
            .bind(&row.id)
            .execute(&mut *conn)
            .await?;
    }

    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, NewDocument};
    use gw_auth::{Permission, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Visibility};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    fn empty_body() -> Block {
        Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        }
    }

    /// A page. Restricted unless said otherwise, so a test that forgets a grant fails
    /// closed rather than passing because everything was public.
    async fn page(store: &Store, parent: Option<&str>, title: &str, vis: Visibility) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(str::to_string),
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: vis,
                    body: empty_body(),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap()
    }

    /// A REAL account, in the database. `Principal::test` is not one — `tasks.assignee`
    /// carries a foreign key to `principals`, and the assignment rule loads the assignee's
    /// own groups and teams to ask what THEY may read, which a synthetic principal has no
    /// row to answer from.
    async fn account(store: &Store, username: &str) -> Principal {
        store
            .create_local_principal(username, username, None, "irrelevanter-hash")
            .await
            .unwrap()
    }

    async fn grant(store: &Store, path: &str, who: &Principal, permission: Permission) {
        store
            .add_grant(path, Subject::Principal(who.id.clone()), permission)
            .await
            .unwrap();
    }

    fn anchored(doc_id: &str) -> TaskHome {
        TaskHome::Anchored {
            doc_id: doc_id.to_string(),
            block_id: None,
        }
    }

    fn standalone(project_id: &str) -> TaskHome {
        TaskHome::Standalone {
            project_id: project_id.to_string(),
        }
    }

    fn new_task(home: TaskHome, title: &str) -> NewTask {
        NewTask {
            home,
            title: title.into(),
            status: TaskStatus::Offen,
            assignee: None,
            due_at: None,
            position: 0,
        }
    }

    fn done(outcome: TaskOutcome) -> Task {
        match outcome {
            TaskOutcome::Done(task) => *task,
            other => panic!("expected the change to be accepted, got {other:?}"),
        }
    }

    /// A page, a project homed on it, and somebody who may write that page.
    async fn project_fixture() -> (Store, Principal, String) {
        let store = store().await;
        page(&store, None, "Projekt", Visibility::Restricted).await;
        let chef = account(&store, "chef").await;
        grant(&store, "/projekt", &chef, Permission::Write).await;
        let project = store
            .create_project(&chef, "/projekt", None)
            .await
            .unwrap()
            .expect("creating the project was refused");
        (store, chef, project.id)
    }

    // --- the schema is the source of truth --------------------------------------------

    /// A raw INSERT, bypassing Rust entirely: the closed set of D-9 is enforced by SQLite,
    /// not merely by an enum somebody could go around.
    #[tokio::test]
    async fn sqlite_refuses_a_status_outside_the_fixed_set() {
        let store = store().await;
        let doc = page(&store, None, "Seite", Visibility::Public).await;

        for bogus in ["Erledigt", "offen", "LÄUFT", "Läuft ", "In Bearbeitung", ""] {
            let result = sqlx::query(
                "INSERT INTO tasks (id, doc_id, title, status) VALUES (?1, ?2, 'x', ?3)",
            )
            .bind(uuid::Uuid::now_v7().to_string())
            .bind(&doc)
            .bind(bogus)
            .execute(&store.pool)
            .await;
            assert!(
                result.is_err(),
                "SQLite accepted the status {bogus:?}, so the closed set is only a Rust promise"
            );
        }
    }

    /// The other half, and the one that catches a `ä` written two ways: every spelling the
    /// Rust enum produces has to be a spelling the CHECK constraint accepts, and has to read
    /// back as the same variant.
    #[tokio::test]
    async fn every_status_the_rust_enum_knows_is_accepted_by_the_schema() {
        let store = store().await;
        let doc = page(&store, None, "Seite", Visibility::Public).await;

        for status in TaskStatus::ALL {
            let id = uuid::Uuid::now_v7().to_string();
            sqlx::query("INSERT INTO tasks (id, doc_id, title, status) VALUES (?1, ?2, 'x', ?3)")
                .bind(&id)
                .bind(&doc)
                .bind(status.as_str())
                .execute(&store.pool)
                .await
                .unwrap_or_else(|e| {
                    panic!(
                        "the schema refused {:?}, stored as {:?} ({:?}): {e}",
                        status,
                        status.as_str(),
                        status.as_str().as_bytes()
                    )
                });

            let stored: String = sqlx::query_scalar("SELECT status FROM tasks WHERE id = ?1")
                .bind(&id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
            assert_eq!(TaskStatus::from_stored(&stored), Some(status));
        }
        // And the composed form is what is actually on disk, byte for byte.
        assert_eq!(TaskStatus::Laeuft.as_str().as_bytes(), b"L\xc3\xa4uft");
    }

    #[tokio::test]
    async fn the_schema_refuses_a_task_with_no_governing_page_and_one_with_two() {
        let (store, _chef, project) = project_fixture().await;
        let elsewhere = page(&store, None, "Anderswo", Visibility::Public).await;

        // Neither: nothing could ever authorise a reader for this row.
        let neither = sqlx::query("INSERT INTO tasks (id, title) VALUES (?1, 'x')")
            .bind(uuid::Uuid::now_v7().to_string())
            .execute(&store.pool)
            .await;
        assert!(neither.is_err(), "a task with no governing page was stored");

        // Both: two answers to "which page governs this", free to disagree (D-3).
        let both = sqlx::query(
            "INSERT INTO tasks (id, doc_id, project_id, title) VALUES (?1, ?2, ?3, 'x')",
        )
        .bind(uuid::Uuid::now_v7().to_string())
        .bind(&elsewhere)
        .bind(&project)
        .execute(&store.pool)
        .await;
        assert!(both.is_err(), "a task with two governing pages was stored");
    }

    #[tokio::test]
    async fn the_schema_refuses_a_marker_or_a_block_with_no_page_behind_it() {
        let (store, _chef, project) = project_fixture().await;

        for (column, value) in [("detached", "1"), ("block_id", "'irgendein-block'")] {
            let sql = format!(
                "INSERT INTO tasks (id, project_id, title, {column}) VALUES (?1, ?2, 'x', {value})"
            );
            let result = sqlx::query(&sql)
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(&project)
                .execute(&store.pool)
                .await;
            assert!(
                result.is_err(),
                "a standalone card was stored with {column} set, which names a page it has not got"
            );
        }
    }

    #[tokio::test]
    async fn a_task_cannot_rest_on_somebody_who_is_not_an_account() {
        let store = store().await;
        let doc = page(&store, None, "Seite", Visibility::Public).await;
        let result =
            sqlx::query("INSERT INTO tasks (id, doc_id, title, assignee) VALUES (?1, ?2, 'x', ?3)")
                .bind(uuid::Uuid::now_v7().to_string())
                .bind(&doc)
                .bind("niemand-mit-diesem-namen")
                .execute(&store.pool)
                .await;
        assert!(
            result.is_err(),
            "an assignee with no principal row was stored"
        );
    }

    // --- what a delete takes with it ----------------------------------------------------

    /// The choice made in `0010_tasks.sql`, and the reason it is not D-8's `detached`.
    ///
    /// D-8 keeps a task whose LINE was deleted: the page is still there, still says who may
    /// read it, and the card sits on its board marked. A PURGE is different in kind — the
    /// page and its grants are gone, and the card carries a copy of that page's words (D-2).
    /// Keeping it would leave restricted text on whatever board the card happened to sit on,
    /// with nothing left to check it against.
    #[tokio::test]
    async fn purging_a_page_takes_its_cards_with_it() {
        let store = store().await;
        let doc = page(&store, None, "Seite", Visibility::Restricted).await;
        let chef = account(&store, "chef").await;
        grant(&store, "/seite", &chef, Permission::Write).await;
        done(
            store
                .create_task(&chef, &new_task(anchored(&doc), "Stuhlprobe einschicken"))
                .await
                .unwrap(),
        );

        sqlx::query("DELETE FROM documents WHERE id = ?1")
            .bind(&doc)
            .execute(&store.pool)
            .await
            .unwrap();

        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            left, 0,
            "a card outlived the page whose words it holds and whose grants decided who \
             could read them"
        );
    }

    #[tokio::test]
    async fn deleting_a_project_takes_its_standalone_cards_but_not_the_pages_own_tasks() {
        let (store, chef, project) = project_fixture().await;
        let unterseite = page(&store, Some("/projekt"), "Befunde", Visibility::Restricted).await;

        done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Lose Karte"))
                .await
                .unwrap(),
        );
        let anchored_task = done(
            store
                .create_task(&chef, &new_task(anchored(&unterseite), "In der Seite"))
                .await
                .unwrap(),
        );

        assert!(store.delete_project(&chef, &project).await.unwrap());

        let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM tasks")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            ids,
            vec![anchored_task.id],
            "deleting a project must take the cards that belong to it and nothing else — a \
             task written into a page is governed by that page, not by the board it showed on"
        );
    }

    #[tokio::test]
    async fn purging_a_projects_home_page_takes_the_project_and_its_cards() {
        let (store, chef, project) = project_fixture().await;
        done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Lose Karte"))
                .await
                .unwrap(),
        );

        sqlx::query("DELETE FROM documents WHERE path = '/projekt'")
            .execute(&store.pool)
            .await
            .unwrap();

        let projects: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        let tasks: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!((projects, tasks), (0, 0), "the cascade stopped half way");
    }

    // --- the ordinary shape of the thing ------------------------------------------------

    #[tokio::test]
    async fn a_task_is_created_read_changed_and_deleted() {
        let (store, chef, project) = project_fixture().await;

        let created = done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Termin ausmachen"))
                .await
                .unwrap(),
        );
        assert_eq!(created.status, TaskStatus::Offen);
        assert!(!created.detached);
        assert_eq!(created.project_id.as_deref(), Some(project.as_str()));

        let read = store.task_for(&chef, &created.id).await.unwrap().unwrap();
        assert_eq!(read, created);

        let changed = done(
            store
                .update_task(
                    &chef,
                    &created.id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Laeuft),
                        due_at: Some(Some("2026-09-01".into())),
                        position: Some(3),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(changed.status, TaskStatus::Laeuft);
        assert_eq!(changed.due_at.as_deref(), Some("2026-09-01"));
        assert_eq!(changed.position, 3);
        assert_eq!(
            changed.title, "Termin ausmachen",
            "an untouched field moved"
        );

        // And a due date can be cleared again, which `COALESCE(?, column)` could not express.
        let cleared = done(
            store
                .update_task(
                    &chef,
                    &created.id,
                    &TaskUpdate {
                        due_at: Some(None),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(cleared.due_at, None);

        assert!(store.delete_task(&chef, &created.id).await.unwrap());
        assert!(store.task_for(&chef, &created.id).await.unwrap().is_none());
        assert!(!store.delete_task(&chef, &created.id).await.unwrap());
    }

    #[tokio::test]
    async fn a_project_is_created_read_listed_changed_and_deleted() {
        let (store, chef, project) = project_fixture().await;

        let read = store.project_for(&chef, &project).await.unwrap().unwrap();
        assert_eq!(read.home_path, "/projekt");
        assert_eq!(read.home_title, "Projekt");
        assert_eq!(read.tag_id, None);

        assert!(store
            .set_project_tag(&chef, &project, Some("thema-darm"))
            .await
            .unwrap());
        let tagged = store.project_for(&chef, &project).await.unwrap().unwrap();
        assert_eq!(tagged.tag_id.as_deref(), Some("thema-darm"));

        let listed = store.projects_for(&chef).await.unwrap();
        assert_eq!(listed, vec![tagged]);

        assert!(store.delete_project(&chef, &project).await.unwrap());
        assert!(store.project_for(&chef, &project).await.unwrap().is_none());
        assert!(store.projects_for(&chef).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_project_is_made_and_administered_only_by_somebody_who_may_write_its_home_page() {
        // Read on the page is not enough for any of the three. A project is a claim made
        // about a page and the subtree below it, and deleting one takes its cards with it.
        let (store, chef, project) = project_fixture().await;
        let leser = account(&store, "leser").await;
        grant(&store, "/projekt", &leser, Permission::Read).await;
        page(&store, None, "Zweites", Visibility::Restricted).await;
        grant(&store, "/zweites", &leser, Permission::Read).await;

        assert!(
            store
                .create_project(&leser, "/zweites", None)
                .await
                .unwrap()
                .is_none(),
            "read on a page was enough to make it a project home"
        );
        assert!(
            !store
                .set_project_tag(&leser, &project, Some("thema"))
                .await
                .unwrap(),
            "read on the home page was enough to retag the project"
        );
        assert!(
            !store.delete_project(&leser, &project).await.unwrap(),
            "read on the home page was enough to delete the project"
        );

        // Anti-vacuity: with write, each of them goes through.
        grant(&store, "/zweites", &chef, Permission::Write).await;
        assert!(store
            .create_project(&chef, "/zweites", None)
            .await
            .unwrap()
            .is_some());
        assert!(store
            .set_project_tag(&chef, &project, Some("thema"))
            .await
            .unwrap());
        assert!(store.delete_project(&chef, &project).await.unwrap());
    }

    #[tokio::test]
    async fn a_board_carries_the_pages_tasks_as_well_as_its_loose_cards_in_column_order() {
        let (store, chef, project) = project_fixture().await;
        let unterseite = page(&store, Some("/projekt"), "Befunde", Visibility::Restricted).await;
        grant(&store, "/projekt/befunde", &chef, Permission::Write).await;

        for (home, title, status, position) in [
            (standalone(&project), "Karte C", TaskStatus::Fertig, 0),
            (standalone(&project), "Karte A", TaskStatus::Offen, 1),
            (anchored(&unterseite), "Zeile B", TaskStatus::Laeuft, 0),
            (anchored(&unterseite), "Zeile A", TaskStatus::Offen, 0),
        ] {
            done(
                store
                    .create_task(
                        &chef,
                        &NewTask {
                            status,
                            position,
                            ..new_task(home, title)
                        },
                    )
                    .await
                    .unwrap(),
            );
        }

        let board = store.board_for(&chef, &project).await.unwrap();
        let titles: Vec<&str> = board.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Zeile A", "Karte A", "Zeile B", "Karte C"],
            "the board is ordered Offen, Läuft, Fertig — D-9's columns — and by position \
             within each"
        );
    }

    /// The prefix bug, in the one place it would be a disclosure rather than a nuisance:
    /// `/projektierung` is not inside `/projekt`, and a `LIKE '/projekt%'` would put its
    /// tasks on somebody else's board.
    #[tokio::test]
    async fn a_subtree_boundary_is_a_segment_and_not_a_prefix() {
        assert!(within("/projekt", "/projekt"));
        assert!(within("/projekt", "/projekt/befunde"));
        assert!(!within("/projekt", "/projektierung"));
        assert!(!within("/projekt", "/anderes"));

        let (store, chef, project) = project_fixture().await;
        let nachbar = page(&store, None, "Projektierung", Visibility::Public).await;
        grant(&store, "/projektierung", &chef, Permission::Write).await;
        done(
            store
                .create_task(&chef, &new_task(anchored(&nachbar), "Fremde Zeile"))
                .await
                .unwrap(),
        );

        let board = store.board_for(&chef, &project).await.unwrap();
        assert!(
            board.is_empty(),
            "a page whose path merely starts with the project's landed on its board: {board:?}"
        );
    }

    // --- the disclosure surface ---------------------------------------------------------

    /// The property the design names as the one most likely to be got wrong by an aggregate
    /// query written in a hurry, and D-3's consequence: the filtering is PER DOCUMENT.
    ///
    /// `/projekt` is readable by `leser` and `/projekt/geheim` is not, and both are inside
    /// the one home subtree. A board that trusted the subtree — the natural thing to write,
    /// because a project IS a subtree — would hand over the secret page's card, and the
    /// card's title is that page's own words (D-2).
    #[tokio::test]
    async fn a_board_omits_a_card_whose_page_the_caller_may_not_read() {
        let (store, chef, project) = project_fixture().await;
        let offen = page(&store, Some("/projekt"), "Offen", Visibility::Public).await;
        let geheim = page(&store, Some("/projekt"), "Geheim", Visibility::Restricted).await;
        grant(&store, "/projekt/geheim", &chef, Permission::Write).await;

        done(
            store
                .create_task(&chef, &new_task(anchored(&offen), "Harmlos"))
                .await
                .unwrap(),
        );
        done(
            store
                .create_task(&chef, &new_task(anchored(&geheim), "Befund besprechen"))
                .await
                .unwrap(),
        );
        done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Lose Karte"))
                .await
                .unwrap(),
        );

        let leser = account(&store, "leser").await;
        grant(&store, "/projekt", &leser, Permission::Read).await;

        let board = store.board_for(&leser, &project).await.unwrap();
        let titles: Vec<&str> = board.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(
            titles,
            vec!["Harmlos", "Lose Karte"],
            "a card leaked a page the caller cannot read, or omitted one they can"
        );

        // Anti-vacuity: the card really is on the board for somebody who may read the page,
        // so the assertion above is about filtering and not about an empty fixture.
        let all = store.board_for(&chef, &project).await.unwrap();
        assert_eq!(
            all.len(),
            3,
            "the fixture never had a card to hide: {all:?}"
        );
    }

    /// A card that cannot say where its line was written is much less useful: "where did
    /// I write this?" is the first question anybody asks of a board.
    ///
    /// The page is the one the permission-checked accessor handed back — its own `path`
    /// and `title`, taken off the document that was authorised, never a second lookup keyed
    /// by the card's `doc_id`. The tempting version of that lookup is safe TODAY only because
    /// the card was filtered a few lines earlier, which makes the safety of one query a
    /// property of another one.
    ///
    /// A **loose** card names no page, and that is not a missing feature: no page holds it,
    /// and naming its project's home — the page that governs it — would say a line exists
    /// somewhere that never held one.
    #[tokio::test]
    async fn a_card_names_the_page_its_line_was_written_on_and_a_loose_card_names_none() {
        let (store, chef, project) = project_fixture().await;
        let befunde = page(&store, Some("/projekt"), "Befunde", Visibility::Restricted).await;
        grant(&store, "/projekt/befunde", &chef, Permission::Write).await;

        done(
            store
                .create_task(&chef, &new_task(anchored(&befunde), "Zeile"))
                .await
                .unwrap(),
        );
        done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Lose Karte"))
                .await
                .unwrap(),
        );

        let board = store.board_for(&chef, &project).await.unwrap();
        let named: Vec<(&str, Option<(&str, &str)>)> = board
            .iter()
            .map(|task| {
                (
                    task.title.as_str(),
                    task.page
                        .as_ref()
                        .map(|page| (page.path.as_str(), page.title.as_str())),
                )
            })
            .collect();
        assert_eq!(
            named,
            vec![
                ("Zeile", Some(("/projekt/befunde", "Befunde"))),
                ("Lose Karte", None),
            ],
            "a card did not name the page its line was written on, or a loose card named \
             one that never held it"
        );

        // The same card, read on its own and read on its page: one answer, three ways in.
        let one = store.task_for(&chef, &board[0].id).await.unwrap().unwrap();
        assert_eq!(one.page, board[0].page);
        let on_the_page = store.tasks_for_document(&chef, &befunde).await.unwrap();
        assert_eq!(on_the_page[0].page, board[0].page);
    }

    /// The page is resolved when the board is read, never copied onto the record when the
    /// card is made — D-5's rule for a link, which holds for a card for the same reason.
    /// Renaming a page must not leave a board saying what it used to be called.
    #[tokio::test]
    async fn a_renamed_page_is_named_on_the_board_by_what_it_is_called_now() {
        let (store, chef, project) = project_fixture().await;
        let befunde = page(&store, Some("/projekt"), "Befunde", Visibility::Restricted).await;
        grant(&store, "/projekt/befunde", &chef, Permission::Write).await;
        done(
            store
                .create_task(&chef, &new_task(anchored(&befunde), "Zeile"))
                .await
                .unwrap(),
        );

        sqlx::query("UPDATE documents SET title = ?2 WHERE id = ?1")
            .bind(&befunde)
            .bind("Laborbefunde")
            .execute(&store.pool)
            .await
            .unwrap();

        let board = store.board_for(&chef, &project).await.unwrap();
        assert_eq!(
            board[0].page.as_ref().map(|page| page.title.as_str()),
            Some("Laborbefunde"),
            "the board named the page by a title it no longer has"
        );
    }

    #[tokio::test]
    async fn a_board_is_refused_entirely_to_somebody_who_may_not_read_the_project_home() {
        let (store, chef, project) = project_fixture().await;
        done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Lose Karte"))
                .await
                .unwrap(),
        );

        let fremder = account(&store, "fremder").await;
        assert!(
            store
                .board_for(&fremder, &project)
                .await
                .unwrap()
                .is_empty(),
            "a board answered somebody who may not read the page it belongs to"
        );
        assert!(store
            .project_for(&fremder, &project)
            .await
            .unwrap()
            .is_none());
        assert!(store.projects_for(&fremder).await.unwrap().is_empty());

        // And a project that does not exist is the same empty answer, not a distinguishable
        // error — otherwise the call answers "does this project exist" to anybody who asks.
        assert!(store
            .board_for(&chef, "gibt-es-nicht")
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn a_single_task_is_refused_to_somebody_who_may_not_read_its_governing_page() {
        let (store, chef, project) = project_fixture().await;
        let geheim = page(&store, None, "Geheim", Visibility::Restricted).await;
        grant(&store, "/geheim", &chef, Permission::Write).await;

        let card = done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Lose Karte"))
                .await
                .unwrap(),
        );
        let line = done(
            store
                .create_task(&chef, &new_task(anchored(&geheim), "Befund besprechen"))
                .await
                .unwrap(),
        );

        let fremder = account(&store, "fremder").await;
        for id in [&card.id, &line.id] {
            assert!(
                store.task_for(&fremder, id).await.unwrap().is_none(),
                "a task answered somebody who may not read the page that governs it"
            );
        }
        // Anti-vacuity.
        assert!(store.task_for(&chef, &card.id).await.unwrap().is_some());
        assert!(store.task_for(&chef, &line.id).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn a_page_in_the_trash_takes_its_tasks_off_the_board() {
        let (store, chef, project) = project_fixture().await;
        let unterseite = page(&store, Some("/projekt"), "Befunde", Visibility::Public).await;
        let task = done(
            store
                .create_task(&chef, &new_task(anchored(&unterseite), "Zeile"))
                .await
                .unwrap(),
        );
        assert_eq!(store.board_for(&chef, &project).await.unwrap().len(), 1);

        sqlx::query("UPDATE documents SET deleted_at = datetime('now') WHERE id = ?1")
            .bind(&unterseite)
            .execute(&store.pool)
            .await
            .unwrap();

        assert!(
            store.board_for(&chef, &project).await.unwrap().is_empty(),
            "a soft-deleted page's card stayed on the board"
        );
        assert!(store.task_for(&chef, &task.id).await.unwrap().is_none());
        // The record is still there: the page is in the trash, not purged, and restoring it
        // must bring the due dates back with it.
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(left, 1);
    }

    #[tokio::test]
    async fn a_pages_tasks_are_refused_to_somebody_who_may_not_read_it() {
        let store = store().await;
        let geheim = page(&store, None, "Geheim", Visibility::Restricted).await;
        let chef = account(&store, "chef").await;
        grant(&store, "/geheim", &chef, Permission::Write).await;
        done(
            store
                .create_task(&chef, &new_task(anchored(&geheim), "Befund besprechen"))
                .await
                .unwrap(),
        );

        let fremder = account(&store, "fremder").await;
        assert!(
            store
                .tasks_for_document(&fremder, &geheim)
                .await
                .unwrap()
                .is_empty(),
            "a page's tasks were listed to somebody who may not read the page"
        );
        assert_eq!(
            store
                .tasks_for_document(&chef, &geheim)
                .await
                .unwrap()
                .len(),
            1,
            "the fixture never had a task to hide"
        );
    }

    // --- D-10: who may assign whom ------------------------------------------------------
    //
    // Four clauses, four tests. The rule is stated in full on `Store::create_task`.

    /// Clause 1. The governing page is the ANCHOR for an anchored task and the PROJECT HOME
    /// for a standalone one — and nothing else is. `anker` may write `/anker` only and
    /// `mia` may write `/projekt` only, so each of them is refused exactly the task the
    /// other governs. Write on some other page in the wiki confers nothing.
    #[tokio::test]
    async fn the_governing_page_is_the_anchor_for_an_anchored_task_and_the_project_home_for_a_standalone_one(
    ) {
        let (store, chef, project) = project_fixture().await;
        let anker_doc = page(&store, None, "Anker", Visibility::Restricted).await;

        let anker = account(&store, "anker").await;
        grant(&store, "/anker", &anker, Permission::Write).await;
        let mia = account(&store, "mia").await;
        grant(&store, "/projekt", &mia, Permission::Write).await;

        // Each is created by the person who governs it, which is already half the clause:
        // `chef` may write /projekt and could not have written the anchored one.
        let line = done(
            store
                .create_task(&anker, &new_task(anchored(&anker_doc), "Zeile"))
                .await
                .unwrap(),
        );
        assert_eq!(
            store
                .create_task(&chef, &new_task(anchored(&anker_doc), "Zeile"))
                .await
                .unwrap(),
            TaskOutcome::Refused,
            "write on the project home created a task anchored to another page"
        );
        let card = done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Karte"))
                .await
                .unwrap(),
        );

        let rename = |to: &str| TaskUpdate {
            title: Some(to.to_string()),
            ..Default::default()
        };

        // The anchored task follows /anker.
        done(
            store
                .update_task(&anker, &line.id, &rename("A"))
                .await
                .unwrap(),
        );
        assert_eq!(
            store
                .update_task(&mia, &line.id, &rename("B"))
                .await
                .unwrap(),
            TaskOutcome::Refused,
            "write on the project home reached a task anchored to another page"
        );

        // The standalone card follows /projekt.
        done(
            store
                .update_task(&mia, &card.id, &rename("C"))
                .await
                .unwrap(),
        );
        assert_eq!(
            store
                .update_task(&anker, &card.id, &rename("D"))
                .await
                .unwrap(),
            TaskOutcome::Refused,
            "write on an unrelated page reached a card governed by the project home"
        );
    }

    /// Clause 2. Write on the governing page is what permits creating and changing a task,
    /// including setting its assignee. Read is not enough, and neither is being the person
    /// the task rests on.
    #[tokio::test]
    async fn writing_the_governing_page_is_what_permits_creating_and_changing_a_task() {
        let (store, chef, project) = project_fixture().await;
        let leser = account(&store, "leser").await;
        grant(&store, "/projekt", &leser, Permission::Read).await;

        assert_eq!(
            store
                .create_task(&leser, &new_task(standalone(&project), "Karte"))
                .await
                .unwrap(),
            TaskOutcome::Refused,
            "read on the governing page created a task"
        );

        let card = done(
            store
                .create_task(
                    &chef,
                    &NewTask {
                        assignee: Some(leser.id.clone()),
                        ..new_task(standalone(&project), "Karte")
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(card.assignee.as_deref(), Some(leser.id.as_str()));

        // The assignee may READ the board — that is what made the assignment legal — and
        // still may not move their own card, or reassign it to somebody else.
        for update in [
            TaskUpdate {
                status: Some(TaskStatus::Fertig),
                ..Default::default()
            },
            TaskUpdate {
                assignee: Some(Some(chef.id.clone())),
                ..Default::default()
            },
            TaskUpdate {
                assignee: Some(None),
                ..Default::default()
            },
        ] {
            assert_eq!(
                store.update_task(&leser, &card.id, &update).await.unwrap(),
                TaskOutcome::Refused,
                "read on the governing page changed a task: {update:?}"
            );
        }
        assert!(
            !store.delete_task(&leser, &card.id).await.unwrap(),
            "read on the governing page deleted a task"
        );

        // Anti-vacuity: with write, every one of those goes through.
        done(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Fertig),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        assert!(store.delete_task(&chef, &card.id).await.unwrap());
    }

    /// Clause 3, and the security-relevant one. A task may not be assigned to somebody who
    /// may not READ its governing page: the obligation would be invisible to them, and the
    /// card's title would tell them what a page they may not open is called.
    #[tokio::test]
    async fn a_task_may_not_be_assigned_to_somebody_who_may_not_read_its_governing_page() {
        let (store, chef, project) = project_fixture().await;
        let fremder = account(&store, "fremder").await;
        let leser = account(&store, "leser").await;
        grant(&store, "/projekt", &leser, Permission::Read).await;

        // At creation.
        assert_eq!(
            store
                .create_task(
                    &chef,
                    &NewTask {
                        assignee: Some(fremder.id.clone()),
                        ..new_task(standalone(&project), "Karte")
                    }
                )
                .await
                .unwrap(),
            TaskOutcome::AssigneeMayNotRead,
        );
        let stored: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(stored, 0, "the refused task was written anyway");

        // And on an existing task.
        let card = done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Karte"))
                .await
                .unwrap(),
        );
        assert_eq!(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        assignee: Some(Some(fremder.id.clone())),
                        ..Default::default()
                    }
                )
                .await
                .unwrap(),
            TaskOutcome::AssigneeMayNotRead,
        );
        assert_eq!(
            store
                .task_for(&chef, &card.id)
                .await
                .unwrap()
                .unwrap()
                .assignee,
            None,
            "the refusal still wrote the name"
        );

        // A name that is not an account at all is the same answer: an obligation cannot
        // rest on somebody who cannot sign in.
        assert_eq!(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        assignee: Some(Some("niemand".into())),
                        ..Default::default()
                    }
                )
                .await
                .unwrap(),
            TaskOutcome::AssigneeMayNotRead,
        );

        // Anti-vacuity: somebody who may read the page CAN be assigned, so the refusals
        // above are about the read and not about assignment being broken outright.
        let assigned = done(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        assignee: Some(Some(leser.id.clone())),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(assigned.assignee.as_deref(), Some(leser.id.as_str()));
    }

    /// Clause 4. Unassigning needs only Write on the governing page. It asks nothing about
    /// the person being removed — who by then may well have lost the read that made the
    /// assignment legal in the first place. Without this, revoking somebody's access would
    /// pin their name to the card forever.
    #[tokio::test]
    async fn unassigning_needs_only_write_on_the_governing_page_not_the_assignees_read() {
        let (store, chef, project) = project_fixture().await;
        let kollegin = account(&store, "kollegin").await;
        grant(&store, "/projekt", &kollegin, Permission::Read).await;

        let card = done(
            store
                .create_task(
                    &chef,
                    &NewTask {
                        assignee: Some(kollegin.id.clone()),
                        ..new_task(standalone(&project), "Karte")
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(card.assignee.as_deref(), Some(kollegin.id.as_str()));

        // She leaves. The grant goes, and with it any right she had to see this page.
        store
            .remove_grant(
                "/projekt",
                &Subject::Principal(kollegin.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        assert!(
            !store
                .may_be_assigned(&kollegin.id, "/projekt")
                .await
                .unwrap(),
            "the fixture did not actually take her access away"
        );

        // Re-assigning her now would be refused — that is clause 3 — but clearing the stale
        // name must still work.
        assert_eq!(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        assignee: Some(Some(kollegin.id.clone())),
                        ..Default::default()
                    }
                )
                .await
                .unwrap(),
            TaskOutcome::AssigneeMayNotRead,
        );
        let cleared = done(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        assignee: Some(None),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(
            cleared.assignee, None,
            "a stale assignee could not be cleared"
        );
    }

    // --- moving a card, which moves its governing page ----------------------------------

    #[tokio::test]
    async fn moving_a_card_needs_write_on_the_board_it_is_going_to_as_well() {
        let (store, chef, project) = project_fixture().await;
        page(&store, None, "Anderes", Visibility::Restricted).await;
        let owner = account(&store, "anderer").await;
        grant(&store, "/anderes", &owner, Permission::Write).await;
        let other = store
            .create_project(&owner, "/anderes", None)
            .await
            .unwrap()
            .unwrap();

        let card = done(
            store
                .create_task(&chef, &new_task(standalone(&project), "Karte"))
                .await
                .unwrap(),
        );
        let move_it = TaskUpdate {
            project_id: Some(other.id.clone()),
            ..Default::default()
        };
        assert_eq!(
            store.update_task(&chef, &card.id, &move_it).await.unwrap(),
            TaskOutcome::Refused,
            "a card was walked onto a board the caller may not write — and with it, out of \
             the reach of the page that was governing it"
        );

        // Being able to READ the destination is not enough, and this is what tells the two
        // apart: without it, asking the destination for Read instead of Write would answer
        // the same as asking for Write, because chef could do neither.
        grant(&store, "/anderes", &chef, Permission::Read).await;
        assert_eq!(
            store.update_task(&chef, &card.id, &move_it).await.unwrap(),
            TaskOutcome::Refused,
            "read on the destination board was enough to move a card onto it"
        );

        // Anti-vacuity: with write on both ends the move goes through.
        grant(&store, "/anderes", &chef, Permission::Write).await;
        let moved = done(store.update_task(&chef, &card.id, &move_it).await.unwrap());
        assert_eq!(moved.project_id.as_deref(), Some(other.id.as_str()));
    }

    #[tokio::test]
    async fn moving_a_card_does_not_carry_its_assignee_onto_a_board_they_cannot_read() {
        let (store, chef, project) = project_fixture().await;
        page(&store, None, "Anderes", Visibility::Restricted).await;
        grant(&store, "/anderes", &chef, Permission::Write).await;
        let other = store
            .create_project(&chef, "/anderes", None)
            .await
            .unwrap()
            .unwrap();

        // She may read /projekt and not /anderes, so the assignment is legal where the card
        // is now and is exactly what clause 3 forbids where it is going.
        let kollegin = account(&store, "kollegin").await;
        grant(&store, "/projekt", &kollegin, Permission::Read).await;
        let card = done(
            store
                .create_task(
                    &chef,
                    &NewTask {
                        assignee: Some(kollegin.id.clone()),
                        ..new_task(standalone(&project), "Karte")
                    },
                )
                .await
                .unwrap(),
        );

        assert_eq!(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        project_id: Some(other.id.clone()),
                        ..Default::default()
                    }
                )
                .await
                .unwrap(),
            TaskOutcome::AssigneeMayNotRead,
            "a move carried an assignee onto a board whose page they may not open"
        );
        assert_eq!(
            store
                .task_for(&chef, &card.id)
                .await
                .unwrap()
                .unwrap()
                .project_id
                .as_deref(),
            Some(project.as_str()),
            "the refused move happened anyway"
        );

        // Clearing the name in the same call is the way through, and it must work.
        let moved = done(
            store
                .update_task(
                    &chef,
                    &card.id,
                    &TaskUpdate {
                        project_id: Some(other.id.clone()),
                        assignee: Some(None),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        assert_eq!(moved.project_id.as_deref(), Some(other.id.as_str()));
        assert_eq!(moved.assignee, None);
    }

    #[tokio::test]
    async fn an_anchored_task_cannot_be_filed_under_a_project_by_hand() {
        // D-3 decides which project an anchored task belongs to — the one whose home
        // subtree its page is in. A second answer on the card could disagree with the page
        // it is visibly written on.
        let (store, chef, project) = project_fixture().await;
        let doc = page(&store, None, "Seite", Visibility::Restricted).await;
        grant(&store, "/seite", &chef, Permission::Write).await;
        let line = done(
            store
                .create_task(&chef, &new_task(anchored(&doc), "Zeile"))
                .await
                .unwrap(),
        );

        assert_eq!(
            store
                .update_task(
                    &chef,
                    &line.id,
                    &TaskUpdate {
                        project_id: Some(project.clone()),
                        ..Default::default()
                    }
                )
                .await
                .unwrap(),
            TaskOutcome::Refused,
        );
    }

    // --- reconciliation on publish (D-2, D-6, D-7, D-8) --------------------------------

    /// One checklist line, unticked, carrying **no id** — exactly what
    /// `gw_core::markdown::convert` produces and what the editor sends for a line somebody
    /// has just typed.
    fn task_line(text: &str) -> Block {
        let mut item = Block {
            kind: BlockKind::TaskItem,
            attrs: Default::default(),
            content: vec![Block {
                kind: BlockKind::Paragraph,
                attrs: Default::default(),
                content: vec![Block {
                    kind: BlockKind::Text,
                    attrs: Default::default(),
                    content: Vec::new(),
                    text: Some(text.into()),
                    marks: Vec::new(),
                }],
                text: None,
                marks: Vec::new(),
            }],
            text: None,
            marks: Vec::new(),
        };
        item.attrs
            .insert("checked".into(), serde_json::Value::Bool(false));
        item
    }

    /// A document body holding one checklist with these lines.
    fn body_with_tasks(lines: &[&str]) -> Block {
        let list = Block {
            kind: BlockKind::TaskList,
            attrs: Default::default(),
            content: lines.iter().map(|t| task_line(t)).collect(),
            text: None,
            marks: Vec::new(),
        };
        Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: vec![list],
            text: None,
            marks: Vec::new(),
        }
    }

    /// A page and somebody who may write it.
    async fn writable_page(title: &str, path: &str) -> (Store, Principal, String) {
        let store = store().await;
        let doc = page(&store, None, title, Visibility::Restricted).await;
        let chef = account(&store, "chef").await;
        grant(&store, path, &chef, Permission::Write).await;
        (store, chef, doc)
    }

    /// The body the store actually holds for a document, parsed back.
    async fn stored_body(store: &Store, doc: &str) -> Block {
        let json: String = sqlx::query_scalar("SELECT body FROM documents WHERE id = ?1")
            .bind(doc)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        serde_json::from_str(&json).unwrap()
    }

    /// Every `id` a task block in this tree carries, in document order.
    fn block_ids(body: &Block) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(block: &Block, out: &mut Vec<String>) {
            if block.kind == BlockKind::TaskItem {
                if let Some(id) = block.attrs.get("id").and_then(|v| v.as_str()) {
                    out.push(id.to_string());
                }
            }
            for child in &block.content {
                walk(child, out);
            }
        }
        walk(body, &mut out);
        out
    }

    #[tokio::test]
    async fn a_checkbox_line_becomes_a_task_on_publish() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;

        store
            .publish_revision(
                &chef,
                &doc,
                &body_with_tasks(&["Stuhlprobe einschicken"]),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");

        let tasks = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(tasks.len(), 1, "a checkbox line is a task (D-6)");
        assert_eq!(tasks[0].title, "Stuhlprobe einschicken");
        assert_eq!(tasks[0].status, TaskStatus::Offen);
        assert!(!tasks[0].detached);
        assert_eq!(
            tasks[0].block_id.as_deref(),
            block_ids(&stored_body(&store, &doc).await)
                .first()
                .map(String::as_str),
            "the record points at the id the stored block carries"
        );
    }

    /// **The test to trust most.** Publishing the same content twice must not mint a second
    /// id: if it does, the first task is orphaned, marked detached, and the board sheds a
    /// card — with its due date and its assignee — on every single save.
    #[tokio::test]
    async fn publishing_the_same_content_twice_keeps_one_task_with_the_same_id() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;

        for _ in 0..3 {
            store
                .publish_revision(&chef, &doc, &body_with_tasks(&["Kaffee kaufen"]), None)
                .await
                .unwrap()
                .expect("the publish was refused");
        }

        let tasks = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(
            tasks.len(),
            1,
            "publishing the same content again minted a second task: {tasks:#?}"
        );
        assert!(
            !tasks[0].detached,
            "the task detached itself while its line never moved"
        );
    }

    /// The other half of idempotence, and the half that is easy to get wrong invisibly: if
    /// the ids are minted into a copy and the revision is written from the original, every
    /// publish re-mints forever and the test above only passes by accident of adoption.
    #[tokio::test]
    async fn the_body_that_is_stored_is_the_body_the_ids_were_minted_into() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;

        let revision = store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Kaffee kaufen"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let ids = block_ids(&stored_body(&store, &doc).await);
        assert_eq!(ids.len(), 1, "the stored page carries no minted id");

        // And the revision, which is what a restore republishes and what an export reads.
        let stored = store
            .revision_for(&chef, &revision)
            .await
            .unwrap()
            .expect("the revision was refused");
        let body: Block = serde_json::from_str(&stored.body).unwrap();
        assert_eq!(
            block_ids(&body),
            ids,
            "the revision carries a different tree"
        );
        assert_eq!(
            stored.byte_size as usize,
            stored.body.len(),
            "the recorded size describes a body that was never written"
        );
    }

    /// Change the words of the first checklist line, leaving its id where it is — which is
    /// what editing a line in the editor does, and what a *retyped* line pointedly is not.
    fn reword(body: &mut Block, text: &str) -> bool {
        if body.kind == BlockKind::TaskItem {
            if let Some(leaf) = body
                .content
                .iter_mut()
                .find(|c| c.kind == BlockKind::Paragraph)
                .and_then(|p| p.content.first_mut())
            {
                leaf.text = Some(text.into());
                return true;
            }
        }
        body.content.iter_mut().any(|child| reword(child, text))
    }

    /// D-2 in one test: republishing with the words changed updates the title and touches
    /// nothing else. Get this backwards and dragging a card is undone by the next save.
    #[tokio::test]
    async fn the_page_owns_the_words_and_the_record_owns_the_state() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Termin machen"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let task = store
            .tasks_for_document(&chef, &doc)
            .await
            .unwrap()
            .remove(0);
        done(
            store
                .update_task(
                    &chef,
                    &task.id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Laeuft),
                        assignee: Some(Some(chef.id.clone())),
                        due_at: Some(Some("2026-09-01".into())),
                        position: Some(42),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );

        // The stored tree, reworded in place: the line keeps its identity and changes its
        // words, exactly as typing into it in the editor does.
        let mut edited = stored_body(&store, &doc).await;
        assert!(reword(&mut edited, "Termin verschieben"));
        store
            .publish_revision(&chef, &doc, &edited, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(after.len(), 1, "editing the words made a second task");
        assert_eq!(after[0].id, task.id, "editing the words made a new record");
        assert_eq!(
            after[0].title, "Termin verschieben",
            "the page owns the words"
        );
        assert_eq!(
            after[0].status,
            TaskStatus::Laeuft,
            "the record owns the state"
        );
        assert_eq!(after[0].assignee.as_deref(), Some(chef.id.as_str()));
        assert_eq!(after[0].due_at.as_deref(), Some("2026-09-01"));
        assert_eq!(after[0].position, 42);
    }

    /// D-8: the record survives its line, marked. Deleting it would discard a due date and
    /// an assignee somebody set on a board, which is the whole reason the column exists.
    #[tokio::test]
    async fn a_line_that_disappears_leaves_its_record_detached_rather_than_deleted() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Rezept holen"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        let task = store
            .tasks_for_document(&chef, &doc)
            .await
            .unwrap()
            .remove(0);
        done(
            store
                .update_task(
                    &chef,
                    &task.id,
                    &TaskUpdate {
                        due_at: Some(Some("2026-09-01".into())),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );

        store
            .publish_revision(&chef, &doc, &empty_body(), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(after.len(), 1, "the record was deleted with its line");
        assert_eq!(after[0].id, task.id);
        assert!(after[0].detached, "the record was not marked detached");
        assert_eq!(
            after[0].due_at.as_deref(),
            Some("2026-09-01"),
            "the due date somebody set went with the line"
        );
    }

    /// D-8's stated consequence, and the reason detachment is visible rather than silent: a
    /// retyped line is a NEW to-do, because it carries no id. One task does not quietly
    /// become another.
    #[tokio::test]
    async fn retyping_a_line_makes_a_new_record_and_leaves_the_old_one_detached() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Alt"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        let first = store
            .tasks_for_document(&chef, &doc)
            .await
            .unwrap()
            .remove(0);

        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Neu"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(
            after.len(),
            2,
            "expected the old record and a new one: {after:#?}"
        );
        let old = after
            .iter()
            .find(|t| t.id == first.id)
            .expect("the old record is gone");
        assert_eq!(old.title, "Alt");
        assert!(old.detached, "the old record was mutated into the new one");
        let new = after
            .iter()
            .find(|t| t.id != first.id)
            .expect("no new record");
        assert_eq!(new.title, "Neu");
        assert!(!new.detached);
    }

    /// The decision this task had to make deliberately, and it is documented on
    /// [`reconcile_tasks`]: a detached record whose BLOCK comes back re-attaches, keeping
    /// its due date. The same id returning is the same line returning — which is what an
    /// editor undo and a revision restore both produce.
    #[tokio::test]
    async fn a_line_that_comes_back_re_attaches_its_record_with_its_due_date() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Blutbild"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        // The tree as it was stored — ids and all. This is what an undo restores.
        let with_ids = stored_body(&store, &doc).await;

        let task = store
            .tasks_for_document(&chef, &doc)
            .await
            .unwrap()
            .remove(0);
        done(
            store
                .update_task(
                    &chef,
                    &task.id,
                    &TaskUpdate {
                        due_at: Some(Some("2026-10-02".into())),
                        status: Some(TaskStatus::Laeuft),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );
        store
            .publish_revision(&chef, &doc, &empty_body(), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        assert!(store.tasks_for_document(&chef, &doc).await.unwrap()[0].detached);

        store
            .publish_revision(&chef, &doc, &with_ids, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(
            after.len(),
            1,
            "the returning line doubled the card: {after:#?}"
        );
        assert_eq!(after[0].id, task.id);
        assert!(
            !after[0].detached,
            "the record stayed detached with its line back"
        );
        assert_eq!(after[0].due_at.as_deref(), Some("2026-10-02"));
        assert_eq!(after[0].status, TaskStatus::Laeuft);
    }

    /// The same decision, end to end and through the door people actually use it by.
    #[tokio::test]
    async fn restoring_a_revision_brings_its_cards_back_rather_than_doubling_them() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        let with_tasks = store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Anruf", "Brief"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        store
            .publish_revision(&chef, &doc, &empty_body(), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        store
            .restore_revision(&chef, &with_tasks)
            .await
            .unwrap()
            .expect("the restore was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(after.len(), 2, "restoring doubled the board: {after:#?}");
        assert!(
            after.iter().all(|t| !t.detached),
            "the restored page still says its cards are gone: {after:#?}"
        );
    }

    /// Publishing needs Write, so reconciliation does. Nothing here asks a second time —
    /// which is exactly why this test exists rather than a comment saying so.
    #[tokio::test]
    async fn a_reader_cannot_cause_a_task_to_be_created() {
        let (store, _chef, doc) = writable_page("Seite", "/seite").await;
        let leser = account(&store, "leser").await;
        grant(&store, "/seite", &leser, Permission::Read).await;

        assert!(
            store
                .publish_revision(&leser, &doc, &body_with_tasks(&["Heimlich"]), None)
                .await
                .unwrap()
                .is_none(),
            "a reader published a revision"
        );

        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM tasks")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(rows, 0, "a reader caused {rows} task row(s) to be written");
    }

    /// The importer's path. A seeded page arrives with no ids at all, and its first publish
    /// is a `create_document` rather than a `publish_revision`, so reconciliation has to
    /// cope inside creation's transaction too.
    #[tokio::test]
    async fn a_page_imported_from_markdown_gets_its_tasks_on_its_first_publish() {
        let store = store().await;
        let doc = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Darm".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: gw_core::markdown::markdown_to_blocks(
                        "- [ ] Stuhlprobe einschicken\n- [x] Termin bestätigt\n",
                    ),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();

        let chef = account(&store, "chef").await;
        let mut tasks = store.tasks_for_document(&chef, &doc).await.unwrap();
        tasks.sort_by(|a, b| a.title.cmp(&b.title));
        assert_eq!(tasks.len(), 2, "the import produced {tasks:#?}");
        assert_eq!(tasks[0].title, "Stuhlprobe einschicken");
        // D-7: a line adopted as it was written. Once the page renders its checkbox from
        // the record, a ticked line that arrived Offen would visibly untick itself.
        assert_eq!(tasks[1].title, "Termin bestätigt");
        assert_eq!(tasks[1].status, TaskStatus::Fertig);
        assert_eq!(tasks[0].status, TaskStatus::Offen);

        // Creation writes the body TWICE — once into `documents` with the INSERT, once
        // again as the revision — so it is the path where the stored page most easily ends
        // up being the tree nothing was minted into.
        let mut stored = block_ids(&stored_body(&store, &doc).await);
        stored.sort();
        let mut recorded: Vec<_> = tasks.iter().filter_map(|t| t.block_id.clone()).collect();
        recorded.sort();
        assert_eq!(
            stored, recorded,
            "the created page and its records disagree"
        );
    }

    /// `seed --update` republishes freshly converted markdown, which carries no ids. If
    /// that minted a new id every run, every seed would shed every card on the page.
    #[tokio::test]
    async fn re_importing_the_same_markdown_keeps_the_same_records() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        let markdown = "- [ ] Stuhlprobe einschicken\n- [ ] Blutbild\n";

        store
            .publish_revision(
                &chef,
                &doc,
                &gw_core::markdown::markdown_to_blocks(markdown),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");
        let before = store.tasks_for_document(&chef, &doc).await.unwrap();
        let ids: Vec<_> = before.iter().map(|t| t.id.clone()).collect();

        // Twice more, exactly as running the seeder again would.
        for _ in 0..2 {
            store
                .publish_revision(
                    &chef,
                    &doc,
                    &gw_core::markdown::markdown_to_blocks(markdown),
                    None,
                )
                .await
                .unwrap()
                .expect("the publish was refused");
        }

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(
            after.len(),
            2,
            "re-importing multiplied the board: {after:#?}"
        );
        assert_eq!(
            after.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
            ids,
            "re-importing the same file replaced the records behind its lines"
        );
        assert!(after.iter().all(|t| !t.detached));
    }

    /// A checklist under a checklist line is still a checklist, and its lines are still
    /// to-dos — but the parent's card must not swallow their words.
    #[tokio::test]
    async fn a_checklist_nested_under_a_line_is_reconciled_too() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;

        let mut outer = task_line("Reise buchen");
        outer.content.push(Block {
            kind: BlockKind::TaskList,
            attrs: Default::default(),
            content: vec![task_line("Flug"), task_line("Hotel")],
            text: None,
            marks: Vec::new(),
        });
        let body = Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: vec![Block {
                kind: BlockKind::TaskList,
                attrs: Default::default(),
                content: vec![outer],
                text: None,
                marks: Vec::new(),
            }],
            text: None,
            marks: Vec::new(),
        };

        store
            .publish_revision(&chef, &doc, &body, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let mut titles: Vec<_> = store
            .tasks_for_document(&chef, &doc)
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.title)
            .collect();
        titles.sort();
        assert_eq!(titles, vec!["Flug", "Hotel", "Reise buchen"]);
    }

    /// Pressing Enter in a checklist makes an empty line. It must not put a nameless card
    /// on a board — and typing into it afterwards must produce exactly one.
    #[tokio::test]
    async fn an_empty_checkbox_line_is_not_yet_a_task() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["", "   "]), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        assert!(
            store
                .tasks_for_document(&chef, &doc)
                .await
                .unwrap()
                .is_empty(),
            "an empty line put a nameless card on the board"
        );
        assert!(
            block_ids(&stored_body(&store, &doc).await).is_empty(),
            "an empty line was given an identity it has no record for"
        );

        store
            .publish_revision(
                &chef,
                &doc,
                &body_with_tasks(&["Endlich Worte", "   "]),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");
        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].title, "Endlich Worte");
    }

    /// Copy and paste in the editor duplicates the attrs it copied, id and all. Two blocks
    /// sharing one record would put one card on the board for two lines — and the next edit
    /// to either of them would rewrite the other's title.
    #[tokio::test]
    async fn a_pasted_line_carrying_an_id_already_in_use_becomes_a_record_of_its_own() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Kaffee"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        // The stored tree, with its one line duplicated exactly as a paste would.
        let mut pasted = stored_body(&store, &doc).await;
        let line = pasted.content[0].content[0].clone();
        pasted.content[0].content.push(line);

        store
            .publish_revision(&chef, &doc, &pasted, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(
            after.len(),
            2,
            "the pasted line shares a record: {after:#?}"
        );
        assert!(after.iter().all(|t| !t.detached));
        let ids = block_ids(&stored_body(&store, &doc).await);
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1], "two lines were stored under one identity");
    }

    /// Two lines reading the same words are genuinely interchangeable, and both are to-dos.
    /// Adoption by title must not collapse them into one.
    #[tokio::test]
    async fn two_lines_with_the_same_words_keep_two_records() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        for _ in 0..2 {
            store
                .publish_revision(&chef, &doc, &body_with_tasks(&["Kaffee", "Kaffee"]), None)
                .await
                .unwrap()
                .expect("the publish was refused");
        }
        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(after.len(), 2, "two identical lines produced {after:#?}");
        assert!(after.iter().all(|t| !t.detached));
    }

    /// An anchored task that no line ever authored — created through the API on a page — is
    /// none of reconciliation's business. Detaching it would be this code asserting
    /// something about a record it did not write, and nothing could ever re-attach it.
    #[tokio::test]
    async fn a_task_with_no_block_behind_it_is_left_alone() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        let by_hand = done(
            store
                .create_task(&chef, &new_task(anchored(&doc), "Von Hand"))
                .await
                .unwrap(),
        );

        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Geschrieben"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.task_for(&chef, &by_hand.id).await.unwrap().unwrap();
        assert!(
            !after.detached,
            "a task with no line behind it was detached"
        );
        assert_eq!(after.title, "Von Hand");
    }

    /// The other half of the tick rule. A new record takes its status from the box (D-7);
    /// an existing one never does. Reading it every publish would undo a card somebody had
    /// dragged, from a copy of the state that the page is not even the owner of (D-2).
    #[tokio::test]
    async fn a_ticked_box_does_not_reopen_or_close_a_record_that_already_exists() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Rechnung"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");
        let task = store
            .tasks_for_document(&chef, &doc)
            .await
            .unwrap()
            .remove(0);
        done(
            store
                .update_task(
                    &chef,
                    &task.id,
                    &TaskUpdate {
                        status: Some(TaskStatus::Fertig),
                        ..Default::default()
                    },
                )
                .await
                .unwrap(),
        );

        // The stored tree still says the box is unticked, because ticking it is a board
        // action and the board does not rewrite pages.
        let stale = stored_body(&store, &doc).await;
        store
            .publish_revision(&chef, &doc, &stale, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(
            after[0].status,
            TaskStatus::Fertig,
            "the page's stale checkbox reopened a finished task"
        );
    }

    /// The atomicity claim, forced: reconciliation runs before the revision INSERT, so a
    /// failure there is the one ordering in which cards could outlive the revision that
    /// authored them. Mirrors `a_failed_publish_leaves_no_edges`.
    #[tokio::test]
    async fn a_failed_publish_leaves_no_tasks() {
        let (store, chef, doc) = writable_page("Seite", "/seite").await;
        store
            .publish_revision(&chef, &doc, &body_with_tasks(&["Bleibt"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        sqlx::query(
            "CREATE TRIGGER refuse_revisions BEFORE INSERT ON revisions
             BEGIN SELECT RAISE(ABORT, 'nope'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        assert!(
            store
                .publish_revision(&chef, &doc, &body_with_tasks(&["Neu"]), None)
                .await
                .is_err(),
            "the publish should have failed"
        );

        let after = store.tasks_for_document(&chef, &doc).await.unwrap();
        assert_eq!(
            after
                .iter()
                .map(|t| (t.title.as_str(), t.detached))
                .collect::<Vec<_>>(),
            vec![("Bleibt", false)],
            "the revision was rolled back and the board it implies was not: {after:#?}"
        );
    }
}
