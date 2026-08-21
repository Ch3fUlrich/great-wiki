//! Block tree → markdown, proven the only way that means anything: convert the markdown
//! back and compare the trees.
//!
//! A test that asserts on the *text* of the output pins a formatting choice, not fidelity —
//! it passes on output that re-imports as something else, and fails on output that is
//! prettier but identical. So every case here goes markdown → Block → markdown → Block and
//! asserts the two trees are the same JSON.

use gw_api::export::blocks_to_markdown;
use gw_core::{markdown, Block};

/// The tree a markdown source produces, as JSON — the same shape the store persists.
fn tree(md: &str) -> serde_json::Value {
    serde_json::to_value(markdown::convert(md).doc).unwrap()
}

/// Import `md`, export the result, import it again, and return both trees.
fn round_trip(md: &str) -> (serde_json::Value, serde_json::Value, String) {
    let first: Block = markdown::convert(md).doc;
    let rendered = blocks_to_markdown(&first);
    let second: Block = markdown::convert(&rendered).doc;
    (
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(&second).unwrap(),
        rendered,
    )
}

/// Assert that `md` survives Block → markdown → Block unchanged.
#[track_caller]
fn survives(md: &str) {
    let (before, after, rendered) = round_trip(md);
    assert_eq!(
        before, after,
        "the exported markdown re-imports as a different tree\n--- exported ---\n{rendered}\n---"
    );
}

#[test]
fn an_empty_document_exports_as_nothing() {
    assert_eq!(blocks_to_markdown(&markdown::convert("").doc), "");
}

#[test]
fn a_paragraph_survives() {
    survives("Ein Satz.\n");
}

#[test]
fn headings_keep_their_level() {
    survives("# Eins\n\n## Zwei\n\n###### Sechs\n");
}

#[test]
fn german_text_survives_byte_for_byte() {
    let (_, _, rendered) = round_trip("# Größe und Maß\n\nÜber Öl und Äpfel.\n");
    assert!(rendered.contains("Größe und Maß"), "{rendered}");
    assert!(rendered.contains("Über Öl und Äpfel."), "{rendered}");
    survives("# Größe und Maß\n\nÜber Öl und Äpfel.\n");
}

#[test]
fn bullet_and_ordered_lists_survive_including_a_non_default_start() {
    survives("- eins\n- zwei\n");
    survives("1. eins\n2. zwei\n");
    survives("7. sieben\n8. acht\n");
}

#[test]
fn deeply_nested_lists_survive() {
    survives("- eins\n  - eins-a\n    - eins-a-i\n      - tiefer\n- zwei\n");
    survives("1. eins\n   1. eins-a\n      - gemischt\n");
}

#[test]
fn a_list_item_with_two_paragraphs_does_not_merge_them() {
    // Rendered with a single newline between them the second becomes a lazy continuation
    // of the first, and one item silently holds one paragraph instead of two.
    survives("- erster Absatz\n\n  zweiter Absatz\n");
}

#[test]
fn a_blockquote_survives_including_a_nested_one() {
    survives("> Zitat.\n");
    survives("> Zitat.\n>\n> Zweiter Absatz.\n");
    survives("> außen\n>\n> > innen\n");
}

#[test]
fn code_keeps_its_language_line_breaks_and_backticks() {
    survives("```rust\nfn main() {}\n```\n");
    survives("```\nplain\n```\n");
    survives("```sh\neins\nzwei\n```\n");
    // A code block whose content contains a fence needs a longer fence than three.
    survives("````md\n```rust\nfn main() {}\n```\n````\n");
}

#[test]
fn a_table_survives_with_its_alignment_and_empty_cells() {
    survives("| Feld | Wert |\n| --- | --- |\n| Größe | 42 |\n");
    survives("| l | c | r | d |\n|:---|:---:|---:|---|\n| 1 | 2 | 3 | 4 |\n");
    survives("| a | b |\n| --- | --- |\n| x |  |\n|  | y |\n");
    survives("| Probe | Anteil |\n|---|---:|\n| Ähre | <0,5 % |\n| Öl | 12 % |\n");
}

