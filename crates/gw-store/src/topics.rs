//! Topics: the subjects a page is about, the tree they form, and browsing by them.
//!
//! D-4 decided that topics are **not** nodes in the graph — nodes are pages, edges are
//! links somebody deliberately wrote — and stated the consequence as a requirement rather
//! than a nicety: *"topics are invisible in the graph, so browsing by topic needs its own
//! view — a topic page listing its documents. That is the ONLY way topics are reachable."*
//! This module is that view, and the write path that fills it.
//!
//! # The three decisions this implements
//!
//! 1. **Topics live in frontmatter and in a control.** A page states its topics in its
//!    YAML, so they survive import and export, and they are editable through the API
//!    without rewriting the file. The store is the source of truth (AGENTS.md rule 1); the
//!    frontmatter is how they travel, exactly as `title` and `visibility` already do.
//! 2. **Free text, but suggested.** There is no managed list and no pre-creation step:
//!    typing a topic creates it. [`Store::topics_for`] is what lets an interface offer the
//!    ones that already exist, so people reuse rather than re-invent.
//! 3. **Nested.** `Medizin/Darm` is the topic `Darm` inside the topic `Medizin`, and the
//!    tree is the topics' own — independent of where the pages live.
//!
//! # Listing a topic means that topic AND everything inside it
//!
//! Every listing query here has to answer this and the two answers are both defensible, so
//! the reasoning is written down rather than left as an accident somebody trips over in six
//! months. **Descendants are included, always, and it is not an option.**
//!
//! Three things decided it:
//!
//! * **A topic page is the only way in (D-4).** A `Medizin` that showed two documents while
//!   forty sat under `Medizin/Darm` and `Medizin/Leber` would be a browsing dead end, and
//!   the person looking at it has no other route to those forty — no graph edge, no
//!   backlink. Exact-match would make a parent topic *less* useful the more carefully
//!   people filed things under it, which is precisely backwards.
//! * **Somebody who wrote `Medizin/Darm` did say the page is about Medizin.** That is what
//!   nesting means. Reading it as "about Darm, and not about Medizin" contradicts the
//!   syntax they used.
//! * **It is the answer this codebase already gives to "a root names a subtree".**
//!   [`Store::graph_for`]'s `root`, [`Store::board_for`]'s project home and
//!   `Store::tree_for` are all inclusive of the root and of everything below it. A fourth
//!   subtree question with a different answer is how two answers start.
//!
//! Both were not offered, and that was deliberate too: with a flag, every caller — and
//! every caller written later — picks, and two callers that pick differently disagree about
//! what "the Medizin topic" holds. If an exact listing is ever genuinely wanted, the honest
//! shape is a second, differently *named* method, not a boolean on this one.
//!
//! Switch back if topics ever stop being a tree of subjects and start being a tree of
//! *collections* — a topic meaning "these exact pages, and the ones under me are a separate
//! set". Nothing suggests that today.
//!
//! # A topic is a disclosure surface, and its NAME is half of it
//!
//! The design's Security section says every aggregate view here must filter **per document**
//! through the permission-checked accessor, never once per subtree. That much is the same
//! rule [`Store::graph_for`] and [`Store::board_for`] follow and it is followed here.
//!
//! Topics add a second leak that a board does not have: **a topic's existence and its name
//! say something on their own.** A topic called `Kündigung Mietvertrag`, carried only by
//! pages nobody but its author may read, tells a reader that such a page exists and roughly
//! what it says — even with an empty list of documents under it. A backlinks panel cannot
//! do that, because it has no names of its own; every string it could print belongs to a
//! page it has already checked.
//!
//! So the rule here is one sentence: **a topic exists, for a given caller, exactly when that
//! caller may read at least one document filed under it or under a topic inside it.** A
//! topic they can see no document of is not listed, is not counted, is not offered as a
//! suggestion, and answers `None` when asked about by name — the same answer as a topic
//! nobody ever typed. `docs/decisions/0011-what-a-topic-discloses.md` records why, what the
//! residual channel is, and what would make this worth revisiting.
//!
//! That rule is also what the pruning below is for. A topic no page carries is deleted, so
//! "this topic exists" and "this topic has at least one document" are the same statement
//! about the table as well as about a caller — which is what stops an invisible, permanently
//! unreachable row from accumulating a name nobody chose to keep.

use crate::Store;
use anyhow::Result;
use gw_auth::{Action, Principal};
use gw_core::slugify;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// The longest a single topic name may be, in characters.
///
/// A limit rather than none, because a topic name is the one string in this system that one
/// person writes and **everybody who shares any page with them has to look at**: it is
/// rendered in the index, in every listing, and beside every page carrying it. A page body
/// is long only for its own readers; a 100 kB topic name would wreck the index for all of
/// them. 100 is generous for a subject and short enough to render in a chip.
pub const MAX_TOPIC_NAME_CHARS: usize = 100;

/// How deeply topics may nest — `Medizin/Darm/Labor/Werte/…`.
///
/// Same argument, one dimension along: the path is a key in a UNIQUE column and is walked
/// segment by segment on every write. Eight is far past anything a person files by hand and
/// bounds the walk.
pub const MAX_TOPIC_DEPTH: usize = 8;

/// The character that separates a topic from the topic inside it.
///
/// `/`, not the `›` the design writes in prose. `›` is a *breadcrumb* — a rendering
/// decision, and the interface's to make — whereas this is a key: it goes in frontmatter,
/// in a URL and in a UNIQUE column, it is on every keyboard, and it is already what
/// `documents.path` uses for exactly the same job. A name may therefore not contain it; see
/// [`parse_stated`].
pub const TOPIC_SEPARATOR: char = '/';

/// One topic, by the two names it has.
///
/// Carries no id, deliberately, for the reason [`crate::Backlink`] and
/// [`crate::links::GraphNode`]'s wire types drop theirs: `path` identifies a topic just as
/// uniquely, is already the thing a URL and a frontmatter line have to spell, and an
/// internal uuid on the wire is one more identifier for a client to keep a table of.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Topic {
    /// The canonical key: `/medizin/darm`. Slugs, so `Medizin` and `medizin` are one topic.
    pub path: String,
    /// The leaf as somebody typed it: `Darm`.
    pub name: String,
    /// The whole ancestry as somebody typed it: `Medizin/Darm`.
    ///
    /// **This is the string frontmatter states and an export writes back**, which is why it
    /// is a stored-and-derived field rather than something a caller assembles: the file, the
    /// API and the listing all have to spell a topic the same way, and three assemblers of
    /// one string are three chances to disagree.
    pub display_path: String,
}

