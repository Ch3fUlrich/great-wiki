//! Embedding a page, or one section of it, inside another (D-27 … D-30).
//!
//! An embed is a **reference**, in the same sense a placement is: `documents.body` holds
//! which page and which section, and nothing whatever of the target. Everything a reader
//! sees inside the frame is fetched here, when the page is read, against that reader — which
//! is not an optimisation but the whole disclosure property. A copy would go on showing
//! somebody words they had lost access to, and would go stale the moment the source changed,
//! which is the one thing an embed exists not to do.
//!
//! Three things live in this module and each is one end of the same rule.
//!
//! **[`mint_heading_ids`]** gives every heading a stable id on publish. A section needs an
//! anchor that survives an edit, and a heading's words change and its position certainly
//! does — so the anchor is a uuid, minted where a task's is minted, by the store, inside the
//! transaction that writes the revision. For **every** heading rather than only the ones
//! something embeds: the alternative needs to know who embeds this page before it is
//! published, which is knowledge of another document's body at the wrong moment, and it
//! would leave the editor's section picker with nothing to offer until somebody had
//! published the target for a second time.
//!
//! **[`settle_embeds`] and [`Store::resolve_embeds`]** are ADR 0019's two-step, run for a
//! block instead of a mark. A seed into a fresh database mints every document id anew, so no
//! `einbettung:<uuid>` in a restored corpus can match anything; the file carries the
//! target's path beside the id as a fallback, `settle_embeds` exchanges an unknown id for
//! that path inside the publish transaction, and `resolve_embeds` exchanges the path back
//! for *this* database's id at the next publish, for a page the **author** may read. A block
//! reaching `documents.body` therefore names its target by identity or by address and never
//! by both.
//!
//! **[`Store::embeds_for`]** is the read side, and it is where every rule the owner decided
//! actually is:
//!
//! - **Filtered per document, against the reader**, through the same accessor a page read
//!   itself ends in. A target the reader may not read, one that does not exist, one in the
//!   Papierkorb and one that was purged are all simply absent from the map, and an embed with
//!   no entry renders as the author's own label — telling those four apart is itself the
//!   disclosure.
//! - **Depth is one.** The blocks handed back may themselves hold embeds; those render as
//!   their labels. That closes the recursion by construction rather than by remembering to
//!   carry a visited set, and it bounds the authorisation cost — every resolution is one
//!   query through the single SQLite connection the whole application shares.
//! - **An orphan keeps its frame** (D-29). A section whose anchor has been deleted or merged
//!   away comes back with `body: None` and the target's name, so the reader can say the
//!   section is gone and link to where it went. Never the whole page instead: that quietly
//!   swaps a dosage table for a page the author never meant to quote.
//! - **A cycle is stopped at render and named** (see [`Embed::cycle`]). It is allowed to
//!   exist — refusing a publish because somebody else's page points back here would mean
//!   reading pages the author may not read, and `links` is a plain edge table with no
//!   acyclicity constraint.

use crate::acl::Baseline;
use crate::links::Reference;
use crate::{Store, StoredDocument};
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::{Block, BlockKind};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// How many DISTINCT embeds one page expands.
///
/// [`crate::links::MAX_REFERENCES_PER_PAGE`]'s reasoning with the numbers moved: expanding an
/// embed costs an authorisation, a row, a JSON parse of somebody else's whole body and a
/// second resolution pass over what comes back, where resolving a reference costs one
/// authorisation and two columns. Nothing caps how many blocks a body holds — it is JSON over
/// the collaboration socket — and `Store::open` gives the pool one connection, so a page
/// carrying a few hundred embeds, fetched repeatedly, is a lever on the availability of the
/// whole deployment.
///
/// **Over the cap the remainder simply do not resolve**, which is the state a forbidden or
/// absent target is already in — so the cap discloses nothing and needs no message.
pub const MAX_EMBEDS_PER_PAGE: usize = 16;

/// How many documents one page's cycle search may read, in total, across every embed on it.
///
/// A budget for the page rather than for each embed, for the reason above: per-embed it would
/// multiply by [`MAX_EMBEDS_PER_PAGE`]. Running out of budget reports no cycle, which is safe
/// in the only sense that matters — depth is one, so nothing can recurse whether a cycle is
/// named or not. What is lost is the *naming*, on a page with a great many embeds.
const MAX_CYCLE_WALK: usize = 32;