#[test]
fn a_pipe_inside_a_cell_does_not_split_the_column() {
    survives("| Feld | Wert |\n| --- | --- |\n| a \\| b | c |\n");
}

#[test]
fn markdown_punctuation_in_prose_stays_prose() {
    // The database holds text, not markup. Text that *looks* like markup must come back
    // as the same text — otherwise a page saying `**wichtig**` re-imports as `wichtig`,
    // and the two asterisks the author typed are gone.
    survives("Ein \\*Stern\\* und ein \\_Unterstrich\\_ und ein \\`Backtick\\`.\n");
    survives("Eine \\[eckige Klammer\\] und ein \\<Winkel\\>.\n");
    survives("Ein Bereich 3-5 % und ein Wert <0,5 % und ein Ausdruck a|b.\n");
    survives("snake\\_case\\_name bleibt snake\\_case\\_name.\n");
    survives("Ein Ampersand \\&amp; bleibt stehen.\n");
    survives("Zwei Tilden \\~\\~weg\\~\\~ bleiben Tilden.\n");
}

#[test]
fn a_paragraph_that_starts_like_a_block_marker_stays_a_paragraph() {
    // The riskiest class: the text is harmless mid-line and structural at line start.
    survives("\\# kein Titel\n");
    survives("\\> kein Zitat\n");
    survives("\\- keine Liste\n");
    survives("1\\. keine Nummerierung\n");
    survives("\\--- kein Trennstrich\n");
}

#[test]
fn two_adjacent_lists_do_not_merge_into_one() {
    // A blank line between two bullet lists makes ONE loose list, so a document holding
    // two lists comes back holding one. Only a change of marker separates them.
    let doc: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"bulletList","content":[{"kind":"listItem","content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"eins"}]}]}]},
             {"kind":"bulletList","content":[{"kind":"listItem","content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"zwei"}]}]}]}]}"#,
    )
    .unwrap();
    let rendered = blocks_to_markdown(&doc);
    assert_eq!(
        tree(&rendered),
        serde_json::to_value(&doc).unwrap(),
        "two lists merged into one\n--- exported ---\n{rendered}\n---"
    );
}

#[test]
fn marks_round_trip_through_markdown() {
    let md = "Ein **fetter** Satz mit [einem Link](https://example.org) und `code`.";
    let doc = gw_core::markdown::convert(md).doc;
    let out = gw_api::export::render(&doc);
    assert!(
        out.markdown.contains("**fetter**"),
        "bold was lost: {}",
        out.markdown
    );
    assert!(out.markdown.contains("[einem Link](https://example.org)"));
    assert!(out.markdown.contains("`code`"));

    // The real proof: re-importing must give the same tree.
    let again = gw_core::markdown::convert(&out.markdown).doc;
    assert_eq!(
        serde_json::to_value(&doc).unwrap(),
        serde_json::to_value(&again).unwrap()
    );
}

#[test]
fn a_link_whose_text_is_emphasised_survives_whichever_way_it_was_nested() {
    // `[**a**](url)` and `**[a](url)**` describe the same document, and the importer
    // stores one canonical mark order for both (`gw_core::MARK_ORDER`). Whichever the
    // author wrote, the export re-imports as the very same tree — before this was pinned,
    // the marks came out of the importer in source order, the exporter wrote them in its
    // own, and every `[**a**](url)` in the corpus was REFUSED rather than exported.
    survives("Siehe [**das Handbuch**](/handbuch).\n");
    survives("Siehe [*das Handbuch*](/handbuch).\n");
    survives("Siehe [~~das Handbuch~~](/handbuch).\n");
    survives("Siehe **[das Handbuch](/handbuch)**.\n");
    survives("Siehe [`code`](/handbuch).\n");
    survives("Siehe **`code`**.\n");
}

#[test]
fn a_mark_run_covering_several_leaves_is_opened_once_and_closed_once() {
    // `*kursiv und **beides***` is two leaves — one carrying em, one carrying em and
    // strong — under ONE run of emphasis. A renderer that wraps each leaf on its own
    // closes the emphasis in the middle and reopens it, producing `*kursiv und ****beides***`:
    // four asterisks that no CommonMark parser reads as the author's document.
    survives("*kursiv und **beides***\n");
    survives("**fett und *beides***\n");
    survives("~~weg und **fett**~~\n");
    survives("**fett und [ein Link](https://example.org)**\n");
    survives("[ein **fetter** Link](https://example.org)\n");
    survives("**fett und `code` zusammen**\n");

    // The one place this suite asserts on the text: a doubled delimiter is the fingerprint
    // of a run that was closed and reopened, and it is exactly what a tree comparison can
    // miss when the mangled markdown happens to re-parse the same way.
    for md in [
        "*kursiv und **beides***\n",
        "**fett und *beides***\n",
        "~~weg und **fett**~~\n",
    ] {
        let (_, _, rendered) = round_trip(md);
        assert_eq!(rendered, md, "the mark run was not written as one run");
    }
}

#[test]
fn a_mark_run_the_renderer_cannot_write_is_reported_rather_than_mangled() {
    // Emphasis cannot open before a space or close after one — `**fett **und` is not bold
    // at all, it is four literal asterisks. Only the editor can build such a leaf (the
    // importer cannot parse one into existence), and `render` must SAY so: `problems` is
    // what `render_file` refuses on, and an empty `problems` on inexpressible output is a
    // silent corruption rather than a refusal.
    let doc: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"fett ","marks":[{"kind":"strong"}]},
             {"kind":"text","text":"und"}]}]}"#,
    )
    .unwrap();
    let out = gw_api::export::render(&doc);
    assert!(
        !out.problems.is_empty(),
        "the renderer produced markdown it cannot express and said nothing: {}",
        out.markdown
    );
}