/// A topic in a listing, with how many documents are under it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicSummary {
    pub topic: Topic,
    /// **Documents the caller may read**, in this topic and in every topic inside it.
    ///
    /// It counts what survived the filter, never what the filter removed — it is the length
    /// of the very list [`Store::topic_for`] would hand the same caller, computed from the
    /// same filtered set rather than beside it. That is the distinction ADR 0010 draws for
    /// `may_write`: a number *about the omitted rows* is a disclosure and is forbidden here
    /// as it is on a board; a number about the rows being shown is the list's own length.
    pub documents: usize,
}

/// One page filed under a topic. Every field comes from a document the caller may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicDocument {
    pub path: String,
    pub title: String,
}

/// A topic page: the topic, the documents under it, and the topics inside it.
///
/// `children` is here rather than behind a second request because D-4 leaves this listing as
/// the only way topics are reachable at all — a topic page that could not offer the topics
/// inside it would be a dead end for exactly the nesting decision 3 asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicListing {
    pub topic: Topic,
    /// Every document the caller may read under this topic **or any topic inside it**, by
    /// path. See the module header for why descendants are included.
    pub documents: Vec<TopicDocument>,
    /// The topics directly inside this one that the caller may see, same rule as the index.
    pub children: Vec<TopicSummary>,
}

/// The answer to setting a page's topics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopicOutcome {
    /// The page's topics are now exactly these, in canonical order.
    Done(Vec<Topic>),
    /// No such page, or the caller may not Write it — one answer for both, exactly as
    /// [`Store::document_for`] returns `None` for "absent" and "not permitted" alike.
    Refused,
    /// Something stated is not usable as a topic. The message names it and says why, so a
    /// typo is visible as a typo rather than as a page that quietly lost a topic.
    Rejected(String),
}

/// One segment of a topic path: what it is called, and the key it reduces to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Segment {
    name: String,
    slug: String,
}

/// Split a stated topic — `Medizin/Darm` — into its segments, or say why it is not one.
///
/// **`/` is structural and a name may not contain it.** `Vor/Nachteile` therefore *is* the
/// topic `Nachteile` inside `Vor`; there is no escaping and no second separator. That is not
/// a limitation being papered over, it is what makes the round trip safe: a name holding a
/// separator would export as one string and re-import as two topics, and
/// `gw_api::export::render_file` compares the strings, so it would not notice.
///
/// Whitespace runs collapse to one space and the ends are trimmed, so `Medizin  Darm` and
/// `Medizin Darm` are one topic with one spelling rather than two that look identical in a
/// list. A control character is refused outright — it would put a newline in a YAML scalar
/// and in every listing that renders the name.
fn parse_stated(stated: &str) -> Result<Vec<Segment>, String> {
    let mut out = Vec::new();
    for raw in stated.split(TOPIC_SEPARATOR) {
        let name = collapse_whitespace(raw);
        if name.is_empty() {
            return Err(format!(
                "`{stated}` is not a topic: `{TOPIC_SEPARATOR}` separates a topic from the \
                 one inside it, so every part of it has to be something"
            ));
        }
        if name.chars().any(char::is_control) {
            return Err(format!(
                "`{stated}` is not a topic: a topic name cannot contain a control character"
            ));
        }
        if name.chars().count() > MAX_TOPIC_NAME_CHARS {
            return Err(format!(
                "`{name}` is too long for a topic name ({MAX_TOPIC_NAME_CHARS} characters at \
                 most) — everybody who shares a page with you has to read it in a list"
            ));
        }
        let slug = slugify(&name);
        if slug.is_empty() {
            return Err(format!(
                "`{name}` cannot be a topic: nothing in it survives being turned into a key, \
                 so there would be no way to tell it from any other such name"
            ));
        }
        out.push(Segment { name, slug });
    }
    if out.len() > MAX_TOPIC_DEPTH {
        return Err(format!(
            "`{stated}` nests {} deep and {MAX_TOPIC_DEPTH} is the limit",
            out.len()
        ));
    }
    Ok(out)
}

/// The canonical path a stated topic reduces to — `Medizin/Darm` → `/medizin/darm` — or
/// the reason it is not a topic.
///
/// Public and pure: no database, no principal, no decision about anybody's access. It
/// exists because the importer has to answer "does this file still say what the wiki holds"
/// **without writing anything**, and comparing the strings a file states against the
/// spellings the store holds would call `darm` a change to `Darm`. Re-deriving the rule in
/// `gw_api::seed` instead would be a second canonicalisation, and the day the two disagree
/// every import rewrites every page's topics on every run.
pub fn canonical_topic(stated: &str) -> Result<String, String> {
    Ok(canonical_path(&parse_stated(stated)?))
}

