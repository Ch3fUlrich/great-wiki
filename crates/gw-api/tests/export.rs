//! End-to-end tests for `great-wiki export`, against a real (in-memory) store and a real
//! temporary directory. They exercise `export::run`, the same entry point the binary calls.
//!
//! The central one is `the_example_corpus_survives_export_then_seed`: it seeds the shipped
//! corpus, exports it, seeds the export into a second, empty store, and compares the two
//! databases field by field. That is the claim the command makes, checked against a corpus
//! with nested folders, umlauts in titles and slugs, aligned tables, empty cells, deeply
//! nested lists and a restricted page — not against a fixture written to pass.

use gw_api::export::{self, ExportReport, FIDELITY_FILE};
use gw_api::seed;
use gw_auth::{Action, Principal};
use gw_core::{Block, DocumentType, Visibility};
use gw_store::{Author, NewDocument, Store};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// No reference resolves to anything, which is the honest state for a test with no store
/// behind it: a reference is then written bare, with no fallback path beside it. The
/// fallback is a property of the EXPORT RUN — `Store::references_for`'s answer for the
/// account doing it — and inventing one here would be a second, unfiltered answer.
fn no_paths() -> export::ReferencePaths {
    export::ReferencePaths::new()
}

/// In `admins`, which D-M2-1 gives read reach over everything — including the corpus's
/// deliberately restricted page, which a lesser account would silently leave out.
fn admin() -> Principal {
    Principal::test("export-test", &["admins"], &[])
}

/// A local account with no groups: public pages and nothing else.
fn guest() -> Principal {
    Principal::test("gast", &[], &[])
}

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content-example")
        .canonicalize()
        .expect("content-example must exist at the repository root")
}

async fn seeded(dir: &Path) -> Store {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let report = seed::run(&store, dir).await.unwrap();
    assert!(report.is_complete(), "{report}");
    store
}

/// Every document `principal` can see, as `path -> everything that document is`.
///
/// The body is compared as its parsed JSON rather than as the stored string: two identical
/// trees may differ by key order, and a test that failed on that would be noise, while a
/// test comparing only `plain_text` would call a table flattened into paragraphs equal.
async fn snapshot(store: &Store, principal: &Principal) -> BTreeMap<String, serde_json::Value> {
    fn walk(nodes: &[gw_store::TreeNode], out: &mut Vec<String>) {
        for node in nodes {
            out.push(node.path.clone());
            walk(&node.children, out);
        }
    }
    let mut paths = Vec::new();
    walk(&store.tree_for(principal).await.unwrap(), &mut paths);

    let mut out = BTreeMap::new();
    for path in paths {
        let doc = store
            .document_for(principal, &path, Action::Read)
            .await
            .unwrap()
            .expect("the tree listed it, so it is readable");
        // Topics travel in frontmatter, so they are part of what "the same document came
        // back" means. `display_path` is the string the file states, and comparing it is
        // what would catch an export that dropped one, spelled one differently, or
        // reordered them — any of which re-imports as a page filed under something else.
        let topics: Vec<String> = store
            .document_topics_for(principal, &path)
            .await
            .unwrap()
            .expect("the tree listed it, so it is readable")
            .iter()
            .map(|t| t.display_path.clone())
            .collect();
        out.insert(
            path,
            serde_json::json!({
                "title": doc.title,
                "type": doc.doc_type,
                "visibility": doc.visibility,
                "language": doc.language,
                "sort_key": doc.sort_key,
                "slug": doc.slug,
                "parent_path": doc.parent_path,
                "body": serde_json::from_str::<serde_json::Value>(&doc.body).unwrap(),
                "topics": topics,
            }),
        );
    }
    out
}

/// An account that really exists in `store` and may write `path` — what publishing needs.
async fn signed_in_writer(store: &Store, path: &str) -> Principal {
    let username = "autorin";
    store
        .create_local_principal(username, "Autorin", None, "$argon2id$fake")
        .await
        .unwrap();
    let (principal, _) = store
        .principal_by_username(username)
        .await
        .unwrap()
        .unwrap();
    store
        .add_grant(
            path,
            gw_auth::Subject::Principal(principal.id.clone()),
            gw_auth::Permission::Write,
        )
        .await
        .unwrap();
    principal
}

async fn export_to(store: &Store, principal: &Principal, dir: &Path) -> ExportReport {
    export::run(store, principal, dir).await.unwrap()
}

// --- the round trip ------------------------------------------------------------------

#[tokio::test]
async fn the_example_corpus_survives_export_then_seed() {
    let source = seeded(&example_dir()).await;
    let before = snapshot(&source, &admin()).await;
    assert!(
        before.len() >= 6,
        "the corpus must have real shape to prove anything: {}",
        before.len()
    );

    let out = tempfile::tempdir().unwrap();
    let report = export_to(&source, &admin(), out.path()).await;
    assert!(report.is_complete(), "{report}");
    assert_eq!(
        report.written.len(),
        before.len(),
        "every readable document must reach a file: {report}"
    );

    // Re-importing the export reports NOTHING lossy. That is a stronger statement than the
    // comparison below: the seeder notes every construct it cannot model, and the original
    // corpus produces dozens of those lines (bold, italic, links). An export that produces
    // none is an export already expressed entirely in what `Block` can hold — so a second,
    // third and hundredth cycle cannot degrade further.
    let reloaded = Store::open("sqlite::memory:").await.unwrap();
    let reload = seed::run(&reloaded, out.path()).await.unwrap();
    assert!(reload.is_complete(), "{reload}");
    assert!(
        reload.notes.is_empty(),
        "re-importing an export must lose nothing: {reload}"
    );
    let after = snapshot(&reloaded, &admin()).await;

    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the tree changed shape"
    );
    for (path, expected) in &before {
        assert_eq!(
            &after[path], expected,
            "`{path}` came back different after export → seed"
        );
    }
}

#[tokio::test]
async fn the_export_lays_the_tree_out_as_folders() {
    let source = seeded(&example_dir()).await;
    let out = tempfile::tempdir().unwrap();
    export_to(&source, &admin(), out.path()).await;

    // A child of `/rundgang` is a file inside a `rundgang/` directory — that is the whole
    // mechanism by which `seed` rebuilds the tree, so it is asserted rather than assumed.
    assert!(out.path().join("rundgang.md").is_file());
    assert!(out.path().join("rundgang").is_dir());

    // Every file is named by the SLUG the database holds, never by the title and never by
    // whatever the source file happened to be called: `seed` reads a file's own name only
    // to derive its directory, so a filename that is not the slug would import the page at
    // one path and export it to another. ASCII-only, so a macOS/Linux Unicode-normalisation
    // difference cannot move a page either.
    for (path, file) in paths_and_files(&source, &admin()).await {
        assert!(
            out.path().join(&file).is_file(),
            "`{path}` should be at `{}`",
            file.display()
        );
        assert!(
            file.to_string_lossy().is_ascii(),
            "`{}` is not ASCII",
            file.display()
        );
    }
    assert!(out
        .path()
        .join("rundgang/groesse-und-mass-deutsch-im-system.md")
        .is_file());
}