/// One embed, resolved for one reader.
///
/// Every field is a disclosure about the target page, so none of them exists for a reader who
/// may not read it: [`Store::embeds_for`] has no entry at all for that embed, and there is no
/// third state for a caller to get wrong. Exactly [`Reference`]'s shape and exactly its
/// reasoning, one layer up.
#[derive(Debug, Clone, Serialize)]
pub struct Embed {
    /// Where the target is **now**.
    pub path: String,
    /// What the target is called **now** (D-28: the frame names its source, and the name is
    /// live rather than a snapshot the author took).
    pub title: String,
    /// The blocks to draw inside the frame: the target's whole body, or the section beneath
    /// the anchored heading.
    ///
    /// `None` is D-29 — the anchor is gone, so the frame says the section no longer exists
    /// and links the source — and it is also what a cycle produces, because a frame that
    /// named a cycle and then drew something anyway would be describing two different states.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Block>,
    /// Where the **embedded** body's own document references point, for this same reader.
    ///
    /// The target's references, resolved against the person reading the host page and not
    /// against its author — the same rule, asked again, one document further out. Without
    /// this every `dok:` inside an embedded section would render as unlinked words, which is
    /// the state that means "you may not read that", asserted about pages the reader very
    /// possibly may.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub references: BTreeMap<String, Reference>,
    /// The titles of the pages that lead from this target back to the page being read, host
    /// last — empty when there is no cycle.
    ///
    /// Named rather than merely refused, because "this embed shows nothing" and "these three
    /// pages quote each other in a ring" are different problems and only the second one can
    /// be fixed by the person reading it. Every title in it is a page this reader may read:
    /// the walk goes through the same accessor everything else here does, so a ring passing
    /// through a page they may not see is simply not found, exactly as it is not listed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<String>,
}

/// What one embed block names, lifted out of its attributes.
struct Target {
    key: String,
    doc: Option<String>,
    path: Option<String>,
    heading: Option<String>,
}

fn attr(block: &Block, key: &str) -> Option<String> {
    block
        .attrs
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Every embed in this body, in document order, deduplicated by [`Block::embed_key`].
fn collect_embeds(body: &Block, into: &mut Vec<Target>, seen: &mut BTreeSet<String>) {
    if body.kind == BlockKind::Transclusion {
        if let Some(key) = body.embed_key() {
            if seen.insert(key.clone()) {
                into.push(Target {
                    key,
                    doc: attr(body, "doc"),
                    path: attr(body, "path"),
                    heading: attr(body, "heading"),
                });
            }
        }
        return;
    }
    for child in &body.content {
        collect_embeds(child, into, seen);
    }
}

/// Give every heading in this body a stable id, and say whether anything was minted.
///
/// A pure walk with a uuid generator: **the same shape `reconcile_tasks` mints a task id
/// with, and for the same reason it is here rather than in `gw_core::markdown`**. That
/// converter is a pure function of its input and has to stay one, because `gw_api::export`
/// re-imports its own output and compares the trees — an id invented per render would differ
/// on every run and refuse the page for ever.
///
/// An id already present is left alone, unconditionally. That is the whole of what makes an
/// anchor stable: re-minting on every publish would move every section embed in the wiki each
/// time somebody fixed a typo, which is exactly the failure a slug already has and the reason
/// this exists.
///
/// **Every heading, including one nested in a blockquote or a list item.** Those cannot be
/// anchored to — [`Block::section`] is a run of siblings and only exists at the level the run
/// is a run of — but minting there too costs nothing and means a heading that is later pulled
/// out to the top level arrives with the identity it already had.
pub(crate) fn mint_heading_ids(body: &mut Block) -> bool {
    let mut minted = false;
    if body.kind == BlockKind::Heading && !body.attrs.contains_key("id") {
        body.attrs.insert(
            "id".into(),
            serde_json::Value::String(uuid::Uuid::now_v7().to_string()),
        );
        minted = true;
    }
    for child in &mut body.content {
        minted |= mint_heading_ids(child);
    }
    minted
}

/// Settle every freshly imported embed against the documents THIS database holds, and say
/// whether anything changed.
///
/// [`crate::links::settle_references`] for a block instead of a mark, with the same two
/// outcomes in the same order, because the order is the whole decision:
///
/// - **The id names a live document here** — the embed stands and the carried path is
///   dropped, *even when the two disagree*. A page renamed after the export has a stale path
///   in the file and a perfectly good identity, and preferring the path would reintroduce
///   exactly the breakage identity exists to prevent, quietly, on a restore.
/// - **The id names nothing here** — which is what a fresh seed produces — so the embed
///   becomes one that names its target by address. [`Store::resolve_embeds`] exchanges that
///   for this database's id at the next publish, and until then the reader resolves it by
///   path, through the same permission-checked accessor.
///
/// With no path carried — an older export, or one whose target the exporting account could
/// not read — an unknown id stays an unknown id and the embed renders as its label, which is
/// the state [`Store::embeds_for`] defines for everything it cannot resolve.
///
/// Takes a CONNECTION for [`crate::links::replace_links`]' reason: it runs inside the
/// caller's transaction, on the body that is about to be stored, so a rollback takes it back
/// with everything else. It asks only whether a row exists — not who may read it — because it
/// decides nothing about what is shown: the reader's verdict is re-asked on every read.
pub(crate) async fn settle_embeds(
    conn: &mut sqlx::SqliteConnection,
    body: &mut Block,
) -> Result<bool> {
    let mut changed = false;
    let mut settled: HashMap<String, bool> = HashMap::new();
    settle_into(conn, body, &mut settled, &mut changed).await?;
    Ok(changed)
}

/// The walk behind [`settle_embeds`]. Separate because recursion in an `async fn` needs a
/// boxed future, and `Box::pin` belongs at the one place that recurses.
fn settle_into<'a>(
    conn: &'a mut sqlx::SqliteConnection,
    body: &'a mut Block,
    settled: &'a mut HashMap<String, bool>,
    changed: &'a mut bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if body.kind == BlockKind::Transclusion {
            if let (Some(id), Some(path)) = (attr(body, "doc"), attr(body, "path")) {
                // Memoised per body: a page embedding one target four times asks once.
                let live = match settled.get(&id) {
                    Some(live) => *live,
                    None => {
                        let live: Option<String> = sqlx::query_scalar(
                            "SELECT id FROM documents WHERE id = ?1 AND deleted_at IS NULL",
                        )
                        .bind(&id)
                        .fetch_optional(&mut *conn)
                        .await?;
                        let live = live.is_some();
                        settled.insert(id.clone(), live);
                        live
                    }
                };
                if live {
                    body.attrs.remove("path");
                } else {
                    body.attrs.remove("doc");
                    body.attrs
                        .insert("path".into(), serde_json::Value::String(path));
                }
                *changed = true;
            }
        }
        for child in &mut body.content {
            settle_into(&mut *conn, child, settled, changed).await?;
        }
        Ok(())
    })
}

