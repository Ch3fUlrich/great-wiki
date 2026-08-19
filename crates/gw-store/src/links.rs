//! The graph: which page points at which, extracted from a body when it is published.
//!
//! The `links` table *is* the graph (0009). Everything here either fills it — inside the
//! transaction that writes the revision the edges were read out of — or reads it back
//! filtered by what the caller may see.
//!
//! **What counts as a link to a page.** D-5 says an internal link stores the target's
//! document **id**, and [`gw_core::Mark::target_doc`] is how a mark states one. But nothing
//! in this system writes such a mark: `gw_core::markdown` imports `[text](dest)` as
//! [`gw_core::Mark::link_to_url`], and the editor's link control writes an `href` too.
//! Collecting `doc` marks alone would therefore leave this table permanently empty and the
//! graph edgeless. So an `href` counts as well, when it names a page in this wiki — see
//! [`wiki_path`] for exactly which shapes those are and why.
//!
//! The author's body is NOT rewritten on the way past. Canonicalising an `href` into a
//! `doc` mark would edit what somebody wrote, on publish, without being asked; the edge is
//! recorded and the body is left alone. What that costs is stated in [`wiki_path`]: an edge
//! from an `href` is resolved through the path, so moving the target page afterwards leaves
//! the edge correct (it is stored by id) but the *link text in the body* stale, which is
//! precisely the breakage D-5 exists to avoid and is not this task's to fix.
//!
//! **An unresolvable internal link is not an error.** A link to a page that does not exist,
//! or was deleted, is a fact about the body — the publish records no edge for it and
//! proceeds. That is why targets are resolved before they are inserted rather than being
//! inserted hopefully: SQLite's conflict resolution does *not* apply to FOREIGN KEY
//! constraints, so `INSERT OR IGNORE` of an edge to a document that is not there aborts the
//! statement, and with it the publish.

use crate::Store;
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::{Block, Mark, MarkKind};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap, HashSet};
use url::Url;

/// One page that links to the page being read.
#[derive(Debug, Clone, Serialize)]
pub struct Backlink {
    pub id: String,
    pub path: String,
    pub title: String,
}

/// One page in the graph. Every field is taken from a document the caller may read.
#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub id: String,
    pub path: String,
    pub title: String,
}

/// One link, by document id. Both ends are always present in [`Graph::nodes`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

/// The pages the caller may read and the links between them (D-4).
#[derive(Debug, Clone, Default, Serialize)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Everything one body points at, before any of it is known to exist.
#[derive(Default)]
struct Targets {
    /// Document ids named outright by a `doc` mark (D-5).
    docs: BTreeSet<String>,
    /// Wiki paths an `href` names, normalised by [`wiki_path`].
    paths: BTreeSet<String>,
}

/// Every link target in this body, deduplicated, in no particular order.
///
/// `from` is the linking document's OWN path — the base an `href` with no leading slash is
/// resolved against, per [`wiki_path`].
fn collect(body: &Block, into: &mut Targets, from: &str, public_origin: Option<&Url>) {
    for mark in &body.marks {
        // A link carries EITHER `doc` or `href`, never both — `gw_core::Mark` says so — and
        // `else` rather than a second `if` keeps that true here even if one ever did.
        if let Some(doc) = mark.target_doc() {
            into.docs.insert(doc.to_string());
        } else if let Some(path) = internal_path(mark, from, public_origin) {
            into.paths.insert(path);
        }
    }
    for child in &body.content {
        collect(child, into, from, public_origin);
    }
}

/// The wiki path a link mark's `href` names, if it names one at all.
fn internal_path(mark: &Mark, from: &str, public_origin: Option<&Url>) -> Option<String> {
    if mark.kind != MarkKind::Link {
        return None;
    }
    wiki_path(mark.attrs.get("href")?.as_str()?, from, public_origin)
}