/// Each document's path paired with the file it must export to.
async fn paths_and_files(store: &Store, principal: &Principal) -> Vec<(String, PathBuf)> {
    snapshot(store, principal)
        .await
        .into_keys()
        .map(|path| {
            let mut file = PathBuf::from(path.trim_start_matches('/'));
            file.set_extension("md");
            (path, file)
        })
        .collect()
}

#[tokio::test]
async fn a_second_export_over_the_first_is_idempotent_byte_for_byte() {
    // If two exports of an unchanged wiki differ, the output cannot be kept in git and
    // every diff is noise. Compared over every file rather than a chosen one, since the
    // instability would be in whichever file the choice missed.
    let source = seeded(&example_dir()).await;
    let out = tempfile::tempdir().unwrap();

    export_to(&source, &admin(), out.path()).await;
    let first = contents(out.path(), &source).await;
    export_to(&source, &admin(), out.path()).await;
    let second = contents(out.path(), &source).await;

    assert_eq!(first, second);
    assert!(first.values().any(|c| c.contains('|')), "{first:?}");
}

/// Every exported file's text, keyed by its name.
async fn contents(dir: &Path, source: &Store) -> BTreeMap<PathBuf, String> {
    let mut out = BTreeMap::new();
    for (_, file) in paths_and_files(source, &admin()).await {
        let text = std::fs::read_to_string(dir.join(&file)).unwrap();
        out.insert(file, text);
    }
    out
}

#[tokio::test]
async fn re_seeding_an_export_reports_every_page_unchanged() {
    // The strongest statement the pair can make: export, then import the export back over
    // the SAME wiki with updating allowed, and nothing is written at all.
    let store = seeded(&example_dir()).await;
    let out = tempfile::tempdir().unwrap();
    export_to(&store, &admin(), out.path()).await;

    let report = seed::run_as(
        &store,
        out.path(),
        seed::Options {
            principal: Some(&admin()),
            update: true,
        },
    )
    .await
    .unwrap();

    assert!(report.is_complete(), "{report}");
    assert_eq!(report.count(seed::Outcome::Updated), 0, "{report}");
    assert_eq!(report.count(seed::Outcome::Created), 0, "{report}");
    assert_eq!(
        report.count(seed::Outcome::Unchanged),
        report.applied.len(),
        "{report}"
    );
    assert!(report.absent.is_empty(), "{report}");
}

// --- permissions ---------------------------------------------------------------------

#[tokio::test]
async fn the_export_holds_only_what_the_account_may_read() {
    // AGENTS.md rule 2: filtering happens in the retriever, not afterwards. A restricted
    // page must not reach a file just because the file is on the operator's own disk.
    let source = seeded(&example_dir()).await;

    let everything = paths_and_files(&source, &admin()).await;
    let out = tempfile::tempdir().unwrap();
    let report = export_to(&source, &guest(), out.path()).await;

    assert!(
        !out.path().join("rundgang/nur-intern.md").exists(),
        "the restricted page reached a file: {report}"
    );
    assert_eq!(
        report.written.len(),
        everything.len() - 1,
        "exactly the one restricted page must be missing, and nothing else: {report}"
    );
    for (path, file) in everything {
        if path == "/rundgang/nur-intern" {
            continue;
        }
        assert!(
            out.path().join(&file).exists(),
            "the public pages must still be there: {report}"
        );
    }
    assert!(
        report.to_string().contains("gast"),
        "the report must name who it ran as: {report}"
    );
}

// --- refusing to degrade -------------------------------------------------------------

#[tokio::test]
async fn a_document_markdown_cannot_hold_is_refused_and_no_file_is_written() {
    // A table cell holding two paragraphs is legal in the block tree and impossible in
    // GFM. Exporting it would silently fuse them, and the page would come back with one
    // paragraph where the author put two.
    let store = Store::open("sqlite::memory:").await.unwrap();
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"table","content":[
             {"kind":"tableRow","content":[{"kind":"tableHeader","content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"Feld"}]}]}]},
             {"kind":"tableRow","content":[{"kind":"tableCell","content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"erster"}]},
               {"kind":"paragraph","content":[{"kind":"text","text":"zweiter"}]}]}]}]}]}"#,
    )
    .unwrap();
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: "Zwei Absätze in einer Zelle".into(),
                slug: None,
                language: "de".into(),
                visibility: Visibility::Public,
                body,
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();

    let out = tempfile::tempdir().unwrap();
    let report = export_to(&store, &admin(), out.path()).await;

    assert!(
        !report.is_complete(),
        "a document that cannot be written must fail the run: {report}"
    );
    assert_eq!(report.written.len(), 0, "{report}");
    assert_eq!(report.refused.len(), 1, "{report}");
    let reason = &report.refused[0].reason;
    assert!(
        reason.contains("/zwei-absaetze-in-einer-zelle"),
        "the refusal must name the page: {reason}"
    );
    assert!(
        reason.contains("cell"),
        "the refusal must say what could not be written: {reason}"
    );
    assert!(
        std::fs::read_dir(out.path())
            .unwrap()
            .filter_map(Result::ok)
            .all(|e| e.file_name() != "zwei-absaetze-in-einer-zelle.md"),
        "a refused document must leave NO file, or the next `seed` loads the mangled copy"
    );
}

#[tokio::test]
async fn a_heading_that_repeats_the_title_is_refused_rather_than_dropped_on_the_way_back() {
    // `seed` deletes a leading `# Title` that only repeats the frontmatter title. A body
    // holding one — which only the editor can produce, since the importer strips it — would
    // therefore export happily and come back one heading shorter.
    let store = Store::open("sqlite::memory:").await.unwrap();
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"heading","attrs":{"level":1},
              "content":[{"kind":"text","text":"Notiz"}]},
             {"kind":"paragraph","content":[{"kind":"text","text":"Text."}]}]}"#,
    )
    .unwrap();
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: "Notiz".into(),
                slug: None,
                language: "de".into(),
                visibility: Visibility::Public,
                body,
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap();

    let out = tempfile::tempdir().unwrap();
    let report = export_to(&store, &admin(), out.path()).await;
    assert!(!report.is_complete(), "{report}");
    assert!(
        report.refused[0].reason.contains("DIFFERENT"),
        "{}",
        report.refused[0].reason
    );
}

