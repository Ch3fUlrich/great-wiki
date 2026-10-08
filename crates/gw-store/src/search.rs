//! Full-text search over pages: the index, and nothing else (M7, ADR 0024).
//!
//! **This module does not decide who may see what.** It answers "which documents' words
//! match these words" and hands back *candidates* — a document id, a rank and a snippet —
//! for the permission-checked accessor to accept or drop, one document at a time, through
//! `document_for_id`. That is architecture rule 2: the filter is in the retriever, never a
//! post-filter on something already shown. The consequence is the shape of this API:
//!
//! - [`Candidate`], [`SearchIndex`] and [`Fts5Index`] are crate-private. Nothing outside
//!   `gw-store` can obtain an unfiltered hit, so a handler cannot forget to filter one.
//! - A candidate's snippet is made from the index's copy of the text. It is a ranking
//!   artefact, not content: whoever builds a response takes the excerpt from the document
//!   the accessor returned, never from here.
//!
//! The trait is the seam ADR 0003 promised: Tantivy or an external engine would be a second
//! implementation of [`SearchIndex`], and nothing above it would change.

use crate::acl::Baseline;
use crate::{Store, StoredDocument, TaskStatus};
use anyhow::Result;
use gw_auth::{Action, Principal};
use serde::Serialize;
use sqlx::SqlitePool;
use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

/// How many words of a query are used. A query is a few words; a pasted paragraph would
/// otherwise become a MATCH expression with hundreds of terms for no better result.
const MAX_TERMS: usize = 12;

/// How long one word of a query may be, in characters. No indexed word is longer in
/// practice; the cap keeps a hostile megabyte-long "word" out of the query planner.
const MAX_TERM_CHARS: usize = 64;

/// One document the index thinks matches. **Unfiltered**: it has not been through any
/// permission check, and a trashed page is the only thing already excluded.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Candidate {
    pub doc_id: String,
    /// `bm25()` — lower is better, as SQLite defines it. Candidates arrive best first.
    pub rank: f64,
    /// An excerpt of the index's own text, unmarked. See the module doc for why a response
    /// must not use it.
    pub snippet: String,
}

/// Something that can say which documents match a query.
pub(crate) trait SearchIndex {
    /// At most `limit` candidates, best first. A query with nothing searchable in it is
    /// an empty answer, not an error — the caller cannot tell it from "no match", which is
    /// what stops the endpoint being an oracle for what a query was parsed as.
    async fn candidates(&self, query: &str, limit: usize) -> Result<Vec<Candidate>>;
}

/// The SQLite FTS5 implementation (ADR 0003): the index lives in the same database as the
/// documents, kept current by triggers (`0016_search.sql`).
pub(crate) struct Fts5Index {
    pool: SqlitePool,
}

impl Fts5Index {
    pub(crate) fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl SearchIndex for Fts5Index {
    async fn candidates(&self, query: &str, limit: usize) -> Result<Vec<Candidate>> {
        let Some(expression) = match_expression(query) else {
            return Ok(Vec::new());
        };
        // `deleted_at IS NULL` is read from `documents` here, at query time, because the
        // index holds no state (ADR 0024): trash and restore never touch it.
        //
        // bm25 weights are per column in declaration order — title 10, body 1 — so a page
        // named for a thing outranks a page that merely mentions it (ADR 0003 asked for
        // exactly this). Column -1 lets `snippet` pick whichever column matched best.
        let rows: Vec<(String, f64, String)> = sqlx::query_as(
            "SELECT d.id, bm25(search_pages, 10.0, 1.0) AS rank, \
                    snippet(search_pages, -1, '', '', '…', 16) \
             FROM search_pages \
             JOIN documents d ON d.rowid = search_pages.rowid \
             WHERE search_pages MATCH ?1 AND d.deleted_at IS NULL \
             ORDER BY rank, d.id \
             LIMIT ?2",
        )
        .bind(expression)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|(doc_id, rank, snippet)| Candidate {
                doc_id,
                rank,
                snippet,
            })
            .collect())
    }
}

/// The FTS5 MATCH expression for what somebody typed, or `None` if nothing in it is a word.
///
/// **User text never reaches MATCH raw.** FTS5's query language has operators (`AND`, `OR`,
/// `NOT`, `NEAR(`), column filters (`title:x`), prefix stars, grouping and phrase quotes, and
/// a stray one is a syntax error that turns a search box into a 500 — or a working operator
/// that lets a caller shape the query. So the text is cut into words (runs of alphanumeric
/// characters, which is also what the index's tokenizer indexes), and every word is emitted
/// as a quoted FTS5 string. Inside quotes an operator is just a word to look for, and since a
/// word contains no `"` there is nothing to escape. The last word gets a prefix star, so a
/// search-as-you-type query finds `Dar` → `Darm`; the words are ANDed.
///
/// The words are cut by [`query_words`], the same cut the in-memory matching of topics and
/// tasks uses, so the two halves of a search cannot disagree about what was asked. A word
/// that was cut short for length also gets the star wherever it sits: it stands for a longer
/// word, and a quoted fragment without one would match nothing.
pub(crate) fn match_expression(query: &str) -> Option<String> {
    let words = query_words(query);
    let last = words.len().checked_sub(1)?;
    Some(
        words
            .iter()
            .enumerate()
            .map(|(i, (w, truncated))| {
                if i == last || *truncated {
                    format!("\"{w}\"*")
                } else {
                    format!("\"{w}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

/// The words of a query, each with whether it was cut for length.
///
/// The query is put in composed form (NFC) first, so `u` + combining diaeresis and `ü` are
/// one word to the index and to the in-memory matching alike. A word that the tokenizer would
/// index as nothing — a run of characters Rust calls alphanumeric and `unicode61` calls
/// separators, like `ⓐ` — is dropped: quoted, it would be an empty phrase.
fn query_words(query: &str) -> Vec<(String, bool)> {
    let query: String = query.nfc().collect();
    word_ranges(&query)
        .into_iter()
        .map(|(s, e)| &query[s..e])
        .filter(|w| !fold(w).is_empty())
        .take(MAX_TERMS)
        .map(|w| {
            (
                w.chars().take(MAX_TERM_CHARS).collect::<String>(),
                w.chars().count() > MAX_TERM_CHARS,
            )
        })
        .collect()
}

/// The longest query that is searched, in characters, after trimming. A longer one is
/// answered as an empty search rather than cut: a truncated query would quietly look for
/// something other than what was typed, and "nothing found" is the answer the endpoint
/// already gives for every query it will not run.
pub const MAX_QUERY_CHARS: usize = 200;

/// The most hits one group of an answer holds, whatever the caller asked for.
const MAX_LIMIT: usize = 50;

/// The most candidates the index is asked for, which is every match up to this ceiling.
///
/// Permission is applied *after* the index, one candidate at a time, and the visible hits are
/// then ranked here (see `search_for`), so the index must hand over all of them: a window
/// of "the best N" would be chosen by bm25 over pages the caller may not read, and a readable
/// page ranked below it would silently vanish. Past this ceiling — a word on more than a
/// thousand pages — which candidates are kept is still decided by the index's order, and a
/// caller may get fewer hits than exist. That fails by under-reporting, never by
/// over-disclosing, and says nothing about how many were dropped (ADR 0024).
const MAX_CANDIDATES: usize = 1000;

/// Words of context kept before and after the first match in a snippet.
const WORDS_BEFORE: usize = 6;
const WORDS_AFTER: usize = 14;

/// One run of a snippet. The wire form of "this part is the match": a client renders the
/// segments as text, emphasising the `hit` ones, and never needs an HTML sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Segment {
    pub text: String,
    pub hit: bool,
}

/// A page the caller may read, found by words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PageHit {
    pub title: String,
    /// Where the page is **now**; the index holds no path (ADR 0024).
    pub path: String,
    pub snippet: Vec<Segment>,
}

/// A topic the caller may see, found by its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TopicHit {
    pub name: String,
    pub display_path: String,
    pub path: String,
    /// As `topics_for` gave it to this caller: the length of the list they would be handed.
    pub documents: usize,
}

/// A task on a board the caller may read, found by its title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskHit {
    pub id: String,
    pub title: String,
    pub status: TaskStatus,
    /// The page the card hangs off, if it hangs off one — and only ever the path the board
    /// already cleared for this caller (`Task::page`), never one looked up here.
    pub page_path: Option<String>,
}

/// Everything a search found **for this caller**.
///
/// There is deliberately no count, total or "n more" anywhere in here: a number about the
/// hits that were left out is a number about pages the caller may not read. The length of a
/// list is the only count, and it is the length of what they were given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct SearchResults {
    pub pages: Vec<PageHit>,
    pub topics: Vec<TopicHit>,
    pub tasks: Vec<TaskHit>,
}

/// The query as it will be searched, or `None` if it is not to be searched at all.
fn normalise(query: &str) -> Option<String> {
    let query: String = query.trim().nfc().collect();
    if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
        return None;
    }
    Some(query)
}

