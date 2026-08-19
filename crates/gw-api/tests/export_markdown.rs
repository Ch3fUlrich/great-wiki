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