// --- not overwriting anybody's writing -----------------------------------------------

#[tokio::test]
async fn exporting_over_hand_written_markdown_is_refused() {
    // The single most expensive mistake available: `export --content content-example`.
    // Those files hold bold and links the database never kept; an export would replace
    // them with a copy that has neither.
    let store = seeded(&example_dir()).await;
    let out = tempfile::tempdir().unwrap();
    std::fs::write(
        out.path().join("handbuch.md"),
        "---\ntitle: Handbuch\n---\n",
    )
    .unwrap();

    let err = export::run(&store, &admin(), out.path())
        .await
        .expect_err("a directory holding somebody's markdown must not be exported over");
    let message = err.to_string();
    assert!(message.contains("refusing"), "{message}");
    assert!(
        std::fs::read_to_string(out.path().join("handbuch.md"))
            .unwrap()
            .contains("Handbuch"),
        "the existing file must be untouched"
    );
}

#[tokio::test]
async fn a_previous_export_may_be_written_over_and_leftovers_are_reported_not_deleted() {
    let store = seeded(&example_dir()).await;
    let out = tempfile::tempdir().unwrap();
    export_to(&store, &admin(), out.path()).await;

    // A file from an older export whose page has since been renamed or removed.
    std::fs::write(out.path().join("alt.md"), "---\ntitle: Alt\n---\nText.\n").unwrap();
    let report = export_to(&store, &admin(), out.path()).await;

    assert!(report.is_complete(), "{report}");
    assert_eq!(
        report.stale.len(),
        1,
        "the leftover must be reported: {report}"
    );
    assert_eq!(report.stale[0].file, PathBuf::from("alt.md"));
    assert!(
        out.path().join("alt.md").exists(),
        "export must never delete: removing a page and removing its file are two decisions"
    );
}

// --- the warning that has to outlive the terminal ------------------------------------

#[tokio::test]
async fn the_export_leaves_its_fidelity_warning_in_the_directory() {
    let store = seeded(&example_dir()).await;
    let out = tempfile::tempdir().unwrap();
    let report = export_to(&store, &admin(), out.path()).await;

    let note = std::fs::read_to_string(out.path().join(FIDELITY_FILE))
        .expect("the warning must be written next to the files, not only to the terminal");
    for fragment in ["bold", "italic", "links", "NOT", "Do not overwrite"] {
        assert!(note.contains(fragment), "{note}");
    }
    assert!(
        report.to_string().contains("Do not overwrite"),
        "and the run must say it too: {report}"
    );

    // It must not become a page: `seed` walks `.md` files, so the warning has to be named
    // something the walk ignores, or every export grows a page nobody wrote.
    assert!(!FIDELITY_FILE.ends_with(".md"));
    let reloaded = seeded(out.path()).await;
    assert!(!reloaded.document_exists("/export-readme").await.unwrap());
}

// --- degenerate cases ----------------------------------------------------------------

#[tokio::test]
async fn an_empty_wiki_exports_an_empty_directory_and_succeeds() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let out = tempfile::tempdir().unwrap();
    let report = export_to(&store, &admin(), out.path()).await;
    assert!(report.is_complete(), "{report}");
    assert_eq!(report.written.len(), 0);
}

#[tokio::test]
async fn the_export_directory_is_created_when_it_does_not_exist() {
    let store = seeded(&example_dir()).await;
    let parent = TempDir::new().unwrap();
    let out = parent.path().join("neu/tiefer");
    let report = export_to(&store, &admin(), &out).await;
    assert!(report.is_complete(), "{report}");
    assert!(out.join("rundgang.md").is_file());
}

// --- the seam a handler calls ---------------------------------------------------------

#[test]
fn one_document_can_be_rendered_without_a_store_or_a_filesystem() {
    // The MCP server the owner has chosen will sit on an HTTP API, not on a directory
    // walk. `run` is this function plus a walk; a handler answering "show me this page as
    // markdown" calls it with a `StoredDocument`'s own fields and gets the same bytes, the
    // same frontmatter and the same refusal.
    let meta = export::FileMeta {
        title: "Größe und Maß".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 3,
        slug: "groesse-und-mass".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph",
             "content":[{"kind":"text","text":"Ein Satz."}]}]}"#,
    )
    .unwrap();

    let file =
        export::render_file(&meta, &body, &no_paths()).expect("this document is expressible");
    assert!(file.starts_with("---\n"), "{file}");
    assert!(file.contains("title: Größe und Maß"), "{file}");
    assert!(file.contains("slug: groesse-und-mass"), "{file}");
    assert!(file.ends_with("Ein Satz.\n"), "{file}");
    assert_eq!(
        export::file_for("/rundgang/groesse-und-mass"),
        PathBuf::from("rundgang/groesse-und-mass.md")
    );

    // And it refuses on the same terms, without a store to refuse into.
    let broken: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph",
             "content":[{"kind":"text","text":"eine\nzweite Zeile"}]}]}"#,
    )
    .unwrap();
    assert!(export::render_file(&meta, &broken, &no_paths())
        .unwrap_err()
        .contains("line break"));
}

#[test]
fn a_link_carrying_the_editors_own_attributes_still_exports() {
    // C1, and the reason this test is on the Rust side of a bug whose cause is in the web
    // side: the refusal happens HERE. `web/src/lib/editor/extensions.ts` now trims the Link
    // mark's declared attributes to `href` alone, so nothing new is written this way — but a
    // Y.Doc and every revision filed before that trim already hold the full set, and a
    // document the exporter refuses is a document the owner cannot back up, forever. The
    // whole run fails on one such page (`main.rs` turns a non-empty `refused` into a
    // `bail!`), so a single link written by the shipped editor made the entire wiki
    // unexportable.
    //
    // The attribute set is TipTap `Link`'s own, read out of the installed
    // `@tiptap/extension-link@3.30.0`'s `addAttributes()`: `target` and `rel` default to the
    // extension's `HTMLAttributes`, `class` and `title` default to `null`. `computeAttrs`
    // fills every one of them in, `marksToAttributes` writes the whole map onto the wire and
    // `gw-collab::attrs_to_marks` copies it verbatim into `Mark::attrs`.
    let meta = export::FileMeta {
        title: "Verweise".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "verweise".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"Siehe "},
             {"kind":"text","text":"die Anleitung","marks":[{"kind":"link","attrs":{
               "href":"https://example.org","target":"_blank",
               "rel":"noopener noreferrer nofollow","class":null,"title":null}}]},
             {"kind":"text","text":"."}]}]}"#,
    )
    .unwrap();

    let file = export::render_file(&meta, &body, &no_paths()).unwrap_or_else(|e| {
        panic!("a link written by the editor must still export, and this one did not: {e}")
    });
    assert!(
        file.contains("Siehe [die Anleitung](https://example.org)."),
        "the link must reach the file with its address intact: {file}"
    );

    // And the tolerance is exactly that wide: it forgives attributes markdown never carried
    // in the first place, it does not forgive a link with no address to write.
    let no_href: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"intern","marks":[{"kind":"link","attrs":{
               "doc":"019ff0","target":"_blank"}}]}]}]}"#,
    )
    .unwrap();
    assert!(
        export::render_file(&meta, &no_href, &no_paths())
            .unwrap_err()
            .contains("href"),
        "a link the renderer cannot address must still be refused, not exported as bare text"
    );
}