// ---------------------------------------------------------------------------------------
// Checklists
//
// `gw_core::BlockKind` grew `TaskList`/`TaskItem` before this renderer knew about them, and
// because the enum is `#[non_exhaustive]` nothing failed to compile: `render` fell into its
// "newer than this exporter" arm, `render_file` turned that into an `Err`, and
// `great-wiki export` refused every page holding a checkbox — the owner's backup path, shut
// by one line of markdown. Loud, which is right, but the fix is to write the line out.
// ---------------------------------------------------------------------------------------

#[test]
fn a_checkbox_survives_with_its_state() {
    survives("- [ ] offen\n- [x] erledigt\n");
    // The text is asserted directly as well, because "survives" is also true of a renderer
    // that writes every box unticked and then re-imports its own mistake consistently.
    let (_, _, rendered) = round_trip("- [ ] offen\n- [x] erledigt\n");
    assert!(rendered.contains("- [ ] offen"), "{rendered}");
    assert!(rendered.contains("- [x] erledigt"), "{rendered}");
}

#[test]
fn a_mixed_list_stays_two_lists_rather_than_becoming_one() {
    // The split is `gw_core::markdown`'s doing (D-6: a checkbox line IS a task, so no plain
    // line may be given one). The exporter's job is not to undo it — and writing both runs
    // with the same bullet is what lets the importer split them apart again identically.
    survives("- [ ] a\n- plain\n");
    survives("- plain\n- [ ] a\n");
    survives("- [ ] a\n- plain\n- [x] b\n- weiter\n");
}