/// The document path an address names, or `None` if it does not name one in this wiki.
///
/// **Internal is "a relative reference with no scheme and no authority" — or an absolute
/// one whose origin matches this deployment's configured `public_origin` exactly.**
/// `web/src/routes` serves documents from `/[...path]`, so a page's `documents.path` *is*
/// its URL path and `/rundgang/tabellen` needs no translation. Anything carrying a scheme
/// (`https:`, `mailto:`, `javascript:`) addresses some other origin by default — this crate
/// still has no idea which origin is its own; the deployment host is configuration, not
/// schema — unless the caller hands that configuration in: when `public_origin` is `Some`
/// and the address's origin matches it exactly (see [`internal_path_from_absolute`]), the
/// URL resolves to the page it names, same as a relative reference would. With
/// `public_origin` unset, every absolute URL is external, exactly as it always was before
/// this parameter existed — this function still never guesses at a hostname on its own and
/// draws edges from it.
///
/// Two limitations remain regardless of configuration. An authority with no scheme
/// (`//example.org/x`, "protocol-relative") stays external unconditionally: it never
/// reaches the branch above (see [`has_scheme`]), so `public_origin` is never consulted for
/// it. And `public_origin` only shapes what THIS publish records: a page published before
/// the origin was configured — or before an absolute self-link in it was ever recognised —
/// keeps the edges that publish produced until something republishes it; nothing here
/// backfills a page that has not changed since.
///
/// **A reference with no leading slash is resolved against `from` — the linking document's
/// OWN path — never against the site root.** `web/src/app.html` sets no `<base>`, so a
/// browser resolves `href="nachbar"` written on `/rundgang/tabellen` against *that page's*
/// URL: the click lands on `/rundgang/nachbar`, never on `/nachbar`. Root-anchoring it
/// instead — what this function did before the review that found this — named a page the
/// link does not go to. That is not a disclosure (both ends are still permission-filtered
/// by [`Store::backlinks_for`]), but a backlinks panel that lists the wrong linker while
/// omitting the real one is worse than recording nothing, and the earlier doc comment on
/// this very function said "that is what a browser does with it too, and the graph should
/// agree with the link" one paragraph after doing the opposite of that.
///
/// The resolution itself goes through [`url`]'s WHATWG implementation, against a
/// placeholder origin holding `from` — the same trick `safeHref` in
/// `web/src/lib/blocks/render.ts` plays on the client, and for the same reason: that is the
/// actual algorithm a browser runs, so this is not a second, possibly-disagreeing one. It
/// also makes `.` and `..` work, which is a consequence of resolving against a real base
/// rather than a goal in itself — nothing here special-cases them.
///
/// A query or a fragment addresses part of a page rather than a different one, so `?x=1`
/// and `#abschnitt` are trimmed before any resolution happens — and an address that is
/// *only* a query, a fragment, `/`, or empty names the page it is already on (or nothing),
/// which is refused here without ever consulting `from`.
fn wiki_path(href: &str, from: &str, public_origin: Option<&Url>) -> Option<String> {
    let href = href.trim();
    if has_scheme(href) {
        // An address carrying a scheme is internal only when a public origin is configured
        // AND this address's origin matches it exactly — see `internal_path_from_absolute`.
        // With none configured this is unchanged from before that method existed: external,
        // unconditionally, because there is nothing to compare against.
        return public_origin.and_then(|origin| internal_path_from_absolute(href, origin));
    }
    if href.starts_with("//") {
        return None;
    }
    // `?` and `#` both end the path, in whichever order they turn up.
    let path = href.split(['?', '#']).next().unwrap_or_default();
    // A trailing slash is how a browser spells the same page, never a different one.
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    if let Some(rooted) = path.strip_prefix('/') {
        return Some(format!("/{rooted}"));
    }
    // No leading slash: resolve against the document this href was written on, exactly as
    // a browser would with no `<base>` in scope. `from` always has one (`Store::create_
    // document`'s `resolved_path` guarantees it), so this base always parses.
    let base = Url::parse(&format!("https://wiki.invalid{from}")).ok()?;
    let resolved = base.join(path).ok()?;
    let resolved = resolved.path().trim_end_matches('/');
    if resolved.is_empty() {
        return None;
    }
    Some(resolved.to_string())
}

/// The wiki path an absolute URL names, when its origin matches `public_origin` EXACTLY —
/// scheme, host AND port, per [`url::Url::origin`]'s equality — or `None` when it points
/// elsewhere, or does not carry a hierarchical origin at all.
///
/// `href` has already passed [`has_scheme`], so [`Url::parse`] succeeds for anything RFC
/// 3986 would call an absolute URI. A scheme this wiki has no origin for in the first place
/// — `mailto:`, `javascript:`, `data:` — parses to [`url::Origin::Opaque`], which can never
/// equal `public_origin`'s [`url::Origin::Tuple`], so nothing here has to name those schemes
/// specially: the comparison itself already refuses them.
///
/// Both origins come out of `Url::parse`, which lower-cases a scheme and a domain host as
/// part of parsing (WHATWG URL, not a rule stated again here) — so `HTTPS://WIKI.example/x`
/// and `https://wiki.example` already compare equal without this function doing anything
/// case-insensitive on purpose. `http://` and `https://` on the same host do NOT compare
/// equal, nor does the same scheme and host on a different port: `Origin::Tuple` carries all
/// three, and comparing by hostname alone is exactly the mistake D-5's gap analysis warns
/// against (a public deployment terminates TLS ahead of this process; treating scheme as a
/// don't-care here would make an internal `http://` probe indistinguishable from the public
/// `https://` origin).
fn internal_path_from_absolute(href: &str, public_origin: &Url) -> Option<String> {
    let parsed = Url::parse(href).ok()?;
    if parsed.origin() != public_origin.origin() {
        return None;
    }
    // Query and fragment are already separate components of a parsed URL, unlike the
    // relative-reference branch above, which has to split them out of a raw string by hand.
    let path = parsed.path().trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    Some(path.to_string())
}

/// Whether an address begins with a URI scheme, as RFC 3986 defines one.
///
/// Judged on the characters rather than on a list of schemes: the question is "does this
/// address name its own scheme", and `data:`, `tel:` or one nobody has thought of answers it
/// just as `https:` does. A colon appearing after a `/`, `?` or `#` is part of the path.
fn has_scheme(href: &str) -> bool {
    let Some(colon) = href.find(':') else {
        return false;
    };
    let scheme = &href[..colon];
    if scheme.contains(['/', '?', '#']) {
        return false;
    }
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// A `root` as [`Store::graph_for`] compares it: leading slash, no trailing one.
///
/// `/darm/`, `darm` and `/darm` all name the same subtree, and a caller who spells it the
/// second way must not silently get an empty graph. `/` and the empty string name the whole
/// wiki, which is what `None` already means — normalised to it rather than to a prefix
/// everything happens to start with.
fn normalise_root(root: &str) -> String {
    format!("/{}", root.trim().trim_matches('/'))
}

/// Whether `path` is inside the subtree at `root`. `None` is the whole wiki.
///
/// The root itself is in its own subtree — asking for `/darm` and not being shown `/darm`
/// would be a surprise — and the boundary is a SEGMENT, so `/darmspiegelung` is outside
/// `/darm`. A bare `starts_with` would pull it in, which is the ordinary prefix bug: it
/// would add pages nobody asked for, and if this were the permission check rather than a
/// view narrowing it would be a disclosure. It is not the permission check; see
/// [`Store::graph_for`].
fn within_root(root: Option<&str>, path: &str) -> bool {
    let Some(root) = root else {
        return true;
    };
    if root == "/" {
        return true;
    }
    path == root || path.starts_with(&format!("{root}/"))
}

/// The id of the live document at `path`, or `None`.
async fn document_at(conn: &mut sqlx::SqliteConnection, path: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM documents WHERE path = ?1 AND deleted_at IS NULL")
            .bind(path)
            .fetch_optional(conn)
            .await?,
    )
}