#[test]
fn a_task_carrying_the_id_the_store_minted_still_exports() {
    // The same shape of bug as `a_link_carrying_the_editors_own_attributes_still_exports`,
    // and reached from the other end. A task block carries a uuid in `attrs` — minted by the
    // store on publish, and by the editor when somebody types a new checkbox line — because
    // that uuid is what ties the line to its record on the board: its status, its assignee
    // and its due date. `gw_core::markdown` deliberately mints none, so the markdown this
    // renderer writes re-imports as `{checked}` alone while the stored block says
    // `{checked, id}`.
    //
    // Without a reduction those two trees differ and `render_file` refuses the page — and
    // `run()` pushes a `Refused` and CONTINUES, so ONE checkbox anywhere in the wiki would
    // leave that page quietly missing from every export directory from then on, under a
    // `FIDELITY` file calling the directory a faithful copy of the database — for a
    // difference that is not a difference in the document at all. The id is database identity; markdown has no
    // spelling for it and never will.
    let meta = export::FileMeta {
        title: "Einkauf".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "einkauf".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"taskList","content":[
             {"kind":"taskItem","attrs":{"checked":false,
               "id":"0199c0de-0000-7000-8000-000000000001"},"content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"Milch kaufen"}]}]},
             {"kind":"taskItem","attrs":{"checked":true,
               "id":"0199c0de-0000-7000-8000-000000000002"},"content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"Brot geholt"}]}]}]}]}"#,
    )
    .unwrap();

    let file = export::render_file(&meta, &body, &no_paths()).unwrap_or_else(|e| {
        panic!("a task the store has reconciled must still export, and this one did not: {e}")
    });
    assert!(file.contains("- [ ] Milch kaufen"), "{file}");
    assert!(file.contains("- [x] Brot geholt"), "{file}");

    // The tolerance is exactly that wide. `checked` is a difference in the DOCUMENT — an
    // unticked box and a ticked one are two different pages — so it must still be compared,
    // or the reduction would hide the one thing markdown does carry.
    let mangled: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"taskList","content":[
             {"kind":"taskItem","attrs":{"checked":true},"content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"a"}]},
               {"kind":"paragraph","content":[{"kind":"text","text":"b\nc"}]}]}]}]}"#,
    )
    .unwrap();
    assert!(
        export::render_file(&meta, &mangled, &no_paths()).is_err(),
        "the reduction must not start forgiving real differences"
    );
}

#[test]
fn a_task_item_that_states_no_checked_at_all_is_refused_rather_than_guessed_at() {
    // Pins `checked` INTO the reduction's allow-list, which the test above does not: its
    // counter-example differs by a newline in the prose, so emptying the allow-list
    // altogether leaves every assertion in it passing. Verified by mutation — reducing
    // `TASK_ITEM_ATTRS` to `[]` broke nothing until this test existed.
    //
    // The allow-list is a safety net rather than a live code path: the renderer writes
    // `[x]`/`[ ]` from `checked`, so the two sides normally agree by construction and no
    // ordinary page can tell the difference. That is exactly why it needs pinning — a
    // narrowing goes unnoticed until the day something else writes a task block, and then
    // the round-trip check that is supposed to catch it has already been switched off.
    //
    // A stored task item with no `checked` is malformed: import always states one. Refusing
    // is the rule this whole module is held to — nothing is quietly degraded — and the
    // alternative is writing a file that says `- [ ]` about a document that never said so.
    let meta = export::FileMeta {
        title: "Einkauf".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "einkauf".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"taskList","content":[
             {"kind":"taskItem","attrs":{},"content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"Milch kaufen"}]}]}]}]}"#,
    )
    .unwrap();

    assert!(
        export::render_file(&meta, &body, &no_paths()).is_err(),
        "a task item stating no `checked` must be refused, not exported as unticked"
    );
}

#[test]
fn a_link_whose_address_markdown_would_mangle_is_refused_rather_than_truncated() {
    // Pins `href` INTO the reduction's allow-list, the same way the test above pins
    // `checked`. Verified by mutation: reducing `LINK_ATTRS` to `[]` broke no test in this
    // file until this one existed, even though it switches off the comparison that is the
    // whole reason `render_file` re-imports its own output.
    //
    // A TRAILING space is the shape that pins it, and the choice is not arbitrary. An
    // address mangled in the middle — `.../a b` — is refused anyway, because the tail
    // falls out of the link and the two trees differ structurally; that proves nothing
    // about the allow-list, which only ever compares a mark's attributes. A trailing space
    // is trimmed by the parser on the way back in and changes nothing else at all, so the
    // href is the ONLY thing left that differs. Drop `href` from the reduction and this
    // page exports clean.
    //
    // Refusing is right: a link that goes somewhere else is not the same link, and this
    // module's rule is that nothing is quietly degraded.
    let meta = export::FileMeta {
        title: "Quellen".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "quellen".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"Studie",
              "marks":[{"kind":"link","attrs":{"href":"https://example.invalid/a "}}]}]}]}"#,
    )
    .unwrap();

    assert!(
        export::render_file(&meta, &body, &no_paths()).is_err(),
        "an address markdown cannot state must be refused, not silently truncated"
    );
}