/// Trim, and collapse every run of whitespace to a single space.
fn collapse_whitespace(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The canonical path a run of segments names: `/medizin/darm`.
fn canonical_path(segments: &[Segment]) -> String {
    let mut path = String::new();
    for segment in segments {
        path.push(TOPIC_SEPARATOR);
        path.push_str(&segment.slug);
    }
    path
}

/// Every ancestor path of `path`, longest first, excluding `path` itself.
///
/// `/medizin/darm/labor` → `/medizin/darm`, `/medizin`. The schema guarantees each of these
/// is itself a row (`tags_parent_must_be_one_segment_up_insert`), so a walk up a path never
/// has to ask whether the step above exists.
fn ancestors(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut here = path;
    while let Some(cut) = here.rfind(TOPIC_SEPARATOR) {
        if cut == 0 {
            break;
        }
        here = &here[..cut];
        out.push(here.to_string());
    }
    out
}

/// Whether `path` is `root` or is inside it, on a SEGMENT boundary.
///
/// `/medizin-recht` is not inside `/medizin`; a bare prefix match would put its documents in
/// somebody else's listing. Same shape, and the same reason, as `within` in
/// [`crate::tasks`].
fn within(root: &str, path: &str) -> bool {
    path == root || path.starts_with(&format!("{root}{TOPIC_SEPARATOR}"))
}

/// Every topic in the wiki, by canonical path, with the name of its leaf.
///
/// Unfiltered on purpose and crate-private: it is names and keys with no documents attached,
/// and every caller below turns it into an answer only after the documents under a topic
/// have been through the accessor. Nothing here reaches a caller by itself.
type TopicNames = BTreeMap<String, String>;

/// The display path of `path`, assembled from the names of it and its ancestors.
///
/// `None` when any step is missing, which the schema makes impossible and which is therefore
/// treated as "not a topic" rather than papered over with the slug: a display path with a
/// slug in the middle of it would be a spelling nobody typed, appearing in a file.
fn display_path(names: &TopicNames, path: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut walk = vec![path.to_string()];
    walk.extend(ancestors(path));
    for step in walk.iter().rev() {
        parts.push(names.get(step)?.as_str());
    }
    Some(parts.join(&TOPIC_SEPARATOR.to_string()))
}

impl Store {
    /// Every topic the caller may see, by canonical path.
    ///
    /// This is both the index — the page that answers "which topics exist" — and the
    /// suggestion source that makes decision 2 work: free text, but offered, so people reuse
    /// a topic rather than re-inventing it three ways.
    ///
    /// **A topic the caller can see no document of is absent entirely**, not listed with a
    /// count of zero and not listed at all. See the module header: the name is the
    /// disclosure. That the suggestion list is therefore narrower than the truth is the
    /// cost, and it is the right way round — a suggestion is a convenience, and a topic
    /// somebody cannot see any page of is one they have no reason to file under.
    pub async fn topics_for(&self, principal: &Principal) -> Result<Vec<TopicSummary>> {
        let names = self.topic_names().await?;
        let counts = self.readable_counts(principal).await?;
        let mut out = Vec::new();
        for path in names.keys() {
            let documents = subtree_count(&counts, path);
            if documents == 0 {
                continue;
            }
            let Some(topic) = topic_at(&names, path) else {
                continue;
            };
            out.push(TopicSummary { topic, documents });
        }
        Ok(out)
    }

    /// One topic: the pages under it and the topics inside it, or `None`.
    ///
    /// `None` covers "no such topic" and "you may read no document under it" alike, which is
    /// the same closed conflation [`Store::document_for`] makes and is the whole point here:
    /// distinguishing them would answer "is there a topic called `Kündigung Mietvertrag`" to
    /// anybody who asks.
    ///
    /// `wanted` may be written as a canonical path (`/medizin/darm`), without its leading
    /// slash, or with the names spelled as somebody typed them (`Medizin/Darm`) — all three
    /// reduce to the same key, because reducing them is what [`parse_stated`] does anyway.
    pub async fn topic_for(
        &self,
        principal: &Principal,
        wanted: &str,
    ) -> Result<Option<TopicListing>> {
        let Ok(segments) = parse_stated(wanted.trim_matches(TOPIC_SEPARATOR)) else {
            return Ok(None);
        };
        let path = canonical_path(&segments);

        let names = self.topic_names().await?;
        if !names.contains_key(&path) {
            return Ok(None);
        }
        let Some(topic) = topic_at(&names, &path) else {
            return Ok(None);
        };

        let readable = self.readable_documents(principal).await?;
        let mut documents: BTreeMap<String, TopicDocument> = BTreeMap::new();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (topic_path, document) in &readable {
            if within(&path, topic_path) {
                documents.insert(document.path.clone(), document.clone());
            }
            *counts.entry(topic_path.clone()).or_default() += 1;
        }
        if documents.is_empty() {
            return Ok(None);
        }

        let mut children = Vec::new();
        for child in names.keys() {
            if ancestors(child).first() != Some(&path) {
                continue;
            }
            let documents = subtree_count(&counts, child);
            if documents == 0 {
                continue;
            }
            if let Some(topic) = topic_at(&names, child) {
                children.push(TopicSummary { topic, documents });
            }
        }

        Ok(Some(TopicListing {
            topic,
            documents: documents.into_values().collect(),
            children,
        }))
    }

    /// The topics on one page, or `None` when the caller may not read that page.
    ///
    /// Nothing is returned at all to somebody who may not read the page, exactly as
    /// [`Store::backlinks_for`] refuses a page's backlinks: what a page is about is a fact
    /// about that page.
    pub async fn document_topics_for(
        &self,
        principal: &Principal,
        document_path: &str,
    ) -> Result<Option<Vec<Topic>>> {
        let Some(document) = self
            .document_for(principal, document_path, Action::Read)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(self.topics_on(&document.id).await?))
    }

    /// Replace a page's topics with exactly `stated`. Needs **Write** on the page.
    ///
    /// Write and not Read, and for the reason the seeder gives about frontmatter generally:
    /// a topic decides which listings a page turns up in, and putting a page in front of a
    /// different audience is an edit of that page. It is the same bar
    /// [`Store::publish_revision`] applies to its words.
    ///
    /// **Replace, not merge.** The whole set arrives at once, because that is what a
    /// frontmatter line says and what a file drop has to be able to mean; a merge would make
    /// a topic impossible to remove by editing the file that put it there.
    ///
    /// No revision is filed. A topic is not prose — the page's words are unchanged, and a
    /// revision saying "somebody re-filed this" would put a row in the history that no diff
    /// can show. D-2 draws the same line for a task: the page owns the words, the record
    /// owns the state.
    pub async fn set_document_topics(
        &self,
        principal: &Principal,
        document_path: &str,
        stated: &[String],
    ) -> Result<TopicOutcome> {
        let Some(document) = self
            .document_for(principal, document_path, Action::Write)
            .await?
        else {
            return Ok(TopicOutcome::Refused);
        };

        // Parsed BEFORE anything is written, so a page with one bad topic in its list keeps
        // the topics it had rather than half of the new ones.
        let wanted = match parse_all(stated) {
            Ok(wanted) => wanted,
            Err(reason) => return Ok(TopicOutcome::Rejected(reason)),
        };

        let mut tx = self.pool.begin().await?;
        replace_rows(&mut tx, &document.id, &wanted).await?;
        prune_empty_topics(&mut tx).await?;
        tx.commit().await?;

        Ok(TopicOutcome::Done(self.topics_on(&document.id).await?))
    }

    /// The topics on a document **with no permission check at all**.
    ///
    /// Crate-private and named plainly for what it is not, as
    /// [`Store::document_by_path_unchecked`] is. Both callers have already put the document
    /// through the accessor; a page's own topics are not a further disclosure once its words
    /// are readable.
    pub(crate) async fn topics_on(&self, document_id: &str) -> Result<Vec<Topic>> {
        let names = self.topic_names().await?;
        let paths: Vec<String> = sqlx::query_scalar(
            "SELECT t.path FROM document_tags dt JOIN tags t ON t.id = dt.tag_id \
             WHERE dt.doc_id = ?1 ORDER BY t.path",
        )
        .bind(document_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(paths
            .iter()
            .filter_map(|path| topic_at(&names, path))
            .collect())
    }

    /// Every topic's canonical path and leaf name.
    async fn topic_names(&self) -> Result<TopicNames> {
        let rows: Vec<(String, String)> = sqlx::query_as("SELECT path, name FROM tags")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().collect())
    }

    /// Every (topic path, readable document) pair in the wiki, filtered **per document**
    /// through the permission-checked accessor.
    ///
    /// The filtering is per candidate and never in the SQL, for the reason
    /// [`Store::backlinks_for`] gives: a `WHERE path LIKE` prefix is a second, weaker answer
    /// to a question `can()` already answers, and D-3 makes membership per document rather
    /// than per subtree, so a prefix cannot express it in the first place.
    ///
    /// The baseline comes out of the loop — it is a property of the caller, exactly as
    /// [`Store::tree_for`] treats it — and each distinct document is authorised once
    /// however many topics it carries, which is what keeps a page with six topics from
    /// being six authorisations.
    async fn readable_documents(
        &self,
        principal: &Principal,
    ) -> Result<Vec<(String, TopicDocument)>> {
        let rows: Vec<(String, String)> = sqlx::query_as(
            "SELECT t.path, d.path FROM document_tags dt \
             JOIN tags t ON t.id = dt.tag_id \
             JOIN documents d ON d.id = dt.doc_id AND d.deleted_at IS NULL \
             ORDER BY t.path, d.path",
        )
        .fetch_all(&self.pool)
        .await?;

        let baseline = self.baseline_for(principal).await?;
        let mut verdicts: HashMap<String, Option<TopicDocument>> = HashMap::new();
        let mut out = Vec::new();
        for (topic_path, document_path) in rows {
            let known = match verdicts.get(&document_path) {
                Some(known) => known.clone(),
                None => {
                    let known = self
                        .document_for_with_baseline(
                            principal,
                            &document_path,
                            Action::Read,
                            baseline,
                        )
                        .await?
                        .map(|doc| TopicDocument {
                            path: doc.path,
                            title: doc.title,
                        });
                    verdicts.insert(document_path.clone(), known.clone());
                    known
                }
            };
            if let Some(document) = known {
                out.push((topic_path, document));
            }
        }
        Ok(out)
    }

    /// How many readable documents sit directly on each topic — the input every subtree
    /// count is summed from.
    async fn readable_counts(&self, principal: &Principal) -> Result<HashMap<String, usize>> {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (topic_path, _) in self.readable_documents(principal).await? {
            *counts.entry(topic_path).or_default() += 1;
        }
        Ok(counts)
    }

    /// The id of the topic `stated` names, or `None` when it names none.
    ///
    /// **This is the foreign key `0011_tags.sql` explains SQLite will not let this schema
    /// add.** `projects.tag_id` has no constraint behind it, so the one guarantee a
    /// constraint would have given — you cannot point at something that is not there — has
    /// to be made by the writers instead. [`Store::create_project`] and
    /// [`Store::set_project_tag`] are those writers and there are no others.
    ///
    /// It takes a topic the way everything else in this crate does — by the name somebody
    /// typed, or by its canonical path — rather than by the uuid the column holds. A uuid
    /// is deliberately not on the wire (see [`Topic`]), so an id is a value no caller can
    /// obtain, and a parameter nothing can supply is not a check, it is a wall.
    ///
    /// **It answers regardless of who is asking, and its callers are what make that safe.**
    /// Both need Write on the project's home page before they get here, and the answer they
    /// take from it is one bit that never reaches a response body: a topic they may not see
    /// is refused exactly as a topic that does not exist is. Nothing here is a listing.
    pub(crate) async fn topic_id_for(&self, stated: &str) -> Result<Option<String>> {
        let Ok(segments) = parse_stated(stated.trim_matches(TOPIC_SEPARATOR)) else {
            return Ok(None);
        };
        Ok(sqlx::query_scalar("SELECT id FROM tags WHERE path = ?1")
            .bind(canonical_path(&segments))
            .fetch_optional(&self.pool)
            .await?)
    }
}

