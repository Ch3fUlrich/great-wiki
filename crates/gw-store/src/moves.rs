//! Renaming and moving a page, and the forward its old address leaves behind.
//!
//! The owner's decisions are in the roadmap (2026-09-24) and the reasoning is in ADR 0023.

use crate::acl::{grants_on, permits, Baseline};
use crate::trash::{refuse_a_hole_in_the_tree, SUBTREE};
use crate::Store;
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::{slugify, Visibility, title_problem};
use serde::Serialize;
use serde_json::json;
use std::str::FromStr;

/// Where a page should go and what it should be called.
#[derive(Debug, Clone)]
pub struct MoveRequest {
    /// The page it will sit under. `None` is the top level.
    pub parent: Option<String>,
    pub title: String,
    /// The last segment of its address. `None` or blank derives it from the title, exactly as
    /// creating a page does; the dialog fills in the current one so that fixing a typo in a
    /// title does not change the address.
    pub slug: Option<String>,
}

/// Whether the move happens, or only measures itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveMode {
    Preview,
    Commit,
}

#[derive(Debug)]
pub enum MoveOutcome {
    /// Not signed in, or may not write the page. The API turns this into ADR 0022's answer.
    Refused,
    /// The move cannot be made as asked, and this says why. Nothing was measured.
    Blocked(String),
    /// The move, measured. See [`MovePlan::committed`] and [`MovePlan::refusal`].
    Planned(MovePlan),
}

/// A move, measured across the move itself.
#[derive(Debug, Clone, Serialize)]
pub struct MovePlan {
    pub from: String,
    pub to: String,
    pub title: String,
    /// Live pages that move: the page and every live page under it.
    pub pages: usize,
    /// Everybody who may read at least one of those pages afterwards and could not before.
    pub gains: Vec<ReaderChange>,
    /// Everybody who could read at least one of them before and may not afterwards.
    pub losses: Vec<ReaderChange>,
    /// Set when the plan is complete and may not be carried out by this caller: somebody
    /// would gain access, and the caller does not administer the destination.
    pub refusal: Option<String>,
    /// Whether it happened. Never true for a preview, nor with a refusal.
    pub committed: bool,
}

/// One person whose reading access a move changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReaderChange {
    /// Everybody who has not signed in — an `anyone` grant is how a move can reach them.
    pub anonymous: bool,
    pub name: String,
    pub username: Option<String>,
    /// How many of the moved pages this person's access changes on.
    pub pages: usize,
}