#[test]
fn a_document_reference_round_trips_through_markdown_unchanged() {
    // The pin for both halves of the `dok:` syntax at once (ADR 0019), and it goes through
    // `render_file` rather than through `render` for the reason every test for this feature
    // has to: the refusal for a reference happens in the COMPARISON, not in the renderer, so
    // a unit test of the renderer passes while the export refuses the page.
    //
    // Import → export → re-import, and the two trees are compared whole rather than through
    // the exporter's own reduction: the reduction is what the comparison forgives, and a
    // test that used it could not tell a round trip from a tolerance.
    let id = "0199c0de-0000-7000-8000-00000000000a";
    let source = format!("Siehe [Blutbild](dok:{id}) und [außen](https://example.invalid/x).\n");
    let imported = gw_core::markdown::markdown_to_blocks(&source);

    // The importer really produced a reference and not an href — otherwise everything below
    // would hold just as well for a link that never became one.
    let json = serde_json::to_value(&imported).unwrap();
    let marks = json["content"][0]["content"][1]["marks"].clone();
    assert_eq!(marks[0]["attrs"]["doc"], serde_json::json!(id), "{json:#}");
    assert_eq!(marks[0]["attrs"].get("href"), None, "{json:#}");

    let meta = export::FileMeta {
        title: "Befunde".into(),
        doc_type: "page".into(),
        visibility: "restricted".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "befunde".into(),
        tags: Vec::new(),
    };
    let file = export::render_file(&meta, &imported, &no_paths()).unwrap_or_else(|e| {
        panic!("a page holding a reference must export, and this did not: {e}")
    });
    assert!(
        file.contains(&format!("[Blutbild](dok:{id})")),
        "the reference is written as the scheme, never as the target's path:\n{file}"
    );

    let (_, body) = gw_core::split_frontmatter(&file);
    let reimported = gw_core::markdown::markdown_to_blocks(body);
    assert_eq!(
        serde_json::to_value(&reimported).unwrap(),
        serde_json::to_value(&imported).unwrap(),
        "the tree that came back is not the tree that went out:\n{file}"
    );

    // And the same round trip with the target's path beside the id. The FILE gains a link
    // title; the DOCUMENT must not. `render_file` succeeding is what asserts that, because
    // the comparison inside it is the thing the title could leak into — a stored `{doc}` and
    // the `{doc, path}` its own markdown re-imports as have to compare equal, or a page
    // holding a reference is refused from every export from then on.
    let mut paths = export::ReferencePaths::new();
    paths.insert(
        id.to_string(),
        gw_store::Reference {
            path: "/darm/blutbild".into(),
            title: "Blutbild".into(),
        },
    );
    let with_path = export::render_file(&meta, &imported, &paths)
        .unwrap_or_else(|e| panic!("the fallback path leaked into the round-trip comparison: {e}"));
    assert!(
        with_path.contains(&format!("[Blutbild](dok:{id} \"/darm/blutbild\")")),
        "the target's path is not beside the id:\n{with_path}"
    );

    let (_, carried) = gw_core::split_frontmatter(&with_path);
    let back = gw_core::markdown::markdown_to_blocks(carried);
    let mark = serde_json::to_value(&back).unwrap()["content"][0]["content"][1]["marks"][0].clone();
    assert_eq!(mark["attrs"]["doc"], serde_json::json!(id), "{mark}");
    assert_eq!(
        mark["attrs"]["path"],
        serde_json::json!("/darm/blutbild"),
        "the fallback did not come back, so nothing carries a restored corpus"
    );
}

/// A page at `slug` whose only content is `body`. Returns its id.
async fn reference_page(store: &Store, slug: &str, title: &str, body: Block) -> String {
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: title.into(),
                slug: Some(slug.into()),
                language: "de".into(),
                visibility: Visibility::Public,
                body,
                sort_key: 0,
                topics: Vec::new(),
            },
            None,
        )
        .await
        .unwrap()
}

/// The attributes of the one link mark in the body stored at `path`.
async fn stored_link(store: &Store, path: &str) -> serde_json::Value {
    let doc = store
        .document_for(&admin(), path, Action::Read)
        .await
        .unwrap()
        .unwrap();
    let body: serde_json::Value = serde_json::from_str(&doc.body).unwrap();
    fn find(node: &serde_json::Value) -> Option<serde_json::Value> {
        for mark in node["marks"].as_array().into_iter().flatten() {
            if mark["kind"] == serde_json::json!("link") {
                return Some(mark["attrs"].clone());
            }
        }
        node["content"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(find)
    }
    find(&body).unwrap_or_else(|| panic!("`{path}` holds no link at all: {body}"))
}

#[tokio::test]
async fn a_corpus_re_seeded_into_an_empty_database_keeps_the_connection_its_references_named() {
    // The cost identity was about to charge, and the reason it no longer does. A seed mints
    // every id fresh — `SeedMeta` has no `id` key — so a `dok:` in a restored corpus can never
    // match anything there, and without the target's path beside it every internal link in a
    // restored backup would come back as plain words: the text intact, the connection gone.
    //
    // End to end, because nothing smaller proves it: build a wiki whose page really holds a
    // `doc` reference, export it, seed that export into an EMPTY store, and find the
    // connection again there.
    let source = Store::open("sqlite::memory:").await.unwrap();
    // `anker` before `quelle` alphabetically, and that is load-bearing: the seeder walks the
    // directory in filename order, and `replace_links` records no edge to a page that does
    // not exist yet (SQLite's conflict resolution does not apply to foreign keys, so an edge
    // to an absent document would abort the import). That is pre-existing behaviour for every
    // address-shaped link and is unchanged here — a restored link whose target seeds LATER is
    // still a working link, it simply waits for something to republish the page before it is
    // an edge. Naming the pages this way keeps this test about the fallback rather than about
    // that.
    let ziel = reference_page(
        &source,
        "anker",
        "Anker",
        serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"Hier ist der Anker."}]}]}"#,
        )
        .unwrap(),
    )
    .await;
    let von = reference_page(
        &source,
        "quelle",
        "Quelle",
        serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                 {"kind":"text","text":"siehe dort",
                  "marks":[{"kind":"link","attrs":{"href":"/anker"}}]}]}]}"#,
        )
        .unwrap(),
    )
    .await;

    // Publishing is what exchanges an author's address for the target's identity, so the
    // fixture reaches that state by being written rather than by being assembled.
    let autor = signed_in_writer(&source, "/quelle").await;
    let body: Block = serde_json::from_str(
        &source
            .document_for(&admin(), "/quelle", Action::Read)
            .await
            .unwrap()
            .unwrap()
            .body,
    )
    .unwrap();
    source
        .publish_revision(&autor, &von, &body, None)
        .await
        .unwrap()
        .expect("the publish was refused");
    assert_eq!(
        stored_link(&source, "/quelle").await["doc"],
        serde_json::json!(ziel),
        "the fixture never held a reference, so nothing below is proved"
    );

    let out = tempfile::tempdir().unwrap();
    let report = export_to(&source, &admin(), out.path()).await;
    assert!(report.is_complete(), "{report}");
    let file = std::fs::read_to_string(out.path().join("quelle.md")).unwrap();
    assert!(
        file.contains(&format!("(dok:{ziel} \"/anker\")")),
        "the export carries the id but not the path to fall back to:\n{file}"
    );

    // A FRESH store. Nothing in it has ever heard of the ids in that directory.
    let restored = Store::open("sqlite::memory:").await.unwrap();
    let reload = seed::run(&restored, out.path()).await.unwrap();
    assert!(reload.is_complete(), "{reload}");

    let neues_ziel = restored
        .document_for(&admin(), "/anker", Action::Read)
        .await
        .unwrap()
        .unwrap()
        .id;
    assert_ne!(
        neues_ziel, ziel,
        "a seed must mint a new id, or this test proves nothing at all"
    );

    // It fell back to the path, and the connection is in the restored wiki's graph — which is
    // the whole claim. A `doc` mark naming a stranger's id would have recorded no edge.
    let mark = stored_link(&restored, "/quelle").await;
    assert_eq!(
        mark["href"],
        serde_json::json!("/anker"),
        "the reference did not fall back to the path it carried: {mark}"
    );
    assert_eq!(
        mark.get("doc"),
        None,
        "a dead id was kept beside the address, so the two can disagree: {mark}"
    );
    let backlinks = restored.backlinks_for(&admin(), &neues_ziel).await.unwrap();
    assert_eq!(
        backlinks.len(),
        1,
        "the connection is not in the restored graph"
    );
    assert_eq!(backlinks[0].path, "/quelle");

    // And the next publish there makes it an identity again, against THAT database's ids —
    // so a restored corpus is not merely readable, it is back where it started.
    let neuer_autor = signed_in_writer(&restored, "/quelle").await;
    let neues_von = restored
        .document_for(&admin(), "/quelle", Action::Read)
        .await
        .unwrap()
        .unwrap();
    let neuer_koerper: Block = serde_json::from_str(&neues_von.body).unwrap();
    restored
        .publish_revision(&neuer_autor, &neues_von.id, &neuer_koerper, None)
        .await
        .unwrap()
        .expect("the publish was refused");
    assert_eq!(
        stored_link(&restored, "/quelle").await["doc"],
        serde_json::json!(neues_ziel),
        "the restored reference never became an identity again"
    );
}

