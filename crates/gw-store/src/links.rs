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
//! **An internal `href` IS rewritten into a `doc` reference when the page is published**,
//! and the paragraph this replaces said the opposite. It said that canonicalising would
//! edit what somebody wrote without being asked — and named what leaving it alone costs:
//! moving the target afterwards leaves the edge correct (it is stored by id) but the *link
//! in the body* stale, which is precisely the breakage D-5 exists to avoid. The owner chose
//! to pay the first cost rather than the second, so [`Store::resolve_references`] runs on
//! publish: an `href` naming a page the AUTHOR may read becomes `{doc: <id>}` and the
//! `href` is dropped, because a mark carrying both is a mark that can disagree with itself
//! the day the target moves.
//!
//! Two things it deliberately does not do. It does not run on IMPORT — `Store::create_
//! document` has no principal to authorise against, and the importer's job is fidelity to
//! the file. And it never resolves an `href` to a page the author cannot read: that would
//! be this crate answering a question about a page on behalf of somebody who may not ask
//! it, and the link is left exactly as it was written.
//!
//! **An unresolvable internal link is not an error.** A link to a page that does not exist,
//! or was deleted, is a fact about the body — the publish records no edge for it and
//! proceeds. That is why targets are resolved before they are inserted rather than being
//! inserted hopefully: SQLite's conflict resolution does *not* apply to FOREIGN KEY
//! constraints, so `INSERT OR IGNORE` of an edge to a document that is not there aborts the
//! statement, and with it the publish.

use crate::acl::Baseline;
use crate::{Store, StoredDocument};
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::{Block, Mark, MarkKind};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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

/// A reference resolved for one reader: where the target is **now** and what it is called
/// **now** (D-5).
///
/// Both fields are disclosures about the target page, so neither exists for a reader who
/// may not read it — [`Store::references_for`] simply has no entry for that id, and there
/// is no third state for a caller to get wrong. ADR 0019.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub path: String,
    pub title: String,
}

/// How many DISTINCT references one page has resolved, in either direction.
///
/// Nothing caps how many marks a body holds — it is JSON over the collaboration socket —
/// and every resolution is one authorisation through the single SQLite connection
/// `Store::open` configures (`max_connections(1)`, deliberately: every query in the
/// application is serialised through it). A page carrying a few thousand references would
/// therefore stall the whole deployment for every other reader, repeatedly, on demand.
///
/// **Over the cap the remainder are simply not resolved**, which is the state a forbidden
/// or absent target is already in — so the cap discloses nothing and needs no message. 256
/// is far past any page a person writes and far short of a lever.
pub const MAX_REFERENCES_PER_PAGE: usize = 256;

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
    // An embed is at least as strong a connection as a link — it puts another page's words
    // on this one — and it is invisible to the walk below, which reads MARKS: what an embed
    // names is a block ATTRIBUTE. So it is read here, by identity or by address, and inherits
    // `graph_for`'s both-ends filter for free.
    if body.kind == gw_core::BlockKind::Transclusion {
        let attr = |key: &str| body.attrs.get(key).and_then(|v| v.as_str());
        if let Some(doc) = attr("doc") {
            into.docs.insert(doc.to_string());
        } else if let Some(path) = attr("path").and_then(|p| wiki_path(p, from, public_origin)) {
            into.paths.insert(path);
        }
    }
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