#[test]
fn a_split_numbered_list_keeps_the_numbers_its_plain_runs_had() {
    // `1. [ ] a` / `2. plain` / `3. [ ] c` / `4. plain` imports as four blocks whose starts
    // are `[None, 2, None, 4]`: the checkbox runs lose their numbering (markdown has no
    // numbered checklist — the importer notes `Unsupported::OrderedTaskList`), and the plain
    // runs keep theirs, or the page would come back reading "1. plain".
    let source = "1. [ ] a\n2. plain\n3. [ ] c\n4. plain\n";
    let starts: Vec<Option<u64>> = markdown::convert(source)
        .doc
        .content
        .iter()
        .map(|b| b.attrs.get("start").and_then(|v| v.as_u64()))
        .collect();
    assert_eq!(
        starts,
        vec![None, Some(2), None, Some(4)],
        "fixture changed"
    );
    survives(source);
}

#[test]
fn a_checklist_nested_under_a_checkbox_survives_at_its_own_depth() {
    // The indent is the trap. A task item's marker is `- [ ] `, six columns wide, but its
    // CONTENT column is two — the brackets are part of the first paragraph, not part of the
    // marker. Indenting a continuation by the marker's width puts it four columns past the
    // content, which is an indented CODE BLOCK, and the nested checklist comes back as
    // literal text inside a fence.
    survives("- [ ] a\n  - [x] b\n");
    survives("- [ ] a\n  - plain\n");
    survives("- a\n  - [x] b\n");
    survives("- [ ] a\n\n  noch etwas\n");
    survives("- [ ] a\n  - [x] b\n    - [ ] c\n");
    survives("1. eins\n   - [x] b\n");
    // Every prefix that indents its own content, since each one composes with the item
    // indent above rather than replacing it.
    survives("> - [x] zitiert\n");
    survives("> - [ ] a\n>   - [x] b\n");
    survives("| Feld | Wert |\n| --- | --- |\n| a | \\[x\\] kein Kasten |\n");
}

#[test]
fn a_checkbox_line_keeps_its_marks_and_its_punctuation() {
    survives("- [x] **fett** und *kursiv* und `code`\n");
    survives("- [ ] [ein Link](https://example.org)\n");
    survives("- [ ] Größe: 5 % — ja\n");
}

#[test]
fn text_that_merely_looks_like_a_checkbox_does_not_become_one() {
    // `[` and `]` are always escaped on the way out, so a bullet whose words happen to be
    // "[ ] a" stays a bullet. Without that, exporting a page would silently create a task
    // on somebody's board out of a line nobody ticked.
    survives("- \\[ \\] a\n");
    let (before, _, rendered) = round_trip("- \\[ \\] a\n");
    assert!(
        before.to_string().contains("bulletList"),
        "fixture changed: {before}"
    );
    assert!(rendered.contains("\\[ \\] a"), "{rendered}");
}

#[test]
fn two_adjacent_checklists_do_not_merge_into_one() {
    // The same hazard as `two_adjacent_lists_do_not_merge_into_one`, and worse: two lists
    // written with one marker come back as ONE list of checkboxes, so the boundary the
    // author put between two checklists is gone and no refusal names it. Only the importer
    // can produce a single list here, so this tree is built by hand — the editor is what
    // makes it reachable.
    let doc: Block = serde_json::from_str(
        r#"{"kind":"doc","content":[
             {"kind":"taskList","content":[{"kind":"taskItem","attrs":{"checked":false},
               "content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins"}]}]}]},
             {"kind":"taskList","content":[{"kind":"taskItem","attrs":{"checked":true},
               "content":[{"kind":"paragraph","content":[{"kind":"text","text":"zwei"}]}]}]}]}"#,
    )
    .unwrap();
    let rendered = blocks_to_markdown(&doc);
    assert_eq!(
        tree(&rendered),
        serde_json::to_value(&doc).unwrap(),
        "two checklists merged into one\n--- exported ---\n{rendered}\n---"
    );
}