#[test]
fn a_reference_to_something_that_is_not_a_document_id_is_refused_rather_than_written() {
    // D-21f. A `doc` value is an arbitrary string that reached `documents.body` over the
    // collaboration socket without passing any validation, and `Renderer::wrap` interpolates
    // a destination with NO escaping — so a writer that emitted what it was handed would put
    // attacker text into the owner's backup file: this one would export as
    // `[Titel](dok:x) [siehe](https://angreifer.example/`, which is a second link the author
    // never wrote.
    //
    // Refusing names the page and writes nothing. The round-trip comparison would also have
    // caught this one, and that is not the design: it catches it only because the smuggled
    // text happens to re-parse differently, and "happens to" is not a guarantee.
    let meta = export::FileMeta {
        title: "Quellen".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "quellen".into(),
        tags: Vec::new(),
    };
    for target in [
        "x) [siehe](https://angreifer.example/",
        "etwas",
        "",
        "../../etc/passwd",
    ] {
        let body: Block = serde_json::from_str(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[
                 {{"kind":"text","text":"Titel",
                  "marks":[{{"kind":"link","attrs":{{"doc":{}}}}}]}}]}}]}}"#,
            serde_json::Value::String(target.into())
        ))
        .unwrap();
        assert!(
            export::render_file(&meta, &body, &no_paths()).is_err(),
            "`{target}` is not a document id and must be refused, not written"
        );
    }
}

#[test]
fn a_link_carrying_a_minted_null_attribute_still_exports() {
    // D-21b, and it is the `LINK_ATTRS` disaster arrived at from the other side.
    //
    // The editor's `Anchor` has to DECLARE `doc`, or y-tiptap deletes it from the Y.Doc and
    // broadcasts the deletion — a reference destroyed by somebody else typing a word in the
    // same paragraph. But a declared attribute is one ProseMirror MINTS: `computeAttrs` fills
    // the `null` default in, `marksToAttributes` writes the whole map, and `attrs_to_marks`
    // copies it verbatim, so every ordinary external link on the site is stored
    // `{href: "https://…", doc: null}` the first time anybody edits its paragraph.
    //
    // An allow-list keeps a key without inspecting its value, so `LINK_ATTRS` alone is blind
    // to that: the stored mark reduces to two keys, its own markdown re-imports as one, and
    // the page is refused from every export from then on. Nobody edited its content.
    //
    // The same holds for a block's allow-listed attributes, so the fence below carries a
    // minted `language: null` beside the link.
    let meta = export::FileMeta {
        title: "Quellen".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "quellen".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"paragraph","content":[
               {"kind":"text","text":"Studie",
                "marks":[{"kind":"link","attrs":{"href":"https://example.invalid/a","doc":null}}]}]},
             {"kind":"codeBlock","attrs":{"language":null},
              "content":[{"kind":"text","text":"eins"}]}]}"#,
    )
    .unwrap();

    let file = export::render_file(&meta, &body, &no_paths()).unwrap_or_else(|e| {
        panic!("a minted null must compare equal to an absent key, and did not: {e}")
    });
    assert!(
        file.contains("[Studie](https://example.invalid/a)"),
        "{file}"
    );

    // And the tolerance is exactly that wide: a `doc` that carries a VALUE beside an `href`
    // is a mark saying two different things, and it is still refused.
    let both: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"Studie","marks":[{"kind":"link","attrs":{
               "href":"https://example.invalid/a",
               "doc":"0199c0de-0000-7000-8000-00000000000a"}}]}]}]}"#,
    )
    .unwrap();
    assert!(
        export::render_file(&meta, &both, &no_paths()).is_err(),
        "a link stating both an address and a reference must not be quietly halved"
    );
}