/// The same id back if a live document holds it, or `None`.
async fn document_with_id(conn: &mut sqlx::SqliteConnection, id: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT id FROM documents WHERE id = ?1 AND deleted_at IS NULL")
            .bind(id)
            .fetch_optional(conn)
            .await?,
    )
}

/// Replace this document's edges. Takes a CONNECTION, not the pool, so it joins the
/// caller's transaction: a publish that fails afterwards must leave no edges behind for a
/// revision that does not exist. Same reasoning as [`crate::revisions::append_revision`],
/// which is its only caller — and the pool would not do even if the reasoning were absent,
/// because it holds a single connection and asking it for a second one inside a transaction
/// waits for the one the transaction is holding until it times out.
///
/// **This write is deliberately unfiltered.** `from_doc` may hold `links` edges to a
/// document its own author cannot Read — nothing here asks. That is correct only because
/// [`Store::backlinks_for`] gates on the *target* at read time; the read side carries the
/// whole disclosure property this table has, and any later consumer of `links` (Task 9's
/// graph, most concretely) must filter both ends rather than assume a row here already
/// implies the author could see where it points.
pub(crate) async fn replace_links(
    conn: &mut sqlx::SqliteConnection,
    from_doc: &str,
    from_path: &str,
    body: &Block,
    public_origin: Option<&Url>,
) -> Result<()> {
    let mut found = Targets::default();
    collect(body, &mut found, from_path, public_origin);

    sqlx::query("DELETE FROM links WHERE from_doc = ?1")
        .bind(from_doc)
        .execute(&mut *conn)
        .await?;

    // Resolved to ids first, because that is what an edge points at (D-5) and because a
    // target that resolves to nothing must be dropped rather than inserted: see the module
    // comment on foreign keys. Two spellings of one page — a `doc` mark and an `href`, or
    // `/ziel` and `ziel/` — collapse into one edge here.
    let mut edges = BTreeSet::new();
    for id in &found.docs {
        match document_with_id(&mut *conn, id).await? {
            Some(resolved) => {
                edges.insert(resolved);
            }
            // Silent otherwise: a `doc` mark naming a deleted or never-existing document is
            // an ordinary fact about the body (see the module comment), not an error. But
            // silent all the way to "nothing at all" leaves "why is my backlinks panel
            // empty" with no diagnosis, so it is at least named at debug level.
            None => tracing::debug!(
                target: "gw_store::links",
                %from_doc,
                doc = %id,
                "a `doc` mark named no live document; no edge recorded"
            ),
        }
    }
    for path in &found.paths {
        match document_at(&mut *conn, path).await? {
            Some(resolved) => {
                edges.insert(resolved);
            }
            None => tracing::debug!(
                target: "gw_store::links",
                %from_doc,
                %path,
                "an internal-looking href resolved to no live document; no edge recorded"
            ),
        }
    }

    for to_doc in edges {
        // A page linking to itself is not an edge worth drawing, and it would render as a
        // self-loop in the graph and as a backlink to the page you are already reading.
        if to_doc == from_doc {
            continue;
        }
        // OR IGNORE for the primary key alone, and belt-and-braces even there since the set
        // above has already deduplicated. It is NOT what makes an absent target harmless —
        // conflict resolution does not apply to foreign keys, and the resolution above is.
        sqlx::query("INSERT OR IGNORE INTO links (from_doc, to_doc) VALUES (?1, ?2)")
            .bind(from_doc)
            .bind(&to_doc)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

impl Store {
    /// The pages that link *to* `document_id`, filtered to those the caller may read.
    ///
    /// **A backlink the caller may not read is omitted entirely.** Not listed as "a page you
    /// cannot see", and not counted: either would say that the page exists and how many
    /// there are, which is the whole of what a private page's title was hiding.
    ///
    /// The filtering is per candidate, through [`Store::document_for`] — the crate's one
    /// permission-checked document accessor — and never in the SQL. A `WHERE path LIKE`
    /// prefix would be a second, weaker answer to a question `can()` already answers, and
    /// D-3 makes membership per document rather than per subtree, so a prefix cannot express
    /// it in the first place.
    ///
    /// Nothing is returned at all to somebody who may not read the page being asked about,
    /// exactly as [`Store::revisions_for`] refuses history: which pages point at a page is a
    /// fact about that page. An empty list is the answer both for "no backlinks" and for
    /// "not for you", which is the same closed conflation as everywhere else in this crate.
    pub async fn backlinks_for(
        &self,
        principal: &Principal,
        document_id: &str,
    ) -> Result<Vec<Backlink>> {
        if !self.may(principal, document_id, Action::Read).await? {
            return Ok(Vec::new());
        }

        // The candidates, as paths and nothing else. A path is what `document_for` takes,
        // and it is the least this has to read in order to ask: the id and the title are
        // taken from the document that accessor hands back, so no value reaches a caller
        // without having gone through it.
        let candidates: Vec<String> = sqlx::query_scalar(
            "SELECT d.path FROM links l JOIN documents d ON d.id = l.from_doc \
             WHERE l.to_doc = ?1 ORDER BY d.path",
        )
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for path in candidates {
            let Some(doc) = self.document_for(principal, &path, Action::Read).await? else {
                continue;
            };
            out.push(Backlink {
                id: doc.id,
                path: doc.path,
                title: doc.title,
            });
        }
        Ok(out)
    }

    /// The pages the caller may read and the links between them (D-4), optionally narrowed
    /// to the subtree at `root`.
    ///
    /// **An edge is emitted only when the caller may read BOTH ends.** An edge with one
    /// unreadable end says that page exists, and its label would say what it is called —
    /// which is the whole of what a restricted title was hiding. Drawing the far end as an
    /// anonymous node is not a fix: "there is something here you may not see" is the same
    /// disclosure with the name filed off. It is omitted entirely, and so is a node that no
    /// surviving edge touches.
    ///
    /// **`root` narrows the view; it never decides anything.** A path prefix is a poor
    /// permission answer — D-3 makes membership per document, so a project spanning two
    /// subtrees with different grants is normal — and the two must not be confused. The
    /// prefix is applied first because it is cheap and discloses nothing, and then every
    /// surviving candidate is put through [`Store::document_for`], which is what actually
    /// decides. Never the other way round.
    ///
    /// A `root` naming a subtree that does not exist, or one the caller may not read,
    /// answers an empty graph rather than an error — the same closed conflation the rest of
    /// this crate makes, and for the same reason: distinguishing them would answer "does
    /// /geheim exist" to anybody who asks.
    ///
    /// **The permission question is asked once per DOCUMENT, never once per edge.** This is
    /// [`Store::backlinks_for`]'s shape run over the whole corpus rather than over one
    /// page's candidates, so what is a fixed cost there is N+1 here: a hub page with forty
    /// links would otherwise be authorised forty times, and a page at both ends of forty
    /// edges eighty. Two hoists make it once. The baseline comes out of the loop entirely —
    /// it is a property of the caller, exactly as [`Store::tree_for`] treats it — and the
    /// verdict per document is memoised in `readable`/`refused`, which is also what makes
    /// the edge filter below a pair of hash lookups rather than two more queries.
    pub async fn graph_for(&self, principal: &Principal, root: Option<&str>) -> Result<Graph> {
        // The candidates, with the paths their ends live at. A JOIN, so that resolving forty
        // edges to eighty paths is one round trip rather than eighty — and it discloses
        // nothing, because nothing here is returned: `document_for` below decides what is,
        // exactly as `backlinks_for`'s own candidate query hands paths to the same accessor.
        // `deleted_at IS NULL` on both ends because a soft-deleted page is not in the graph;
        // `document_for` would refuse it anyway, and dropping it here saves asking.
        let rows: Vec<(String, String, String, String)> = sqlx::query_as(
            "SELECT l.from_doc, f.path, l.to_doc, t.path FROM links l \
             JOIN documents f ON f.id = l.from_doc AND f.deleted_at IS NULL \
             JOIN documents t ON t.id = l.to_doc AND t.deleted_at IS NULL \
             ORDER BY f.path, t.path",
        )
        .fetch_all(&self.pool)
        .await?;

        let root = root.map(normalise_root);
        // Once, for the whole walk. See `tree_for`, which hoists it for the same reason.
        let baseline = self.baseline_for(principal).await?;

        let mut readable: HashMap<String, GraphNode> = HashMap::new();
        let mut refused: HashSet<String> = HashSet::new();
        for (id, path) in rows
            .iter()
            .flat_map(|(f, f_path, t, t_path)| [(f, f_path), (t, t_path)])
        {
            if readable.contains_key(id) || refused.contains(id) {
                continue;
            }
            if !within_root(root.as_deref(), path) {
                refused.insert(id.clone());
                continue;
            }
            match self
                .document_for_with_baseline(principal, path, Action::Read, baseline)
                .await?
            {
                Some(doc) => {
                    readable.insert(
                        id.clone(),
                        GraphNode {
                            id: doc.id,
                            path: doc.path,
                            title: doc.title,
                        },
                    );
                }
                None => {
                    refused.insert(id.clone());
                }
            }
        }

        // BOTH ends, and `&&` is the whole property: with `||` an edge would name a page the
        // caller cannot read at its far end.
        let edges: Vec<GraphEdge> = rows
            .into_iter()
            .filter(|(from, _, to, _)| readable.contains_key(from) && readable.contains_key(to))
            .map(|(from, _, to, _)| GraphEdge { from, to })
            .collect();

        // A node no surviving edge touches is dropped: it entered this list only because it
        // was one end of a candidate edge, and without that edge it is not part of any
        // connection anybody drew. Sorted by path so the answer is stable across calls —
        // `HashMap` iteration order is not, and a graph that reshuffles itself between two
        // identical requests looks broken.
        let touched: HashSet<&String> = edges.iter().flat_map(|e| [&e.from, &e.to]).collect();
        let mut nodes: Vec<GraphNode> = readable
            .values()
            .filter(|node| touched.contains(&node.id))
            .cloned()
            .collect();
        nodes.sort_by(|a, b| a.path.cmp(&b.path));

        Ok(Graph { nodes, edges })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, NewDocument};
    use gw_auth::{Permission, Subject};
    use gw_core::{BlockKind, DocumentType, Mark, Visibility};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    fn leaf(text: &str, mark: Mark) -> Block {
        Block {
            kind: BlockKind::Text,
            attrs: Default::default(),
            content: Vec::new(),
            text: Some(text.into()),
            marks: vec![mark],
        }
    }

    fn wrap(kind: BlockKind, children: Vec<Block>) -> Block {
        Block {
            kind,
            attrs: Default::default(),
            content: children,
            text: None,
            marks: Vec::new(),
        }
    }

    /// A body carrying one link per mark: the first in a top-level paragraph, the rest
    /// inside a blockquote, so the walk has to recurse rather than look one level down.
    fn linking_body(marks: Vec<Mark>) -> Block {
        let mut content = Vec::new();
        for (i, mark) in marks.into_iter().enumerate() {
            let paragraph = wrap(BlockKind::Paragraph, vec![leaf("siehe dort", mark)]);
            content.push(if i == 0 {
                paragraph
            } else {
                wrap(BlockKind::Blockquote, vec![paragraph])
            });
        }
        wrap(BlockKind::Doc, content)
    }

    /// A body linking to each document **by id** — a `doc` mark, per D-5.
    fn body_linking_to(targets: &[&str]) -> Block {
        linking_body(targets.iter().map(|id| Mark::link_to_doc(id)).collect())
    }

    /// A body linking to each `href` — what the markdown importer and the editor's link
    /// control actually write today.
    fn body_linking_to_hrefs(hrefs: &[&str]) -> Block {
        linking_body(hrefs.iter().map(|h| Mark::link_to_url(h)).collect())
    }

    async fn page(store: &Store, title: &str, visibility: Visibility) -> String {
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
                    body: body_linking_to(&[]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap()
    }

    /// `/von`, `/ziel-a` and `/ziel-b`, plus somebody who may edit the first.
    async fn fixture_with_three_pages() -> (Store, Principal, String, String, String) {
        let store = store().await;
        let from = page(&store, "Von", Visibility::Public).await;
        let a = page(&store, "Ziel A", Visibility::Public).await;
        let b = page(&store, "Ziel B", Visibility::Public).await;

        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/von",
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        (store, chef, from, a, b)
    }

    async fn edges(store: &Store) -> Vec<(String, String)> {
        sqlx::query_as("SELECT from_doc, to_doc FROM links ORDER BY to_doc")
            .fetch_all(&store.pool)
            .await
            .unwrap()
    }

    // --- what a publish records ---------------------------------------------------------

    #[tokio::test]
    async fn publishing_records_the_links_in_the_body() {
        let (store, chef, from, a, b) = fixture_with_three_pages().await;
        store
            .publish_revision(&chef, &from, &body_linking_to(&[&a, &b]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let rows = edges(&store).await;
        assert_eq!(
            rows.len(),
            2,
            "expected one edge per linked document, got {rows:?}"
        );
        assert!(rows.iter().all(|(f, _)| *f == from));
    }

    #[tokio::test]
    async fn republishing_replaces_rather_than_accumulates() {
        let (store, chef, from, a, b) = fixture_with_three_pages().await;
        store
            .publish_revision(&chef, &from, &body_linking_to(&[&a, &b]), None)
            .await
            .unwrap();
        store
            .publish_revision(&chef, &from, &body_linking_to(&[&a]), None)
            .await
            .unwrap();

        let n: i64 = sqlx::query_scalar("SELECT count(*) FROM links WHERE from_doc = ?1")
            .bind(&from)
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            n, 1,
            "the removed link is still an edge — edges accumulated instead of being replaced"
        );
    }

    #[tokio::test]
    async fn a_failed_publish_leaves_no_edges() {
        let (store, chef, from, a, b) = fixture_with_three_pages().await;

        // One published body first, so the failed publish below has an edge to destroy as
        // well as an edge to write. Without it this test passes over an implementation that
        // extracts nothing at all, which is the shape the very first skeleton had.
        store
            .publish_revision(&chef, &from, &body_linking_to(&[&a]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        // Fail at exactly the point where a body could be committed without its edges — or
        // edges without their revision. A trigger is the only way to force it from outside.
        sqlx::query(
            "CREATE TRIGGER refuse_revisions BEFORE INSERT ON revisions
             BEGIN SELECT RAISE(ABORT, 'nope'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        let result = store
            .publish_revision(&chef, &from, &body_linking_to(&[&b]), None)
            .await;
        assert!(result.is_err(), "the publish should have failed");

        assert_eq!(
            edges(&store).await,
            vec![(from, a)],
            "the revision was rolled back and its edges were not — both the edge it wrote \
             and the edge it deleted are the graph describing a revision that does not exist"
        );
    }

    // --- which links are edges ----------------------------------------------------------

    #[test]
    fn which_addresses_name_a_page_in_this_wiki() {
        // The rule itself, stated once and away from the database, because the tests below
        // can only show that a resolvable address resolved — not why an unresolvable one
        // was never looked up. `from` is the linking document's own path; most rows below
        // use a root-level one (`/von`) because it does not matter to them, but the ones
        // that DO care about it are the point of this test after the review that found
        // Task 7 root-anchoring a bare relative reference instead of resolving it — see the
        // dedicated block below the table.
        for (href, from, expected) in [
            ("/ziel-a", "/von", Some("/ziel-a")),
            // A source page at the ROOT makes root-anchoring and "resolve against `from`"
            // agree, which is exactly why this shape alone cannot tell the two apart — the
            // block below the table is what actually exercises resolution.
            ("ziel-a", "/von", Some("/ziel-a")),
            ("/ziel/a/", "/von", Some("/ziel/a")),
            ("/ziel-a?von=hier", "/von", Some("/ziel-a")),
            ("/ziel-a#abschnitt", "/von", Some("/ziel-a")),
            ("  /ziel-a  ", "/von", Some("/ziel-a")),
            // A colon AFTER a separator is part of the path, not a scheme.
            ("/ziel:a", "/von", Some("/ziel:a")),
            // A colon before one is a scheme, even when it looks like a slug. That is what
            // a browser does with it too, which is the only reading that matches the link.
            ("ziel:a", "/von", None),
            ("https://example.org/ziel-a", "/von", None),
            ("HTTPS://example.org/ziel-a", "/von", None),
            ("mailto:jemand@example.org", "/von", None),
            ("javascript:alert(1)", "/von", None),
            // An authority, so some other origin — even though the scheme is missing.
            ("//example.org/ziel-a", "/von", None),
            // Part of a page rather than a different page. None of these ever reach `from`.
            ("#abschnitt", "/von", None),
            ("?von=hier", "/von", None),
            ("/", "/von", None),
            ("", "/von", None),
            ("   ", "/von", None),
        ] {
            assert_eq!(
                wiki_path(href, from, None).as_deref(),
                expected,
                "the address `{href}` from `{from}` was read wrongly"
            );
        }

        // The property the review actually found broken: a bare relative reference — no
        // leading slash — is resolved against the page it was WRITTEN on, not against the
        // root. The probe that found it: `/rundgang/tabellen` linking to `nachbar` is a
        // click to `/rundgang/nachbar`, because `web/src/app.html` sets no `<base>` and a
        // browser resolves it against its own URL. Root-anchoring it (what this function
        // did before this fix) would have named `/nachbar` — a page the link does not go
        // to. `.` and `..` are exercised here too, as a consequence of resolving against a
        // real base rather than as cases of their own.
        for (href, from, expected) in [
            ("nachbar", "/rundgang/tabellen", Some("/rundgang/nachbar")),
            ("./nachbar", "/rundgang/tabellen", Some("/rundgang/nachbar")),
            ("../ziel-a", "/rundgang/tabellen", Some("/ziel-a")),
            // Two levels up from a page one level deep does not reach past the root; a
            // browser resolving this treats the extra `..` as a no-op rather than an error.
            ("../../ziel-a", "/rundgang", Some("/ziel-a")),
        ] {
            assert_eq!(
                wiki_path(href, from, None).as_deref(),
                expected,
                "the address `{href}` from `{from}` did not resolve against its own page"
            );
        }
    }

    /// The gap this test closes: `wiki_path` used to treat ANY scheme-carrying address as
    /// external, unconditionally, because `gw-store` had no idea what its own origin was —
    /// see the module's doc comment. Once an origin IS configured, an absolute URL whose
    /// origin matches it EXACTLY is this wiki after all; a different host, a different
    /// scheme or a different port is still external, and `from` never matters here — an
    /// absolute URL does not resolve against the page it was written on.
    #[test]
    fn an_absolute_url_at_the_configured_origin_is_internal() {
        let origin = Url::parse("https://wiki.ohje.ooguy.com").unwrap();

        for (href, expected) in [
            // The exact shape from the report: pasted from the address bar.
            (
                "https://wiki.ohje.ooguy.com/darm/labor",
                Some("/darm/labor"),
            ),
            // Trailing slash, query and fragment behave exactly as the relative case does.
            (
                "https://wiki.ohje.ooguy.com/darm/labor/",
                Some("/darm/labor"),
            ),
            (
                "https://wiki.ohje.ooguy.com/darm/labor?x=1",
                Some("/darm/labor"),
            ),
            (
                "https://wiki.ohje.ooguy.com/darm/labor#abschnitt",
                Some("/darm/labor"),
            ),
            // Host AND scheme are compared case-insensitively — the URL parser lowercases
            // both — so shouting the address bar's contents does not change the origin.
            (
                "HTTPS://WIKI.OHJE.OOGUY.COM/darm/labor",
                Some("/darm/labor"),
            ),
            // A different host is a different origin, full stop — including one that
            // merely CONTAINS the real host as a substring or a suffix.
            ("https://evil.example/darm/labor", None),
            ("https://wiki.ohje.ooguy.com.evil.example/darm/labor", None),
            // `https://wiki.ohje.ooguy.com` and `http://wiki.ohje.ooguy.com` are different
            // origins — that is the entire point of comparing by `url::Url::origin` rather
            // than by hostname — even though a human would call them "the same site".
            ("http://wiki.ohje.ooguy.com/darm/labor", None),
            // Ditto for a non-default port.
            ("https://wiki.ohje.ooguy.com:8443/darm/labor", None),
            // The site root names no particular page, exactly as a bare `/` does for a
            // relative reference.
            ("https://wiki.ohje.ooguy.com/", None),
            ("https://wiki.ohje.ooguy.com", None),
        ] {
            assert_eq!(
                wiki_path(href, "/von", Some(&origin)).as_deref(),
                expected,
                "the address `{href}` against the configured origin `{origin}` was read \
                 wrongly"
            );
        }
    }

    /// The safety property: unset behaves exactly as before this existed, for an address
    /// that would otherwise have matched — and nothing here panics on `None`.
    #[test]
    fn with_no_origin_configured_an_absolute_url_is_still_external() {
        assert_eq!(
            wiki_path("https://wiki.ohje.ooguy.com/darm/labor", "/von", None),
            None,
        );
    }

    #[tokio::test]
    async fn a_bare_relative_href_resolves_against_its_own_page_not_the_root() {
        // The end-to-end shape of the probe that found this: a page at `/rundgang/tabellen`
        // links to `nachbar` with no leading slash. A page genuinely exists at the WRONG
        // target, `/nachbar`, so root-anchoring (what publishing used to do) would not have
        // recorded no edge — it would have recorded a real, wrong one, to a page this link
        // does not go to. The correct target, `/rundgang/nachbar`, also exists, so this
        // proves resolution lands on it rather than merely failing safe onto neither.
        let store = store().await;
        let wrong_target = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Nachbar (Wurzel)".into(),
                    slug: Some("nachbar".into()),
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body_linking_to(&[]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();
        let source = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: Some("/rundgang".into()),
                    doc_type: DocumentType::Page,
                    title: "Tabellen".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body_linking_to(&[]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();
        let right_target = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: Some("/rundgang".into()),
                    doc_type: DocumentType::Page,
                    title: "Nachbar".into(),
                    slug: Some("nachbar".into()),
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body_linking_to(&[]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();

        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/rundgang/tabellen",
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        store
            .publish_revision(&chef, &source, &body_linking_to_hrefs(&["nachbar"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            edges(&store).await,
            vec![(source, right_target)],
            "a bare relative href must resolve against its own document's path; an edge to \
             {wrong_target} would mean the graph named /nachbar, which the link does not go to"
        );
    }

    #[tokio::test]
    async fn a_link_written_as_a_path_is_an_edge_too() {
        // The shape that actually exists: the markdown importer and the editor's link
        // control both write an `href`, and nothing in this system writes a `doc` yet.
        let (store, chef, from, a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(
                &chef,
                &from,
                &body_linking_to_hrefs(&["/ziel-a", "ziel-a/", "/ziel-a#abschnitt"]),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            edges(&store).await,
            vec![(from.clone(), a.clone())],
            "three spellings of one page are one edge"
        );
    }

    #[tokio::test]
    async fn a_link_out_of_this_wiki_is_not_an_edge() {
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(
                &chef,
                &from,
                &body_linking_to_hrefs(&[
                    "https://example.org/ziel-a",
                    "mailto:jemand@example.org",
                    "//example.org/ziel-a",
                    "#abschnitt",
                ]),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            edges(&store).await,
            vec![],
            "an address outside is not a page"
        );
    }

    /// The end-to-end shape of the gap this closes: an absolute URL pasted from the address
    /// bar of a deployment whose public origin IS configured must be an edge, exactly as its
    /// relative spelling already is. This rebuilds the fixture with a configured origin
    /// rather than reusing `fixture_with_three_pages`, to prove the configuration actually
    /// reaches `replace_links` through `Store::publish_revision` — not merely through
    /// `wiki_path` called directly, which `an_absolute_url_at_the_configured_origin_is_internal`
    /// already covers on its own.
    #[tokio::test]
    async fn an_absolute_url_at_the_configured_origin_is_an_edge_too() {
        let store = Store::open("sqlite::memory:")
            .await
            .unwrap()
            .with_public_origin(Some(Url::parse("https://wiki.ohje.ooguy.com").unwrap()));
        let from = page(&store, "Von", Visibility::Public).await;
        let a = page(&store, "Ziel A", Visibility::Public).await;

        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/von",
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        store
            .publish_revision(
                &chef,
                &from,
                &body_linking_to_hrefs(&["https://wiki.ohje.ooguy.com/ziel-a"]),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            edges(&store).await,
            vec![(from, a)],
            "an absolute URL at the configured origin must be an edge"
        );
    }

    /// The other half of the safety property, end to end: with NO origin configured — the
    /// default `fixture_with_three_pages` store — the very same absolute URL that the test
    /// above records as an edge stays external, and publishing it does not panic.
    #[tokio::test]
    async fn without_a_configured_origin_the_same_absolute_url_stays_external() {
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(
                &chef,
                &from,
                &body_linking_to_hrefs(&["https://wiki.ohje.ooguy.com/ziel-a"]),
                None,
            )
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(edges(&store).await, vec![]);
    }

    #[tokio::test]
    async fn a_link_that_resolves_to_nothing_is_not_an_error() {
        // An unresolvable internal link is a fact about the body, not a reason to refuse
        // the publish — and neither is a `doc` id naming a document that has been deleted.
        //
        // "../ziel-a" is deliberately NOT one of these examples any more: `from` here is
        // `/von`, a root-level page, and `..` off a root-level page's directory now
        // resolves to `/ziel-a` — a page that exists — since bare relative references
        // resolve against `from` rather than being root-anchored (see
        // `which_addresses_name_a_page_in_this_wiki`). "../nirgendwo" keeps this test
        // exercising a relative reference while still resolving to nothing.
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        let mut body = body_linking_to_hrefs(&["/gibt-es-nicht", "../nirgendwo"]);
        body.content
            .extend(body_linking_to(&["nicht-mal-eine-id"]).content);

        store
            .publish_revision(&chef, &from, &body, None)
            .await
            .unwrap()
            .expect("an unresolvable link refused the whole publish");

        assert_eq!(edges(&store).await, vec![]);
    }

    #[tokio::test]
    async fn a_page_linking_to_itself_is_not_an_edge() {
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(&chef, &from, &body_linking_to_hrefs(&["/von"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(edges(&store).await, vec![]);
    }

    #[tokio::test]
    async fn creating_a_page_records_the_links_its_first_revision_carries() {
        // Creation publishes revision 1 through the same `append_revision`, so it extracts
        // links too. A page imported with links must be in the graph before anybody edits it.
        let store = store().await;
        let a = page(&store, "Ziel A", Visibility::Public).await;
        let from = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Von".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body_linking_to_hrefs(&["/ziel-a"]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();

        assert_eq!(edges(&store).await, vec![(from, a)]);
    }

    // --- and who may see them -----------------------------------------------------------

    #[tokio::test]
    async fn a_backlink_to_a_page_the_caller_cannot_read_is_not_listed() {
        // `leser` may read /ziel but NOT /geheim. /geheim links to /ziel.
        let store = store().await;
        let ziel = page(&store, "Ziel", Visibility::Public).await;
        let geheim = store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Geheim".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body: body_linking_to_hrefs(&["/ziel"]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();

        let leser = Principal::test("leser", &[], &[]);
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/geheim",
                Subject::Principal(chef.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();

        let back = store.backlinks_for(&leser, &ziel).await.unwrap();
        assert!(
            back.is_empty(),
            "a backlink revealed a page the caller cannot read: {back:?}"
        );

        // Anti-vacuity: chef DOES see it, so the fixture really contains the link.
        let seen = store.backlinks_for(&chef, &ziel).await.unwrap();
        assert_eq!(seen.len(), 1, "the fixture never had a backlink to hide");
        assert_eq!(seen[0].id, geheim);
        assert_eq!(seen[0].path, "/geheim");
        assert_eq!(seen[0].title, "Geheim");
    }

    #[tokio::test]
    async fn backlinks_are_refused_to_somebody_who_may_not_read_the_page_they_are_about() {
        // The other end of the same disclosure. Which pages point at a page is a fact about
        // that page, so it follows the page's own read — and the source being public does
        // not make it askable, or a restricted page's inbound links would be readable
        // through any public page that happened to mention it.
        let store = store().await;
        let ziel = page(&store, "Ziel", Visibility::Restricted).await;
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: "Quelle".into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body_linking_to_hrefs(&["/ziel"]),
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap();

        let leser = Principal::test("leser", &[], &[]);
        assert!(
            store.backlinks_for(&leser, &ziel).await.unwrap().is_empty(),
            "a page nobody may read answered questions about itself"
        );

        // Anti-vacuity, again: with a read on /ziel the same call answers.
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/ziel",
                Subject::Principal(chef.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();
        assert_eq!(store.backlinks_for(&chef, &ziel).await.unwrap().len(), 1);
    }

    // --- the graph ----------------------------------------------------------------------

    /// A page with `body`, an explicit slug, and a visibility.
    async fn page_with(
        store: &Store,
        title: &str,
        slug: &str,
        visibility: Visibility,
        body: Block,
    ) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: Some(slug.into()),
                    language: "de".into(),
                    visibility,
                    body,
                    sort_key: 0,
                },
                None,
            )
            .await
            .unwrap()
    }

    /// `/oeffentlich` -> `/geheim`, with `chef` the only one who may read the target.
    async fn graph_fixture() -> (Store, Principal, Principal) {
        let store = store().await;
        // The target first: an edge is only recorded to a document that already exists.
        page_with(
            &store,
            "Geheim",
            "geheim",
            Visibility::Restricted,
            body_linking_to(&[]),
        )
        .await;
        page_with(
            &store,
            "Oeffentlich",
            "oeffentlich",
            Visibility::Public,
            body_linking_to_hrefs(&["/geheim"]),
        )
        .await;

        let leser = Principal::test("leser", &[], &[]);
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/geheim",
                Subject::Principal(chef.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();

        (store, leser, chef)
    }

    #[tokio::test]
    async fn an_edge_needs_both_ends_readable() {
        // /oeffentlich -> /geheim, and `leser` may read only /oeffentlich.
        let (store, leser, chef) = graph_fixture().await;

        let g = store.graph_for(&leser, None).await.unwrap();
        assert!(
            g.edges.is_empty(),
            "an edge leaked a page the caller cannot read: {:?}",
            g.edges
        );
        assert!(
            !g.nodes.iter().any(|n| n.path == "/geheim"),
            "a node leaked an unreadable title: {:?}",
            g.nodes
        );

        // Anti-vacuity: chef sees exactly one edge, so the fixture really has one.
        let seen = store.graph_for(&chef, None).await.unwrap();
        assert_eq!(
            seen.edges.len(),
            1,
            "the fixture never had an edge to hide: {seen:?}"
        );
        assert!(seen.nodes.iter().any(|n| n.path == "/geheim"));
    }

    #[test]
    fn a_root_names_the_same_subtree_however_it_is_spelt() {
        for spelling in ["/darm", "darm", "/darm/", " /darm "] {
            assert_eq!(normalise_root(spelling), "/darm", "spelt as {spelling:?}");
        }
        // The whole wiki, which is what `None` already means.
        assert_eq!(normalise_root("/"), "/");
        assert_eq!(normalise_root(""), "/");
    }

    #[test]
    fn a_subtree_boundary_is_a_segment_and_not_a_prefix() {
        assert!(within_root(None, "/irgendwas"));
        assert!(within_root(Some("/"), "/irgendwas"));
        // The root is in its own subtree.
        assert!(within_root(Some("/darm"), "/darm"));
        assert!(within_root(Some("/darm"), "/darm/befunde"));
        // The prefix bug: a bare `starts_with` would pull this in.
        assert!(!within_root(Some("/darm"), "/darmspiegelung"));
        assert!(!within_root(Some("/darm"), "/anderes"));
    }

    #[tokio::test]
    async fn a_root_narrows_the_view_to_one_subtree() {
        // `/darm` -> `/darm/befunde`, and a second, unrelated pair outside it.
        let store = store().await;
        page_with(
            &store,
            "Befunde",
            "befunde",
            Visibility::Public,
            body_linking_to(&[]),
        )
        .await;
        page_with(
            &store,
            "Darm",
            "darm",
            Visibility::Public,
            body_linking_to_hrefs(&["/befunde"]),
        )
        .await;
        // Same first four letters, a different subtree. `/darm` must not collect it.
        page_with(
            &store,
            "Darmspiegelung",
            "darmspiegelung",
            Visibility::Public,
            body_linking_to_hrefs(&["/befunde"]),
        )
        .await;

        let leser = Principal::test("leser", &[], &[]);
        let whole = store.graph_for(&leser, None).await.unwrap();
        assert_eq!(whole.edges.len(), 2, "{whole:?}");

        // Nothing beneath `/darm`, and `/darm`'s own edge leaves it, so narrowing to it
        // keeps neither edge — the far end is outside the view.
        let narrowed = store.graph_for(&leser, Some("/darm")).await.unwrap();
        assert!(narrowed.edges.is_empty(), "{narrowed:?}");
        assert!(
            !narrowed.nodes.iter().any(|n| n.path == "/darmspiegelung"),
            "a prefix match pulled in a page from another subtree: {narrowed:?}"
        );
    }

    #[tokio::test]
    async fn a_root_that_names_nothing_answers_an_empty_graph() {
        // Not an error and not a distinction: "no such subtree" and "none of it is yours"
        // answer the same thing, or the answer itself would say which pages exist.
        let (store, leser, _chef) = graph_fixture().await;
        let g = store
            .graph_for(&leser, Some("/gibt-es-nicht"))
            .await
            .unwrap();
        assert!(g.nodes.is_empty() && g.edges.is_empty(), "{g:?}");
    }

    #[tokio::test]
    async fn a_node_no_surviving_edge_touches_is_not_drawn() {
        // `/oeffentlich` is readable, but its only edge points somewhere `leser` may not
        // go. A lone node with no edges is not a page anybody connected to anything, and
        // drawing it would say "this page links somewhere you cannot see".
        let (store, leser, _chef) = graph_fixture().await;
        let g = store.graph_for(&leser, None).await.unwrap();
        assert!(g.nodes.is_empty(), "{g:?}");
    }
}