/// The [`Topic`] at `path`, or `None` when any step of its ancestry is missing.
fn topic_at(names: &TopicNames, path: &str) -> Option<Topic> {
    Some(Topic {
        path: path.to_string(),
        name: names.get(path)?.clone(),
        display_path: display_path(names, path)?,
    })
}

/// How many readable documents are under `root` — on it, or on any topic inside it.
fn subtree_count(counts: &HashMap<String, usize>, root: &str) -> usize {
    counts
        .iter()
        .filter(|(path, _)| within(root, path))
        .map(|(_, count)| *count)
        .sum()
}

/// Every stated topic, canonicalised and deduplicated, keyed by canonical path — or the
/// reason one of them is not a topic.
///
/// A `BTreeMap` rather than a `Vec` because it does both jobs at once: `Darm`, `darm` and
/// ` Darm ` collapse to one entry, and the entries come out in canonical order, which is
/// what makes a page's topics render the same however they were typed. That order is
/// load-bearing for the export round trip — see [`Topic::display_path`].
fn parse_all(stated: &[String]) -> Result<BTreeMap<String, Vec<Segment>>, String> {
    let mut wanted = BTreeMap::new();
    for one in stated {
        let segments = parse_stated(one)?;
        wanted.insert(canonical_path(&segments), segments);
    }
    Ok(wanted)
}

/// Replace a document's topic rows with `wanted`, creating any topic that is new.
///
/// Takes a CONNECTION, not the pool, so it joins the caller's transaction — the same
/// reasoning [`crate::links::replace_links`] gives: a create that fails afterwards must
/// leave no topic rows behind for a page that does not exist. (And the pool would not do
/// even if the reasoning were absent: it holds a single connection, so asking it for a
/// second one inside a transaction waits for the one the transaction is holding.)
///
/// It does **not** prune. Nothing here can empty a topic — rows are only added for the
/// document being written — and [`Store::set_document_topics`], which can, prunes itself.
async fn replace_rows(
    conn: &mut sqlx::SqliteConnection,
    document_id: &str,
    wanted: &BTreeMap<String, Vec<Segment>>,
) -> Result<()> {
    sqlx::query("DELETE FROM document_tags WHERE doc_id = ?1")
        .bind(document_id)
        .execute(&mut *conn)
        .await?;
    for segments in wanted.values() {
        let tag_id = ensure_topic(&mut *conn, segments).await?;
        sqlx::query("INSERT OR IGNORE INTO document_tags (doc_id, tag_id) VALUES (?1, ?2)")
            .bind(document_id)
            .bind(&tag_id)
            .execute(&mut *conn)
            .await?;
    }
    Ok(())
}

/// File a **new** document's stated topics, inside the transaction that creates it.
///
/// The counterpart of [`Store::set_document_topics`] for the one write in this crate that
/// takes an [`crate::Author`] rather than a principal — see [`crate::NewDocument::topics`]
/// for why topics ride along with a create instead of following it.
///
/// A topic that is not usable as one is an **error**, so it takes the whole create with it.
/// There is no half-created page here to report a rejection about.
pub(crate) async fn apply_stated(
    conn: &mut sqlx::SqliteConnection,
    document_id: &str,
    stated: &[String],
) -> Result<()> {
    let wanted = parse_all(stated).map_err(|reason| anyhow::anyhow!("{reason}"))?;
    replace_rows(conn, document_id, &wanted).await
}