#[test]
fn a_code_block_carrying_a_second_attribute_is_reduced_rather_than_refused() {
    // The `LINK_ATTRS` disaster, arriving where diagrams, formulas and highlighted
    // listings have just given people a reason to write fences.
    //
    // Nothing validates a block attribute on the write path: `read_attributes` in
    // `gw-collab` copies whatever the Yjs element carries and `publish_revision`
    // serialises the tree as given, which `doc.rs`'s own fixture demonstrates by
    // round-tripping a paragraph carrying `{"": "leer", "größe": 1, "a b": "c"}`. So
    // anybody with write access on ONE page can store a second attribute on a fence over
    // the collaboration socket. Without a reduction the exported markdown re-imports as
    // `{language}` alone, the trees differ, `render_file` refuses — and `run()` pushes a
    // `Refused` and continues, so that page is silently missing from every export
    // directory from then on while `FIDELITY_WARNING` leaves a sentence there calling the
    // directory a faithful copy of the database. A document that cannot be exported
    // cannot be exported later either: it is permanent.
    //
    // Markdown has a spelling for exactly one thing about a fence — the language on its
    // info string. Everything else a fence could carry (a theme, line numbers, a cached
    // drawing, a parse-error flag) is renderer or component state and has no home in a
    // file, which is why the allow-list has one entry and why adding a second DECLARED
    // attribute in the editor's schema is still refused: ProseMirror would mint it onto
    // every block it touches, the reduction would then drop it from both sides, and the
    // backup would quietly omit it instead of loudly refusing the page.
    let meta = export::FileMeta {
        title: "Ablauf".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "ablauf".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"codeBlock",
             "attrs":{"language":"mermaid","theme":"dark","zeilen":3},
             "content":[{"kind":"text","text":"graph TD;\n  A-->B;"}]}]}"#,
    )
    .unwrap();

    let file = export::render_file(&meta, &body, &no_paths()).unwrap_or_else(|e| {
        panic!("a fence carrying a stray attribute must still export, and this one did not: {e}")
    });
    assert!(file.contains("```mermaid"), "{file}");
    assert!(file.contains("  A-->B;"), "{file}");
}

#[test]
fn a_code_block_whose_language_its_own_markdown_would_lose_is_refused() {
    // Pins `language` INTO the reduction's allow-list, the way the two tests above pin
    // `checked` and `href`. Verified by mutation: emptying `CODE_BLOCK_ATTRS` breaks
    // nothing else in this file, because the test above differs only by attributes the
    // reduction is supposed to discard.
    //
    // A comma is the shape that pins it. The importer keeps the info string's FIRST
    // comma- or space-separated token (`markdown.rs`), so a stored `rust,ignore` is
    // written onto the fence whole and comes back as `rust` — everything else about the
    // block is identical, so the language is the only thing left that differs. A language
    // containing whitespace or a backtick cannot pin it: `Renderer::code` refuses those
    // outright, before any comparison happens.
    //
    // Refusing is right: a fence that says it is one language and re-reads as another is
    // not the same block, and this module's rule is that nothing is quietly degraded.
    let meta = export::FileMeta {
        title: "Ablauf".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "ablauf".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"codeBlock",
             "attrs":{"language":"rust,ignore"},
             "content":[{"kind":"text","text":"let x = 1;"}]}]}"#,
    )
    .unwrap();

    assert!(
        export::render_file(&meta, &body, &no_paths()).is_err(),
        "a fence whose language markdown cannot state must be refused, not quietly changed"
    );
}

// --- topics ----------------------------------------------------------------------------

/// The frontmatter key is `tags` and the domain word is *topic*, deliberately: the design's
/// own data model spells the tables `tags` / `document_tags` and calls the thing a topic,
/// and `SeedMeta` reserved the key for exactly this before anything read it. What matters
/// here is that the file says `tags:` and that what comes back out of it is what went in.
#[test]
fn a_pages_topics_are_stated_in_its_frontmatter_and_re_import_unchanged() {
    let meta = export::FileMeta {
        title: "Laborwerte".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "laborwerte".into(),
        tags: vec!["Ernährung".into(), "Medizin/Darm".into()],
    };
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph",
             "content":[{"kind":"text","text":"Ein Satz."}]}]}"#,
    )
    .unwrap();

    // `render_file` re-imports its own output and compares the metadata, so a file that
    // came back with different topics would be REFUSED rather than returned. Getting a
    // file at all is therefore already half the assertion.
    let file =
        export::render_file(&meta, &body, &no_paths()).expect("this document is expressible");
    assert!(file.contains("tags:"), "{file}");
    assert!(file.contains("Medizin/Darm"), "{file}");
    assert!(file.contains("Ernährung"), "{file}");

    // And the file the exporter wrote is the file the importer reads: parsed back with the
    // same function `seed` uses, the list is identical, in order.
    let (yaml, _) = gw_core::split_frontmatter(&file);
    let back = gw_core::SeedMeta::parse(yaml, "the file just rendered").unwrap();
    assert_eq!(back.tags, meta.tags);
}

#[test]
fn a_page_about_nothing_still_states_the_key() {
    // Nothing is omitted "because it is the default" — the rule `frontmatter` already
    // follows for `visibility` and `slug`. Here the reason is smaller and still real: an
    // exported file is the only documentation of the format most people will ever read, and
    // a key that appears only on pages that happen to use it is a key nobody discovers.
    let meta = export::FileMeta {
        title: "Ohne".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "ohne".into(),
        tags: Vec::new(),
    };
    let body: Block = serde_json::from_str(r#"{"kind":"doc","content":[]}"#).unwrap();
    let file = export::render_file(&meta, &body, &no_paths()).unwrap();
    assert!(file.contains("tags: []"), "{file}");
}

#[tokio::test]
async fn a_pages_topics_survive_export_and_a_second_import() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    store
        .create_document(
            Author::Import,
            &NewDocument {
                parent_path: None,
                doc_type: DocumentType::Page,
                title: "Laborwerte".into(),
                slug: None,
                language: "de".into(),
                visibility: Visibility::Public,
                body: serde_json::from_str(
                    r#"{"kind":"doc","content":[{"kind":"paragraph",
                         "content":[{"kind":"text","text":"Ein Satz."}]}]}"#,
                )
                .unwrap(),
                sort_key: 0,
                topics: vec!["Medizin/Darm".into(), "Ernährung".into()],
            },
            None,
        )
        .await
        .unwrap();

    let before = snapshot(&store, &admin()).await;
    assert_eq!(
        before["/laborwerte"]["topics"],
        serde_json::json!(["Ernährung", "Medizin/Darm"]),
        "the fixture must actually carry topics, or this proves nothing"
    );

    let out = tempfile::tempdir().unwrap();
    let report = export_to(&store, &admin(), out.path()).await;
    assert!(report.is_complete(), "{report}");

    let reloaded = Store::open("sqlite::memory:").await.unwrap();
    let reload = seed::run(&reloaded, out.path()).await.unwrap();
    assert!(reload.is_complete(), "{reload}");
    assert_eq!(snapshot(&reloaded, &admin()).await, before);
}