impl Store {
    /// Turn every address-named embed in `body` into one named by identity, for the pages
    /// `author` may read. `Ok(true)` when something changed.
    ///
    /// [`Store::resolve_references`]'s block half, with all of its reasoning: an address
    /// breaks the day the page moves, so it is exchanged for the page's identity at the one
    /// moment the system knows both — when somebody publishes — and it runs just before the
    /// transaction rather than inside it because authorising a path needs
    /// [`Store::document_for`], which goes to the pool, and the pool holds one connection.
    ///
    /// **Read, not Write**, exactly as `resolve_references` is: being able to quote a page is
    /// being able to find it, which is what reading it already licenses.
    pub(crate) async fn resolve_embeds(
        &self,
        author: &Principal,
        body: &mut Block,
    ) -> Result<bool> {
        let mut targets = Vec::new();
        collect_embeds(body, &mut targets, &mut BTreeSet::new());
        let paths: BTreeSet<String> = targets
            .into_iter()
            .filter(|t| t.doc.is_none())
            .filter_map(|t| t.path)
            .collect();
        if paths.is_empty() {
            return Ok(false);
        }
        let baseline = self.baseline_for(author).await?;
        let mut resolved: HashMap<String, String> = HashMap::new();
        for path in paths.into_iter().take(MAX_EMBEDS_PER_PAGE) {
            if let Some(doc) = self
                .document_for_with_baseline(author, &path, Action::Read, baseline)
                .await?
            {
                resolved.insert(path, doc.id);
            }
        }
        if resolved.is_empty() {
            return Ok(false);
        }
        apply_embeds(body, &resolved);
        Ok(true)
    }

