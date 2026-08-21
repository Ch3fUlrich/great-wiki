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
//! - **Who may change a card**, including who may set its assignee, is whoever may Write
//!   that same page.
//! - **Who may be assigned** is anybody who may Read it. This is the answer to D-10's open
//!   question, and [`Store::create_task`] states it in full.
//!
//! Reconciling a page's task blocks against these rows on publish — minting a task for a
//! new checkbox line and marking one [`Task::detached`] when its line disappears (D-6, D-8)
//! — is a later task. Nothing here writes `block_id` or `detached`; the columns exist so
//! that work is a query and not a migration.

use crate::acl::Baseline;
use crate::{Store, StoredDocument};
use anyhow::{bail, Result};
use gw_auth::{Action, Principal};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::HashMap;

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

fn row_to_task(row: TaskRow) -> Result<Task> {
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
    /// Crate-private and unexported. It answers "which page decides?", never "may you"; the
    /// only callers are [`Store::governing_document`] and [`Store::board_for`], both of
    /// which put the path straight into `document_for`.
    async fn governing_path(&self, home: &TaskHome) -> Result<Option<String>> {
        Ok(match home {
            TaskHome::Anchored { doc_id, .. } => {
                sqlx::query_scalar("SELECT path FROM documents WHERE id = ?1")
                    .bind(doc_id)
                    .fetch_optional(&self.pool)
                    .await?
            }
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
        let Some(page) = self
            .governing_document(principal, &new.home, Action::Write, baseline)
            .await?
        else {
            return Ok(TaskOutcome::Refused);
        };

        if let Some(assignee) = &new.assignee {
            if !self.may_be_assigned(assignee, &page.path).await? {
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
        Ok(TaskOutcome::Done(Box::new(row_to_task(row)?)))
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
        if self
            .governing_document(principal, &home, Action::Read, baseline)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        Ok(Some(row_to_task(row)?))
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
        let Some(page) = self
            .governing_document(principal, &home, Action::Write, baseline)
            .await?
        else {
            return Ok(TaskOutcome::Refused);
        };

        // Where the card will be governed from once this update lands.
        let mut governing_path = page.path;
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
        Ok(TaskOutcome::Done(Box::new(row_to_task(row)?)))
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
    pub async fn tasks_for_document(
        &self,
        principal: &Principal,
        document_id: &str,
    ) -> Result<Vec<Task>> {
        if !self.may(principal, document_id, Action::Read).await? {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, TaskRow>(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE doc_id = ?1"
        ))
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = rows
            .into_iter()
            .map(row_to_task)
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
        // just been authorised for, so no second question arises for these.
        let mut out: Vec<Task> = sqlx::query_as::<_, TaskRow>(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE project_id = ?1"
        ))
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(row_to_task)
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

        let mut verdict: HashMap<String, bool> = HashMap::new();
        for (task_id, path) in anchored {
            // Defence in depth against the SQL above: the prefix narrows, `within` is the
            // same boundary stated where a human can read it, and neither decides.
            if !within(&home_page.path, &path) {
                continue;
            }
            let readable = match verdict.get(&path) {
                Some(known) => *known,
                None => {
                    let known = self
                        .document_for_with_baseline(principal, &path, Action::Read, baseline)
                        .await?
                        .is_some();
                    verdict.insert(path.clone(), known);
                    known
                }
            };
            if !readable {
                continue;
            }
            let Some(row) = self.task_row_unchecked(&task_id).await? else {
                continue;
            };
            out.push(row_to_task(row)?);
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
}