// --- placed files (D-15) -----------------------------------------------------------------

/// The metadata every placement test below shares. Its content is irrelevant to them; only
/// the body differs.
fn befund_meta() -> export::FileMeta {
    export::FileMeta {
        title: "Befunde".into(),
        doc_type: "page".into(),
        visibility: "public".into(),
        language: "de".into(),
        sort_key: 0,
        slug: "befunde".into(),
        tags: Vec::new(),
    }
}

#[test]
fn a_page_that_places_a_file_exports_and_re_imports_as_the_same_page() {
    // `render_file` is the whole of the check: it re-imports its own output and compares the
    // trees, so getting a file back at all already says the placement survived. The
    // assertions on the text are there to pin the SYNTAX, which is the half a reader of an
    // exported file sees and the half `gw_core::markdown` has to be able to read back.
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"paragraph","content":[{"kind":"text","text":"Der Befund vom März:"}]},
             {"kind":"attachment","attrs":{"filename":"befund.png","alt":"Befund vom März"}},
             {"kind":"paragraph","content":[{"kind":"text","text":"Und danach."}]}]}"#,
    )
    .unwrap();

    let file = export::render_file(&befund_meta(), &body, &no_paths())
        .unwrap_or_else(|e| panic!("a page that places a file must still export: {e}"));
    assert!(
        file.contains("![Befund vom März](anhang:befund.png)"),
        "{file}"
    );
    // A reference and nothing else: no type, no size, and above all no content address —
    // D-16 makes a download authorised against the page it was reached through, and a digest
    // in an exported file is a digest somebody can hold.
    assert!(!file.contains("image/png"), "{file}");
    assert!(!holds_a_content_address(&file), "{file}");
}

/// Whether the text holds a run of hex long enough to be a content address.
fn holds_a_content_address(s: &str) -> bool {
    let mut run = 0usize;
    for c in s.chars() {
        run = if c.is_ascii_hexdigit() { run + 1 } else { 0 };
        if run >= 40 {
            return true;
        }
    }
    false
}

#[test]
fn a_placement_is_compared_with_its_attributes_whole_and_no_reduction_forgives_them() {
    // The counterpart of `LINK_ATTRS` and `TASK_ITEM_ATTRS`, arrived at from the other side:
    // there is deliberately NO reduction for a placement, because there is nothing to
    // forgive. Those two exist because the EDITOR mints attributes `gw_core::markdown` never
    // writes — stock TipTap `Link` declares five, and a task carries the store's uuid — and a
    // byte-equal comparison then refused every page holding one, permanently, on the owner's
    // backup path.
    //
    // `web/src/lib/editor/extensions.ts` declares exactly `filename` and `alt` on its
    // `attachment` node, which is exactly what the importer writes, so the two sides agree by
    // construction and a third attribute can only arrive from something that has gone wrong.
    // This test is what makes that a decision rather than an accident: add a reduction here
    // and it goes red, so the day somebody widens the editor's declaration they have to come
    // back and say why.
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"attachment",
              "attrs":{"filename":"befund.png","alt":"Befund","width":640}}]}"#,
    )
    .unwrap();
    assert!(
        export::render_file(&befund_meta(), &body, &no_paths()).is_err(),
        "an attribute markdown cannot state must be refused, not quietly dropped"
    );
}

#[test]
fn a_placement_that_names_no_file_is_refused_rather_than_written_as_an_empty_picture() {
    for attrs in [r#"{"alt":"Befund"}"#, r#"{"filename":"","alt":""}"#] {
        let body: Block = serde_json::from_str(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"attachment","attrs":{attrs}}}]}}"#
        ))
        .unwrap();
        assert!(
            export::render_file(&befund_meta(), &body, &no_paths()).is_err(),
            "a placement naming no file must be refused: {attrs}"
        );
    }
}

#[test]
fn a_description_holding_a_line_break_is_refused_the_way_a_paragraph_is() {
    // Markdown has no way to put a newline inside `![…]` that survives a re-import — the
    // same refusal a paragraph gets, and reached through the same escaping pass so the two
    // cannot answer differently.
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"attachment","attrs":{"filename":"a.png","alt":"eins\nzwei"}}]}"#,
    )
    .unwrap();
    assert!(export::render_file(&befund_meta(), &body, &no_paths()).is_err());
}

#[test]
fn a_placement_the_importer_could_never_read_back_is_refused_by_name() {
    // A filename with a slash in it: `canonical_filename` refuses one, so no page can carry
    // it, and `attachment_destination` therefore has no way to write it. The refusal names
    // the file rather than exporting a destination that would re-import as a relative image.
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"attachment","attrs":{"filename":"a/b.png","alt":""}}]}"#,
    )
    .unwrap();
    let refusal = export::render_file(&befund_meta(), &body, &no_paths())
        .expect_err("a name no page can give a file must be refused");
    assert!(refusal.contains("a/b.png"), "{refusal}");
}

#[test]
fn a_placement_nested_where_the_importer_will_not_read_one_is_refused_rather_than_written() {
    // The editor's schema admits `attachment` in `doc` and nowhere else, and the importer
    // reads one back only at the top level — so a body with one inside a blockquote can only
    // have been hand-written. It exports as `> ![…](anhang:…)`, which re-imports as a
    // paragraph of text, and the comparison refuses the page. Loud, which is the rule; the
    // point of pinning it is that "loud" must not quietly become "silently different".
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"blockquote","content":[
             {"kind":"attachment","attrs":{"filename":"a.png","alt":"x"}}]}]}"#,
    )
    .unwrap();
    assert!(export::render_file(&befund_meta(), &body, &no_paths()).is_err());
}

#[test]
fn a_placement_naming_a_file_nothing_is_attached_to_still_exports_exactly_as_written() {
    // The deliberate answer to the one state D-15 makes possible: the list is the authority
    // on what is attached, so a block naming a file that is not there is a REFERENCE that
    // resolves to nothing — not a broken document. This module has no store and must not
    // acquire one to answer it; the reader is where that question gets asked, against the
    // page's own `Anhänge` list. So the exporter writes it exactly as it stands, and a page
    // whose file was detached is still a page that can be backed up.
    let body: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"attachment","attrs":{"filename":"gibtsnicht.png","alt":"Fehlt"}}]}"#,
    )
    .unwrap();
    let file = export::render_file(&befund_meta(), &body, &no_paths())
        .expect("a reference to a file that is not attached is not a broken document");
    assert!(file.contains("![Fehlt](anhang:gibtsnicht.png)"), "{file}");
}