    /// Everything the embeds in `body` may show **this** reader, keyed by
    /// [`Block::embed_key`].
    ///
    /// The whole of the transclusion permission rule is here and in nothing else. See this
    /// module's own header for the four parts of it; what follows is only what the code does.
    ///
    /// `host` is the document the body belongs to, and it is needed for one thing: an embed
    /// of the page it is written on, and any ring of embeds leading back to it, is a cycle.
    pub async fn embeds_for(
        &self,
        principal: &Principal,
        host: &str,
        body: &Block,
    ) -> Result<BTreeMap<String, Embed>> {
        let mut targets = Vec::new();
        collect_embeds(body, &mut targets, &mut BTreeSet::new());
        if targets.is_empty() {
            // Not merely an optimisation: `baseline_for` is a query, and a body with no embed
            // in it is every page on the site today.
            return Ok(BTreeMap::new());
        }
        // Once, for the whole page. See `graph_for` and `references_for`, which hoist it for
        // the same reason.
        let baseline = self.baseline_for(principal).await?;
        let mut out = BTreeMap::new();
        let mut budget = MAX_CYCLE_WALK;
        for target in targets.into_iter().take(MAX_EMBEDS_PER_PAGE) {
            let Some(doc) = self.readable_embed(principal, &target, baseline).await? else {
                continue;
            };
            let Ok(target_body) = serde_json::from_str::<Block>(&doc.body) else {
                // A stored body that will not parse is not this function's to repair, and it
                // is not a state to report either: the embed renders as its label, which is
                // what every other unresolvable target renders as.
                tracing::warn!(
                    target: "gw_store::transclusion",
                    path = %doc.path,
                    "a stored body would not parse; it is not expanded into an embed"
                );
                continue;
            };
            let cycle = self
                .cycle_through(principal, host, &doc, &target_body, baseline, &mut budget)
                .await?;
            // A cycle is drawn as a cycle and nothing else. Expanding it as well would put a
            // frame on the page that both names a ring and quotes half of it.
            let section = if cycle.is_empty() {
                match &target.heading {
                    Some(anchor) => target_body.section(anchor),
                    None => Some(target_body),
                }
            } else {
                None
            };
            let references = match &section {
                Some(section) => self.references_for(principal, section).await?,
                None => BTreeMap::new(),
            };
            out.insert(
                target.key,
                Embed {
                    path: doc.path,
                    title: doc.title,
                    body: section,
                    references,
                    cycle,
                },
            );
        }
        Ok(out)
    }

    /// The document an embed names, if `principal` may read it.
    ///
    /// THE permission-checked accessor, reached with the caller's baseline already in hand —
    /// one for an id and one for a path, because an embed names its target by identity or by
    /// address and both forms reach `documents.body`. Never `document_by_path_unchecked`, and
    /// never a batch query justified by "the targets came out of a body this caller may
    /// already read". They did, and that says nothing whatever about the pages they point at.
    ///
    /// **One call site each, in a function of its own, deliberately** — exactly as
    /// `crate::links`'s `readable_target` is: `scripts/mutate.sh` replaces exactly these two
    /// lines with the row lookups underneath them, and a mutation that can be written as one
    /// line is one that cannot quietly miss half of what it was aimed at. The expansion and
    /// the cycle walk share this, so there is no second verdict for them to differ by.
    async fn readable_embed(
        &self,
        principal: &Principal,
        target: &Target,
        baseline: Baseline,
    ) -> Result<Option<StoredDocument>> {
        match (&target.doc, &target.path) {
            (Some(id), _) => {
                self.document_for_id_with_baseline(principal, id, Action::Read, baseline)
                    .await
            }
            (None, Some(path)) => {
                self.document_for_with_baseline(principal, path, Action::Read, baseline)
                    .await
            }
            (None, None) => Ok(None),
        }
    }