#[test]
fn a_checklist_beside_an_ordinary_list_keeps_both_boundaries() {
    // Every adjacency of the three list kinds, hand-built, because the importer's own split
    // can only produce some of them and the editor can produce all of them. What must not
    // happen is a boundary disappearing: two blocks coming back as one, or one as two.
    let list = |kind: &str, item: &str, text: &str, attrs: &str| {
        format!(
            r#"{{"kind":"{kind}","content":[{{"kind":"{item}"{attrs},"content":[
                 {{"kind":"paragraph","content":[{{"kind":"text","text":"{text}"}}]}}]}}]}}"#
        )
    };
    let task = |text: &str| {
        list(
            "taskList",
            "taskItem",
            text,
            r#","attrs":{"checked":false}"#,
        )
    };
    let bullet = |text: &str| list("bulletList", "listItem", text, "");

    for parts in [
        vec![task("a"), bullet("b")],
        vec![bullet("a"), task("b")],
        vec![task("a"), task("b")],
        vec![task("a"), bullet("b"), task("c")],
        vec![bullet("a"), bullet("b"), task("c")],
        vec![task("a"), task("b"), bullet("c")],
        vec![task("a"), bullet("b"), bullet("c")],
        vec![task("a"), task("b"), task("c")],
        vec![task("a"), task("b"), bullet("c"), task("d")],
    ] {
        let json = format!(r#"{{"kind":"doc","content":[{}]}}"#, parts.join(","));
        let doc: Block = serde_json::from_str(&json).unwrap();
        let rendered = blocks_to_markdown(&doc);
        assert_eq!(
            tree(&rendered),
            serde_json::to_value(&doc).unwrap(),
            "a boundary moved\n--- source ---\n{json}\n--- exported ---\n{rendered}\n---"
        );
    }
}

#[test]
fn a_page_holding_a_checkbox_is_no_longer_refused() {
    // The regression that shut the export: `BlockKind` is `#[non_exhaustive]`, so adding
    // `TaskList` broke no compile anywhere and the exporter's catch-all arm reported it as
    // "newer than this exporter". `render_file` turns any problem into a refusal, and
    // `export` fails the whole run on the first one.
    let doc: Block = markdown::convert("- [x] erledigt\n").doc;
    assert!(
        gw_api::export::render(&doc).problems.is_empty(),
        "{:?}",
        gw_api::export::render(&doc).problems
    );
}

#[test]
fn the_shipped_example_corpus_survives_every_file() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content-example");
    let mut checked = 0;
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                let raw = std::fs::read_to_string(&path).unwrap();
                let (_, body) = gw_core::split_frontmatter(&raw);
                let (before, after, rendered) = round_trip(body);
                assert_eq!(
                    before,
                    after,
                    "{} does not survive the round trip\n--- exported ---\n{rendered}\n---",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 6, "only {checked} files were checked");
}

// ---------------------------------------------------------------------------------------
// What the renderer refuses
//
// These trees are legal in the block schema and impossible in markdown. Only the editor can
// produce most of them, so nothing in the markdown-in/markdown-out tests above can reach
// them — and a refusal nobody exercises is a refusal that quietly stops working.
// ---------------------------------------------------------------------------------------

/// The problems `render` reports for a hand-built tree.
fn problems(json: &str) -> Vec<String> {
    let doc: Block = serde_json::from_str(json).unwrap();
    gw_api::export::render(&doc).problems
}

#[track_caller]
fn refuses(json: &str, fragment: &str) {
    let found = problems(json);
    assert!(
        found.iter().any(|p| p.contains(fragment)),
        "expected a refusal mentioning `{fragment}`, got {found:?}"
    );
}

/// A one-row, one-column table with the given header and body cell attributes.
fn table(header_attrs: &str, body_attrs: &str) -> String {
    format!(
        r#"{{"kind":"doc","content":[{{"kind":"table","content":[
             {{"kind":"tableRow","content":[{{"kind":"tableHeader"{header_attrs},"content":[
               {{"kind":"paragraph","content":[{{"kind":"text","text":"a"}}]}}]}}]}},
             {{"kind":"tableRow","content":[{{"kind":"tableCell"{body_attrs},"content":[
               {{"kind":"paragraph","content":[{{"kind":"text","text":"1"}}]}}]}}]}}]}}]}}"#
    )
}