/// Settle every freshly imported reference against the documents THIS database holds, and
/// say whether anything changed.
///
/// `gw_core::markdown` reads `[Titel](dok:<id> "/pfad")` into a mark carrying both the id and
/// the path the target had when the file was written ([`gw_core::Mark::FALLBACK_ATTR`]). That
/// pair is transient by design and this is where it ends: a mark reaching `documents.body`
/// carries an id or an address, never both, which is the invariant `gw_core::Mark` states and
/// `Mark::target_doc` depends on.
///
/// Two outcomes, and the order between them is the whole decision:
///
/// - **The id names a live document here.** The reference stands and the path is dropped. It
///   is dropped even when it disagrees — a page RENAMED after the export has a stale path in
///   the file and a perfectly good identity, and preferring the path would reintroduce
///   exactly the breakage D-5 exists to prevent.
/// - **The id names nothing here.** That is what re-seeding an export into a FRESH database
///   produces: `SeedMeta` has no `id` key, so every page is minted anew and no `dok:` in the
///   file can match. The reference becomes an ordinary `href` to the carried path, which
///   `replace_links` records as an edge as soon as the target exists and
///   [`Store::resolve_references`] exchanges for *this* database's id on the next publish. A
///   restored backup therefore keeps every connection whose target was restored too.
///
/// With no path carried — an older export, or one whose target the exporting account could
/// not read — an unknown id stays an unresolved reference and renders as the author's own
/// text, which is the state [`Store::references_for`] defines.
///
/// Takes a CONNECTION for [`replace_links`]' reason: it runs inside the caller's transaction,
/// on the body that is about to be stored, so a rollback takes it back with everything else.
/// It asks only whether a row exists — not who may read it — because it decides nothing about
/// what is shown: the reader's verdict is re-asked on every read by [`Store::references_for`],
/// and an author who may write this page may already name any id they like in it.
pub(crate) async fn settle_references(
    conn: &mut sqlx::SqliteConnection,
    body: &mut Block,
) -> Result<bool> {
    let mut changed = false;
    let mut settled: HashMap<String, bool> = HashMap::new();
    settle_into(conn, body, &mut settled, &mut changed).await?;
    Ok(changed)
}

/// The walk behind [`settle_references`]. Separate because recursion in an `async fn` needs a
/// boxed future, and `Box::pin` belongs at the one place that recurses.
fn settle_into<'a>(
    conn: &'a mut sqlx::SqliteConnection,
    body: &'a mut Block,
    settled: &'a mut HashMap<String, bool>,
    changed: &'a mut bool,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        for mark in &mut body.marks {
            let Some(id) = mark.target_doc().map(str::to_string) else {
                continue;
            };
            let fallback = mark.fallback_path().map(str::to_string);
            if fallback.is_none() {
                continue;
            }
            // Memoised per body: a page linking to one target forty times asks once.
            let live = match settled.get(&id) {
                Some(live) => *live,
                None => {
                    let live = document_with_id(&mut *conn, &id).await?.is_some();
                    settled.insert(id.clone(), live);
                    live
                }
            };
            *mark = if live {
                Mark::link_to_doc(&id)
            } else {
                Mark::link_to_url(&fallback.expect("checked above"))
            };
            *changed = true;
        }
        for child in &mut body.content {
            settle_into(&mut *conn, child, settled, changed).await?;
        }
        Ok(())
    })
}

/// Replace this document's edges. Takes a CONNECTION, not the pool, so it joins the/// Replace this document's edges. Takes a CONNECTION, not the pool, so it joins the
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

/// Every distinct document id a `doc` mark in this body names, in no particular order.
///
/// Separate from [`collect`] because the two ask different questions: that one is building
/// the graph and wants an `href` as well, this one is resolving references for a reader and
/// an `href` is already an address the reader can follow.
fn collect_docs(body: &Block, into: &mut BTreeSet<String>) {
    // An embed's target too, so that `references_for` answers for it as well. `gw_api::export`
    // is what needs that: it writes the target's CURRENT path beside the id as the fallback a
    // restored backup falls back to, and it takes every path it writes from this one
    // permission-checked resolver and from nowhere else.
    if body.kind == gw_core::BlockKind::Transclusion {
        if let Some(doc) = body.attrs.get("doc").and_then(|v| v.as_str()) {
            into.insert(doc.to_string());
        }
    }
    for mark in &body.marks {
        if let Some(doc) = mark.target_doc() {
            into.insert(doc.to_string());
        }
    }
    for child in &body.content {
        collect_docs(child, into);
    }
}

/// Every distinct wiki path an `href` in this body names, in no particular order.
fn collect_hrefs(body: &Block, into: &mut BTreeSet<String>, from: &str, origin: Option<&Url>) {
    for mark in &body.marks {
        // A mark already carrying a `doc` is already a reference — resolving its `href` too
        // would be resolving a mark that says two things, and `Mark` says it never does.
        if mark.target_doc().is_some() {
            continue;
        }
        if let Some(path) = internal_path(mark, from, origin) {
            into.insert(path);
        }
    }
    for child in &body.content {
        collect_hrefs(child, into, from, origin);
    }
}

