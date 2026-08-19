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
use std::collections::BTreeSet;

/// One page that links to the page being read.
#[derive(Debug, Clone, Serialize)]
pub struct Backlink {
    pub id: String,
    pub path: String,
    pub title: String,
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
fn collect(body: &Block, into: &mut Targets) {
    for mark in &body.marks {
        // A link carries EITHER `doc` or `href`, never both — `gw_core::Mark` says so — and
        // `else` rather than a second `if` keeps that true here even if one ever did.
        if let Some(doc) = mark.target_doc() {
            into.docs.insert(doc.to_string());
        } else if let Some(path) = internal_path(mark) {
            into.paths.insert(path);
        }
    }
    for child in &body.content {
        collect(child, into);
    }
}

/// The wiki path a link mark's `href` names, if it names one at all.
fn internal_path(mark: &Mark) -> Option<String> {
    if mark.kind != MarkKind::Link {
        return None;
    }
    wiki_path(mark.attrs.get("href")?.as_str()?)
}

/// The document path an address names, or `None` if it does not name one in this wiki.
///
/// **Internal is "a relative reference with no scheme and no authority".** `web/src/routes`
/// serves documents from `/[...path]`, so a page's `documents.path` *is* its URL path and
/// `/rundgang/tabellen` needs no translation. Anything carrying a scheme (`https:`,
/// `mailto:`, `javascript:`) or an authority (`//example.org/x`) addresses some other
/// origin, and this crate has no idea which origin is its own — the deployment host is
/// configuration, not schema — so an absolute `https://…` URL is treated as external even
/// when it happens to point back here. Guessing otherwise would mean inventing a hostname
/// and drawing edges from it.
///
/// Everything else is root-anchored: `ziel/a` and `/ziel/a` are the same page, because a
/// document path always begins at the root and a wiki has no working directory. A query or
/// a fragment is addressing part of a page rather than a different one, so `?x=1` and
/// `#abschnitt` are trimmed — and an address that is *only* a fragment names the page it is
/// already on, which is not an edge.
///
/// Nothing here resolves `.` or `..`: they are relative to a base this function is not
/// given, and root-anchoring them produces a path that `slugify` can never have made, so
/// they resolve to no document and record no edge. That is the same outcome as any other
/// link into nothing, and it is deliberately not a special case.
fn wiki_path(href: &str) -> Option<String> {
    let href = href.trim();
    if has_scheme(href) || href.starts_with("//") {
        return None;
    }
    // `?` and `#` both end the path, in whichever order they turn up.
    let path = href.split(['?', '#']).next().unwrap_or_default();
    // A trailing slash is how a browser spells the same page, never a different one.
    let path = path.trim_end_matches('/');
    if path.is_empty() {
        return None;
    }
    Some(format!("/{}", path.trim_start_matches('/')))
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
pub(crate) async fn replace_links(
    conn: &mut sqlx::SqliteConnection,
    from_doc: &str,
    body: &Block,
) -> Result<()> {
    let mut found = Targets::default();
    collect(body, &mut found);

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
        edges.extend(document_with_id(&mut *conn, id).await?);
    }
    for path in &found.paths {
        edges.extend(document_at(&mut *conn, path).await?);
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
        // was never looked up.
        for (href, expected) in [
            ("/ziel-a", Some("/ziel-a")),
            // Root-anchored: a document path starts at the root and a wiki has no working
            // directory, so these are the same page.
            ("ziel-a", Some("/ziel-a")),
            ("/ziel/a/", Some("/ziel/a")),
            ("/ziel-a?von=hier", Some("/ziel-a")),
            ("/ziel-a#abschnitt", Some("/ziel-a")),
            ("  /ziel-a  ", Some("/ziel-a")),
            // A colon AFTER a separator is part of the path, not a scheme.
            ("/ziel:a", Some("/ziel:a")),
            // A colon before one is a scheme, even when it looks like a slug. That is what
            // a browser does with it too, which is the only reading that matches the link.
            ("ziel:a", None),
            ("https://example.org/ziel-a", None),
            ("HTTPS://example.org/ziel-a", None),
            ("mailto:jemand@example.org", None),
            ("javascript:alert(1)", None),
            // An authority, so some other origin — even though the scheme is missing.
            ("//example.org/ziel-a", None),
            // Part of a page rather than a different page.
            ("#abschnitt", None),
            ("?von=hier", None),
            ("/", None),
            ("", None),
            ("   ", None),
        ] {
            assert_eq!(
                wiki_path(href).as_deref(),
                expected,
                "the address `{href}` was read wrongly"
            );
        }
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

    #[tokio::test]
    async fn a_link_that_resolves_to_nothing_is_not_an_error() {
        // An unresolvable internal link is a fact about the body, not a reason to refuse
        // the publish — and neither is a `doc` id naming a document that has been deleted.
        let (store, chef, from, _a, _b) = fixture_with_three_pages().await;
        let mut body = body_linking_to_hrefs(&["/gibt-es-nicht", "../ziel-a"]);
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
}