/// Create the topic these segments name, and every ancestor it needs, returning its id.
///
/// Idempotent: an existing topic is found by its canonical path and its name is left alone.
/// **First writer wins the spelling** — see `0011_tags.sql`. Renaming one is not something
/// this system does, so the alternative (last writer wins) would mean a topic's name in
/// every listing changed whenever anybody re-tagged a page, with nothing recording that it
/// had.
async fn ensure_topic(conn: &mut sqlx::SqliteConnection, segments: &[Segment]) -> Result<String> {
    let mut parent: Option<String> = None;
    let mut path = String::new();
    for segment in segments {
        path.push(TOPIC_SEPARATOR);
        path.push_str(&segment.slug);
        let existing: Option<String> = sqlx::query_scalar("SELECT id FROM tags WHERE path = ?1")
            .bind(&path)
            .fetch_optional(&mut *conn)
            .await?;
        parent = Some(match existing {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::now_v7().to_string();
                sqlx::query("INSERT INTO tags (id, path, name, parent_id) VALUES (?1, ?2, ?3, ?4)")
                    .bind(&id)
                    .bind(&path)
                    .bind(&segment.name)
                    .bind(parent.as_deref())
                    .execute(&mut *conn)
                    .await?;
                id
            }
        });
    }
    parent.ok_or_else(|| anyhow::anyhow!("a topic with no segments cannot be created"))
}