/// Lowercased and stripped of diacritics — the folding the FTS5 tokenizer
/// (`unicode61 remove_diacritics 2`) applies, so a topic or a task title is found by the
/// same typing a page is. `ß` is not expanded, as there.
fn fold(word: &str) -> String {
    word.nfd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Is `c` part of a word, as the index's tokenizer sees words?
///
/// Rust's `is_alphanumeric` is wider than `unicode61`'s letters, numbers and private-use
/// characters: it includes circled and squared Latin letters, which are symbols to the
/// tokenizer and so separate words there. Those ranges are excluded, so a query word is never
/// one the index would read as nothing.
fn is_word_char(c: char) -> bool {
    let symbol_letter = matches!(c as u32, 0x24B6..=0x24E9 | 0x1F130..=0x1F189);
    (c.is_alphanumeric() && !symbol_letter) || is_combining_mark(c)
}

/// Byte ranges of the words of `text`.
fn word_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (is_word_char(c), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

/// The folded words of a query — the same cut `match_expression` makes, for the parts of the
/// search that do not go through the index.
fn query_terms(query: &str) -> Vec<String> {
    query_words(query)
        .into_iter()
        .map(|(word, _)| fold(&word))
        .collect()
}

/// Does this word start with one of the terms? Prefix, as the last term is in the index.
fn word_hits(word: &str, terms: &[String]) -> bool {
    let word = fold(word);
    terms.iter().any(|t| word.starts_with(t.as_str()))
}

/// How well a page the caller may read answers a query: ten for each query word that starts
/// a word of the title, one for each word of the text that starts with a query word.
///
/// A function of this page and the query only — no collection statistics — which is what
/// keeps the order of the answer from depending on pages the caller was not shown.
fn page_score(title: &str, text: &str, terms: &[String]) -> u64 {
    let title_words: Vec<String> = word_ranges(title)
        .into_iter()
        .map(|(s, e)| fold(&title[s..e]))
        .collect();
    let in_title = terms
        .iter()
        .filter(|t| title_words.iter().any(|w| w.starts_with(t.as_str())))
        .count() as u64;
    let in_text = word_ranges(text)
        .into_iter()
        .filter(|&(s, e)| word_hits(&text[s..e], terms))
        .count() as u64;
    in_title * 10 + in_text
}

/// Does `text` hold, for every term, a word that term is a prefix of?
fn text_matches(text: &str, terms: &[String]) -> bool {
    let words: Vec<String> = word_ranges(text)
        .into_iter()
        .map(|(s, e)| fold(&text[s..e]))
        .collect();
    terms
        .iter()
        .all(|t| words.iter().any(|w| w.starts_with(t.as_str())))
}

/// An excerpt of `text` around its first matching word, as runs that say which are matches.
///
/// Built from whatever text it is given, so the caller decides whose text that is: for a
/// response it is always the accessor's document, never the index's copy.
fn segments(text: &str, terms: &[String]) -> Vec<Segment> {
    let words = word_ranges(text);
    if words.is_empty() {
        return Vec::new();
    }
    let hits: Vec<bool> = words
        .iter()
        .map(|&(s, e)| word_hits(&text[s..e], terms))
        .collect();
    let first = hits.iter().position(|h| *h).unwrap_or(0);
    let from = first.saturating_sub(WORDS_BEFORE);
    let to = (first + WORDS_AFTER).min(words.len());

    let mut out: Vec<Segment> = Vec::new();
    let mut push = |text: &str, hit: bool| {
        if text.is_empty() {
            return;
        }
        match out.last_mut() {
            Some(last) if last.hit == hit => last.text.push_str(text),
            _ => out.push(Segment {
                text: text.to_string(),
                hit,
            }),
        }
    };
    if from > 0 {
        push("…", false);
    }
    let mut cursor = words[from].0;
    for (i, &(s, e)) in words.iter().enumerate().take(to).skip(from) {
        push(&text[cursor..s], false);
        push(&text[s..e], hits[i]);
        cursor = e;
    }
    if to < words.len() {
        push("…", false);
    }
    out
}

impl Store {
    /// Search pages, topics and tasks **as `principal`**, at most `limit` of each.
    ///
    /// **Every page hit has been through the accessor.** The index (`Fts5Index`, crate-private)
    /// yields candidates — ids and a rank, no permission — and each one is put through
    /// [`Store::document_for_id_with_baseline`] for `Action::Read`; a `None` is dropped without
    /// trace. The title, the path and the snippet in a hit are those of the document the
    /// accessor returned, not of the index row: the index's copy of the text is a ranking
    /// artefact and is never shown. Topics and tasks come from [`Store::topics_for`] and
    /// [`Store::board_for`], which filter by the same rule, and are matched in memory.
    ///
    /// **Nothing here counts what was left out.** No total, no "n hidden", and the answer is cut
    /// to `limit` visible hits after ranking, by a score that never sees a withheld page.
    ///
    /// A blank, over-long or wordless query is an empty answer, the same value a query that
    /// matched nothing returns.
    pub async fn search_for(
        &self,
        principal: &Principal,
        query: &str,
        limit: usize,
    ) -> Result<SearchResults> {
        let Some(query) = normalise(query) else {
            return Ok(SearchResults::default());
        };
        let terms = query_terms(&query);
        let limit = limit.min(MAX_LIMIT);
        if terms.is_empty() || limit == 0 {
            return Ok(SearchResults::default());
        }

        // Once, for the caller. It is a property of who is asking, so one value serves every
        // candidate — and a value belonging to anyone else would be a hole that reads like
        // an optimisation (see `document_for_id_with_baseline`).
        let baseline = self.baseline_for(principal).await?;

        // Every candidate the index has, not a window of the best: see `MAX_CANDIDATES`.
        let candidates = Fts5Index::new(self.pool.clone())
            .candidates(&query, MAX_CANDIDATES)
            .await?;
        let mut visible: Vec<(u64, PageHit)> = Vec::new();
        for candidate in candidates {
            let Some(document) = self
                .readable_candidate(principal, &candidate.doc_id, baseline)
                .await?
            else {
                continue;
            };
            let text = gw_core::body_plain_text(&document.body);
            let score = page_score(&document.title, &text, &terms);
            visible.push((
                score,
                PageHit {
                    title: document.title,
                    path: document.path,
                    snippet: segments(&text, &terms),
                },
            ));
        }
        // Ranked here, from the pages the caller was handed, and NOT by the index's bm25.
        // bm25 weighs a word by how rare it is across *every* indexed page, withheld ones
        // included, so an order that followed it would let a caller learn how many pages
        // mention a word they may not read. The score is a function of the visible page and
        // the query alone; ties go by path, which the caller already holds.
        visible.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
        visible.truncate(limit);
        let pages: Vec<PageHit> = visible.into_iter().map(|(_, hit)| hit).collect();

        let topics = self
            .topics_for(principal)
            .await?
            .into_iter()
            .filter(|summary| text_matches(&summary.topic.display_path, &terms))
            .take(limit)
            .map(|summary| TopicHit {
                name: summary.topic.name,
                display_path: summary.topic.display_path,
                path: summary.topic.path,
                documents: summary.documents,
            })
            .collect();

        let tasks = self
            .board_for(principal, None)
            .await?
            .into_iter()
            .filter(|task| text_matches(&task.title, &terms))
            .take(limit)
            .map(|task| TaskHit {
                id: task.id,
                title: task.title,
                status: task.status,
                page_path: task.page.map(|page| page.path),
            })
            .collect();

        Ok(SearchResults {
            pages,
            topics,
            tasks,
        })
    }

    /// One candidate, through the one accessor. A function of its own so that it is the
    /// single place a search result is authorised.
    async fn readable_candidate(
        &self,
        principal: &Principal,
        document_id: &str,
        baseline: Baseline,
    ) -> Result<Option<StoredDocument>> {
        self.document_for_id_with_baseline(principal, document_id, Action::Read, baseline)
            .await
    }

    /// Give every page that predates `0016_search.sql` its `body_text`, then rebuild the
    /// index from `documents`. Run by [`Store::open`]; returns how many rows it filled.
    ///
    /// **Why open-time Rust and not SQL in the migration:** the flattener is
    /// `gw_core::body_plain_text`, and a second implementation in SQL would be a second rule
    /// about where words end. **Why `rebuild` as well:** the migration indexed these rows by
    /// title alone and the triggers have kept the index consistent since, so this is belt and
    /// braces — but it is the one command that repairs an index that has drifted for any
    /// reason, and it costs milliseconds at this corpus size, so it runs at EVERY start: the
    /// index joins `documents` on its implicit rowid, which a `VACUUM` may renumber, and a
    /// stale mapping would otherwise answer with the wrong pages. **Idempotent.** A row a future writer inserts
    /// without `body_text` is picked up at the next start rather than staying invisible for
    /// ever.
    pub(crate) async fn backfill_search_text(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, body FROM documents WHERE body_text IS NULL")
                .fetch_all(&mut *tx)
                .await?;
        for (id, body) in &rows {
            sqlx::query("UPDATE documents SET body_text = ?2 WHERE id = ?1")
                .bind(id)
                .bind(gw_core::body_plain_text(body))
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("INSERT INTO search_pages (search_pages) VALUES ('rebuild')")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(rows.len() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trash::{Purge, PurgeOutcome};
    use crate::{Author, NewDocument, NewTask, TaskHome, TaskOutcome};
    use gw_auth::{Permission, Principal, Subject};
    use gw_core::{Block, DocumentType, Visibility};

    use crate::moves::{MoveMode, MoveOutcome, MoveRequest};

    async fn store() -> Store {
        Store::open("sqlite::memory:").await.unwrap()
    }

    fn body(text: &str) -> Block {
        serde_json::from_value(serde_json::json!({
            "kind": "doc",
            "content": [{"kind": "paragraph", "content": [{"kind": "text", "text": text}]}]
        }))
        .unwrap()
    }

    async fn page_under(store: &Store, parent: Option<&str>, title: &str, text: &str) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Public,
                    body: body(text),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    async fn page(store: &Store, title: &str, text: &str) -> String {
        page_under(store, None, title, text).await
    }

    async fn ids(store: &Store, query: &str) -> Vec<String> {
        Fts5Index::new(store.pool.clone())
            .candidates(query, 50)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.doc_id)
            .collect()
    }

    async fn scalar(store: &Store, sql: &str) -> i64 {
        sqlx::query_scalar(sql)
            .fetch_one(&store.pool)
            .await
            .unwrap()
    }

    // --- the MATCH expression: a pure function, tested on hostile strings --------------

    #[test]
    fn words_are_quoted_anded_and_the_last_takes_a_prefix_star() {
        assert_eq!(
            match_expression("Darm Polypen").as_deref(),
            Some("\"Darm\" AND \"Polypen\"*")
        );
        assert_eq!(match_expression("Müller").as_deref(), Some("\"Müller\"*"));
    }

    #[test]
    fn nothing_searchable_is_no_expression() {
        for q in ["", "   ", "\"", "*", "()", "-- ;", "\u{0}"] {
            assert_eq!(match_expression(q), None, "for {q:?}");
        }
    }

    #[test]
    fn operators_become_words_and_nothing_else_survives() {
        for (q, expected) in [
            ("a AND b", "\"a\" AND \"AND\" AND \"b\"*"),
            ("NEAR(a b)", "\"NEAR\" AND \"a\" AND \"b\"*"),
            ("title:geheim", "\"title\" AND \"geheim\"*"),
            ("\"phrase\" OR x*", "\"phrase\" AND \"OR\" AND \"x\"*"),
            ("a\" OR \"1\"=\"1", "\"a\" AND \"OR\" AND \"1\" AND \"1\"*"),
            ("NOT -x ^y {z}", "\"NOT\" AND \"x\" AND \"y\" AND \"z\"*"),
        ] {
            let got = match_expression(q).unwrap();
            assert_eq!(got, expected, "for {q:?}");
            // The structural claim, independent of the examples: every character that is
            // not inside a quoted word is one of the three this function writes itself.
            let outside: String = got
                .split('"')
                .enumerate()
                .filter(|(i, _)| i % 2 == 0)
                .map(|(_, s)| s)
                .collect();
            assert!(
                outside.chars().all(|c| " AND*".contains(c)),
                "unquoted text in {got:?}"
            );
        }
    }

    #[test]
    fn a_long_query_is_cut_not_forwarded() {
        let many = (0..100)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            match_expression(&many).unwrap().matches(" AND ").count(),
            MAX_TERMS - 1
        );
        let huge = "x".repeat(10_000);
        assert_eq!(match_expression(&huge).unwrap().len(), MAX_TERM_CHARS + 3);
    }

    // --- finding ------------------------------------------------------------------------

    #[tokio::test]
    async fn a_page_is_found_by_its_title_and_by_its_body() {
        let store = store().await;
        let id = page(&store, "Koloskopie", "Vorbereitung am Vortag").await;
        page(&store, "Anderes", "nichts dazu").await;

        assert_eq!(ids(&store, "Koloskopie").await, vec![id.clone()]);
        assert_eq!(ids(&store, "Vortag").await, vec![id.clone()]);
        assert_eq!(ids(&store, "Vorbereitung Vortag").await, vec![id.clone()]);
        assert_eq!(
            ids(&store, "Vorb").await,
            vec![id],
            "the last word is a prefix"
        );
        assert!(ids(&store, "Gastroskopie").await.is_empty());
    }

    #[tokio::test]
    async fn umlauts_and_their_plain_letters_find_each_other() {
        let store = store().await;
        let id = page(&store, "Müller", "Größe").await;
        assert_eq!(ids(&store, "muller").await, vec![id.clone()]);
        assert_eq!(ids(&store, "MÜLLER").await, vec![id.clone()]);
        assert_eq!(
            ids(&store, "grosse").await.len(),
            0,
            "ß is not folded to ss"
        );
        assert_eq!(ids(&store, "Größe").await, vec![id]);
    }

    #[tokio::test]
    async fn a_candidate_carries_an_id_a_rank_and_a_snippet() {
        let store = store().await;
        let id = page(&store, "Notiz", "Der Polyp wurde entfernt").await;
        let hits = Fts5Index::new(store.pool.clone())
            .candidates("Polyp", 10)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, id);
        assert!(hits[0].rank < 0.0, "bm25 is negative for a match");
        assert!(
            hits[0].snippet.contains("Polyp"),
            "got {:?}",
            hits[0].snippet
        );
    }

    #[tokio::test]
    async fn the_limit_is_honoured() {
        let store = store().await;
        for i in 0..5 {
            page(&store, &format!("Seite {i}"), "gemeinsames Wort").await;
        }
        let hits = Fts5Index::new(store.pool.clone())
            .candidates("gemeinsames", 3)
            .await
            .unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[tokio::test]
    async fn a_title_outranks_a_body() {
        let store = store().await;
        let in_body = page(&store, "Alltag", "Hier steht Diabetes im Text").await;
        let in_title = page(&store, "Diabetes", "Hier steht etwas anderes").await;
        assert_eq!(ids(&store, "Diabetes").await, vec![in_title, in_body]);
    }

    // --- staying current ----------------------------------------------------------------

    #[tokio::test]
    async fn editing_a_page_changes_what_finds_it() {
        let store = store().await;
        let id = page(&store, "Notiz", "alte Wörter").await;
        let autorin = Principal::test("autorin", &[], &[]);
        store
            .add_grant(
                "/notiz",
                Subject::Principal(autorin.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        store
            .publish_revision(&autorin, &id, &body("neue Begriffe"), None)
            .await
            .unwrap()
            .expect("the writer may publish");

        assert!(
            ids(&store, "alte").await.is_empty(),
            "the old words still find it"
        );
        assert_eq!(ids(&store, "Begriffe").await, vec![id]);
    }

    #[tokio::test]
    async fn a_page_made_from_a_restored_revision_is_found_by_the_restored_words() {
        let store = store().await;
        let id = page(&store, "Notiz", "erste Fassung").await;
        let autorin = Principal::test("autorin", &[], &[]);
        store
            .add_grant(
                "/notiz",
                Subject::Principal(autorin.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        let first = store.revisions_for(&autorin, &id).await.unwrap()[0]
            .id
            .clone();
        store
            .publish_revision(&autorin, &id, &body("zweite Fassung"), None)
            .await
            .unwrap();
        assert_eq!(ids(&store, "zweite").await, vec![id.clone()]);
        store.restore_revision(&autorin, &first).await.unwrap();
        assert!(ids(&store, "zweite").await.is_empty());
        assert_eq!(ids(&store, "erste").await, vec![id]);
    }

    #[tokio::test]
    async fn renaming_a_page_changes_what_finds_it() {
        let store = store().await;
        page(&store, "Ordner", "x").await;
        let id = page_under(&store, Some("/ordner"), "Altname", "Inhalt").await;
        let autorin = Principal::test("autorin", &[], &[]);
        store
            .add_grant(
                "/ordner",
                Subject::Principal(autorin.id.clone()),
                Permission::Write,
            )
            .await
            .unwrap();
        let outcome = store
            .move_document(
                &autorin,
                "/ordner/altname",
                &MoveRequest {
                    parent: Some("/ordner".into()),
                    title: "Neuname".into(),
                    slug: None,
                },
                false,
                MoveMode::Commit,
            )
            .await
            .unwrap();
        assert!(
            matches!(outcome, MoveOutcome::Planned(ref p) if p.committed),
            "{outcome:?}"
        );

        assert!(ids(&store, "Altname").await.is_empty());
        assert_eq!(ids(&store, "Neuname").await, vec![id]);
    }

    #[tokio::test]
    async fn moving_a_page_keeps_its_hits_and_the_path_is_read_live() {
        let store = store().await;
        page(&store, "A", "x").await;
        page(&store, "B", "x").await;
        let id = page_under(&store, Some("/a"), "Beweglich", "Alleinstellungswort").await;
        let autorin = Principal::test("autorin", &[], &[]);
        for p in ["/a", "/b"] {
            store
                .add_grant(p, Subject::Principal(autorin.id.clone()), Permission::Write)
                .await
                .unwrap();
        }
        let outcome = store
            .move_document(
                &autorin,
                "/a/beweglich",
                &MoveRequest {
                    parent: Some("/b".into()),
                    title: "Beweglich".into(),
                    slug: None,
                },
                false,
                MoveMode::Commit,
            )
            .await
            .unwrap();
        assert!(
            matches!(outcome, MoveOutcome::Planned(ref p) if p.committed),
            "{outcome:?}"
        );

        let found = ids(&store, "Alleinstellungswort").await;
        assert_eq!(found, vec![id.clone()]);
        let path: String = sqlx::query_scalar("SELECT path FROM documents WHERE id = ?1")
            .bind(&found[0])
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(path, "/b/beweglich", "a hit resolves to the NEW path");
    }

    #[tokio::test]
    async fn trashing_hides_and_restoring_brings_back() {
        let store = store().await;
        let id = page(&store, "Flüchtig", "Wort").await;
        sqlx::query("UPDATE documents SET deleted_at = datetime('now'), deleted_root = path, deleted_by = 'x', deleted_by_name = 'x' WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(ids(&store, "Flüchtig").await.is_empty());
        sqlx::query("UPDATE documents SET deleted_at = NULL, deleted_root = NULL, deleted_by = NULL, deleted_by_name = NULL WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(ids(&store, "Flüchtig").await, vec![id]);
    }

    #[tokio::test]
    async fn a_purge_removes_the_page_from_the_index_itself_not_just_from_the_answer() {
        let store = store().await;
        let id = page(&store, "Vernichtet", "Geheimwort").await;
        page(&store, "Bleibt", "Geheimwort").await;
        assert_eq!(
            scalar(&store, "SELECT count(*) FROM search_pages_docsize").await,
            2
        );

        sqlx::query("UPDATE documents SET deleted_at = datetime('now'), deleted_root = path, deleted_by = 'x', deleted_by_name = 'x' WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();
        // Hidden by the filter, but still in the index: the point of the next assertion.
        assert_eq!(
            scalar(&store, "SELECT count(*) FROM search_pages_docsize").await,
            2
        );

        let outcome = store
            .purge_document("admin", "/vernichtet", Purge::Commit)
            .await
            .unwrap();
        assert!(matches!(outcome, PurgeOutcome::Done(_)), "{outcome:?}");

        // Asked of the FTS table directly, with no `deleted_at` filter in sight.
        assert_eq!(
            scalar(&store, "SELECT count(*) FROM search_pages_docsize").await,
            1
        );
        assert_eq!(
            scalar(
                &store,
                "SELECT count(*) FROM search_pages WHERE search_pages MATCH 'vernichtet'"
            )
            .await,
            0,
            "the title survived the purge in the index"
        );
        assert_eq!(
            scalar(
                &store,
                "SELECT count(*) FROM search_pages WHERE search_pages MATCH 'geheimwort'"
            )
            .await,
            1,
            "only the page that was not purged is left"
        );
        // And the index as a whole is still consistent with `documents`.
        sqlx::query("INSERT INTO search_pages (search_pages) VALUES ('integrity-check')")
            .execute(&store.pool)
            .await
            .expect("the external-content index disagrees with documents");
    }

    // --- hostile queries ----------------------------------------------------------------

    #[tokio::test]
    async fn hostile_queries_return_without_error_and_without_operator_semantics() {
        let store = store().await;
        let geheim = page(&store, "Geheim", "Passwort").await;
        page(&store, "Offen", "Hallo Welt").await;

        for q in [
            "\"",
            "\"\"",
            "*",
            "NEAR(",
            "NEAR(a b)",
            "AND",
            "OR",
            "NOT",
            "a AND",
            "title:Geheim",
            "body_text:Passwort",
            "{title}: Geheim",
            "Geheim*",
            "^Geheim",
            "(",
            ")",
            "Offen OR Geheim",
            "",
            "   ",
            "\u{0}",
            "' OR 1=1 --",
        ] {
            let found = ids(&store, q).await; // must not error
                                              // "Offen OR Geheim" would find both under operator semantics; as words, the
                                              // page must contain "OR" too, and neither does.
            if q == "Offen OR Geheim" || q == "NOT" || q == "AND" || q == "OR" {
                assert!(found.is_empty(), "{q:?} acted as an operator: {found:?}");
            }
        }
        // A column filter is a word, so it neither narrows nor widens to the secret page.
        assert!(!ids(&store, "Hallo NOT Geheim").await.contains(&geheim));
        assert_eq!(ids(&store, "Geheim*").await, vec![geheim.clone()]);
        assert!(ids(&store, "body_text:Passwort").await.is_empty());
    }

    // --- the backfill -------------------------------------------------------------------

    #[tokio::test]
    async fn pages_that_predate_the_migration_are_indexed_by_the_backfill() {
        let store = store().await;
        let a = page(&store, "Altbestand", "Vorhandener Text").await;
        let b = page(&store, "Zweiter", "Mehr Text").await;
        // The state a database is in when 0016 has just run over existing rows: the column
        // exists and is NULL, and the index knows them by title alone.
        sqlx::query("UPDATE documents SET body_text = NULL")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(ids(&store, "Vorhandener").await.is_empty());

        assert_eq!(store.backfill_search_text().await.unwrap(), 2);
        assert_eq!(ids(&store, "Vorhandener").await, vec![a.clone()]);
        assert_eq!(ids(&store, "Mehr").await, vec![b.clone()]);
        assert_eq!(ids(&store, "Altbestand").await, vec![a.clone()]);
        assert_eq!(
            scalar(
                &store,
                "SELECT count(*) FROM documents WHERE body_text IS NULL"
            )
            .await,
            0
        );

        // Idempotent: a second start finds nothing to do and breaks nothing.
        assert_eq!(store.backfill_search_text().await.unwrap(), 0);
        assert_eq!(ids(&store, "Vorhandener").await, vec![a]);
    }

    #[tokio::test]
    async fn a_body_that_cannot_be_read_is_still_found_by_its_title() {
        let store = store().await;
        let id = page(&store, "Kaputt", "x").await;
        sqlx::query("UPDATE documents SET body = 'not json', body_text = NULL WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();
        store.backfill_search_text().await.unwrap();
        assert_eq!(ids(&store, "Kaputt").await, vec![id]);
    }

    #[tokio::test]
    async fn creating_a_page_writes_its_text_with_its_body() {
        let store = store().await;
        let id = page(&store, "Notiz", "Hallo   Welt").await;
        let text: Option<String> =
            sqlx::query_scalar("SELECT body_text FROM documents WHERE id = ?1")
                .bind(&id)
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(text.as_deref(), Some("Hallo Welt"));
    }

    // --- the accessor: what a caller is handed ---------------------------------------------
    //
    // The fixture for everything below. `/offen` is public; `/geheim` is restricted.
    // `leser` holds read on `/geheim`; `fremde` is signed in and holds nothing — the invited
    // relative; `chefin` administers `/geheim`.

    async fn restricted(store: &Store, parent: Option<&str>, title: &str, text: &str) -> String {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: parent.map(Into::into),
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Restricted,
                    body: body(text),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap()
    }

    async fn account(store: &Store, name: &str) -> Principal {
        store
            .create_local_principal(name, name, None, "x")
            .await
            .unwrap()
    }

    async fn grant(store: &Store, path: &str, who: &Principal, permission: Permission) {
        store
            .add_grant(path, Subject::Principal(who.id.clone()), permission)
            .await
            .unwrap();
    }

    fn titles(results: &SearchResults) -> Vec<&str> {
        results.pages.iter().map(|p| p.title.as_str()).collect()
    }

    fn flat(snippet: &[Segment]) -> String {
        snippet.iter().map(|s| s.text.as_str()).collect()
    }

    fn trash_sql(id: &str) -> String {
        format!("UPDATE documents SET deleted_at = datetime('now'), deleted_root = path, deleted_by = 'x', deleted_by_name = 'x' WHERE id = '{id}'")
    }

    #[tokio::test]
    async fn a_page_the_caller_may_not_read_is_not_a_hit_and_one_they_may_is() {
        let store = store().await;
        page(&store, "Offen", "Rezept Linsensuppe").await;
        restricted(&store, None, "Geheim", "Rezept Passwort").await;
        let leser = account(&store, "leser").await;
        let chefin = account(&store, "chefin").await;
        let fremde = account(&store, "fremde").await;
        grant(&store, "/geheim", &leser, Permission::Read).await;
        grant(&store, "/geheim", &chefin, Permission::Admin).await;

        let anonym = Principal::anonymous();
        for who in [&fremde, &anonym] {
            let found = store.search_for(who, "Rezept", 10).await.unwrap();
            assert_eq!(titles(&found), vec!["Offen"], "for {}", who.id);
            let found = store.search_for(who, "Passwort", 10).await.unwrap();
            assert_eq!(found, SearchResults::default(), "for {}", who.id);
        }
        // Anti-vacuity: the same queries, from people who may read it.
        for who in [&leser, &chefin] {
            let found = store.search_for(who, "Passwort", 10).await.unwrap();
            assert_eq!(titles(&found), vec!["Geheim"], "for {}", who.id);
            assert_eq!(found.pages[0].path, "/geheim");
        }
    }

    #[tokio::test]
    async fn the_snippet_marks_the_match_and_carries_no_markup() {
        let store = store().await;
        page(
            &store,
            "Notiz",
            "Der Polyp wurde <b>entfernt</b> & \"geprüft\"",
        )
        .await;
        let found = store
            .search_for(&Principal::anonymous(), "polyp", 10)
            .await
            .unwrap();
        let snippet = &found.pages[0].snippet;
        assert!(
            snippet.iter().any(|s| s.hit && s.text == "Polyp"),
            "{snippet:?}"
        );
        assert_eq!(flat(snippet), "Der Polyp wurde <b>entfernt</b> & \"geprüft");
        assert!(snippet.windows(2).all(|w| w[0].hit != w[1].hit));
    }

    #[tokio::test]
    async fn a_long_text_is_cut_around_the_first_match() {
        let store = store().await;
        let words: Vec<String> = (0..60).map(|i| format!("w{i}")).collect();
        let text = format!("{} Ziel {}", words.join(" "), words.join(" "));
        page(&store, "Lang", &text).await;
        let found = store
            .search_for(&Principal::anonymous(), "ziel", 10)
            .await
            .unwrap();
        let flat = flat(&found.pages[0].snippet);
        assert!(flat.starts_with('…') && flat.ends_with('…'), "{flat}");
        assert!(flat.contains("Ziel") && flat.len() < text.len() / 2);
    }

    #[tokio::test]
    async fn the_snippet_is_the_accessors_text_never_the_indexs() {
        let store = store().await;
        let id = page(&store, "Notiz", "Alpha steht im echten Text").await;
        // The index row says something else: the state a drifted or poisoned index is in.
        sqlx::query("UPDATE documents SET body_text = 'Alpha ENTSTELLT' WHERE id = ?1")
            .bind(&id)
            .execute(&store.pool)
            .await
            .unwrap();
        assert_eq!(ids(&store, "ENTSTELLT").await, vec![id], "the index has it");

        let found = store
            .search_for(&Principal::anonymous(), "Alpha", 10)
            .await
            .unwrap();
        let text = flat(&found.pages[0].snippet);
        assert!(text.contains("echten Text"), "{text}");
        assert!(!text.contains("ENTSTELLT"), "{text}");
    }

    #[tokio::test]
    async fn the_loop_stops_at_the_limit_of_visible_hits() {
        let store = store().await;
        for i in 0..5 {
            page(&store, &format!("Seite {i}"), "gemeinsam").await;
        }
        let anonym = Principal::anonymous();
        let n = |limit| {
            let store = &store;
            let anonym = &anonym;
            async move {
                store
                    .search_for(anonym, "gemeinsam", limit)
                    .await
                    .unwrap()
                    .pages
                    .len()
            }
        };
        assert_eq!(n(2).await, 2);
        assert_eq!(n(0).await, 0);
        assert_eq!(n(500).await, 5);
    }

    #[tokio::test]
    async fn withheld_pages_ranking_first_do_not_use_up_the_limit() {
        let store = store().await;
        // Three withheld pages that outrank (title match) three readable ones (body match).
        for i in 0..3 {
            restricted(&store, None, &format!("Dienst {i}"), "x").await;
            page(&store, &format!("Seite {i}"), "Dienst im Text").await;
        }
        let fremde = account(&store, "fremde").await;
        let found = store.search_for(&fremde, "Dienst", 3).await.unwrap();
        assert_eq!(
            found.pages.len(),
            3,
            "the over-fetch reaches past the withheld"
        );
        assert!(found.pages.iter().all(|p| p.title.starts_with("Seite")));
    }

    #[tokio::test]
    async fn trashed_pages_do_not_use_up_the_over_fetch() {
        let store = store().await;
        for i in 0..5 {
            let id = page(&store, &format!("Mull {i}"), "x").await;
            sqlx::query(&trash_sql(&id))
                .execute(&store.pool)
                .await
                .unwrap();
        }
        page(&store, "Seite", "Mull im Text").await;
        // Limit 1 fetches 5: if the index did not drop the trashed, they would fill it.
        let found = store
            .search_for(&Principal::anonymous(), "Mull", 1)
            .await
            .unwrap();
        assert_eq!(titles(&found), vec!["Seite"]);
    }

    #[tokio::test]
    async fn a_moved_page_is_a_hit_at_its_new_address() {
        let store = store().await;
        page(&store, "A", "x").await;
        page(&store, "B", "x").await;
        page_under(&store, Some("/a"), "Beweglich", "Alleinstellungswort").await;
        let autorin = Principal::test("autorin", &[], &[]);
        for p in ["/a", "/b"] {
            grant(&store, p, &autorin, Permission::Write).await;
        }
        store
            .move_document(
                &autorin,
                "/a/beweglich",
                &MoveRequest {
                    parent: Some("/b".into()),
                    title: "Beweglich".into(),
                    slug: None,
                },
                false,
                MoveMode::Commit,
            )
            .await
            .unwrap();
        let found = store
            .search_for(&Principal::anonymous(), "Alleinstellungswort", 10)
            .await
            .unwrap();
        assert_eq!(found.pages[0].path, "/b/beweglich");
    }

    #[tokio::test]
    async fn unsearchable_queries_are_the_empty_answer() {
        let store = store().await;
        page(&store, "Offen", "Hallo").await;
        let anonym = Principal::anonymous();
        let long = "a".repeat(MAX_QUERY_CHARS + 1);
        for q in ["", "   ", "***", "\"", "()", "\u{0}", long.as_str()] {
            assert_eq!(
                store.search_for(&anonym, q, 10).await.unwrap(),
                SearchResults::default(),
                "for {q:?}"
            );
        }
        let edge = "a".repeat(MAX_QUERY_CHARS);
        store.search_for(&anonym, &edge, 10).await.unwrap();
        for q in [
            "NEAR(",
            "title:Offen",
            "Offen OR",
            "Hallo*",
            "'; DROP TABLE documents; --",
        ] {
            store.search_for(&anonym, q, 10).await.unwrap();
        }
    }

    // --- topics and tasks ----------------------------------------------------------------

    async fn topic_page(store: &Store, vis: Visibility, title: &str, topic: &str) {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: vis,
                    body: body("x"),
                    sort_key: 0,
                    topics: vec![topic.into()],
                },
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn a_topic_is_found_by_its_folded_name_only_where_a_readable_page_is_under_it() {
        let store = store().await;
        topic_page(&store, Visibility::Public, "Offen", "Medizin/Größe").await;
        topic_page(
            &store,
            Visibility::Restricted,
            "Geheim",
            "Kündigung Mietvertrag",
        )
        .await;
        let fremde = account(&store, "fremde").await;

        let found = store.search_for(&fremde, "GRÖßE", 10).await.unwrap();
        assert_eq!(found.topics.len(), 1, "{:?}", found.topics);
        let hit = &found.topics[0];
        assert_eq!(hit.name, "Größe");
        assert_eq!(hit.display_path, "Medizin/Größe");
        assert_eq!(hit.documents, 1);
        assert!(hit.path.starts_with("/medizin/"));
        let found = store.search_for(&fremde, "medizin", 10).await.unwrap();
        assert!(found.topics.iter().any(|t| t.display_path == "Medizin"));

        // The restricted topic's name is the disclosure.
        for q in ["kundigung", "Kündigung Mietvertrag", "mietvertrag"] {
            let found = store.search_for(&fremde, q, 10).await.unwrap();
            assert!(found.topics.is_empty(), "{q}: {:?}", found.topics);
        }
        // Anti-vacuity: a reader of that page is shown it, folded.
        let leser = account(&store, "leser").await;
        grant(&store, "/geheim", &leser, Permission::Read).await;
        let found = store.search_for(&leser, "kundigung", 10).await.unwrap();
        assert_eq!(found.topics.len(), 1);
    }

    async fn task_fixture() -> (Store, Principal, Principal) {
        let store = store().await;
        let offen = page(&store, "Offen", "x").await;
        let geheim = restricted(&store, None, "Geheim", "x").await;
        let chef = account(&store, "chef").await;
        let fremde = account(&store, "fremde").await;
        grant(&store, "/offen", &chef, Permission::Write).await;
        grant(&store, "/geheim", &chef, Permission::Write).await;
        for (doc, title) in [(offen, "Offene Aufgabe"), (geheim, "Geheime Aufgabe")] {
            let outcome = store
                .create_task(
                    &chef,
                    &NewTask {
                        home: TaskHome::Anchored {
                            doc_id: doc,
                            block_id: None,
                        },
                        title: title.into(),
                        status: TaskStatus::Laeuft,
                        assignee: None,
                        due_at: None,
                        position: 0,
                    },
                )
                .await
                .unwrap();
            assert!(matches!(outcome, TaskOutcome::Done(_)), "{outcome:?}");
        }
        (store, chef, fremde)
    }

    #[tokio::test]
    async fn a_task_is_found_by_its_title_only_on_a_board_the_caller_may_read() {
        let (store, chef, fremde) = task_fixture().await;
        let found = store.search_for(&fremde, "aufgabe", 10).await.unwrap();
        assert_eq!(found.tasks.len(), 1, "{:?}", found.tasks);
        let hit = &found.tasks[0];
        assert_eq!(hit.title, "Offene Aufgabe");
        assert_eq!(hit.status, TaskStatus::Laeuft);
        assert_eq!(hit.page_path.as_deref(), Some("/offen"));
        assert!(!hit.id.is_empty());
        let found = store.search_for(&fremde, "geheime", 10).await.unwrap();
        assert!(found.tasks.is_empty());
        // Every word must match, not one of them: "Geheime Aufgabe" shares `Aufgabe` with the
        // card this caller may see, and must not be answered with it.
        let found = store
            .search_for(&fremde, "Geheime Aufgabe", 10)
            .await
            .unwrap();
        assert!(found.tasks.is_empty(), "{:?}", found.tasks);
        let found = store
            .search_for(&fremde, "Offene Aufgabe", 10)
            .await
            .unwrap();
        assert_eq!(found.tasks.len(), 1);
        // Anti-vacuity: the person who may read it finds it.
        let found = store.search_for(&chef, "geheime", 10).await.unwrap();
        assert_eq!(found.tasks.len(), 1);
        assert_eq!(found.tasks[0].page_path.as_deref(), Some("/geheim"));
    }

    #[tokio::test]
    async fn a_standalone_task_names_no_page() {
        let store = store().await;
        page(&store, "Projekt", "x").await;
        let chef = account(&store, "chef").await;
        grant(&store, "/projekt", &chef, Permission::Write).await;
        let project = store
            .create_project(&chef, "/projekt", None)
            .await
            .unwrap()
            .unwrap();
        store
            .create_task(
                &chef,
                &NewTask {
                    home: TaskHome::Standalone {
                        project_id: project.id,
                    },
                    title: "Lose Karte".into(),
                    status: TaskStatus::Offen,
                    assignee: None,
                    due_at: None,
                    position: 0,
                },
            )
            .await
            .unwrap();
        let found = store.search_for(&chef, "karte", 10).await.unwrap();
        assert_eq!(found.tasks.len(), 1);
        assert_eq!(found.tasks[0].page_path, None);
    }

    #[test]
    fn a_result_has_no_key_that_could_count_what_it_hid() {
        let value = serde_json::to_value(SearchResults {
            pages: vec![PageHit {
                title: "t".into(),
                path: "/p".into(),
                snippet: vec![Segment {
                    text: "x".into(),
                    hit: true,
                }],
            }],
            topics: vec![TopicHit {
                name: "n".into(),
                display_path: "n".into(),
                path: "/n".into(),
                documents: 1,
            }],
            tasks: vec![TaskHit {
                id: "i".into(),
                title: "t".into(),
                status: TaskStatus::Offen,
                page_path: None,
            }],
        })
        .unwrap();
        fn keys(v: &serde_json::Value, out: &mut Vec<String>) {
            match v {
                serde_json::Value::Object(m) => {
                    for (k, v) in m {
                        out.push(k.clone());
                        keys(v, out);
                    }
                }
                serde_json::Value::Array(a) => a.iter().for_each(|v| keys(v, out)),
                _ => {}
            }
        }
        let mut all = Vec::new();
        keys(&value, &mut all);
        all.sort();
        all.dedup();
        assert_eq!(
            all,
            vec![
                "display_path",
                "documents",
                "hit",
                "id",
                "name",
                "page_path",
                "pages",
                "path",
                "snippet",
                "status",
                "tasks",
                "text",
                "title",
                "topics"
            ]
        );
    }

    // --- review follow-ups ---------------------------------------------------------------

    async fn internal(store: &Store, title: &str, text: &str) {
        store
            .create_document(
                Author::Import,
                &NewDocument {
                    parent_path: None,
                    doc_type: DocumentType::Page,
                    title: title.into(),
                    slug: None,
                    language: "de".into(),
                    visibility: Visibility::Internal,
                    body: body(text),
                    sort_key: 0,
                    topics: Vec::new(),
                },
                None,
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn an_internal_page_is_found_by_members_of_the_internal_group_only() {
        let store = store().await;
        internal(&store, "Hausordnung", "Ruhezeiten beachten").await;
        let fremde = account(&store, "fremde").await;
        let mitglied = Principal::test("mitglied", &["users"], &[]);

        for who in [&fremde, &Principal::anonymous()] {
            let found = store.search_for(who, "Ruhezeiten", 10).await.unwrap();
            assert_eq!(found, SearchResults::default(), "for {}", who.id);
        }
        let found = store.search_for(&mitglied, "Ruhezeiten", 10).await.unwrap();
        assert_eq!(titles(&found), vec!["Hausordnung"]);
    }

    #[tokio::test]
    async fn a_topic_counts_only_the_pages_under_it_that_the_caller_may_read() {
        let store = store().await;
        topic_page(&store, Visibility::Public, "Offen", "Gemischt").await;
        topic_page(&store, Visibility::Restricted, "Intern", "Gemischt").await;
        let fremde = account(&store, "fremde").await;
        let leser = account(&store, "leser").await;
        grant(&store, "/intern", &leser, Permission::Read).await;

        let fremde_sees = store.search_for(&fremde, "gemischt", 10).await.unwrap();
        assert_eq!(fremde_sees.topics[0].documents, 1);
        let leser_sees = store.search_for(&leser, "gemischt", 10).await.unwrap();
        assert_eq!(leser_sees.topics[0].documents, 2);
    }

    #[tokio::test]
    async fn every_word_of_the_query_must_be_in_a_topic_name() {
        let store = store().await;
        topic_page(&store, Visibility::Public, "Offen", "Mietvertrag Wohnung").await;
        topic_page(
            &store,
            Visibility::Restricted,
            "Geheim",
            "Kündigung Mietvertrag",
        )
        .await;
        let fremde = account(&store, "fremde").await;
        let found = store
            .search_for(&fremde, "Kündigung Mietvertrag", 10)
            .await
            .unwrap();
        assert!(found.topics.is_empty(), "{:?}", found.topics);
        let found = store.search_for(&fremde, "Mietvertrag", 10).await.unwrap();
        assert_eq!(found.topics.len(), 1);
    }

    #[tokio::test]
    async fn the_order_of_hits_does_not_depend_on_pages_the_caller_cannot_read() {
        // Two readable pages that differ only in which query word they repeat. bm25 would put
        // them in an order that depends on how common each word is across the whole index,
        // withheld pages included; the order handed out must be the same with or without them.
        async fn order(withheld: usize) -> Vec<String> {
            let store = store().await;
            page(&store, "Pe", "alpha alpha alpha beta").await;
            page(&store, "Qu", "alpha beta beta beta").await;
            for i in 0..withheld {
                restricted(&store, None, &format!("Zeug {i}"), "alpha").await;
            }
            let fremde = account(&store, "fremde").await;
            store
                .search_for(&fremde, "alpha beta", 10)
                .await
                .unwrap()
                .pages
                .into_iter()
                .map(|p| p.path)
                .collect()
        }
        assert_eq!(order(0).await, vec!["/pe", "/qu"]);
        assert_eq!(order(30).await, vec!["/pe", "/qu"]);
    }

    #[tokio::test]
    async fn a_title_match_ranks_above_a_text_match_and_more_mentions_above_fewer() {
        let store = store().await;
        page(&store, "Alltag", "Diabetes Diabetes Diabetes").await;
        page(&store, "Diabetes", "nichts").await;
        page(&store, "Ratgeber", "Diabetes einmal").await;
        let found = store
            .search_for(&Principal::anonymous(), "diabetes", 10)
            .await
            .unwrap();
        let paths: Vec<&str> = found.pages.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(paths, vec!["/diabetes", "/alltag", "/ratgeber"]);
    }

    #[tokio::test]
    async fn however_many_are_asked_for_at_most_the_maximum_comes_back() {
        let store = store().await;
        for i in 0..(MAX_LIMIT + 5) {
            page(&store, &format!("Seite {i}"), "vielfach").await;
        }
        let found = store
            .search_for(&Principal::anonymous(), "vielfach", 500)
            .await
            .unwrap();
        assert_eq!(found.pages.len(), MAX_LIMIT);
    }

    #[tokio::test]
    async fn a_decomposed_query_finds_what_the_composed_one_does() {
        let store = store().await;
        page(&store, "Müller", "x").await;
        let anonym = Principal::anonymous();
        let composed = store.search_for(&anonym, "Müller", 10).await.unwrap();
        let decomposed = store
            .search_for(&anonym, "Mu\u{308}ller", 10)
            .await
            .unwrap();
        assert_eq!(composed.pages.len(), 1);
        assert_eq!(decomposed, composed);
        assert_eq!(
            match_expression("Mu\u{308}ller"),
            match_expression("Müller")
        );
    }

    #[tokio::test]
    async fn characters_the_tokenizer_calls_separators_are_not_searched_for() {
        let store = store().await;
        page(&store, "Darm", "Polypen").await;
        let anonym = Principal::anonymous();
        assert_eq!(match_expression("ⓐ"), None);
        assert_eq!(match_expression("Darm ⓐ").as_deref(), Some("\"Darm\"*"));
        for q in ["ⓐ", "Ⓐⓐ", "🄰", "\u{301}"] {
            assert_eq!(
                store.search_for(&anonym, q, 10).await.unwrap(),
                SearchResults::default(),
                "for {q:?}"
            );
        }
        let found = store.search_for(&anonym, "Darm ⓐ", 10).await.unwrap();
        assert_eq!(titles(&found), vec!["Darm"]);
    }

    #[test]
    fn a_word_cut_for_length_keeps_a_prefix_star_wherever_it_is() {
        let long = "x".repeat(MAX_TERM_CHARS + 20);
        let cut = "x".repeat(MAX_TERM_CHARS);
        assert_eq!(
            match_expression(&format!("{long} b")).unwrap(),
            format!("\"{cut}\"* AND \"b\"*")
        );
        assert_eq!(
            match_expression(&format!("a {cut}")).unwrap(),
            format!("\"a\" AND \"{cut}\"*")
        );
    }

    #[tokio::test]
    async fn opening_a_store_repairs_an_index_whose_rowids_no_longer_match() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite://{}", dir.path().join("wiki.db").display());
        let store = Store::open(&url).await.unwrap();
        let id = page(&store, "Notiz", "Erinnerungswort").await;
        assert_eq!(ids(&store, "Erinnerungswort").await, vec![id.clone()]);

        // What a VACUUM may do to a table with a TEXT primary key.
        sqlx::query("UPDATE documents SET rowid = rowid + 1000")
            .execute(&store.pool)
            .await
            .unwrap();
        assert!(
            ids(&store, "Erinnerungswort").await.is_empty(),
            "the mapping is broken, as set up"
        );
        store.pool.close().await;

        let reopened = Store::open(&url).await.unwrap();
        assert_eq!(ids(&reopened, "Erinnerungswort").await, vec![id]);
    }
}
