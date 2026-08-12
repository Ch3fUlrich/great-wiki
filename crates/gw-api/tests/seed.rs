//! End-to-end tests for `great-wiki seed`, against a real (in-memory) store and a real
//! temporary directory. They exercise `seed::run`, the same entry point the binary calls,
//! so a rule enforced only in `main` would not pass this suite.

use gw_api::seed::{self, SeedReport};
use gw_auth::{Action, Principal};
use gw_store::{Store, StoredDocument};
use std::path::Path;
use tempfile::TempDir;

/// A principal in `admins`, which D-M2-1 gives read reach over everything.
///
/// These tests are about what reached the database, not about who may see it — but they
/// still read through the permission-scoped accessor, because no code outside `gw-store`
/// can obtain an unfiltered document or tree. Asserting on seeded content is not a reason
/// to make an exception; it is a reason to name a principal.
fn admin() -> Principal {
    Principal::test("seed-test", &["admins"], &[])
}

async fn fetch(store: &Store, path: &str) -> Option<StoredDocument> {
    store
        .document_for(&admin(), path, Action::Read)
        .await
        .unwrap()
}

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
    assert_eq!(report.applied.len(), 1);

    // German end to end: umlauts in the title, an ASCII path in the database.
    let doc = fetch(&store, "/groesse-und-mass")
        .await
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
    assert_eq!(report.applied[0].path, "/handbuch", "shallowest first");
    assert_eq!(report.applied[1].path, "/handbuch/erste-schritte");

    let child = fetch(&store, "/handbuch/erste-schritte")
        .await
        .expect("the child must exist");
    assert_eq!(child.parent_path.as_deref(), Some("/handbuch"));
}

#[tokio::test]
async fn a_missing_title_is_reported_with_the_filename_never_guessed() {
    let dir = corpus(&[("ohne-titel.md", "---\ntype: page\n---\nText.\n")]);
    let (store, report) = seed(dir.path()).await;

    assert_eq!(report.applied.len(), 0);
    assert!(!report.is_complete(), "a skipped file must fail the run");

    let reason = reason_for(&report, "ohne-titel.md");
    assert!(reason.contains("ohne-titel.md"), "{reason}");
    assert!(reason.contains("title"), "{reason}");
    // The filename must not have become the title.
    assert!(!store.document_exists("/ohne-titel").await.unwrap());
}