/// Delete every topic that no page carries and nothing else needs.
///
/// A topic has no creation step (decision 2), so a topic no page is filed under is not
/// something anybody asked to keep — it is the residue of an edit. Deleting it is what makes
/// "this topic exists" and "this topic has at least one document" the same statement, which
/// is the sentence the disclosure rule in this module's header rests on.
///
/// **Upwards, repeatedly.** Emptying `Medizin/Darm/Labor` may leave `Medizin/Darm` childless
/// and empty too, and then `Medizin`. One pass per level, until a pass deletes nothing.
///
/// **`projects.tag_id` is consulted by hand**, and that is the price of the foreign key
/// `0011_tags.sql` explains SQLite will not let this schema add: a project pointing at a
/// topic is the one reference to it that lives outside `document_tags`, and pruning it would
/// silently un-tag the project. A foreign key would have refused the DELETE without anybody
/// having to remember; instead there is this clause, and a mutation test on it.
///
/// A document in the trash still counts as carrying its topics: `deleted_at` is reversible,
/// and a topic dropped while a page sits in the trash would not come back with it.
pub(crate) async fn prune_empty_topics(conn: &mut sqlx::SqliteConnection) -> Result<()> {
    loop {
        let done = sqlx::query(
            "DELETE FROM tags WHERE id IN ( \
               SELECT t.id FROM tags t \
                WHERE NOT EXISTS (SELECT 1 FROM document_tags dt WHERE dt.tag_id = t.id) \
                  AND NOT EXISTS (SELECT 1 FROM tags c WHERE c.parent_id = t.id) \
                  AND NOT EXISTS (SELECT 1 FROM projects p WHERE p.tag_id = t.id))",
        )
        .execute(&mut *conn)
        .await?;
        if done.rows_affected() == 0 {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Author, NewDocument};
    use gw_auth::{Permission, Subject};
    use gw_core::{Block, BlockKind, DocumentType, Visibility};

    fn body() -> Block {
        Block {
            kind: BlockKind::Doc,
            attrs: Default::default(),
            content: Vec::new(),
            text: None,
            marks: Vec::new(),
        }
    }

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
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
                    body: body(),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    /// Somebody who may write everything: a grant per page, because there is no baseline
    /// that confers write (D-M2-8) and a test that used one would prove nothing.
    async fn writer(store: &Store, paths: &[&str]) -> Principal {
        let who = Principal::test("chef", &[], &[]);
        for path in paths {
            store
                .add_grant(path, Subject::Principal(who.id.clone()), Permission::Write)
                .await
                .unwrap();
        }
        who
    }

    fn stated(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    async fn topic_paths(store: &Store) -> Vec<String> {
        sqlx::query_scalar("SELECT path FROM tags ORDER BY path")
            .fetch_all(&store.pool)
            .await
            .unwrap()
    }

    fn paths(topics: &[Topic]) -> Vec<&str> {
        topics.iter().map(|t| t.path.as_str()).collect()
    }

    fn displayed(topics: &[Topic]) -> Vec<&str> {
        topics.iter().map(|t| t.display_path.as_str()).collect()
    }

    fn done(outcome: TopicOutcome) -> Vec<Topic> {
        match outcome {
            TopicOutcome::Done(topics) => topics,
            other => panic!("expected the topics to be set, got {other:?}"),
        }
    }

    // --- what a topic IS ----------------------------------------------------------------

    #[tokio::test]
    async fn a_page_keeps_the_topics_it_is_given() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        let topics = done(
            store
                .set_document_topics(&chef, "/seite", &stated(&["Darm", "Ernährung"]))
                .await
                .unwrap(),
        );
        assert_eq!(paths(&topics), ["/darm", "/ernaehrung"]);
        assert_eq!(displayed(&topics), ["Darm", "Ernährung"]);

        let read_back = store
            .document_topics_for(&chef, "/seite")
            .await
            .unwrap()
            .expect("the writer may read the page");
        assert_eq!(read_back, topics);
    }

    #[tokio::test]
    async fn two_spellings_of_one_topic_are_one_topic_and_the_first_spelling_wins() {
        // The whole of "free text, but reused": the second person to type a topic must get
        // the first person's topic, not a near-duplicate nobody can tell apart in a list.
        let store = store().await;
        page(&store, "Eins", Visibility::Public).await;
        page(&store, "Zwei", Visibility::Public).await;
        let chef = writer(&store, &["/eins", "/zwei"]).await;

        store
            .set_document_topics(&chef, "/eins", &stated(&["Medizin"]))
            .await
            .unwrap();
        let second = done(
            store
                .set_document_topics(&chef, "/zwei", &stated(&["MEDIZIN"]))
                .await
                .unwrap(),
        );

        assert_eq!(paths(&second), ["/medizin"]);
        assert_eq!(displayed(&second), ["Medizin"], "the first spelling wins");
        assert_eq!(
            topic_paths(&store).await,
            ["/medizin"],
            "one topic, not two"
        );
    }

    #[tokio::test]
    async fn a_topic_inside_a_topic_creates_the_one_it_is_inside() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        let topics = done(
            store
                .set_document_topics(&chef, "/seite", &stated(&["Medizin/Darm"]))
                .await
                .unwrap(),
        );
        assert_eq!(paths(&topics), ["/medizin/darm"]);
        assert_eq!(displayed(&topics), ["Medizin/Darm"]);
        assert_eq!(
            topic_paths(&store).await,
            ["/medizin", "/medizin/darm"],
            "the parent has to exist for the tree to be a tree"
        );
    }

    #[tokio::test]
    async fn setting_topics_replaces_rather_than_adds() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        store
            .set_document_topics(&chef, "/seite", &stated(&["Darm", "Leber"]))
            .await
            .unwrap();
        let topics = done(
            store
                .set_document_topics(&chef, "/seite", &stated(&["Leber"]))
                .await
                .unwrap(),
        );
        assert_eq!(paths(&topics), ["/leber"]);
    }

    #[tokio::test]
    async fn topics_come_back_in_one_order_whatever_order_they_arrived_in() {
        // Load-bearing for the export round trip: a page whose topics came back in a
        // different order would render a file that re-imports as different metadata, and
        // one refused page fails the whole export.
        let store = store().await;
        page(&store, "Eins", Visibility::Public).await;
        page(&store, "Zwei", Visibility::Public).await;
        let chef = writer(&store, &["/eins", "/zwei"]).await;

        let forwards = done(
            store
                .set_document_topics(&chef, "/eins", &stated(&["Alpha", "Beta", "Gamma"]))
                .await
                .unwrap(),
        );
        let backwards = done(
            store
                .set_document_topics(&chef, "/zwei", &stated(&["Gamma", "Beta", "Alpha"]))
                .await
                .unwrap(),
        );
        assert_eq!(paths(&forwards), paths(&backwards));
    }

    #[tokio::test]
    async fn one_topic_stated_twice_is_stored_once() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        let topics = done(
            store
                .set_document_topics(&chef, "/seite", &stated(&["Darm", "darm", " Darm "]))
                .await
                .unwrap(),
        );
        assert_eq!(paths(&topics), ["/darm"]);
    }

    // --- what is not a topic ------------------------------------------------------------

    #[tokio::test]
    async fn something_that_reduces_to_no_key_at_all_is_refused() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        for hopeless in ["🧬", "…", "///"] {
            let outcome = store
                .set_document_topics(&chef, "/seite", &stated(&[hopeless]))
                .await
                .unwrap();
            assert!(
                matches!(outcome, TopicOutcome::Rejected(_)),
                "`{hopeless}` should not be a topic, got {outcome:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_control_character_in_a_topic_name_is_refused() {
        // It would put a newline in a YAML scalar and in every listing that renders it.
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        let outcome = store
            .set_document_topics(&chef, "/seite", &stated(&["Darm\u{7}Labor"]))
            .await
            .unwrap();
        assert!(matches!(outcome, TopicOutcome::Rejected(_)), "{outcome:?}");
    }

    #[tokio::test]
    async fn a_name_past_the_limit_and_a_tree_past_the_limit_are_both_refused() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        let long = "a".repeat(MAX_TOPIC_NAME_CHARS + 1);
        let deep = ["a"; MAX_TOPIC_DEPTH + 1].join("/");
        for bad in [long, deep] {
            let outcome = store
                .set_document_topics(&chef, "/seite", &stated(&[&bad]))
                .await
                .unwrap();
            assert!(matches!(outcome, TopicOutcome::Rejected(_)), "{outcome:?}");
        }
    }

    #[tokio::test]
    async fn one_unusable_topic_leaves_the_page_with_the_topics_it_had() {
        // Parsed before anything is written, so a list with one mistake in it is not
        // half-applied — which is how a page silently loses a topic.
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        store
            .set_document_topics(&chef, "/seite", &stated(&["Darm"]))
            .await
            .unwrap();
        let outcome = store
            .set_document_topics(&chef, "/seite", &stated(&["Leber", "🧬"]))
            .await
            .unwrap();
        assert!(matches!(outcome, TopicOutcome::Rejected(_)), "{outcome:?}");
        assert_eq!(
            paths(
                &store
                    .document_topics_for(&chef, "/seite")
                    .await
                    .unwrap()
                    .unwrap()
            ),
            ["/darm"]
        );
    }

    // --- the tree cannot loop -----------------------------------------------------------

    #[tokio::test]
    async fn two_topics_cannot_be_made_each_others_parent() {
        // Written against the SCHEMA rather than against this module, because "the Rust
        // code is careful" is not an answer: the guard has to hold for a rename or a move
        // written later, by somebody who has not read this file.
        let store = store().await;
        sqlx::query("INSERT INTO tags (id, path, name) VALUES ('a', '/a', 'A')")
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tags (id, path, name) VALUES ('b', '/b', 'B')")
            .execute(&store.pool)
            .await
            .unwrap();

        let refused = sqlx::query("UPDATE tags SET parent_id = 'b' WHERE id = 'a'")
            .execute(&store.pool)
            .await;
        assert!(refused.is_err(), "a cycle must not be storable");

        let also_refused =
            sqlx::query("INSERT INTO tags (id, path, name, parent_id) VALUES ('c','/c','C','a')")
                .execute(&store.pool)
                .await;
        assert!(
            also_refused.is_err(),
            "a parent must be this topic's own path with the last segment removed"
        );
    }

    #[tokio::test]
    async fn a_topic_cannot_be_reparented_under_its_own_descendant() {
        let store = store().await;
        sqlx::query("INSERT INTO tags (id, path, name) VALUES ('a', '/a', 'A')")
            .execute(&store.pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tags (id, path, name, parent_id) VALUES ('b','/a/b','B','a')")
            .execute(&store.pool)
            .await
            .unwrap();

        let refused = sqlx::query("UPDATE tags SET parent_id = 'b' WHERE id = 'a'")
            .execute(&store.pool)
            .await;
        assert!(refused.is_err(), "`/a` cannot sit inside `/a/b`");
    }

    #[tokio::test]
    async fn a_top_level_topic_is_exactly_one_segment() {
        let store = store().await;
        let refused = sqlx::query("INSERT INTO tags (id, path, name) VALUES ('x','/a/b','B')")
            .execute(&store.pool)
            .await;
        assert!(
            refused.is_err(),
            "a path whose prefix names no topic breaks every subtree listing"
        );
    }

    // --- a topic nobody carries stops existing ------------------------------------------

    #[tokio::test]
    async fn a_topic_no_page_carries_any_more_is_gone() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;

        store
            .set_document_topics(&chef, "/seite", &stated(&["Medizin/Darm/Labor"]))
            .await
            .unwrap();
        assert_eq!(topic_paths(&store).await.len(), 3);

        store
            .set_document_topics(&chef, "/seite", &[])
            .await
            .unwrap();
        assert!(
            topic_paths(&store).await.is_empty(),
            "an empty topic is the residue of an edit, not something somebody asked to keep"
        );
    }

    #[tokio::test]
    async fn a_topic_survives_while_a_topic_inside_it_still_has_a_page() {
        let store = store().await;
        page(&store, "Eins", Visibility::Public).await;
        page(&store, "Zwei", Visibility::Public).await;
        let chef = writer(&store, &["/eins", "/zwei"]).await;

        store
            .set_document_topics(&chef, "/eins", &stated(&["Medizin"]))
            .await
            .unwrap();
        store
            .set_document_topics(&chef, "/zwei", &stated(&["Medizin/Darm"]))
            .await
            .unwrap();

        store
            .set_document_topics(&chef, "/eins", &[])
            .await
            .unwrap();
        assert_eq!(
            topic_paths(&store).await,
            ["/medizin", "/medizin/darm"],
            "`Medizin` still has to exist for `Medizin/Darm` to be inside it"
        );
    }

    #[tokio::test]
    async fn a_topic_a_project_points_at_is_not_pruned() {
        // The check `0011_tags.sql` explains a foreign key cannot make on this schema. A
        // project's tag is the one reference to a topic that does not live in
        // `document_tags`, and pruning it would silently un-tag the project.
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        page(&store, "Projekt", Visibility::Public).await;
        let chef = writer(&store, &["/seite", "/projekt"]).await;

        store
            .set_document_topics(&chef, "/seite", &stated(&["Umbau"]))
            .await
            .unwrap();
        store
            .create_project(&chef, "/projekt", Some("Umbau"))
            .await
            .unwrap()
            .expect("the writer may make their page a project home");

        store
            .set_document_topics(&chef, "/seite", &[])
            .await
            .unwrap();
        assert_eq!(
            topic_paths(&store).await,
            ["/umbau"],
            "a project still points at it"
        );
    }

    #[tokio::test]
    async fn a_project_cannot_point_at_a_topic_that_does_not_exist() {
        let store = store().await;
        page(&store, "Projekt", Visibility::Public).await;
        let chef = writer(&store, &["/projekt"]).await;

        let refused = store
            .create_project(&chef, "/projekt", Some("nicht-vorhanden"))
            .await
            .unwrap();
        assert!(
            refused.is_none(),
            "the foreign key 0011 cannot add is made by hand here, or not at all"
        );
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM projects")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(left, 0, "a refused create writes nothing");
    }

    #[tokio::test]
    async fn a_project_cannot_be_pointed_at_a_topic_that_does_not_exist() {
        let store = store().await;
        page(&store, "Projekt", Visibility::Public).await;
        let chef = writer(&store, &["/projekt"]).await;
        let project = store
            .create_project(&chef, "/projekt", None)
            .await
            .unwrap()
            .unwrap();

        assert!(!store
            .set_project_tag(&chef, &project.id, Some("nicht-vorhanden"))
            .await
            .unwrap());
    }

    // --- who may see what ---------------------------------------------------------------

    /// `/offen` is public and `/geheim` is restricted; both are about `Darm`, and `/geheim`
    /// is also the only page about `Kündigung`. `leser` may read neither restricted page.
    async fn disclosure_fixture() -> (Store, Principal, Principal) {
        let store = store().await;
        page(&store, "Offen", Visibility::Public).await;
        page(&store, "Geheim", Visibility::Restricted).await;
        let chef = writer(&store, &["/offen", "/geheim"]).await;

        store
            .set_document_topics(&chef, "/offen", &stated(&["Darm"]))
            .await
            .unwrap();
        store
            .set_document_topics(&chef, "/geheim", &stated(&["Darm", "Kündigung"]))
            .await
            .unwrap();

        let leser = Principal::test("leser", &[], &[]);
        (store, chef, leser)
    }

    #[tokio::test]
    async fn a_listing_omits_a_document_the_caller_may_not_read() {
        let (store, chef, leser) = disclosure_fixture().await;

        let theirs = store.topic_for(&leser, "/darm").await.unwrap().unwrap();
        assert_eq!(
            theirs
                .documents
                .iter()
                .map(|d| d.path.as_str())
                .collect::<Vec<_>>(),
            ["/offen"]
        );
        assert_eq!(theirs.documents.len(), 1);

        let mine = store.topic_for(&chef, "/darm").await.unwrap().unwrap();
        assert_eq!(mine.documents.len(), 2, "the fixture has something to hide");
    }

    #[tokio::test]
    async fn a_topic_whose_every_document_is_out_of_reach_does_not_exist_for_that_caller() {
        // The name is the disclosure: `Kündigung` on nothing but a restricted page says
        // that such a page exists and roughly what it says.
        let (store, chef, leser) = disclosure_fixture().await;

        assert!(store
            .topic_for(&leser, "/kuendigung")
            .await
            .unwrap()
            .is_none());
        assert!(
            store
                .topic_for(&chef, "/kuendigung")
                .await
                .unwrap()
                .is_some(),
            "the fixture has something to hide"
        );
    }

    #[tokio::test]
    async fn the_index_offers_no_topic_the_caller_can_see_no_page_of() {
        let (store, chef, leser) = disclosure_fixture().await;

        let theirs = store.topics_for(&leser).await.unwrap();
        assert_eq!(
            theirs
                .iter()
                .map(|t| t.topic.path.as_str())
                .collect::<Vec<_>>(),
            ["/darm"]
        );
        assert_eq!(theirs[0].documents, 1, "it counts only what it would show");

        let mine = store.topics_for(&chef).await.unwrap();
        assert_eq!(mine.len(), 2, "the fixture has something to hide");
        assert_eq!(
            mine.iter()
                .find(|t| t.topic.path == "/darm")
                .unwrap()
                .documents,
            2
        );
    }

    #[tokio::test]
    async fn the_index_count_is_the_length_of_the_listing_it_would_hand_back() {
        // Structural: the count is what survived the filter, never what the filter removed.
        let (store, _chef, leser) = disclosure_fixture().await;
        for summary in store.topics_for(&leser).await.unwrap() {
            let listing = store
                .topic_for(&leser, &summary.topic.path)
                .await
                .unwrap()
                .expect("the index offered it, so it exists for this caller");
            assert_eq!(summary.documents, listing.documents.len());
        }
    }

    #[tokio::test]
    async fn a_topic_is_visible_because_a_topic_inside_it_has_a_readable_page() {
        let store = store().await;
        page(&store, "Offen", Visibility::Public).await;
        let chef = writer(&store, &["/offen"]).await;
        store
            .set_document_topics(&chef, "/offen", &stated(&["Medizin/Darm"]))
            .await
            .unwrap();

        let leser = Principal::test("leser", &[], &[]);
        let index = store.topics_for(&leser).await.unwrap();
        assert_eq!(
            index
                .iter()
                .map(|t| t.topic.path.as_str())
                .collect::<Vec<_>>(),
            ["/medizin", "/medizin/darm"]
        );

        let parent = store.topic_for(&leser, "/medizin").await.unwrap().unwrap();
        assert_eq!(
            parent
                .documents
                .iter()
                .map(|d| d.path.as_str())
                .collect::<Vec<_>>(),
            ["/offen"],
            "listing a topic lists the topics inside it too"
        );
        assert_eq!(
            parent
                .children
                .iter()
                .map(|c| c.topic.path.as_str())
                .collect::<Vec<_>>(),
            ["/medizin/darm"]
        );
    }

    #[tokio::test]
    async fn a_topic_a_caller_may_see_no_page_of_is_absent_from_a_parents_children() {
        let store = store().await;
        page(&store, "Offen", Visibility::Public).await;
        page(&store, "Geheim", Visibility::Restricted).await;
        let chef = writer(&store, &["/offen", "/geheim"]).await;
        store
            .set_document_topics(&chef, "/offen", &stated(&["Medizin/Darm"]))
            .await
            .unwrap();
        store
            .set_document_topics(&chef, "/geheim", &stated(&["Medizin/Kündigung"]))
            .await
            .unwrap();

        let leser = Principal::test("leser", &[], &[]);
        let theirs = store.topic_for(&leser, "/medizin").await.unwrap().unwrap();
        assert_eq!(
            theirs
                .children
                .iter()
                .map(|c| c.topic.path.as_str())
                .collect::<Vec<_>>(),
            ["/medizin/darm"]
        );

        let mine = store.topic_for(&chef, "/medizin").await.unwrap().unwrap();
        assert_eq!(mine.children.len(), 2, "the fixture has something to hide");
    }

    #[tokio::test]
    async fn a_page_the_caller_may_not_read_says_nothing_about_its_topics() {
        let (store, _chef, leser) = disclosure_fixture().await;
        assert!(store
            .document_topics_for(&leser, "/geheim")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn setting_a_pages_topics_needs_write_on_it() {
        let store = store().await;
        page(&store, "Seite", Visibility::Public).await;
        let leser = Principal::test("leser", &[], &[]);
        // Public confers READ to anybody; write is only ever an explicit grant (D-M2-8).
        assert!(
            store
                .document_topics_for(&leser, "/seite")
                .await
                .unwrap()
                .is_some(),
            "the fixture is readable, so the refusal below is about writing"
        );

        let outcome = store
            .set_document_topics(&leser, "/seite", &stated(&["Darm"]))
            .await
            .unwrap();
        assert_eq!(outcome, TopicOutcome::Refused);
        assert!(topic_paths(&store).await.is_empty(), "nothing was written");
    }

    #[tokio::test]
    async fn an_absent_page_is_the_same_refusal_as_one_that_may_not_be_written() {
        let store = store().await;
        let chef = Principal::test("chef", &[], &[]);
        let outcome = store
            .set_document_topics(&chef, "/gibt-es-nicht", &stated(&["Darm"]))
            .await
            .unwrap();
        assert_eq!(outcome, TopicOutcome::Refused);
    }

    #[tokio::test]
    async fn a_topic_nobody_typed_is_the_same_answer_as_one_that_is_hidden() {
        let (store, _chef, leser) = disclosure_fixture().await;
        assert!(store
            .topic_for(&leser, "/gibt-es-nicht")
            .await
            .unwrap()
            .is_none());
        assert!(store
            .topic_for(&leser, "/kuendigung")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn a_topic_may_be_asked_for_by_the_name_somebody_typed() {
        let (store, chef, _leser) = disclosure_fixture().await;
        for spelling in ["/kündigung", "Kündigung", "kuendigung", "/kuendigung/"] {
            assert!(
                store.topic_for(&chef, spelling).await.unwrap().is_some(),
                "`{spelling}` names the same topic"
            );
        }
    }

    #[tokio::test]
    async fn a_page_in_the_trash_is_in_no_listing_and_keeps_its_topics() {
        let store = store().await;
        let id = page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;
        store
            .set_document_topics(&chef, "/seite", &stated(&["Darm"]))
            .await
            .unwrap();

        sqlx::query(
            "UPDATE documents SET deleted_at = datetime('now'), deleted_root = id, \
             deleted_by = 'test', deleted_by_name = 'Test' WHERE id = ?1",
        )
        .bind(&id)
        .execute(&store.pool)
        .await
        .unwrap();

        assert!(store.topic_for(&chef, "/darm").await.unwrap().is_none());
        assert_eq!(
            topic_paths(&store).await,
            ["/darm"],
            "the trash is reversible, so the topic has to come back with the page"
        );
    }

    #[tokio::test]
    async fn purging_a_page_takes_its_topic_rows_with_it() {
        let store = store().await;
        let id = page(&store, "Seite", Visibility::Public).await;
        let chef = writer(&store, &["/seite"]).await;
        store
            .set_document_topics(&chef, "/seite", &stated(&["Darm"]))
            .await
            .unwrap();

        sqlx::query("DELETE FROM documents WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();

        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM document_tags")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(
            left, 0,
            "a row pointing at a purged page could never be authorised"
        );
    }

    // --- the parts, on their own --------------------------------------------------------

    #[test]
    fn a_separator_is_structural_and_never_part_of_a_name() {
        let segments = parse_stated("Vor/Nachteile").unwrap();
        assert_eq!(
            segments.len(),
            2,
            "`/` nests; it is not a character in a name"
        );
        assert_eq!(canonical_path(&segments), "/vor/nachteile");
    }

    #[test]
    fn whitespace_is_collapsed_so_two_spellings_do_not_look_identical_in_a_list() {
        let segments = parse_stated("  Medizin\t und   Recht ").unwrap();
        assert_eq!(segments[0].name, "Medizin und Recht");
    }

    #[test]
    fn ancestors_are_every_step_up_and_never_the_root_slash() {
        assert_eq!(ancestors("/a/b/c"), ["/a/b", "/a"]);
        assert_eq!(ancestors("/a"), Vec::<String>::new());
    }

    #[test]
    fn a_topic_is_not_inside_one_whose_name_it_merely_starts_with() {
        assert!(within("/medizin", "/medizin"));
        assert!(within("/medizin", "/medizin/darm"));
        assert!(!within("/medizin", "/medizin-recht"));
    }
}
