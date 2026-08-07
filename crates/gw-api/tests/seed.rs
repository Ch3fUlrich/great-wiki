//! End-to-end tests for `great-wiki seed`, against a real (in-memory) store and a real
//! temporary directory. They exercise `seed::run`, the same entry point the binary calls,
//! so a rule enforced only in `main` would not pass this suite.

use gw_api::seed::{self, SeedReport};
use gw_store::Store;
use std::path::Path;
use tempfile::TempDir;

/// Write `files` into a fresh temporary directory, creating parents as needed.
fn corpus(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, contents) in files {
        let path = dir.path().join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }
    dir
}

async fn seed(dir: &Path) -> (Store, SeedReport) {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let report = seed::run(&store, dir).await.unwrap();
    (store, report)
}

fn reason_for(report: &SeedReport, file: &str) -> String {
    report
        .skipped
        .iter()
        .find(|s| s.file.to_string_lossy() == file)
        .unwrap_or_else(|| panic!("`{file}` should have been skipped: {report}"))
        .reason
        .clone()
}

#[tokio::test]
async fn a_document_lands_at_the_path_its_title_slugifies_to() {
    let dir = corpus(&[(
        "groesse.md",
        "---\ntitle: Größe und Maß\nvisibility: public\n---\n\n# Größe und Maß\n\nEin Satz.\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.inserted.len(), 1);

    // German end to end: umlauts in the title, an ASCII path in the database.
    let doc = store
        .document_by_path("/groesse-und-mass")
        .await
        .unwrap()
        .expect("a German title must produce a transliterated path");
    assert_eq!(doc.title, "Größe und Maß");
    assert_eq!(doc.visibility, "public");
    assert!(doc.body.contains("Ein Satz."));
}

#[tokio::test]
async fn a_child_is_nested_under_the_document_its_directory_names() {
    // The child is written first and sorts first alphabetically; only depth ordering
    // makes its parent exist by the time it is considered.
    let dir = corpus(&[
        (
            "handbuch/erste-schritte.md",
            "---\ntitle: Erste Schritte\n---\nHallo.\n",
        ),
        ("handbuch.md", "---\ntitle: Handbuch\n---\nÜbersicht.\n"),
    ]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.inserted[0].path, "/handbuch", "shallowest first");
    assert_eq!(report.inserted[1].path, "/handbuch/erste-schritte");

    let child = store
        .document_by_path("/handbuch/erste-schritte")
        .await
        .unwrap()
        .expect("the child must exist");
    assert_eq!(child.parent_path.as_deref(), Some("/handbuch"));
}

#[tokio::test]
async fn a_missing_title_is_reported_with_the_filename_never_guessed() {
    let dir = corpus(&[("ohne-titel.md", "---\ntype: page\n---\nText.\n")]);
    let (store, report) = seed(dir.path()).await;

    assert_eq!(report.inserted.len(), 0);
    assert!(!report.is_complete(), "a skipped file must fail the run");

    let reason = reason_for(&report, "ohne-titel.md");
    assert!(reason.contains("ohne-titel.md"), "{reason}");
    assert!(reason.contains("title"), "{reason}");
    // The filename must not have become the title.
    assert!(store
        .document_by_path("/ohne-titel")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn a_file_with_no_visibility_is_restricted_not_public() {
    // AGENTS.md rule 3. A forgotten field must never publish something.
    let dir = corpus(&[("notiz.md", "---\ntitle: Notiz\n---\nGeheim genug.\n")]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = store.document_by_path("/notiz").await.unwrap().unwrap();
    assert_eq!(
        doc.visibility, "restricted",
        "a document with no stated visibility must not be world-readable"
    );
}

#[tokio::test]
async fn two_files_claiming_one_path_name_both_sides() {
    let dir = corpus(&[
        ("a-notiz.md", "---\ntitle: Notiz\n---\nEins.\n"),
        ("b-notiz.md", "---\ntitle: Notiz\n---\nZwei.\n"),
    ]);
    let (store, report) = seed(dir.path()).await;

    assert_eq!(report.inserted.len(), 1);
    assert_eq!(report.skipped.len(), 1);

    let reason = reason_for(&report, "b-notiz.md");
    assert!(
        reason.contains("/notiz"),
        "the path must be named: {reason}"
    );
    assert!(
        reason.contains("a-notiz.md"),
        "the file that won must be named too, or the collision is unfixable: {reason}"
    );

    // The winner is untouched: a collision must never overwrite.
    let doc = store.document_by_path("/notiz").await.unwrap().unwrap();
    assert!(doc.body.contains("Eins."));
}

#[tokio::test]
async fn a_path_already_in_the_database_is_reported_not_overwritten() {
    let dir = corpus(&[("notiz.md", "---\ntitle: Notiz\n---\nNeu.\n")]);
    let store = Store::open("sqlite::memory:").await.unwrap();

    let first = seed::run(&store, dir.path()).await.unwrap();
    assert!(first.is_complete(), "{first}");

    // Seeding is not idempotent by design: a second run must say so rather than
    // silently replacing revisions the database is the source of truth for.
    let second = seed::run(&store, dir.path()).await.unwrap();
    assert!(!second.is_complete());
    let reason = reason_for(&second, "notiz.md");
    assert!(reason.contains("/notiz"), "{reason}");
    assert!(reason.contains("already exists"), "{reason}");
}

#[tokio::test]
async fn a_child_without_a_parent_document_is_skipped_not_given_an_invented_one() {
    let dir = corpus(&[(
        "handbuch/erste-schritte.md",
        "---\ntitle: Erste Schritte\n---\nHallo.\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert_eq!(report.inserted.len(), 0);
    let reason = reason_for(&report, "handbuch/erste-schritte.md");
    assert!(reason.contains("/handbuch"), "{reason}");
    assert!(reason.contains("parent"), "{reason}");

    // No placeholder parent was created.
    assert!(store.document_by_path("/handbuch").await.unwrap().is_none());
    assert!(store.tree().await.unwrap().is_empty());
}

#[tokio::test]
async fn table_text_reaches_the_database_and_the_flattening_is_reported() {
    let dir = corpus(&[(
        "tabelle.md",
        "---\ntitle: Tabelle\n---\n\n| Feld | Wert |\n| --- | --- |\n| Größe | 42 |\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = store.document_by_path("/tabelle").await.unwrap().unwrap();
    let body: gw_core::Block = serde_json::from_str(&doc.body).unwrap();
    // Exact, not `contains`: a substring check passes on the fused token "FeldWert" too,
    // and a fused token is in the index matching nothing anyone would search for.
    assert_eq!(body.plain_text(), "Feld Wert Größe 42");
    assert!(
        report.notes.iter().any(|n| n.detail.contains("table")),
        "a lossy conversion must be reported: {report}"
    );
}

#[tokio::test]
async fn headings_lists_quotes_and_code_all_survive() {
    let dir = corpus(&[(
        "reich.md",
        "---\ntitle: Reich\n---\n\n## Abschnitt\n\n- eins\n- zwei\n\n\
         > Ein Zitat.\n\n```rust\nfn main() {}\n```\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = store.document_by_path("/reich").await.unwrap().unwrap();
    let body: gw_core::Block = serde_json::from_str(&doc.body).unwrap();

    let kinds: Vec<gw_core::BlockKind> = body.content.iter().map(|b| b.kind).collect();
    assert_eq!(
        kinds,
        vec![
            gw_core::BlockKind::Heading,
            gw_core::BlockKind::BulletList,
            gw_core::BlockKind::Blockquote,
            gw_core::BlockKind::CodeBlock,
        ]
    );
    assert_eq!(body.headings()[0].text, "Abschnitt");
    let text = body.plain_text();
    for fragment in ["Abschnitt", "eins", "zwei", "Ein Zitat.", "fn main() {}"] {
        assert!(text.contains(fragment), "`{fragment}` was lost: {text}");
    }
}

#[tokio::test]
async fn an_unread_frontmatter_key_is_reported_rather_than_silently_ignored() {
    // A misspelled `visibilty:` fails closed, which is safe but invisible. Saying so is
    // what turns a silent default into a fixable mistake.
    let dir = corpus(&[(
        "notiz.md",
        "---\ntitle: Notiz\nvisibilty: public\n---\nText.\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    assert!(
        report.notes.iter().any(|n| n.detail.contains("visibilty")),
        "{report}"
    );
    let doc = store.document_by_path("/notiz").await.unwrap().unwrap();
    assert_eq!(doc.visibility, "restricted");
}

#[tokio::test]
async fn the_summary_names_every_skipped_file_and_its_reason() {
    let dir = corpus(&[
        ("gut.md", "---\ntitle: Gut\n---\nText.\n"),
        ("schlecht.md", "kein frontmatter\n"),
    ]);
    let (_store, report) = seed(dir.path()).await;

    let summary = report.to_string();
    assert!(summary.contains("1 inserted, 1 skipped"), "{summary}");
    assert!(summary.contains("/gut"), "{summary}");

    let skip_line = summary
        .lines()
        .find(|l| l.contains("SKIPPED"))
        .expect("the skipped file must have its own line");
    assert!(skip_line.contains("schlecht.md"), "{skip_line}");
    assert_eq!(
        skip_line.matches("schlecht.md").count(),
        1,
        "the filename must appear once, not once from the line and once from the reason"
    );
}

#[tokio::test]
async fn an_empty_directory_succeeds_with_nothing_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let (_store, report) = seed(dir.path()).await;
    assert!(report.is_complete());
    assert_eq!(report.inserted.len(), 0);
}

#[tokio::test]
async fn a_missing_content_directory_is_an_error_not_an_empty_success() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let err = seed::run(&store, Path::new("/definitiv/nicht/da"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("/definitiv/nicht/da"), "{err}");
}

#[tokio::test]
async fn the_shipped_example_corpus_seeds_cleanly() {
    // `content-example/` is what makes a fresh clone runnable, so CI must notice the day
    // someone adds a file to it that the seeder cannot load.
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content-example")
        .canonicalize()
        .expect("content-example must exist at the repository root");
    let (store, report) = seed(&dir).await;

    assert!(report.is_complete(), "{report}");
    assert!(report.inserted.len() >= 4, "{report}");
    assert!(
        !store.tree().await.unwrap().is_empty(),
        "the example corpus must produce a navigable tree"
    );
}