    /// The titles leading from `doc` back to `host`, host last — or empty when they do not.
    ///
    /// A bounded depth-first walk sharing one `budget` with every other embed on the page, so
    /// the arithmetic stays the same whatever the body holds. Every step goes through the
    /// same permission-checked accessor, so a ring passing through a page this reader may not
    /// see is simply not found — which is right rather than merely convenient: naming it
    /// would disclose that page's title, which is the whole of what a restricted title hides.
    async fn cycle_through(
        &self,
        principal: &Principal,
        host: &str,
        doc: &StoredDocument,
        doc_body: &Block,
        baseline: Baseline,
        budget: &mut usize,
    ) -> Result<Vec<String>> {
        if doc.id == host {
            // The simplest ring, and the one somebody actually writes by accident: a page
            // quoting itself.
            return Ok(vec![doc.title.clone()]);
        }
        let mut chain = vec![doc.title.clone()];
        let mut seen: BTreeSet<String> = [doc.id.clone()].into();
        let mut frontier = vec![doc_body.clone()];
        while let Some(body) = frontier.pop() {
            let mut targets = Vec::new();
            collect_embeds(&body, &mut targets, &mut BTreeSet::new());
            for target in targets {
                if *budget == 0 {
                    return Ok(Vec::new());
                }
                *budget -= 1;
                let Some(next) = self.readable_embed(principal, &target, baseline).await? else {
                    continue;
                };
                if next.id == host {
                    chain.push(next.title);
                    return Ok(chain);
                }
                if !seen.insert(next.id.clone()) {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<Block>(&next.body) {
                    chain.push(next.title);
                    frontier.push(parsed);
                }
            }
        }
        Ok(Vec::new())
    }
}

/// Rewrite every address-named embed that `resolved` has an id for into one named by
/// identity.
fn apply_embeds(body: &mut Block, resolved: &HashMap<String, String>) {
    if body.kind == BlockKind::Transclusion {
        if let (None, Some(path)) = (attr(body, "doc"), attr(body, "path")) {
            if let Some(id) = resolved.get(&path) {
                // Removed rather than kept beside it. A block holding both an address and an
                // identity can disagree with itself the moment the target moves, which is the
                // invariant `gw_core::BlockKind::Transclusion` states.
                body.attrs.remove("path");
                body.attrs
                    .insert("doc".into(), serde_json::Value::String(id.clone()));
            }
        }
        return;
    }
    for child in &mut body.content {
        apply_embeds(child, resolved);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, NewDocument};
    use gw_auth::{Permission, Subject};
    use gw_core::{DocumentType, Visibility};

    const SECTION: &str = "0199c0de-0000-7000-8000-00000000000a";
    const GONE: &str = "0199c0de-0000-7000-8000-0000000000ff";

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    fn block(kind: BlockKind) -> Block {
        Block {
            kind,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        }
    }

    fn paragraph(text: &str) -> Block {
        let mut leaf = block(BlockKind::Text);
        leaf.text = Some(text.into());
        let mut p = block(BlockKind::Paragraph);
        p.content.push(leaf);
        p
    }

    fn heading(level: u64, id: Option<&str>, text: &str) -> Block {
        let mut h = block(BlockKind::Heading);
        h.attrs
            .insert("level".into(), serde_json::Value::from(level));
        if let Some(id) = id {
            h.attrs.insert("id".into(), serde_json::Value::from(id));
        }
        let mut leaf = block(BlockKind::Text);
        leaf.text = Some(text.into());
        h.content.push(leaf);
        h
    }

    fn doc(children: Vec<Block>) -> Block {
        let mut d = block(BlockKind::Doc);
        d.content = children;
        d
    }

    /// An embed naming `target` — a document id or a path — optionally anchored.
    fn embed(target: &str, heading: Option<&str>, label: &str) -> Block {
        let mut b = block(BlockKind::Transclusion);
        let key = if target.starts_with('/') {
            "path"
        } else {
            "doc"
        };
        b.attrs.insert(key.into(), serde_json::Value::from(target));
        if let Some(heading) = heading {
            b.attrs
                .insert("heading".into(), serde_json::Value::from(heading));
        }
        b.attrs
            .insert("label".into(), serde_json::Value::from(label));
        doc(vec![b])
    }

    /// The body actually stored at `path`, which is what settling and minting acted on.
    async fn stored_body(store: &Store, path: &str) -> Block {
        let doc = store
            .document_by_path_unchecked(path)
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&doc.body).unwrap()
    }

    async fn page(store: &Store, title: &str, visibility: Visibility, body: Block) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility,
                    body,
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    /// A page with two `##` sections, the second holding a checklist.
    fn sectioned_body() -> Block {
        let mut item = block(BlockKind::TaskItem);
        item.attrs
            .insert("checked".into(), serde_json::Value::from(true));
        item.content.push(paragraph("Probe abgenommen"));
        let mut list = block(BlockKind::TaskList);
        list.content.push(item);
        doc(vec![
            paragraph("Vorspann"),
            heading(2, Some(SECTION), "Dosierung"),
            paragraph("5 mg"),
            heading(2, None, "Nebenwirkungen"),
            list,
        ])
    }

    /// `/quelle` (readable by everybody) and `/geheim` (restricted), plus a reader with no
    /// groups at all — the same default reach an anonymous visitor has.
    async fn fixture() -> (Store, Principal, String, String) {
        let store = store().await;
        let quelle = page(&store, "Quelle", Visibility::Public, sectioned_body()).await;
        let geheim = page(
            &store,
            "Blutbild Müller",
            Visibility::Restricted,
            doc(vec![paragraph("Sehr privat.")]),
        )
        .await;
        (store, Principal::test("gast", &[], &[]), quelle, geheim)
    }

    // --- what a reader may see -------------------------------------------------------------