impl Store {
    /// Rename a page, move it under another, or both — with everything under it.
    ///
    /// **The preview is the move.** Both modes carry the move out inside one transaction and
    /// read who may read each moved page *afterwards* from inside it, through
    /// [`grants_on`] and [`permits`] — the one rule, not a model of it. A preview then rolls
    /// back. This is ADR 0012's argument for the purge: a preview computed by a second piece
    /// of code is a prediction, and the day the two disagree somebody confirmed a different
    /// move from the one that happened.
    ///
    /// **Who may.** A signed-in, active account with write on every page that moves (the
    /// page takes its subtree, for the reason [`crate::trash`] gives) and write on the new
    /// parent. The top level has no page to hold a grant, so putting a page there needs
    /// `administers_destination`. So does a move that lets anybody read a page they could
    /// not read before — the owner's decision of 2026-09-24; a move that only narrows needs
    /// write and nothing more, because the same preview shows who loses.
    ///
    /// `administers_destination` is the API's `path_admin` verdict on the new parent (or on
    /// `/`), passed in rather than decided here for the reason [`crate::admin`] gives: a
    /// second copy of that gate in this crate would be a second rule to disagree with it.
    ///
    /// **What moves with the page**, beyond its rows: the grants written on the moved paths,
    /// and pending invitations naming them. **What does not**: any other page's body. A
    /// reference is an id and is resolved when it is read (ADR 0019); that is the whole reason
    /// this can be one statement rather than a rewrite of the wiki. ADR 0023 has the rest.
    pub async fn move_document(
        &self,
        principal: &Principal,
        path: &str,
        request: &MoveRequest,
        administers_destination: bool,
        mode: MoveMode,
    ) -> Result<MoveOutcome> {
        if !principal.is_authenticated() || !principal.active {
            return Ok(MoveOutcome::Refused);
        }
        let baseline = self.baseline_for(principal).await?;
        let Some(access) = self
            .document_access_with_baseline(principal, path, Action::Write, baseline)
            .await?
        else {
            return Ok(MoveOutcome::Refused);
        };
        let page = access.document;
        let from = page.path.clone();

        let title = request.title.trim();
        if title.is_empty() {
            return Ok(MoveOutcome::Blocked("a page needs a title".into()));
        }
        if let Some(reason) = title_problem(title) {
            return Ok(MoveOutcome::Blocked(reason.to_string()));
        }
        let slug = slugify(
            request
                .slug
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(title),
        );
        if slug.is_empty() {
            return Ok(MoveOutcome::Blocked(format!(
                "«{title}» contains nothing an address can be made of"
            )));
        }

        let parent = request
            .parent
            .as_deref()
            .map(|p| p.trim().trim_matches('/'))
            .filter(|p| !p.is_empty())
            .map(|p| format!("/{p}"));
        match &parent {
            Some(p) => {
                if *p == from || p.starts_with(&format!("{from}/")) {
                    return Ok(MoveOutcome::Blocked(format!(
                        "{from} cannot move into itself or under one of its own subpages"
                    )));
                }
                // Read first, so a destination this caller may not see is refused in exactly
                // the words an absent one is (ADR 0022). Only somebody who can see the page
                // is told that the refusal is about writing it.
                if self
                    .document_access_with_baseline(principal, p, Action::Read, baseline)
                    .await?
                    .is_none()
                {
                    return Ok(MoveOutcome::Blocked(format!("there is no page at {p}")));
                }
                if self
                    .document_access_with_baseline(principal, p, Action::Write, baseline)
                    .await?
                    .is_none()
                {
                    return Ok(MoveOutcome::Blocked(format!(
                        "you may not add pages under {p}"
                    )));
                }
            }
            None if !administers_destination => {
                return Ok(MoveOutcome::Blocked(
                    "only somebody who administers the whole wiki may put a page at the top level"
                        .into(),
                ));
            }
            None => {}
        }

        let to = format!("{}/{slug}", parent.as_deref().unwrap_or(""));
        if to == from && title == page.title {
            return Ok(MoveOutcome::Blocked(format!(
                "{from} is already called «{title}» and already lives there"
            )));
        }

        if to != from {
            // Live or in the trash: a trashed page still holds its address. This tells
            // somebody who may write the destination's parent that an address under it is
            // taken — one bit, told to a writer rather than to a prober, which is the line
            // `crate::trash` draws for its own refusal.
            let taken: Option<(String, bool)> = sqlx::query_as(&format!(
                "SELECT path, deleted_at IS NOT NULL FROM documents WHERE {SUBTREE} \
                 ORDER BY length(path) LIMIT 1"
            ))
            .bind(&to)
            .fetch_optional(&self.pool)
            .await?;
            if let Some((taken, trashed)) = taken {
                return Ok(MoveOutcome::Blocked(if trashed {
                    format!("a page in the trash still holds {taken}: restore or purge it first")
                } else {
                    format!("there is already a page at {taken}")
                }));
            }
        }

        // Every row that moves, the trashed ones included: a page thrown away under this one
        // has to come back under wherever this one is by then.
        let members: Vec<(String, String, bool)> = sqlx::query_as(&format!(
            "SELECT path, visibility, deleted_at IS NOT NULL FROM documents WHERE {SUBTREE} \
             ORDER BY path"
        ))
        .bind(&from)
        .fetch_all(&self.pool)
        .await?;
        for (member, _, trashed) in &members {
            if *member == from {
                continue;
            }
            let writable = if *trashed {
                self.trashed_document_access(principal, member, Action::Write, baseline)
                    .await?
                    .is_some()
            } else {
                self.document_access_with_baseline(principal, member, Action::Write, baseline)
                    .await?
                    .is_some()
            };
            if !writable {
                return Ok(MoveOutcome::Blocked(format!(
                    "{from} has a subpage you may not write, and a page moves with everything \
                     under it — whoever administers that subpage has to agree to the move"
                )));
            }
        }
        let live: Vec<(String, Visibility)> = members
            .iter()
            .filter(|(_, _, trashed)| !trashed)
            .map(|(member, visibility, _)| {
                (
                    member.clone(),
                    Visibility::from_str(visibility).unwrap_or_default(),
                )
            })
            .collect();

        // Everybody a move can change anything for. A deactivated account reads public pages
        // and nothing else, and a move does not change a page's visibility, so it cannot
        // appear; leaving it out saves a baseline lookup each and changes no answer.
        let mut people: Vec<(Principal, Baseline)> =
            vec![(Principal::anonymous(), Baseline::Public)];
        for person in self.list_principals().await? {
            if person.active {
                let baseline = self.baseline_for(&person).await?;
                people.push((person, baseline));
            }
        }
        let mut before = Vec::with_capacity(live.len());
        for (member, visibility) in &live {
            let grants = self.grants_for_path(member).await?;
            before.push(
                people
                    .iter()
                    .map(|(p, b)| permits(p, Action::Read, *visibility, &grants, *b))
                    .collect::<Vec<_>>(),
            );
        }

        let mut tx = self.pool.begin().await?;
        let rename = |column: &str| format!("?2 || substr({column}, length(?1) + 1)");
        if to != from {
            // Before the paths change, so the forwards name where the pages WERE. The trigger
            // in 0015 then drops any forward pointing away from an address one of them lands
            // on — a page moving back to where it came from included.
            sqlx::query(&format!(
                "INSERT OR REPLACE INTO forwards (old_path, document_id) \
                 SELECT path, id FROM documents WHERE {SUBTREE}"
            ))
            .bind(&from)
            .execute(&mut *tx)
            .await?;
            // One statement for the whole subtree. SQLite evaluates every SET expression
            // against the row as it was, so `path = ?1` below still means "the page itself".
            sqlx::query(&format!(
                "UPDATE documents SET path = {}, \
                 parent_path = CASE WHEN path = ?1 THEN ?3 ELSE {} END \
                 WHERE {SUBTREE}",
                rename("path"),
                rename("parent_path"),
            ))
            .bind(&from)
            .bind(&to)
            .bind(parent.as_deref())
            .execute(&mut *tx)
            .await?;
            // OR REPLACE: a grant identical to one already written at the destination — a
            // path keeps its grants after its page is purged — is the same grant.
            sqlx::query(&format!(
                "UPDATE OR REPLACE acl SET path = {} WHERE {SUBTREE}",
                rename("path")
            ))
            .bind(&from)
            .bind(&to)
            .execute(&mut *tx)
            .await?;
            sqlx::query(&format!(
                "UPDATE invites SET path = {} \
                 WHERE {SUBTREE} AND accepted_at IS NULL AND revoked_at IS NULL",
                rename("path")
            ))
            .bind(&from)
            .bind(&to)
            .execute(&mut *tx)
            .await?;
        }
        let sort_key: i64 = if parent == page.parent_path {
            page.sort_key
        } else {
            sqlx::query_scalar(
                "SELECT COALESCE(MAX(sort_key), -1) + 1 FROM documents \
                 WHERE parent_path IS ?1 AND id <> ?2",
            )
            .bind(parent.as_deref())
            .bind(&page.id)
            .fetch_one(&mut *tx)
            .await?
        };
        sqlx::query("UPDATE documents SET title = ?2, slug = ?3, sort_key = ?4 WHERE id = ?1")
            .bind(&page.id)
            .bind(title)
            .bind(&slug)
            .bind(sort_key)
            .execute(&mut *tx)
            .await?;
        refuse_a_hole_in_the_tree(&mut tx).await?;

        let mut gained = vec![0usize; people.len()];
        let mut lost = vec![0usize; people.len()];
        for ((member, visibility), before) in live.iter().zip(&before) {
            let moved = format!("{to}{}", &member[from.len()..]);
            let grants = grants_on(&mut tx, &moved).await?;
            for (i, (person, baseline)) in people.iter().enumerate() {
                let after = permits(person, Action::Read, *visibility, &grants, *baseline);
                match (before[i], after) {
                    (false, true) => gained[i] += 1,
                    (true, false) => lost[i] += 1,
                    _ => {}
                }
            }
        }
        let changes = |counts: &[usize]| -> Vec<ReaderChange> {
            let mut out: Vec<ReaderChange> = people
                .iter()
                .zip(counts)
                .filter(|(_, n)| **n > 0)
                .map(|((person, _), n)| ReaderChange {
                    anonymous: !person.is_authenticated(),
                    name: person.display_name.clone(),
                    username: person.is_authenticated().then(|| person.username.clone()),
                    pages: *n,
                })
                .collect();
            out.sort_by(|a, b| (!a.anonymous, &a.name).cmp(&(!b.anonymous, &b.name)));
            out
        };
        let gains = changes(&gained);
        let losses = changes(&lost);

        let refusal = (!gains.is_empty() && !administers_destination).then(|| {
            format!(
                "this move lets {} read what they cannot read now, and widening access needs \
                 admin rights on {}",
                if gains.len() == 1 {
                    "somebody".to_string()
                } else {
                    format!("{} people", gains.len())
                },
                parent.as_deref().unwrap_or("the whole wiki"),
            )
        });
        let committed = mode == MoveMode::Commit && refusal.is_none();
        let plan = MovePlan {
            from: from.clone(),
            to: to.clone(),
            title: title.to_string(),
            pages: live.len(),
            gains,
            losses,
            refusal,
            committed,
        };

        if !committed {
            tx.rollback().await?;
            return Ok(MoveOutcome::Planned(plan));
        }
        // Scoped to where the page is NOW: the people who administer it from here on are the
        // ones entitled to read how it arrived. The old path is the target, so the entry still
        // says where it came from.
        Self::record_audit(
            &mut *tx,
            Some(&principal.id),
            "document.move",
            Some(&from),
            Some(&to),
            &json!({
                "from": from,
                "to": to,
                "title_before": page.title,
                "title": title,
                "pages": plan.pages,
            }),
        )
        .await?;
        tx.commit().await?;
        Ok(MoveOutcome::Planned(plan))
    }