#[test]
fn a_cell_aligned_differently_from_its_column_is_refused() {
    // Markdown states alignment once per column. Exporting this would silently give the
    // body cell the header's alignment, and a numeric column would arrive ragged.
    assert!(problems(&table(
        r#","attrs":{"align":"right"}"#,
        r#","attrs":{"align":"right"}"#
    ))
    .is_empty());
    refuses(
        &table(
            r#","attrs":{"align":"right"}"#,
            r#","attrs":{"align":"left"}"#,
        ),
        "aligned differently",
    );
    refuses(
        &table(r#","attrs":{"align":"right"}"#, ""),
        "aligned differently",
    );
}

#[test]
fn a_row_that_is_not_as_wide_as_the_header_is_refused() {
    // GFM pads a short row and truncates a long one, so a column would move — invisibly,
    // and in a table that still looks like a table.
    refuses(
        r#"{"kind":"doc","content":[{"kind":"table","content":[
             {"kind":"tableRow","content":[
               {"kind":"tableHeader","content":[{"kind":"paragraph","content":[{"kind":"text","text":"a"}]}]},
               {"kind":"tableHeader","content":[{"kind":"paragraph","content":[{"kind":"text","text":"b"}]}]}]},
             {"kind":"tableRow","content":[
               {"kind":"tableCell","content":[{"kind":"paragraph","content":[{"kind":"text","text":"1"}]}]}]}]}]}"#,
        "cell(s) where the header has",
    );
}

#[test]
fn a_table_with_no_header_row_is_refused() {
    refuses(
        r#"{"kind":"doc","content":[{"kind":"table","content":[
             {"kind":"tableRow","content":[
               {"kind":"tableCell","content":[{"kind":"paragraph","content":[{"kind":"text","text":"1"}]}]}]}]}]}"#,
        "not a header row",
    );
}

#[test]
fn a_header_cell_below_the_first_row_is_refused() {
    refuses(
        r#"{"kind":"doc","content":[{"kind":"table","content":[
             {"kind":"tableRow","content":[
               {"kind":"tableHeader","content":[{"kind":"paragraph","content":[{"kind":"text","text":"a"}]}]}]},
             {"kind":"tableRow","content":[
               {"kind":"tableHeader","content":[{"kind":"paragraph","content":[{"kind":"text","text":"b"}]}]}]}]}]}"#,
        "header cell outside its first row",
    );
}

#[test]
fn a_cell_holding_anything_but_one_paragraph_is_refused() {
    refuses(
        r#"{"kind":"doc","content":[{"kind":"table","content":[
             {"kind":"tableRow","content":[
               {"kind":"tableHeader","content":[{"kind":"paragraph","content":[{"kind":"text","text":"a"}]}]}]},
             {"kind":"tableRow","content":[{"kind":"tableCell","content":[
               {"kind":"bulletList","content":[{"kind":"listItem","content":[
                 {"kind":"paragraph","content":[{"kind":"text","text":"1"}]}]}]}]}]}]}]}"#,
        "other than a single paragraph",
    );
}

#[test]
fn a_line_break_inside_a_paragraph_is_refused_rather_than_turned_into_a_space() {
    // Markdown has no way to carry one back: a soft break and a hard break both re-import
    // as a space, so the paragraph would come back one line shorter than it went in.
    refuses(
        r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
             {"kind":"text","text":"erste Zeile\nzweite Zeile"}]}]}"#,
        "line break",
    );
}

#[test]
fn a_code_block_whose_language_cannot_sit_on_a_fence_is_refused() {
    refuses(
        r#"{"kind":"doc","content":[{"kind":"codeBlock","attrs":{"language":"rust ignore"},
             "content":[{"kind":"text","text":"fn main() {}"}]}]}"#,
        "cannot be written on a fence",
    );
}