    #[tokio::test]
    async fn an_embed_of_a_whole_page_carries_that_pages_words_its_path_and_its_current_title() {
        let (store, gast, quelle, _) = fixture().await;
        let body = embed(&quelle, None, "Quelle");
        let seen = store.embeds_for(&gast, "host", &body).await.unwrap();
        let found = &seen[&quelle];
        assert_eq!(found.path, "/quelle");
        assert_eq!(found.title, "Quelle");
        assert!(found.body.as_ref().unwrap().plain_text().contains("5 mg"));
        assert!(found.cycle.is_empty());
    }

    #[tokio::test]
    async fn an_embed_of_one_section_carries_that_section_and_not_the_rest_of_the_page() {
        let (store, gast, quelle, _) = fixture().await;
        let body = embed(&quelle, Some(SECTION), "Dosierung");
        let seen = store.embeds_for(&gast, "host", &body).await.unwrap();
        let shown = seen[&format!("{quelle}#{SECTION}")]
            .body
            .as_ref()
            .unwrap()
            .plain_text();
        assert_eq!(shown, "Dosierung 5 mg");
        assert!(
            !shown.contains("Nebenwirkungen") && !shown.contains("Vorspann"),
            "the frame quoted more of the page than the author asked for: {shown}"
        );
    }

    #[tokio::test]
    async fn an_embed_whose_anchor_is_gone_keeps_its_frame_and_says_so() {
        // D-29. The entry EXISTS — with the source's name and address, so the reader can be
        // told where the section went — and its body is absent. Falling back to the whole
        // page would quietly swap a dosage table for a page the author never meant to quote.
        let (store, gast, quelle, _) = fixture().await;
        let body = embed(&quelle, Some(GONE), "Dosierung");
        let seen = store.embeds_for(&gast, "host", &body).await.unwrap();
        let orphan = &seen[&format!("{quelle}#{GONE}")];
        assert!(orphan.body.is_none(), "it fell back to something");
        assert_eq!(orphan.path, "/quelle");
        assert_eq!(orphan.title, "Quelle");
    }