    /// Where the page that used to live at `path` lives now — **for a caller who may read it**.
    ///
    /// `None` for an address nothing ever left, for a page this caller may not read, for one
    /// in the trash and for one that was purged, and those are one answer on purpose: a
    /// forward that told a stranger where a withheld page went would be ADR 0022's existence
    /// oracle with a destination attached. The verdict is [`Store::document_for_id`]'s — the
    /// one every id in this crate is authorised through — so a forward can never show
    /// anybody a page that its new address would not.
    pub async fn forward_for(&self, principal: &Principal, path: &str) -> Result<Option<String>> {
        let document_id: Option<String> =
            sqlx::query_scalar("SELECT document_id FROM forwards WHERE old_path = ?1")
                .bind(path)
                .fetch_optional(&self.pool)
                .await?;
        let Some(document_id) = document_id else {
            return Ok(None);
        };
        Ok(self
            .document_for_id(principal, &document_id, Action::Read)
            .await?
            .map(|document| document.path))
    }
}

#[cfg(test)]
mod tests {
    use super::{MoveMode, MoveOutcome, MovePlan, MoveRequest, ReaderChange};
    use crate::{Author, NewDocument, Store};
    use gw_auth::{Permission, Principal, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Mark, Visibility};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    fn body() -> Block {
        Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        }
    }

    async fn page(store: &Store, parent: Option<&str>, title: &str, v: Visibility) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: v,
                    body: body(),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    /// A forward written by hand — the move that writes them for real comes with its own
    /// tests; these are about what a forward answers and when it ends.
    async fn forward(store: &Store, old_path: &str, document_id: &str) {
        sqlx::query("INSERT INTO forwards (old_path, document_id) VALUES (?1, ?2)")
            .bind(old_path)
            .bind(document_id)
            .execute(&store.pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_forward_names_the_current_path_for_a_reader() {
        let store = store().await;
        let id = page(&store, None, "Neu", Visibility::Public).await;
        forward(&store, "/alt", &id).await;

        let anyone = Principal::anonymous();
        assert_eq!(
            store.forward_for(&anyone, "/alt").await.unwrap().as_deref(),
            Some("/neu")
        );
        assert_eq!(
            store.forward_for(&anyone, "/nie-gewesen").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn a_forward_is_silent_for_somebody_who_may_not_read_the_page() {
        let store = store().await;
        let id = page(&store, None, "Geheim", Visibility::Restricted).await;
        forward(&store, "/alt", &id).await;

        let fremde = Principal::test("fremde", &[], &[]);
        assert_eq!(store.forward_for(&fremde, "/alt").await.unwrap(), None);

        let leser = Principal::test("leser", &[], &[]);
        store
            .add_grant(
                "/geheim",
                Subject::Principal(leser.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        assert_eq!(
            store.forward_for(&leser, "/alt").await.unwrap().as_deref(),
            Some("/geheim"),
            "the anti-vacuity half: a reader IS forwarded"
        );
    }

    #[tokio::test]
    async fn creating_a_page_at_a_forwarded_address_drops_the_forward() {
        let store = store().await;
        let id = page(&store, None, "Neu", Visibility::Public).await;
        forward(&store, "/alt", &id).await;

        page(&store, None, "Alt", Visibility::Public).await;

        assert_eq!(
            store
                .forward_for(&Principal::anonymous(), "/alt")
                .await
                .unwrap(),
            None
        );
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM forwards")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            rows, 0,
            "the forward row outlived the page that took its address"
        );
    }

    #[tokio::test]
    async fn a_forward_to_a_page_in_the_trash_is_silent() {
        let store = store().await;
        let id = page(&store, None, "Neu", Visibility::Public).await;
        forward(&store, "/alt", &id).await;
        let schreiber = Principal::test("schreiber", &[], &[]);
        store
            .add_grant(
                "/neu",
                Subject::Principal(schreiber.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        store.trash_document(&schreiber, "/neu").await.unwrap();

        assert_eq!(store.forward_for(&schreiber, "/alt").await.unwrap(), None);
    }

    // ---------------------------------------------------------------------------------
    // Moving.
    // ---------------------------------------------------------------------------------

    /// A signed-in account that exists in `principals`, so the preview can name it.
    async fn account(store: &Store, username: &str) -> Principal {
        store
            .create_local_principal(username, username, None, "$argon2id$fake")
            .await
            .unwrap()
    }

    async fn grant(store: &Store, path: &str, who: &Principal, permission: Permission) {
        store
            .add_grant(path, Subject::Principal(who.id.clone()), permission)
            .await
            .unwrap();
    }

    fn to(parent: Option<&str>, title: &str) -> MoveRequest {
        MoveRequest {
            parent: parent.map(Into::into),
            title: title.into(),
            slug: None,
        }
    }

    async fn path_of(store: &Store, id: &str) -> String {
        sqlx::query_scalar("SELECT path FROM documents WHERE id = ?1")
            .bind(id)
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    async fn count(store: &Store, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    fn planned(outcome: MoveOutcome) -> MovePlan {
        match outcome {
            MoveOutcome::Planned(plan) => plan,
            other => panic!("expected a plan, got {other:?}"),
        }
    }

    fn blocked(outcome: MoveOutcome) -> String {
        match outcome {
            MoveOutcome::Blocked(reason) => reason,
            other => panic!("expected a refusal with a reason, got {other:?}"),
        }
    }

    fn names(changes: &[ReaderChange]) -> Vec<(&str, usize)> {
        changes.iter().map(|c| (c.name.as_str(), c.pages)).collect()
    }

    /// `/a` and `/b`, both restricted and both writable by `schreiber`.
    ///
    /// * `/a` is readable by `anna`; `/a/p` and `/a/p/q` under it carry nothing of their own.
    /// * `/b` is readable by `bernd`.
    /// * `chefin` administers `/b` and writes `/a`.
    struct Fixture {
        store: Store,
        schreiber: Principal,
        chefin: Principal,
        p: String,
        q: String,
    }

    async fn fixture() -> Fixture {
        let store = store().await;
        page(&store, None, "A", Visibility::Restricted).await;
        let p = page(&store, Some("/a"), "P", Visibility::Restricted).await;
        let q = page(&store, Some("/a/p"), "Q", Visibility::Restricted).await;
        page(&store, None, "B", Visibility::Restricted).await;

        let schreiber = account(&store, "schreiber").await;
        let anna = account(&store, "anna").await;
        let bernd = account(&store, "bernd").await;
        let chefin = account(&store, "chefin").await;
        grant(&store, "/a", &schreiber, Permission::Write).await;
        grant(&store, "/b", &schreiber, Permission::Write).await;
        grant(&store, "/a", &anna, Permission::Read).await;
        grant(&store, "/b", &bernd, Permission::Read).await;
        grant(&store, "/a", &chefin, Permission::Write).await;
        grant(&store, "/b", &chefin, Permission::Admin).await;
        Fixture {
            store,
            schreiber,
            chefin,
            p,
            q,
        }
    }

    #[tokio::test]
    async fn the_preview_names_who_gains_and_who_loses_and_changes_nothing() {
        let f = fixture().await;
        let plan = planned(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    false,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
        assert_eq!(plan.from, "/a/p");
        assert_eq!(plan.to, "/b/p");
        assert_eq!(plan.pages, 2, "the page and the one under it");
        assert_eq!(names(&plan.gains), vec![("bernd", 2)]);
        assert_eq!(names(&plan.losses), vec![("anna", 2)]);
        assert!(!plan.committed);

        assert_eq!(
            path_of(&f.store, &f.p).await,
            "/a/p",
            "a preview moved the page"
        );
        assert_eq!(count(&f.store, "SELECT count(*) FROM forwards").await, 0);
        assert_eq!(
            count(
                &f.store,
                "SELECT count(*) FROM audit_log WHERE action = 'document.move'"
            )
            .await,
            0
        );
    }

    #[tokio::test]
    async fn widening_without_admin_on_the_destination_is_refused_and_changes_nothing() {
        let f = fixture().await;
        let plan = planned(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    false,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert!(
            plan.refusal.is_some(),
            "bernd would gain, and schreiber administers nothing"
        );
        assert!(!plan.committed);
        assert_eq!(path_of(&f.store, &f.p).await, "/a/p");
    }

    #[tokio::test]
    async fn an_administrator_of_the_destination_may_widen() {
        let f = fixture().await;
        let plan = planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert!(plan.committed, "{:?}", plan.refusal);
        assert_eq!(path_of(&f.store, &f.p).await, "/b/p");
        assert_eq!(path_of(&f.store, &f.q).await, "/b/p/q");
        let parent: Option<String> =
            sqlx::query_scalar("SELECT parent_path FROM documents WHERE id = ?1")
                .bind(&f.q)
                .fetch_one(&f.store.pool)
                .await
                .unwrap();
        assert_eq!(parent.as_deref(), Some("/b/p"));
    }

    #[tokio::test]
    async fn narrowing_needs_only_write() {
        let f = fixture().await;
        page(&f.store, None, "C", Visibility::Restricted).await;
        grant(&f.store, "/c", &f.schreiber, Permission::Write).await;

        let plan = planned(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/c"), "P"),
                    false,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert!(plan.gains.is_empty());
        // `chefin` writes `/a` and holds nothing on `/c`, so she loses them too.
        assert_eq!(names(&plan.losses), vec![("anna", 2), ("chefin", 2)]);
        assert!(plan.committed, "{:?}", plan.refusal);
        assert_eq!(path_of(&f.store, &f.p).await, "/c/p");
    }

    #[tokio::test]
    async fn somebody_without_an_account_is_one_of_the_people_a_move_can_let_in() {
        let f = fixture().await;
        page(&f.store, None, "Offen", Visibility::Restricted).await;
        f.store
            .add_grant("/offen", Subject::Anyone, Permission::Read)
            .await
            .unwrap();
        grant(&f.store, "/offen", &f.chefin, Permission::Admin).await;

        let plan = planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/offen"), "P"),
                    true,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
        let anonymous: Vec<_> = plan.gains.iter().filter(|c| c.anonymous).collect();
        assert_eq!(anonymous.len(), 1, "{:?}", plan.gains);
        assert_eq!(anonymous[0].pages, 2);
    }

    #[tokio::test]
    async fn a_move_leaves_every_other_body_and_history_alone_and_a_reference_follows_it() {
        let f = fixture().await;
        // A page elsewhere that refers to `/a/p` by identity (ADR 0019).
        let link = Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: vec![Block {
                kind: BlockKind::Paragraph,
                attrs: Default::default(),
                content: vec![Block {
                    kind: BlockKind::Text,
                    attrs: Default::default(),
                    content: Vec::new(),
                    text: Some("siehe dort".into()),
                    marks: vec![Mark::link_to_doc(&f.p)],
                }],
                text: None,
                marks: Vec::new(),
            }],
            text: None,
            marks: Vec::new(),
        };
        f.store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: Some("/b".into()),
                    doc_type: DocumentType::Page,
                    title: "Verweis".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body: link.clone(),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();

        let snapshot = "SELECT group_concat(id || body, '|') FROM (SELECT id, body FROM documents ORDER BY id)";
        let bodies: String = sqlx::query_scalar(snapshot)
            .fetch_one(&f.store.pool)
            .await
            .unwrap();
        let revisions = count(&f.store, "SELECT count(*) FROM revisions").await;

        planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );

        let after: String = sqlx::query_scalar(snapshot)
            .fetch_one(&f.store.pool)
            .await
            .unwrap();
        assert_eq!(bodies, after, "a move rewrote a body");
        assert_eq!(
            revisions,
            count(&f.store, "SELECT count(*) FROM revisions").await
        );

        let references = f.store.references_for(&f.chefin, &link).await.unwrap();
        assert_eq!(
            references[&f.p].path, "/b/p",
            "the reference did not follow the page"
        );
    }

    #[tokio::test]
    async fn grants_written_on_the_moved_pages_travel_with_them() {
        let f = fixture().await;
        let dora = account(&f.store, "dora").await;
        grant(&f.store, "/a/p/q", &dora, Permission::Read).await;
        // Once the page carries rows of its own, nothing inherited reaches it, so the mover
        // needs one there too.
        grant(&f.store, "/a/p/q", &f.chefin, Permission::Write).await;

        planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert_eq!(
            count(&f.store, "SELECT count(*) FROM acl WHERE path = '/a/p/q'").await,
            0
        );
        assert_eq!(
            count(&f.store, "SELECT count(*) FROM acl WHERE path = '/b/p/q'").await,
            2
        );
        assert!(f
            .store
            .document_for(&dora, "/b/p/q", gw_auth::Action::Read)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn pending_invitations_travel_with_the_page_and_spent_ones_stay() {
        let f = fixture().await;
        for (id, spent) in [("offen", false), ("angenommen", true)] {
            sqlx::query(
                "INSERT INTO invites (id, token_hash, username, path, permission, expires_at, accepted_at) \
                 VALUES (?1, ?1, ?1, '/a/p', 'read', datetime('now', '+1 day'), \
                         CASE WHEN ?2 THEN datetime('now') END)",
            )
            .bind(id)
            .bind(spent)
            .execute(&f.store.pool)
            .await
            .unwrap();
        }
        planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        let pending: String = sqlx::query_scalar("SELECT path FROM invites WHERE id = 'offen'")
            .fetch_one(&f.store.pool)
            .await
            .unwrap();
        let spent: String = sqlx::query_scalar("SELECT path FROM invites WHERE id = 'angenommen'")
            .fetch_one(&f.store.pool)
            .await
            .unwrap();
        assert_eq!(pending, "/b/p");
        assert_eq!(
            spent, "/a/p",
            "a spent invitation is a record and was rewritten"
        );
    }

    #[tokio::test]
    async fn the_old_addresses_forward_and_one_move_is_one_audit_entry() {
        let f = fixture().await;
        planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert_eq!(
            f.store
                .forward_for(&f.chefin, "/a/p")
                .await
                .unwrap()
                .as_deref(),
            Some("/b/p")
        );
        assert_eq!(
            f.store
                .forward_for(&f.chefin, "/a/p/q")
                .await
                .unwrap()
                .as_deref(),
            Some("/b/p/q")
        );
        let (target, scope): (String, String) =
            sqlx::query_as("SELECT target, path FROM audit_log WHERE action = 'document.move'")
                .fetch_one(&f.store.pool)
                .await
                .unwrap();
        assert_eq!((target.as_str(), scope.as_str()), ("/a/p", "/b/p"));
    }

    #[tokio::test]
    async fn a_rename_in_place_changes_title_and_address_and_forwards() {
        let f = fixture().await;
        let plan = planned(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/a"), "Protokolle"),
                    false,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert!(plan.committed, "{:?}", plan.refusal);
        assert!(plan.gains.is_empty() && plan.losses.is_empty());
        let (path, title): (String, String) =
            sqlx::query_as("SELECT path, title FROM documents WHERE id = ?1")
                .bind(&f.p)
                .fetch_one(&f.store.pool)
                .await
                .unwrap();
        assert_eq!(
            (path.as_str(), title.as_str()),
            ("/a/protokolle", "Protokolle")
        );
        assert_eq!(
            f.store
                .forward_for(&f.schreiber, "/a/p")
                .await
                .unwrap()
                .as_deref(),
            Some("/a/protokolle")
        );
    }

    #[tokio::test]
    async fn a_new_title_can_keep_the_address() {
        let f = fixture().await;
        let request = MoveRequest {
            parent: Some("/a".into()),
            title: "Protokolle".into(),
            slug: Some("p".into()),
        };
        let plan = planned(
            f.store
                .move_document(&f.schreiber, "/a/p", &request, false, MoveMode::Commit)
                .await
                .unwrap(),
        );
        assert!(plan.committed);
        assert_eq!(plan.to, "/a/p");
        assert_eq!(count(&f.store, "SELECT count(*) FROM forwards").await, 0);
    }

    #[tokio::test]
    async fn a_chain_of_moves_forwards_to_where_the_page_is_now_and_moving_back_ends_it() {
        let f = fixture().await;
        let mv = |from: &'static str, parent: &'static str, title: &'static str| {
            let store = &f.store;
            let chefin = &f.chefin;
            async move {
                planned(
                    store
                        .move_document(
                            chefin,
                            from,
                            &to(Some(parent), title),
                            true,
                            MoveMode::Commit,
                        )
                        .await
                        .unwrap(),
                )
            }
        };
        mv("/a/p", "/b", "P").await;
        mv("/b/p", "/b", "Zwei").await;
        assert_eq!(
            f.store
                .forward_for(&f.chefin, "/a/p")
                .await
                .unwrap()
                .as_deref(),
            Some("/b/zwei")
        );
        mv("/b/zwei", "/a", "P").await;
        assert_eq!(f.store.forward_for(&f.chefin, "/a/p").await.unwrap(), None);
        assert_eq!(
            f.store
                .forward_for(&f.chefin, "/b/zwei")
                .await
                .unwrap()
                .as_deref(),
            Some("/a/p")
        );
    }

    #[tokio::test]
    async fn a_trashed_subpage_moves_with_its_parent() {
        let f = fixture().await;
        f.store
            .trash_document(&f.schreiber, "/a/p/q")
            .await
            .unwrap();
        planned(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert_eq!(path_of(&f.store, &f.q).await, "/b/p/q");
    }

    #[tokio::test]
    async fn the_mover_must_be_signed_in_and_may_write_the_page() {
        let f = fixture().await;
        let anna = f
            .store
            .principal_by_username("anna")
            .await
            .unwrap()
            .unwrap()
            .0;
        for who in [Principal::anonymous(), anna] {
            assert!(matches!(
                f.store
                    .move_document(&who, "/a/p", &to(Some("/b"), "P"), true, MoveMode::Preview)
                    .await
                    .unwrap(),
                MoveOutcome::Refused
            ));
        }
    }

    /// On a path carrying `anyone: write` — a public share link — the write verdict alone
    /// would let somebody who has not said who they are move the page, and a move is
    /// recorded under somebody's name. The account check is what refuses them.
    #[tokio::test]
    async fn anyone_write_does_not_let_an_anonymous_caller_move_a_page() {
        let f = fixture().await;
        f.store
            .add_grant("/a", Subject::Anyone, Permission::Write)
            .await
            .unwrap();
        f.store
            .add_grant("/b", Subject::Anyone, Permission::Write)
            .await
            .unwrap();
        assert!(matches!(
            f.store
                .move_document(
                    &Principal::anonymous(),
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit
                )
                .await
                .unwrap(),
            MoveOutcome::Refused
        ));
        assert_eq!(path_of(&f.store, &f.p).await, "/a/p");
    }

    #[tokio::test]
    async fn what_cannot_be_moved_is_refused_with_a_reason() {
        let f = fixture().await;
        let ask = |path: &'static str, request: MoveRequest| {
            let store = &f.store;
            let who = &f.schreiber;
            async move {
                blocked(
                    store
                        .move_document(who, path, &request, false, MoveMode::Commit)
                        .await
                        .unwrap(),
                )
            }
        };
        // Into itself, or below itself.
        ask("/a/p", to(Some("/a/p"), "P")).await;
        ask("/a", to(Some("/a/p/q"), "A")).await;
        // An address that is taken — by its own parent here.
        ask("/a/p", to(None, "A")).await;
        // Nothing changes.
        ask("/a/p", to(Some("/a"), "P")).await;
        // No title, and a title that makes no address.
        ask("/a/p", to(Some("/a"), "   ")).await;
        ask("/a/p", to(Some("/a"), "???")).await;
        // The top level, without administering the wiki.
        ask("/a/p", to(None, "Oben")).await;
        assert_eq!(path_of(&f.store, &f.p).await, "/a/p");
    }

    #[tokio::test]
    async fn an_occupied_address_is_refused_even_by_a_page_in_the_trash() {
        let f = fixture().await;
        page(&f.store, Some("/b"), "P", Visibility::Restricted).await;
        f.store.trash_document(&f.schreiber, "/b/p").await.unwrap();
        let reason = blocked(
            f.store
                .move_document(
                    &f.chefin,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    true,
                    MoveMode::Commit,
                )
                .await
                .unwrap(),
        );
        assert!(reason.contains("/b/p"), "{reason}");
    }

    #[tokio::test]
    async fn a_subpage_you_may_not_write_blocks_the_move() {
        let f = fixture().await;
        let dora = account(&f.store, "dora").await;
        grant(&f.store, "/a/p/q", &dora, Permission::Write).await;
        blocked(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/b"), "P"),
                    false,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
    }

    #[tokio::test]
    async fn an_unreadable_destination_reads_exactly_as_an_absent_one() {
        let f = fixture().await;
        page(&f.store, None, "Geheim", Visibility::Restricted).await;
        let withheld = blocked(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/geheim"), "P"),
                    false,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
        let absent = blocked(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/gibt-es-nicht"), "P"),
                    false,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
        assert_eq!(
            withheld.replace("/geheim", "X"),
            absent.replace("/gibt-es-nicht", "X")
        );
    }

    #[tokio::test]
    async fn a_readable_destination_you_may_not_write_says_so() {
        let f = fixture().await;
        page(&f.store, None, "Lesbar", Visibility::Public).await;
        let reason = blocked(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/lesbar"), "P"),
                    false,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
        let absent = blocked(
            f.store
                .move_document(
                    &f.schreiber,
                    "/a/p",
                    &to(Some("/gibt-es-nicht"), "P"),
                    false,
                    MoveMode::Preview,
                )
                .await
                .unwrap(),
        );
        assert_ne!(
            reason.replace("/lesbar", "X"),
            absent.replace("/gibt-es-nicht", "X"),
            "somebody who can see the page is told what was refused"
        );
    }

    #[tokio::test]
    async fn an_instance_administrator_may_move_a_page_to_the_top_level() {
        let f = fixture().await;
        let plan = planned(
            f.store
                .move_document(&f.chefin, "/a/p", &to(None, "P"), true, MoveMode::Commit)
                .await
                .unwrap(),
        );
        assert!(plan.committed, "{:?}", plan.refusal);
        assert_eq!(path_of(&f.store, &f.p).await, "/p");
        let parent: Option<String> =
            sqlx::query_scalar("SELECT parent_path FROM documents WHERE id = ?1")
                .bind(&f.p)
                .fetch_one(&f.store.pool)
                .await
                .unwrap();
        assert_eq!(parent, None);
    }
}