/// Rewrite every `href` in this body that `resolved` has an id for into a `doc` reference.
fn apply_references(
    body: &mut Block,
    resolved: &HashMap<String, String>,
    from: &str,
    origin: Option<&Url>,
) {
    for mark in &mut body.marks {
        if mark.target_doc().is_some() {
            continue;
        }
        let Some(path) = internal_path(mark, from, origin) else {
            continue;
        };
        let Some(id) = resolved.get(&path) else {
            continue;
        };
        // Cleared rather than extended. A mark holding BOTH an address and a reference can
        // disagree with itself the moment the target moves, and `gw_core::Mark`'s own doc
        // comment says a link carries one or the other; `target_doc` reading an `href` as an
        // id is the mistake that invariant exists to prevent.
        mark.attrs.clear();
        mark.attrs
            .insert("doc".into(), serde_json::Value::String(id.clone()));
    }
    for child in &mut body.content {
        apply_references(child, resolved, from, origin);
    }
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

    /// What every `doc` reference in `body` points at, **for this caller** — and nothing
    /// about the ones it does not point at for them.
    ///
    /// D-5 says a link stores the target's id and that the title and path are resolved at
    /// render time. This is that resolution, and it is the whole of it: there is no other
    /// one, in this crate or above it.
    ///
    /// # What an unresolved reference renders as, and why it is one state and not four
    ///
    /// A target the caller **may not read**, one that **does not exist**, one that has been
    /// **thrown away** (`deleted_at` set — the Papierkorb) and one that was **purged** are
    /// all simply absent from the returned map, and a caller with no entry for an id renders
    /// **the reference's own text, unlinked: no address, no title, no tooltip, nothing**.
    ///
    /// The split is deliberate and it is the author/target line. *The words are the
    /// author's* — they are in the body, the author wrote them, the caller is already
    /// reading that body, and blanking them would corrupt a sentence to hide something the
    /// sentence does not contain. *The current path and the current title are the target's*,
    /// and they are exactly what `backlinks_for` and `graph_for` refuse to disclose: a path
    /// says a page exists and where, and a title says what it is about. Worse than either in
    /// isolation, they are **live** — a reference written when the reader could see the page
    /// would keep reporting that page's new name after a rename, so a reader who lost access
    /// would go on being told what it is called now. The verdict is therefore re-asked on
    /// every read and there is nothing to invalidate when a grant is revoked.
    ///
    /// The four cases answer identically because distinguishing them is itself the
    /// disclosure — "you may not see this" and "there is nothing here" differ only in
    /// confirming that something exists. It is the same closed conflation
    /// [`Store::document_for`] makes.
    ///
    /// **A reference to the host page itself resolves normally.** It is the one case that
    /// is not a disclosure at all: the reader is already reading that page, so its path and
    /// title are in front of them. The plan's first draft folded it in with the three above
    /// for uniformity; that was written before an internal `href` was resolved on publish,
    /// and with that in place it would mean an ordinary same-page link silently becoming
    /// unlinked text the first time its page was saved.
    ///
    /// # Why it lives here
    ///
    /// Beside `graph_for`, and for its reason. The hoisted `_with_baseline` accessors are
    /// `pub(crate)`, so a resolver written in `gw-api` would pay a fresh
    /// [`Store::baseline_for`] per reference — through the one SQLite connection every other
    /// request in the deployment also needs. Here the baseline is resolved once per page and
    /// the count is capped at [`MAX_REFERENCES_PER_PAGE`].
    pub async fn references_for(
        &self,
        principal: &Principal,
        body: &Block,
    ) -> Result<BTreeMap<String, Reference>> {
        let mut ids = BTreeSet::new();
        collect_docs(body, &mut ids);
        if ids.is_empty() {
            // Not merely an optimisation: `baseline_for` is a query, and a body with no
            // reference in it is every page on the site today.
            return Ok(BTreeMap::new());
        }
        // Once, for the whole page. See `graph_for`, which hoists it for the same reason.
        let baseline = self.baseline_for(principal).await?;
        let mut out = BTreeMap::new();
        for id in ids.into_iter().take(MAX_REFERENCES_PER_PAGE) {
            // THE permission-checked accessor, reached by id, with the baseline already in
            // hand. Not `document_path_unchecked` and not a batch `ids -> (path, title)`
            // query: the ids came out of a body this caller may read, and that says nothing
            // whatever about the pages they point at.
            let Some(doc) = self.readable_target(principal, &id, baseline).await? else {
                continue;
            };
            out.insert(
                id,
                Reference {
                    path: doc.path,
                    title: doc.title,
                },
            );
        }
        Ok(out)
    }

    /// The document `id` names, if `principal` may read it.
    ///
    /// THE permission-checked accessor, reached by an id, with the caller's baseline already
    /// in hand — not `document_path_unchecked`, and not a batch `ids -> (path, title)` query
    /// justified by "the ids came out of a body the caller may already read". They did, and
    /// that says nothing whatever about the pages they point at.
    ///
    /// One line, in a function of its own, deliberately: `scripts/mutate.sh` replaces exactly
    /// this call with the unchecked row lookup underneath it, and a mutation that can be
    /// written as one line is one that cannot quietly miss half of what it was aimed at.
    async fn readable_target(
        &self,
        principal: &Principal,
        id: &str,
        baseline: Baseline,
    ) -> Result<Option<StoredDocument>> {
        self.document_for_id_with_baseline(principal, id, Action::Read, baseline)
            .await
    }

    /// Turn every internal `href` in `body` into a `doc` reference, for the pages `author`
    /// may read. `Ok(true)` when something changed.
    ///
    /// D-5's write half: a link that names a page by path breaks the day the page moves, so
    /// the path is exchanged for the page's identity at the one moment the system knows both
    /// — when somebody publishes. The `href` is dropped in the same breath; see
    /// [`apply_references`].
    ///
    /// **This is the only resolver.** Not the editor: a client that turned a typed path into
    /// an id would be a second answer to a permission question, written where the answer
    /// cannot be trusted, and it would have to be kept in step with this one for ever.
    ///
    /// # Why it is not inside the publish transaction, although its EFFECT is
    ///
    /// The rewrite belongs with `replace_links` — both derive from the body, both must
    /// describe the revision that is actually stored — and `reconcile_tasks` does exactly
    /// that, mutating the tree on the transaction's own connection. This cannot: authorising
    /// a path needs [`Store::document_for`], which goes to the **pool**, and `Store::open`
    /// gives the pool ONE connection (`max_connections(1)`). Asking it for a second one
    /// while a transaction is holding the first waits until it times out — the deadlock
    /// `replace_links`' own doc comment warns about.
    ///
    /// So [`Store::publish_revision`] calls this *before* it opens the transaction and hands
    /// `append_revision` the rewritten body. Nothing is half-done by that: the body that is
    /// stored, the body `replace_links` reads its edges out of and the body a reader gets
    /// are one body, and a rollback discards all of it. What is genuinely outside the
    /// transaction is only the permission *verdicts*, which are the author's own and were
    /// true a moment earlier — a grant revoked in that window resolves one link the author
    /// could have resolved by publishing a moment sooner, and the READER's verdict, which is
    /// the one that discloses anything, is re-asked on every read by [`Store::references_for`].
    pub(crate) async fn resolve_references(
        &self,
        author: &Principal,
        from_path: &str,
        body: &mut Block,
    ) -> Result<bool> {
        let mut paths = BTreeSet::new();
        collect_hrefs(body, &mut paths, from_path, self.public_origin.as_ref());
        if paths.is_empty() {
            return Ok(false);
        }
        let baseline = self.baseline_for(author).await?;
        let mut resolved: HashMap<String, String> = HashMap::new();
        for path in paths.into_iter().take(MAX_REFERENCES_PER_PAGE) {
            // Read, not Write. Being able to point at a page is being able to find it, which
            // is what reading it already licenses; requiring write would mean an author could
            // only make durable links to pages they may edit.
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
        apply_references(body, &resolved, from_path, self.public_origin.as_ref());
        Ok(true)
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
                    topics: Vec::new(),
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
                    topics: Vec::new(),
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
                    topics: Vec::new(),
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
                    topics: Vec::new(),
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
                    topics: Vec::new(),
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
                    topics: Vec::new(),
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
                    topics: Vec::new(),
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

    // --- references: resolved by identity, filtered by the reader -----------------------

    /// Every link mark in `body`, outermost first, as `(href, doc)`.
    fn link_marks(body: &Block) -> Vec<(Option<String>, Option<String>)> {
        let mut out = Vec::new();
        fn walk(b: &Block, out: &mut Vec<(Option<String>, Option<String>)>) {
            for m in &b.marks {
                if m.kind == gw_core::MarkKind::Link {
                    out.push((
                        m.attrs
                            .get("href")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        m.target_doc().map(String::from),
                    ));
                }
            }
            for c in &b.content {
                walk(c, out);
            }
        }
        walk(body, &mut out);
        out
    }

    /// The body `path` currently holds, as a tree.
    async fn stored_body(store: &Store, path: &str) -> Block {
        let doc = store
            .document_by_path_unchecked(path)
            .await
            .unwrap()
            .unwrap();
        serde_json::from_str(&doc.body).unwrap()
    }

    #[tokio::test]
    async fn publishing_exchanges_an_internal_href_for_the_targets_identity() {
        // D-5's write half. The author typed a path — into the link dialog's free field, or
        // in markdown that was pasted — and what is STORED is the target's id, so that
        // moving the target afterwards cannot break the link.
        let (store, chef, from, a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(&chef, &from, &body_linking_to_hrefs(&["/ziel-a"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        let marks = link_marks(&stored_body(&store, "/von").await);
        assert_eq!(
            marks,
            vec![(None, Some(a.clone()))],
            "the href was not exchanged for the target's id, or it was kept beside it"
        );

        // And the edge is still exactly one — the same page reached by a different spelling
        // must not become two links.
        assert_eq!(edges(&store).await, vec![(from.clone(), a.clone())]);
    }

    #[tokio::test]
    async fn a_resolved_reference_follows_the_page_when_it_is_renamed_and_moved() {
        // The whole point of D-5, and the one thing a stored path cannot do. The body is
        // untouched between the two reads below; only the target moved.
        let (store, chef, from, a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(&chef, &from, &body_linking_to_hrefs(&["/ziel-a"]), None)
            .await
            .unwrap()
            .unwrap();
        let body = stored_body(&store, "/von").await;

        let before = store.references_for(&chef, &body).await.unwrap();
        assert_eq!(before[&a].path, "/ziel-a");
        assert_eq!(before[&a].title, "Ziel A");

        // What a move and a rename do, and there is no other kind: the row changes, the
        // bodies pointing at it do not.
        sqlx::query("UPDATE documents SET path = ?1, slug = ?2, title = ?3 WHERE id = ?4")
            .bind("/archiv/befunde")
            .bind("befunde")
            .bind("Befunde 2024")
            .bind(&a)
            .execute(&store.pool)
            .await
            .unwrap();

        let after = store.references_for(&chef, &body).await.unwrap();
        assert_eq!(
            after[&a].path, "/archiv/befunde",
            "the reference did not follow the page to its new address"
        );
        assert_eq!(
            after[&a].title, "Befunde 2024",
            "the reference did not follow the page to its new name"
        );
        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            vec![(None, Some(a))],
            "nothing rewrote the linking page, which is the point of storing identity"
        );
    }

    #[tokio::test]
    async fn a_reference_to_a_page_the_reader_may_not_see_resolves_to_nothing_at_all() {
        // THE disclosure property, and the anti-vacuity half is the second assertion: a
        // reader who MAY see the target gets its current path and title, so the fixture
        // really contains a reference for the first reader to be refused.
        //
        // `backlinks_for` and `graph_for` refuse exactly these two values about a page the
        // caller cannot read, and a reference must not be the way round them — a batch
        // `ids -> (path, title)` query, justified by "the ids came out of a body the caller
        // may already read", would hand `leser` the restricted page's address and name.
        let store = store().await;
        let geheim = page(&store, "Geheim", Visibility::Restricted).await;
        let von = page(&store, "Von", Visibility::Public).await;

        let leser = Principal::test("leser", &[], &[]);
        let chef = Principal::test("chef", &[], &[]);
        for perm in [Permission::Read, Permission::Write] {
            store
                .add_grant("/von", Subject::Principal(chef.id.clone()), perm)
                .await
                .unwrap();
        }
        store
            .add_grant(
                "/geheim",
                Subject::Principal(chef.id.clone()),
                Permission::Read,
            )
            .await
            .unwrap();

        let body = body_linking_to(&[&geheim]);
        store
            .publish_revision(&chef, &von, &body, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        assert!(
            store
                .references_for(&leser, &body)
                .await
                .unwrap()
                .is_empty(),
            "a reference disclosed a page the reader may not read"
        );

        let seen = store.references_for(&chef, &body).await.unwrap();
        assert_eq!(seen.len(), 1, "the fixture never had a reference to hide");
        assert_eq!(seen[&geheim].path, "/geheim");
        assert_eq!(seen[&geheim].title, "Geheim");
    }

    #[tokio::test]
    async fn a_reference_to_a_page_that_is_gone_resolves_to_nothing_at_all() {
        // Thrown away, purged and never-existed answer identically, and identically to
        // "not for you" — distinguishing them is itself the disclosure.
        let store = store().await;
        let weg = page(&store, "Weg", Visibility::Public).await;
        let chef = Principal::test("chef", &[], &[]);
        store
            .add_grant(
                "/weg",
                Subject::Principal(chef.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        let body = body_linking_to(&[&weg, "0199c0de-0000-7000-8000-00000000dead"]);
        let before = store.references_for(&chef, &body).await.unwrap();
        assert_eq!(
            before.len(),
            1,
            "an id naming no document must resolve to nothing, and a live one must resolve"
        );

        store.trash_document(&chef, "/weg").await.unwrap();
        assert!(
            store.references_for(&chef, &body).await.unwrap().is_empty(),
            "a page in the Papierkorb still answered with its address and its name"
        );
    }

    #[tokio::test]
    async fn a_reference_to_the_page_it_is_written_on_still_resolves() {
        // The one case that is not a disclosure: the reader is already reading that page.
        // Folding it in with the three above would mean an ordinary same-page link becoming
        // unlinked text the first time its own page was saved, because publishing exchanges
        // that href for this page's id.
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        store
            .publish_revision(&chef, &from, &body_linking_to_hrefs(&["/von"]), None)
            .await
            .unwrap()
            .unwrap();
        let body = stored_body(&store, "/von").await;
        assert_eq!(link_marks(&body), vec![(None, Some(from.clone()))]);

        let seen = store.references_for(&chef, &body).await.unwrap();
        assert_eq!(seen[&from].path, "/von");

        // …and it is still not an EDGE. A self-loop is not a connection somebody drew.
        assert!(edges(&store).await.is_empty());
    }

    #[tokio::test]
    async fn an_href_to_a_page_the_author_may_not_read_is_left_exactly_as_written() {
        // Resolution is the author's own question, asked with the author's own rights. An
        // unfiltered one would let anybody with write on one page turn `/darm/befund-mueller`
        // into a yes/no answer about whether that page exists — the existence oracle D-21d
        // refuses for the picker, arrived at through the publish path instead.
        let store = store().await;
        let _geheim = page(&store, "Geheim", Visibility::Restricted).await;
        let von = page(&store, "Von", Visibility::Public).await;
        let autor = Principal::test("autor", &[], &[]);
        store
            .add_grant(
                "/von",
                Subject::Principal(autor.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();

        store
            .publish_revision(&autor, &von, &body_linking_to_hrefs(&["/geheim"]), None)
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            vec![(Some("/geheim".into()), None)],
            "an href naming a page the author cannot read was resolved anyway"
        );
    }

    #[tokio::test]
    async fn an_external_link_is_never_turned_into_a_reference() {
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        let hrefs = [
            "https://example.org/x",
            "mailto:a@example.org",
            "/gibt-es-nicht",
        ];
        store
            .publish_revision(&chef, &from, &body_linking_to_hrefs(&hrefs), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            hrefs
                .iter()
                .map(|h| (Some((*h).to_string()), None))
                .collect::<Vec<_>>(),
            "something other than a link to a readable page of this wiki was rewritten"
        );
    }

    #[tokio::test]
    async fn a_page_carrying_more_references_than_the_cap_resolves_the_cap_and_no_more() {
        // Every resolution is one authorisation through the ONE SQLite connection the whole
        // application shares. Nothing caps how many marks a body holds, so without this a
        // single attacker-controlled page fetched repeatedly is a lever on the availability
        // of the entire deployment. Over the cap a reference is in the same state a
        // forbidden one is in, so the cap discloses nothing.
        let store = store().await;
        let chef = Principal::test("chef", &[], &[]);
        let mut ids = Vec::new();
        for i in 0..(MAX_REFERENCES_PER_PAGE + 5) {
            ids.push(page(&store, &format!("Ziel {i:03}"), Visibility::Public).await);
        }
        let body = body_linking_to(&ids.iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(
            store.references_for(&chef, &body).await.unwrap().len(),
            MAX_REFERENCES_PER_PAGE
        );
    }

    #[tokio::test]
    async fn an_imported_reference_whose_id_is_live_here_ignores_the_path_it_carried() {
        // The rule that makes identity worth storing at all, and the one an implementation
        // gets backwards by being helpful: the path in a file is what the target was called
        // WHEN THE FILE WAS WRITTEN. A page renamed or moved since then has a stale path and a
        // perfectly good identity, and preferring the path would reintroduce exactly the
        // breakage D-5 exists to prevent — quietly, on a restore, which is the worst moment.
        let (store, chef, from, a, _b) = fixture_with_three_pages().await;

        // What a move and a rename do. The file being imported below still says `/ziel-a`.
        sqlx::query("UPDATE documents SET path = ?1, slug = ?2, title = ?3 WHERE id = ?4")
            .bind("/archiv/befunde")
            .bind("befunde")
            .bind("Befunde 2024")
            .bind(&a)
            .execute(&store.pool)
            .await
            .unwrap();

        let body = linking_body(vec![Mark::link_to_doc_at(&a, Some("/ziel-a"))]);
        store
            .publish_revision(&chef, &from, &body, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            vec![(None, Some(a.clone()))],
            "the stale path won against a live id, or was kept beside it"
        );
        let stored = stored_body(&store, "/von").await;
        assert_eq!(
            store.references_for(&chef, &stored).await.unwrap()[&a].path,
            "/archiv/befunde",
            "the reference did not follow the page to where it actually is"
        );
    }

    #[tokio::test]
    async fn an_imported_reference_whose_id_means_nothing_here_falls_back_to_its_path() {
        // What a re-seed into a fresh database produces: every id minted anew, so no `dok:`
        // in the file can match. Without the fallback this page would keep its words and lose
        // its link; with it, the reference becomes the address the file carried — and the
        // publish that stores it records the edge and, on the next publish, exchanges the
        // address for THIS database's id.
        let (store, chef, from, a, _b) = fixture_with_three_pages().await;
        let fremd = "0199c0de-0000-7000-8000-00000000dead";

        let body = linking_body(vec![Mark::link_to_doc_at(fremd, Some("/ziel-a"))]);
        store
            .publish_revision(&chef, &from, &body, None)
            .await
            .unwrap()
            .expect("the publish was refused");

        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            vec![(Some("/ziel-a".into()), None)],
            "a dead id was stored as a reference instead of falling back to its path"
        );
        assert_eq!(edges(&store).await, vec![(from.clone(), a.clone())]);

        // Publishing again — which is what an author does next — makes it an identity here.
        let stored = stored_body(&store, "/von").await;
        store
            .publish_revision(&chef, &from, &stored, None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            vec![(None, Some(a))]
        );
    }

    #[tokio::test]
    async fn a_reference_with_no_path_to_fall_back_to_stays_exactly_as_it_arrived() {
        // An older export, or one whose target the exporting account could not read, carries
        // no path — and inventing one is the guess this crate does not make. The reference
        // stays unresolved, which renders as the author's own text.
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        let fremd = "0199c0de-0000-7000-8000-00000000dead";
        store
            .publish_revision(&chef, &from, &body_linking_to(&[fremd]), None)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            link_marks(&stored_body(&store, "/von").await),
            vec![(None, Some(fremd.to_string()))]
        );
        assert!(store
            .references_for(&chef, &stored_body(&store, "/von").await)
            .await
            .unwrap()
            .is_empty());
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
                    topics: Vec::new(),
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