#[tokio::test]
async fn a_file_with_no_visibility_is_restricted_not_public() {
    // AGENTS.md rule 3. A forgotten field must never publish something.
    let dir = corpus(&[("notiz.md", "---\ntitle: Notiz\n---\nGeheim genug.\n")]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = fetch(&store, "/notiz").await.unwrap();
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

    assert_eq!(report.applied.len(), 1);
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
    let doc = fetch(&store, "/notiz").await.unwrap();
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

    assert_eq!(report.applied.len(), 0);
    let reason = reason_for(&report, "handbuch/erste-schritte.md");
    assert!(reason.contains("/handbuch"), "{reason}");
    assert!(reason.contains("parent"), "{reason}");

    // No placeholder parent was created.
    assert!(!store.document_exists("/handbuch").await.unwrap());
    assert!(store.tree_for(&admin()).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_table_reaches_the_database_as_a_table_not_as_paragraphs() {
    let dir = corpus(&[(
        "tabelle.md",
        "---\ntitle: Tabelle\n---\n\n| Feld | Wert |\n| ---: | --- |\n| Größe | 42 |\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = fetch(&store, "/tabelle").await.unwrap();
    let body: gw_core::Block = serde_json::from_str(&doc.body).unwrap();

    let table = &body.content[0];
    assert_eq!(table.kind, gw_core::BlockKind::Table);
    assert_eq!(table.content.len(), 2, "a header row and one body row");
    assert_eq!(
        table.content[0].content[0].kind,
        gw_core::BlockKind::TableHeader
    );
    assert_eq!(
        table.content[1].content[0]
            .attrs
            .get("align")
            .and_then(|v| v.as_str()),
        Some("right"),
        "alignment must survive the store, or a numeric column arrives ragged"
    );

    // Exact, not `contains`: a substring check passes on the fused token "FeldWert" too,
    // and a fused token is in the index matching nothing anyone would search for.
    assert_eq!(body.plain_text(), "Feld Wert Größe 42");
    assert!(
        report.notes.is_empty(),
        "a table is no longer a lossy conversion: {report}"
    );
}

#[tokio::test]
async fn a_body_heading_that_only_repeats_the_title_does_not_reach_the_database() {
    // The reader renders the frontmatter title as the page's `h1`. A file that also opens
    // with `# Title` would show it twice — on the page and in the outline.
    let dir = corpus(&[(
        "notiz.md",
        "---\ntitle: Größe und Maß\n---\n\n# Größe und Maß\n\nEin Satz.\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = fetch(&store, "/groesse-und-mass").await.unwrap();
    let body: gw_core::Block = serde_json::from_str(&doc.body).unwrap();

    assert_eq!(doc.title, "Größe und Maß", "the title itself is untouched");
    assert!(
        body.headings().is_empty(),
        "the duplicate heading must be gone from the outline too: {:?}",
        body.headings()
    );
    assert_eq!(body.plain_text(), "Ein Satz.");
}

#[tokio::test]
async fn a_body_heading_that_differs_from_the_title_is_kept() {
    let dir = corpus(&[(
        "notiz.md",
        "---\ntitle: Handbuch\n---\n\n# Erste Schritte\n\nEin Satz.\n",
    )]);
    let (store, report) = seed(dir.path()).await;

    assert!(report.is_complete(), "{report}");
    let doc = fetch(&store, "/handbuch").await.unwrap();
    let body: gw_core::Block = serde_json::from_str(&doc.body).unwrap();
    assert_eq!(body.headings()[0].text, "Erste Schritte");
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
    let doc = fetch(&store, "/reich").await.unwrap();
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
    let doc = fetch(&store, "/notiz").await.unwrap();
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
    assert!(
        summary.contains("1 created, 0 updated, 0 unchanged, 1 skipped"),
        "{summary}"
    );
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
    assert_eq!(report.applied.len(), 0);
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
    assert!(report.applied.len() >= 4, "{report}");
    assert!(
        !report.notes.iter().any(|n| n.detail.contains("table")),
        "the corpus contains a table and tables are modelled now; a note here means the \
         converter went back to flattening one: {report}"
    );
    assert!(
        !store.tree_for(&admin()).await.unwrap().is_empty(),
        "the example corpus must produce a navigable tree"
    );
}

// ---------------------------------------------------------------------------------------
// Updating: the opt-in that lets a file change a page that already exists
// ---------------------------------------------------------------------------------------

use gw_api::seed::{Options, Outcome};
use gw_auth::{Permission, Subject};

/// An account holding exactly `permission` on `path`, and nothing else.
///
/// Every fixture here carries an explicit grant because writing is only ever an explicit
/// grant (D-M2-8): no baseline confers it, not even the admin one — which is why the
/// `admin()` principal above can read the whole corpus and still not update a line of it.
async fn granted(store: &Store, username: &str, path: &str, permission: Permission) -> Principal {
    let principal = Principal::test(username, &[], &[]);
    store
        .add_grant(path, Subject::Principal(principal.id.clone()), permission)
        .await
        .unwrap();
    principal
}

fn options<'a>(principal: &'a Principal, update: bool) -> Options<'a> {
    Options {
        principal: Some(principal),
        update,
    }
}

/// A store holding `files`, loaded by the identity-less operator path.
async fn loaded(files: &[(&str, &str)]) -> Store {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let dir = corpus(files);
    let report = seed::run(&store, dir.path()).await.unwrap();
    assert!(
        report.is_complete(),
        "the first pass must load cleanly: {report}"
    );
    store
}

/// Import `files` again, as `principal`, with updating on or off.
async fn again(store: &Store, files: &[(&str, &str)], who: &Principal, update: bool) -> SeedReport {
    let dir = corpus(files);
    seed::run_as(store, dir.path(), options(who, update))
        .await
        .unwrap()
}

/// The account these tests update as: an explicit `write` grant on the page, and nothing
/// else. `autorin` because "author" is what a revision records.
async fn autorin(store: &Store, path: &str) -> Principal {
    granted(store, "autorin", path, Permission::Write).await
}

#[tokio::test]
async fn without_the_flag_an_existing_page_is_still_refused_not_overwritten() {
    // The default has not moved: `insert_document`'s UNIQUE constraint stays the authority
    // and a collision is an error. Updating had to be a separate, named act, not a relaxed
    // constraint.
    let store = loaded(&[("notiz.md", "---\ntitle: Notiz\n---\nAlt.\n")]).await;
    let who = autorin(&store, "/notiz").await;
    let report = again(
        &store,
        &[("notiz.md", "---\ntitle: Notiz\n---\nNeu.\n")],
        &who,
        false,
    )
    .await;

    assert!(!report.is_complete(), "{report}");
    assert_eq!(report.count(Outcome::Updated), 0);
    assert!(reason_for(&report, "notiz.md").contains("already exists"));
    assert!(fetch(&store, "/notiz").await.unwrap().body.contains("Alt."));
}

#[tokio::test]
async fn with_the_flag_the_body_is_updated_and_the_old_one_stays_in_the_history() {
    let store = loaded(&[("notiz.md", "---\ntitle: Notiz\n---\nAlt.\n")]).await;
    let who = autorin(&store, "/notiz").await;
    let report = again(
        &store,
        &[("notiz.md", "---\ntitle: Notiz\n---\nNeu.\n")],
        &who,
        true,
    )
    .await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.count(Outcome::Updated), 1, "{report}");
    assert_eq!(report.count(Outcome::Created), 0, "{report}");

    let doc = fetch(&store, "/notiz").await.unwrap();
    assert!(doc.body.contains("Neu."), "{}", doc.body);

    // "Update" must not mean "replace": the previous text is still readable, attributed,
    // and restorable. That is the whole difference between this and `ON CONFLICT DO UPDATE`.
    let autorin = Principal::test("autorin", &[], &[]);
    let history = store.revisions_for(&autorin, &doc.id).await.unwrap();
    assert_eq!(history.len(), 1, "one revision for one change");
    assert!(history[0].body.contains("Neu."));
    assert_eq!(history[0].author_name, "autorin");
    assert!(
        history[0]
            .summary
            .as_deref()
            .is_some_and(|s| s.contains("notiz.md")),
        "the history must say where the change came from: {:?}",
        history[0].summary
    );
}

#[tokio::test]
async fn a_file_that_says_what_the_page_already_holds_writes_nothing_at_all() {
    // A no-op revision per file per run buries the real edits in a timeline nobody can
    // then read — and re-importing an unchanged export is the common case, not the rare one.
    let same = "---\ntitle: Notiz\n---\nGleich.\n";
    let store = loaded(&[("notiz.md", same)]).await;
    let who = autorin(&store, "/notiz").await;
    let report = again(&store, &[("notiz.md", same)], &who, true).await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.count(Outcome::Unchanged), 1, "{report}");
    assert_eq!(report.count(Outcome::Updated), 0, "{report}");

    let doc = fetch(&store, "/notiz").await.unwrap();
    let autorin = Principal::test("autorin", &[], &[]);
    assert!(
        store
            .revisions_for(&autorin, &doc.id)
            .await
            .unwrap()
            .is_empty(),
        "an unchanged page must not appear in its own history"
    );
}

#[tokio::test]
async fn an_import_updates_the_body_and_refuses_to_republish_the_page() {
    // The dangerous field. A stray `visibility: public` in a bulk file drop must not be
    // able to publish a page nobody meant to publish — so the body lands, the visibility
    // does not, and the report says so in as many words.
    let store = loaded(&[("notiz.md", "---\ntitle: Notiz\n---\nAlt.\n")]).await;
    let who = autorin(&store, "/notiz").await;
    let report = again(
        &store,
        &[(
            "notiz.md",
            "---\ntitle: Notiz\nvisibility: public\nsort_key: 9\n---\nNeu.\n",
        )],
        &who,
        true,
    )
    .await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.count(Outcome::Updated), 1, "{report}");

    let doc = fetch(&store, "/notiz").await.unwrap();
    assert!(doc.body.contains("Neu."), "the body must still update");
    assert_eq!(
        doc.visibility, "restricted",
        "a file drop published a page: {report}"
    );
    assert_eq!(doc.sort_key, 0, "and moved it in the navigation: {report}");

    let notes = report
        .notes
        .iter()
        .map(|n| n.detail.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(notes.contains("visibility"), "{notes}");
    assert!(notes.contains("sort_key"), "{notes}");
    assert!(
        notes.contains("NOT"),
        "the note must say the field was refused, not merely that it differs: {notes}"
    );
}

#[tokio::test]
async fn an_account_that_may_read_but_not_write_cannot_update() {
    // The permission-checked path is the point: an import edits through exactly the check
    // a person editing in the browser goes through, and reading a page never implies
    // editing it.
    let store = loaded(&[(
        "notiz.md",
        "---\ntitle: Notiz\nvisibility: public\n---\nAlt.\n",
    )])
    .await;
    let who = granted(&store, "leserin", "/notiz", Permission::Read).await;
    let report = again(
        &store,
        &[(
            "notiz.md",
            "---\ntitle: Notiz\nvisibility: public\n---\nNeu.\n",
        )],
        &who,
        true,
    )
    .await;

    assert!(!report.is_complete(), "{report}");
    assert_eq!(report.count(Outcome::Updated), 0, "{report}");
    let reason = reason_for(&report, "notiz.md");
    assert!(reason.contains("leserin"), "{reason}");
    assert!(reason.contains("may not write"), "{reason}");
    assert!(
        fetch(&store, "/notiz").await.unwrap().body.contains("Alt."),
        "the refused write must not have landed anyway"
    );
}

#[tokio::test]
async fn the_admin_baseline_alone_does_not_let_an_import_rewrite_a_page() {
    // D-M2-8: no baseline confers writing. An account that can read the whole wiki — which
    // is exactly the account somebody would reach for to run a bulk import — still cannot
    // change a line of it without an explicit grant.
    let store = loaded(&[("notiz.md", "---\ntitle: Notiz\n---\nAlt.\n")]).await;
    let chefin = Principal::test("chefin", &["admins"], &[]);
    let report = again(
        &store,
        &[("notiz.md", "---\ntitle: Notiz\n---\nNeu.\n")],
        &chefin,
        true,
    )
    .await;

    assert!(!report.is_complete(), "{report}");
    assert!(reason_for(&report, "notiz.md").contains("may not write"));
    assert!(fetch(&store, "/notiz").await.unwrap().body.contains("Alt."));
}

#[tokio::test]
async fn updating_with_nobody_to_attribute_it_to_is_refused_before_any_file_is_read() {
    let dir = corpus(&[("notiz.md", "---\ntitle: Notiz\n---\nText.\n")]);
    let store = Store::open("sqlite::memory:").await.unwrap();
    let err = seed::run_as(
        &store,
        dir.path(),
        Options {
            principal: None,
            update: true,
        },
    )
    .await
    .expect_err("an update with no author must not run");
    assert!(err.to_string().contains("attribute"), "{err}");
    assert!(
        !store.document_exists("/notiz").await.unwrap(),
        "the run must stop before it writes anything, not part-way through"
    );
}

#[tokio::test]
async fn a_page_in_the_wiki_that_no_file_claims_is_reported_and_left_alone() {
    // Deleting is a different verb. A directory missing a file — a partial export, a
    // `git checkout` gone sideways — must never be able to empty a wiki.
    let notiz = "---\ntitle: Notiz\nvisibility: public\n---\nText.\n";
    let store = loaded(&[
        ("notiz.md", notiz),
        (
            "alt.md",
            "---\ntitle: Alt\nvisibility: public\n---\nBleibt.\n",
        ),
        ("geheim.md", "---\ntitle: Geheim\n---\nNicht sichtbar.\n"),
    ])
    .await;
    let who = autorin(&store, "/notiz").await;
    let report = again(&store, &[("notiz.md", notiz)], &who, true).await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.absent, vec!["/alt".to_string()], "{report}");
    assert!(
        store.document_exists("/alt").await.unwrap(),
        "the page must still be there"
    );
    assert!(
        report.to_string().contains("nothing was deleted"),
        "the report must say what it did not do: {report}"
    );

    // `/geheim` is restricted and this account has no grant on it, so it is missing from
    // the list too — a list of paths is a disclosure, and "you have not covered this page"
    // is exactly as much of one as showing the page would be.
    assert!(
        store.document_exists("/geheim").await.unwrap(),
        "and it is certainly still there"
    );
    assert!(
        !report.absent.iter().any(|p| p == "/geheim"),
        "a page the account cannot read must not be named back to it: {report}"
    );
}

#[tokio::test]
async fn a_run_with_no_account_reports_nothing_absent_because_it_can_see_nothing() {
    // The absent list is a list of paths, and a list of paths is a disclosure. Without an
    // identity there is nobody to filter it for, so it is empty rather than complete.
    let dir = corpus(&[("notiz.md", "---\ntitle: Notiz\n---\nText.\n")]);
    let store = Store::open("sqlite::memory:").await.unwrap();
    seed::run(&store, dir.path()).await.unwrap();

    let empty = corpus(&[]);
    let report = seed::run(&store, empty.path()).await.unwrap();
    assert!(report.absent.is_empty(), "{report}");
}

#[tokio::test]
async fn creating_a_child_needs_write_on_the_branch_it_hangs_under() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let first = corpus(&[(
        "handbuch.md",
        "---\ntitle: Handbuch\nvisibility: public\n---\nText.\n",
    )]);
    seed::run(&store, first.path()).await.unwrap();

    let second = corpus(&[
        (
            "handbuch.md",
            "---\ntitle: Handbuch\nvisibility: public\n---\nText.\n",
        ),
        (
            "handbuch/neu.md",
            "---\ntitle: Neu\nvisibility: public\n---\nText.\n",
        ),
    ]);

    // A reader may not extend the branch.
    let leserin = granted(&store, "leserin", "/handbuch", Permission::Read).await;
    let report = seed::run_as(&store, second.path(), options(&leserin, true))
        .await
        .unwrap();
    let reason = reason_for(&report, "handbuch/neu.md");
    assert!(reason.contains("may not write its parent"), "{reason}");
    assert!(!store.document_exists("/handbuch/neu").await.unwrap());

    // A writer may.
    let autorin = granted(&store, "autorin", "/handbuch", Permission::Write).await;
    let report = seed::run_as(&store, second.path(), options(&autorin, true))
        .await
        .unwrap();
    assert!(report.is_complete(), "{report}");
    assert_eq!(report.count(Outcome::Created), 1, "{report}");
    assert!(store.document_exists("/handbuch/neu").await.unwrap());
}

#[tokio::test]
async fn creating_a_top_level_page_under_an_account_is_refused_rather_than_guessed() {
    // Nothing in this system says who may create a top-level page: there is no parent to
    // check write access on and no permission-checked create anywhere. A rule invented here
    // would be a second, weaker answer sitting next to the real one, so there is none.
    let store = Store::open("sqlite::memory:").await.unwrap();
    let dir = corpus(&[("neu.md", "---\ntitle: Neu\n---\nText.\n")]);
    let chefin = Principal::test("chefin", &["admins"], &[]);

    let report = seed::run_as(&store, dir.path(), options(&chefin, true))
        .await
        .unwrap();
    assert!(!report.is_complete(), "{report}");
    let reason = reason_for(&report, "neu.md");
    assert!(reason.contains("TOP-LEVEL"), "{reason}");
    assert!(!store.document_exists("/neu").await.unwrap());

    // …and the operator path — no account at all — is unchanged and still works, which is
    // how a wiki gets bootstrapped in the first place.
    let report = seed::run(&store, dir.path()).await.unwrap();
    assert!(report.is_complete(), "{report}");
    assert!(store.document_exists("/neu").await.unwrap());
}

#[tokio::test]
async fn a_file_holding_formatting_the_database_cannot_store_says_so_on_an_update_too() {
    // The trap this whole milestone is about. A person edits an exported file, adds bold
    // and a link, and imports it back. The text lands; the emphasis and the destination do
    // not, because `Block` has nowhere to put them. That has to be said out loud on the way
    // in, every time — a silent degradation is the one outcome that cannot be found later.
    let store = loaded(&[(
        "notiz.md",
        "---\ntitle: Notiz\nvisibility: public\n---\nSchlicht.\n",
    )])
    .await;
    let who = autorin(&store, "/notiz").await;
    let report = again(
        &store,
        &[(
            "notiz.md",
            "---\ntitle: Notiz\nvisibility: public\n---\nEin **fettes** Wort und \
             [ein Verweis](/handbuch).\n",
        )],
        &who,
        true,
    )
    .await;

    assert_eq!(report.count(Outcome::Updated), 1, "{report}");
    let notes = report
        .notes
        .iter()
        .map(|n| n.detail.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(notes.contains("inline-marks"), "{notes}");
    assert!(notes.contains("emphasis dropped"), "{notes}");
    assert!(notes.contains("destination dropped"), "{notes}");
}

#[tokio::test]
async fn a_page_whose_file_was_skipped_is_not_also_reported_as_missing() {
    // Found by running the real command: a file that IS in the directory but could not be
    // applied was named twice — once as a skip, once as "in the wiki, not in this
    // directory". The second line is false and points at the wrong fix, which is exactly
    // how somebody ends up deleting a file to make a message go away.
    let notiz = "---\ntitle: Notiz\nvisibility: public\n---\nAlt.\n";
    let store = loaded(&[("notiz.md", notiz)]).await;
    let who = granted(&store, "leserin", "/notiz", Permission::Read).await;

    let report = again(
        &store,
        &[(
            "notiz.md",
            "---\ntitle: Notiz\nvisibility: public\n---\nNeu.\n",
        )],
        &who,
        true,
    )
    .await;

    assert_eq!(report.skipped.len(), 1, "{report}");
    assert!(
        report.absent.is_empty(),
        "the skipped file's page is present in the directory: {report}"
    );
}