#[test]
fn a_block_that_cannot_stand_where_it_stands_is_refused() {
    refuses(
        r#"{"kind":"doc","content":[{"kind":"tableRow","content":[]}]}"#,
        "markdown has no way to write one",
    );
    refuses(
        r#"{"kind":"doc","content":[{"kind":"text","text":"nackt"}]}"#,
        "bare text",
    );
}

#[test]
fn a_checklist_holding_anything_but_a_task_item_is_refused() {
    // Markdown writes a checkbox per ITEM, so a plain list item inside a checklist would be
    // written as one — and would come back as a task nobody ticked. The mirror of the rule
    // `gw_core::markdown` enforces on the way in by splitting the list instead.
    refuses(
        r#"{"kind":"doc","content":[{"kind":"taskList","content":[
             {"kind":"listItem","content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"a"}]}]}]}]}"#,
        "where only `TaskItem`s may go",
    );
    refuses(
        r#"{"kind":"doc","content":[{"kind":"bulletList","content":[
             {"kind":"taskItem","attrs":{"checked":true},"content":[
               {"kind":"paragraph","content":[{"kind":"text","text":"a"}]}]}]}]}"#,
        "where only `ListItem`s may go",
    );
}

#[test]
fn a_task_item_with_no_list_around_it_is_refused() {
    refuses(
        r#"{"kind":"doc","content":[{"kind":"taskItem","attrs":{"checked":false},"content":[
             {"kind":"paragraph","content":[{"kind":"text","text":"a"}]}]}]}"#,
        "outside any list",
    );
}

// ---------------------------------------------------------------------------------------
// End to end
// ---------------------------------------------------------------------------------------

/// A markdown checkbox through every layer that can lose it: the importer, the CRDT the
/// editor really edits, the exporter, and the importer again.
///
/// Each layer has its own test, and each of those passes with the layer *next* to it
/// broken — the exporter's round-trip tests never touch Yjs, and `gw-collab`'s fixtures
/// never touch markdown. `checked` is a boolean that defaults to false in three different
/// places, so the failure this catches is not "the checklist disappeared" but "every box
/// came back empty", which no single-layer test would notice.
#[test]
fn a_checkbox_survives_import_the_crdt_export_and_re_import() {
    let source = "- [ ] Milch kaufen\n- [x] Brot geholt\n  - [x] Vollkorn\n- plain\n";

    let imported = markdown::convert(source).doc;
    assert_eq!(
        boxes(&imported),
        vec![false, true, true],
        "the fixture must hold both states and a nested one"
    );

    let through_crdt = gw_collab::CollabDoc::from_block(&imported).to_block();
    assert_eq!(
        serde_json::to_value(&through_crdt).unwrap(),
        serde_json::to_value(&imported).unwrap(),
        "the CRDT changed the tree"
    );

    let exported = blocks_to_markdown(&through_crdt);
    let reimported = markdown::convert(&exported).doc;
    assert_eq!(
        serde_json::to_value(&reimported).unwrap(),
        serde_json::to_value(&imported).unwrap(),
        "the exported markdown re-imports as a different tree\n--- exported ---\n{exported}\n---"
    );
    assert_eq!(
        boxes(&reimported),
        vec![false, true, true],
        "every box must come back the way it went in: {exported}"
    );
    assert_eq!(reimported.plain_text(), imported.plain_text());
}

/// Every `checked` in the tree, in document order.
fn boxes(block: &Block) -> Vec<bool> {
    let mut out = Vec::new();
    fn walk(block: &Block, out: &mut Vec<bool>) {
        if block.kind == gw_core::BlockKind::TaskItem {
            out.push(
                block
                    .attrs
                    .get("checked")
                    .and_then(|v| v.as_bool())
                    .expect("a task item always states `checked`"),
            );
        }
        for child in &block.content {
            walk(child, out);
        }
    }
    walk(block, &mut out);
    out
}