    #[tokio::test]
    async fn an_embed_of_a_page_this_reader_may_not_read_discloses_nothing_at_all() {
        // THE one. A target the reader may not read, one that does not exist, one in the
        // Papierkorb and one that was purged all answer identically — no entry — because
        // telling them apart is itself the disclosure. Anti-vacuity first: a readable target
        // in the very same call DOES resolve, so this is about the filter and not about a
        // resolver that resolves nothing.
        let (store, gast, quelle, geheim) = fixture().await;
        let mut body = embed(&geheim, None, "Befund");
        body.content
            .push(embed(&quelle, None, "Quelle").content.remove(0));
        let seen = store.embeds_for(&gast, "host", &body).await.unwrap();
        assert!(seen.contains_key(&quelle), "nothing resolved at all");
        assert!(
            !seen.contains_key(&geheim),
            "a restricted page was expanded"
        );

        // And nothing anywhere in the answer says what that page is called or where it is.
        let json = serde_json::to_string(&seen).unwrap();
        assert!(!json.contains("Blutbild"), "{json}");
        assert!(!json.contains("Sehr privat"), "{json}");
        assert!(!json.contains("blutbild"), "{json}");

        // …and somebody who MAY read it gets all three, so the fixture is known to contain
        // something worth hiding.
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/blutbild-mueller",
                Subject::Principal(chef.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        let seen = store.embeds_for(&chef, "host", &body).await.unwrap();
        assert_eq!(seen[&geheim].title, "Blutbild Müller");
        assert!(seen[&geheim]
            .body
            .as_ref()
            .unwrap()
            .plain_text()
            .contains("Sehr privat"));
    }

    #[tokio::test]
    async fn an_embed_naming_an_unreadable_page_by_address_discloses_just_as_little() {
        // The same rule, asked of the other accessor, and it needs its own test: an embed
        // names its target by identity OR by address, both forms reach `documents.body`, and
        // a resolver that filtered one and not the other would be filtered only until
        // somebody restored the corpus from a backup — where every embed is address-named
        // until the next publish.
        let (store, gast, quelle, _) = fixture().await;
        let mut body = embed("/blutbild-mueller", None, "Befund");
        body.content
            .push(embed(&quelle, None, "Quelle").content.remove(0));
        let seen = store.embeds_for(&gast, "host", &body).await.unwrap();
        assert!(seen.contains_key(&quelle), "nothing resolved at all");
        assert!(
            !seen.contains_key("/blutbild-mueller"),
            "a restricted page was expanded because it was named by address"
        );
        let json = serde_json::to_string(&seen).unwrap();
        assert!(!json.contains("Blutbild"), "{json}");
        assert!(!json.contains("Sehr privat"), "{json}");

        // Anti-vacuity: somebody who MAY read it gets it through the very same path.
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/blutbild-mueller",
                Subject::Principal(chef.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        let seen = store.embeds_for(&chef, "host", &body).await.unwrap();
        assert_eq!(seen["/blutbild-mueller"].title, "Blutbild Müller");
    }

    #[tokio::test]
    async fn a_page_that_embeds_itself_is_a_cycle_and_the_frame_names_it() {
        let (store, gast, quelle, _) = fixture().await;
        let body = embed(&quelle, None, "Quelle");
        let seen = store.embeds_for(&gast, &quelle, &body).await.unwrap();
        let found = &seen[&quelle];
        assert_eq!(found.cycle, vec!["Quelle".to_string()]);
        assert!(
            found.body.is_none(),
            "a frame that names a cycle must not also quote half of it"
        );
    }

    #[tokio::test]
    async fn two_pages_quoting_each_other_are_a_cycle_named_in_order() {
        let store = store().await;
        let a = page(&store, "A", Visibility::Public, doc(vec![paragraph("a")])).await;
        let b = page(&store, "B", Visibility::Public, embed(&a, None, "A")).await;
        let gast = Principal::test("gast", &[], &[]);
        let seen = store
            .embeds_for(&gast, &a, &embed(&b, None, "B"))
            .await
            .unwrap();
        assert_eq!(seen[&b].cycle, vec!["B".to_string(), "A".to_string()]);
    }

    #[tokio::test]
    async fn an_embedded_section_resolves_its_own_references_against_the_reader() {
        // The same rule asked one document further out. Without this, every `dok:` inside an
        // embedded section renders as unlinked words — the state that means "you may not read
        // that" — asserted about pages the reader very possibly may.
        let store = store().await;
        let ziel = page(
            &store,
            "Ziel",
            Visibility::Public,
            doc(vec![paragraph("z")]),
        )
        .await;
        let mut leaf = block(BlockKind::Text);
        leaf.text = Some("siehe dort".into());
        leaf.marks = vec![gw_core::Mark::link_to_doc(&ziel)];
        let mut p = block(BlockKind::Paragraph);
        p.content.push(leaf);
        let quelle = page(&store, "Quelle", Visibility::Public, doc(vec![p])).await;

        let gast = Principal::test("gast", &[], &[]);
        let seen = store
            .embeds_for(&gast, "host", &embed(&quelle, None, "Quelle"))
            .await
            .unwrap();
        assert_eq!(seen[&quelle].references[&ziel].path, "/ziel");
    }

    #[tokio::test]
    async fn one_page_expands_a_bounded_number_of_embeds() {
        // An availability control, not a correctness one: every expansion is an
        // authorisation, a row and a JSON parse through the single connection the whole
        // application shares, and nothing caps how many blocks a body holds.
        let (store, gast, quelle, _) = fixture().await;
        let mut body = block(BlockKind::Doc);
        for i in 0..(MAX_EMBEDS_PER_PAGE + 4) {
            // Distinct keys, or the deduplication would do the capping and prove nothing.
            let mut b = block(BlockKind::Transclusion);
            b.attrs
                .insert("doc".into(), serde_json::Value::from(quelle.clone()));
            b.attrs.insert(
                "heading".into(),
                serde_json::Value::from(format!("0199c0de-0000-7000-8000-{i:012}")),
            );
            body.content.push(b);
        }
        let seen = store.embeds_for(&gast, "host", &body).await.unwrap();
        assert_eq!(seen.len(), MAX_EMBEDS_PER_PAGE);
    }

    // --- identity, and what it costs a restore ---------------------------------------------

    #[tokio::test]
    async fn publishing_mints_a_stable_id_onto_every_heading_and_never_moves_one() {
        let store = store().await;
        let quelle = page(
            &store,
            "Quelle",
            Visibility::Public,
            doc(vec![heading(2, None, "Dosierung"), paragraph("5 mg")]),
        )
        .await;
        let body = stored_body(&store, "/quelle").await;
        let first = body.headings()[0]
            .anchor
            .clone()
            .expect("creating a page publishes revision 1, which mints the anchors");

        // Re-publishing must not move it. Re-minting on every save would move every section
        // embed in the wiki each time somebody fixed a typo — the failure a slug already has
        // and the whole reason this id exists.
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/quelle",
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        store
            .publish_revision(&chef, &quelle, &body, None)
            .await
            .unwrap()
            .expect("the publish was refused");
        let again = stored_body(&store, "/quelle").await;
        assert_eq!(again.headings()[0].anchor.as_deref(), Some(first.as_str()));
    }

    #[tokio::test]
    async fn an_embed_naming_an_id_this_database_never_heard_of_falls_back_to_the_path() {
        // What a restored backup produces. The path is what the file carried; the next
        // publish exchanges it for THIS database's id.
        let store = store().await;
        let quelle = page(&store, "Quelle", Visibility::Public, sectioned_body()).await;
        let mut b = block(BlockKind::Transclusion);
        b.attrs.insert(
            "doc".into(),
            serde_json::Value::from("0199c0de-0000-7000-8000-00000000dead"),
        );
        b.attrs
            .insert("path".into(), serde_json::Value::from("/quelle"));
        b.attrs
            .insert("label".into(), serde_json::Value::from("Quelle"));
        let host = page(&store, "Host", Visibility::Public, doc(vec![b])).await;

        let body = stored_body(&store, "/host").await;
        let settled = &body.content[0];
        assert_eq!(
            settled.attrs.get("path").and_then(|v| v.as_str()),
            Some("/quelle"),
            "the unknown id was not exchanged for the path the file carried"
        );
        assert!(
            !settled.attrs.contains_key("doc"),
            "an embed reached the body naming its target twice"
        );

        // A reader resolves it by address in the meantime, through the same accessor.
        let gast = Principal::test("gast", &[], &[]);
        let seen = store.embeds_for(&gast, &host, &body).await.unwrap();
        assert_eq!(seen["/quelle"].title, "Quelle");

        // And the next publish exchanges it for this database's identity.
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/host",
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        store
            .publish_revision(&chef, &host, &body, None)
            .await
            .unwrap()
            .expect("the publish was refused");
        let after = stored_body(&store, "/host").await;
        assert_eq!(
            after.content[0].attrs.get("doc").and_then(|v| v.as_str()),
            Some(quelle.as_str())
        );
        assert!(!after.content[0].attrs.contains_key("path"));
    }

    #[tokio::test]
    async fn a_live_id_wins_against_the_path_a_file_carried() {
        // The order between the two outcomes is the whole decision: a page RENAMED after the
        // export has a stale path in the file and a perfectly good identity.
        let store = store().await;
        let quelle = page(&store, "Quelle", Visibility::Public, sectioned_body()).await;
        let mut b = block(BlockKind::Transclusion);
        b.attrs
            .insert("doc".into(), serde_json::Value::from(quelle.clone()));
        b.attrs
            .insert("path".into(), serde_json::Value::from("/wo-sie-mal-war"));
        b.attrs
            .insert("label".into(), serde_json::Value::from("Quelle"));
        page(&store, "Host", Visibility::Public, doc(vec![b])).await;
        let body = stored_body(&store, "/host").await;
        assert_eq!(
            body.content[0].attrs.get("doc").and_then(|v| v.as_str()),
            Some(quelle.as_str())
        );
        assert!(!body.content[0].attrs.contains_key("path"));
    }

    #[tokio::test]
    async fn an_embed_mints_no_task_records_on_the_page_that_embeds_it() {
        // D-30's reconciliation half, and the reason it holds: an embed is a REFERENCE, so
        // the embedding page's body holds none of the target's blocks and `reconcile_tasks`
        // — which walks `content` — has nothing of somebody else's checklist to mint records
        // for. Anti-vacuity: the source page's own checklist DOES have a record.
        let (store, _, quelle, _) = fixture().await;
        let host = page(
            &store,
            "Host",
            Visibility::Public,
            embed(&quelle, None, "Quelle"),
        )
        .await;
        let count = |id: String| {
            let store = &store;
            async move {
                sqlx::query_scalar::<_, i64>("SELECT count(*) FROM tasks WHERE doc_id = ?1")
                    .bind(id)
                    .fetch_one(&store.pool)
                    .await
                    .unwrap()
            }
        };
        assert_eq!(
            count(quelle).await,
            1,
            "the source page grew no task record"
        );
        assert_eq!(
            count(host).await,
            0,
            "embedding a checklist minted a second record for somebody else's task"
        );
    }

    #[tokio::test]
    async fn an_embed_is_an_edge_in_the_graph() {
        let (store, _, quelle, _) = fixture().await;
        let host = page(
            &store,
            "Host",
            Visibility::Public,
            embed(&quelle, Some(SECTION), "Dosierung"),
        )
        .await;
        let edges: Vec<(String, String)> =
            sqlx::query_as("SELECT from_doc, to_doc FROM links ORDER BY to_doc")
                .fetch_all(&store.pool)
                .await
                .unwrap();
        assert_eq!(edges, vec![(host, quelle)]);
    }
}
