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

use crate::Store;
use anyhow::Result;
use sqlx::SqlitePool;

/// How many words of a query are used. A query is a few words; a pasted paragraph would
/// otherwise become a MATCH expression with hundreds of terms for no better result.
const MAX_TERMS: usize = 12;

/// How long one word of a query may be, in characters. No indexed word is longer in
/// practice; the cap keeps a hostile megabyte-long "word" out of the query planner.
const MAX_TERM_CHARS: usize = 64;

/// One document the index thinks matches. **Unfiltered**: it has not been through any
/// permission check, and a trashed page is the only thing already excluded.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(test), allow(dead_code))] // consumed by `Store::search_pages_for` (Task 2)
pub(crate) struct Candidate {
    pub doc_id: String,
    /// `bm25()` — lower is better, as SQLite defines it. Candidates arrive best first.
    pub rank: f64,
    /// An excerpt of the index's own text, unmarked. See the module doc for why a response
    /// must not use it.
    pub snippet: String,
}

/// Something that can say which documents match a query.
#[cfg_attr(not(test), allow(dead_code))] // consumed by `Store::search_pages_for` (Task 2)
pub(crate) trait SearchIndex {
    /// At most `limit` candidates, best first. A query with nothing searchable in it is
    /// an empty answer, not an error — the caller cannot tell it from "no match", which is
    /// what stops the endpoint being an oracle for what a query was parsed as.
    async fn candidates(&self, query: &str, limit: usize) -> Result<Vec<Candidate>>;
}

/// The SQLite FTS5 implementation (ADR 0003): the index lives in the same database as the
/// documents, kept current by triggers (`0016_search.sql`).
#[cfg_attr(not(test), allow(dead_code))] // consumed by `Store::search_pages_for` (Task 2)
pub(crate) struct Fts5Index {
    pool: SqlitePool,
}

#[cfg_attr(not(test), allow(dead_code))] // consumed by `Store::search_pages_for` (Task 2)
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
pub(crate) fn match_expression(query: &str) -> Option<String> {
    let words: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(MAX_TERMS)
        .map(|w| w.chars().take(MAX_TERM_CHARS).collect())
        .collect();
    let last = words.len().checked_sub(1)?;
    Some(
        words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == last {
                    format!("\"{w}\"*")
                } else {
                    format!("\"{w}\"")
                }
            })
            .collect::<Vec<_>>()
            .join(" AND "),
    )
}

impl Store {
    /// Give every page that predates `0016_search.sql` its `body_text`, then rebuild the
    /// index from `documents`. Run by [`Store::open`]; returns how many rows it filled.
    ///
    /// **Why open-time Rust and not SQL in the migration:** the flattener is
    /// `gw_core::body_plain_text`, and a second implementation in SQL would be a second rule
    /// about where words end. **Why `rebuild` as well:** the migration indexed these rows by
    /// title alone and the triggers have kept the index consistent since, so this is belt and
    /// braces — but it is the one command that repairs an index that has drifted for any
    /// reason, and it costs milliseconds at this corpus size, only when something was filled.
    /// **Idempotent:** with nothing to fill, nothing is touched. A row a future writer inserts
    /// without `body_text` is picked up at the next start rather than staying invisible for
    /// ever.
    pub(crate) async fn backfill_search_text(&self) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id, body FROM documents WHERE body_text IS NULL")
                .fetch_all(&mut *tx)
                .await?;
        if rows.is_empty() {
            return Ok(0);
        }
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
    use crate::{Author, NewDocument};
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
}
