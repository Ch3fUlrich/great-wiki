use crate::slugify;
use serde::{Deserialize, Serialize};

/// The node kinds M1 understands. The registry in M4 adds more; this enum is
/// `#[non_exhaustive]` so adding one is not a breaking change for downstream matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum BlockKind {
    Doc,
    Paragraph,
    Heading,
    BulletList,
    OrderedList,
    ListItem,
    /// A checklist: `TaskItem` children and nothing else.
    ///
    /// A kind of its own rather than a `checked` attribute on `ListItem`, because that is
    /// how the editor models it — TipTap ships `taskList` and `taskItem` as two extensions
    /// — and this enum mirrors the editor exactly so that nothing has to be translated
    /// between what is edited and what is stored.
    ///
    /// It is also what lets a mixed markdown list stay honest. `- [ ] a` followed by
    /// `- plain` imports as a task list *and* a bullet list, not as one list whose second
    /// line acquired `checked: false`. A checkbox line is a to-do (D-6), so inventing one
    /// on a line nobody marked would put a task on somebody's board that they never wrote.
    TaskList,
    /// A checklist line, carrying `checked`.
    ///
    /// `checked` is always written, including when it is `false`: an unticked box and no
    /// box at all are different documents, and an attribute that disappears at its default
    /// makes them the same one.
    ///
    /// Holds block content like a `ListItem` does — a paragraph, not bare text — so a task
    /// can grow a second paragraph or a nested list without changing kind.
    ///
    /// It carries no id. The data model gives a task a uuid, and the *store* mints it
    /// during reconciliation on publish; the markdown converter is a pure function of its
    /// input and must stay one, because the exporter re-imports its own output and
    /// compares it against the stored document.
    TaskItem,
    Blockquote,
    CodeBlock,
    /// A table: `TableRow` children and nothing else.
    Table,
    /// A row: `TableCell` or `TableHeader` children and nothing else.
    TableRow,
    /// A header cell (`th`). ProseMirror models the header as a *cell* kind rather than a
    /// row kind, and so does this: a renderer can then choose `th` over `td` from the cell
    /// alone, without having to know what its ancestors were.
    ///
    /// Cells hold block content — a paragraph, not bare text — so a cell can hold a list
    /// or a second paragraph the day the editor allows one.
    TableHeader,
    /// A body cell (`td`).
    TableCell,
    Text,
}

/// The inline formatting marks M1 understands, shaped exactly like a ProseMirror mark.
/// `#[non_exhaustive]` for the same reason as `BlockKind`: adding one is not a breaking
/// change for downstream matches, which must therefore carry a wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub enum MarkKind {
    Strong,
    Em,
    Code,
    Strike,
    Link,
}

/// The order a text leaf's `marks` are stored in: **outermost first, innermost last**.
///
/// A leaf's `marks` array is a nesting, not a set, and markdown can only write a nesting —
/// `**[a](url)**` and `[**a**](url)` are the same rendered text and two different arrays.
/// The importer sorts every leaf's marks into this order, so both spellings store the same
/// tree, and `gw-api`'s exporter writes them back out in the same order it finds them. That
/// agreement is the whole reason this constant exists in the *lower* crate rather than in
/// the exporter: two orders that agree by coincidence stop agreeing the day one is edited,
/// and the failure is an export that refuses every page holding a nested mark.
///
/// `Code` is last, and that is not arbitrary: a code span's content is literal CommonMark,
/// so nothing can be nested *inside* one. It is a leaf's base representation rather than a
/// wrapper around it, which makes it the innermost mark by definition.
pub const MARK_ORDER: [MarkKind; 5] = [
    MarkKind::Strong,
    MarkKind::Em,
    MarkKind::Strike,
    MarkKind::Link,
    MarkKind::Code,
];

impl MarkKind {
    /// Where this kind sits in [`MARK_ORDER`] — the sort key that puts a leaf's marks in
    /// canonical order. A kind that is not listed (a later milestone's, arriving through
    /// `#[non_exhaustive]`) ranks past the end rather than panicking, so it sorts innermost
    /// and the renderer that cannot write it is the one that refuses.
    pub fn nesting_rank(self) -> usize {
        MARK_ORDER
            .iter()
            .position(|&k| k == self)
            .unwrap_or(usize::MAX)
    }
}

/// Inline formatting on a text leaf, shaped exactly like a ProseMirror mark.
///
/// A link carries EITHER `doc` (an internal target, per D-5) or `href` (external, or an
/// internal one that could not be resolved). Never both: `target_doc` reading an `href`
/// as an id would turn a URL into a document reference.
///
/// Equality is by kind *and* attrs, because that is what decides whether two neighbouring
/// leaves are one run of formatting or two: `[a](u)` beside `[b](u)` is one link, and
/// `[a](u)` beside `[b](v)` is two.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    pub kind: MarkKind,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub attrs: serde_json::Map<String, serde_json::Value>,
}

impl Mark {
    pub fn link_to_doc(id: &str) -> Self {
        let mut attrs = serde_json::Map::new();
        attrs.insert("doc".into(), serde_json::Value::String(id.to_string()));
        Mark {
            kind: MarkKind::Link,
            attrs,
        }
    }

    pub fn link_to_url(url: &str) -> Self {
        let mut attrs = serde_json::Map::new();
        attrs.insert("href".into(), serde_json::Value::String(url.to_string()));
        Mark {
            kind: MarkKind::Link,
            attrs,
        }
    }

    /// `Some` only for a `Link` mark carrying an internal `doc` target; never reads `href`.
    pub fn target_doc(&self) -> Option<&str> {
        if self.kind != MarkKind::Link {
            return None;
        }
        self.attrs.get("doc").and_then(|v| v.as_str())
    }
}

/// A node in the document tree, shaped exactly like a ProseMirror node.
///
/// Matching the editor's own representation means there is no translation layer between
/// what is edited and what is stored — and therefore nowhere for fidelity to be lost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub attrs: serde_json::Map<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<Block>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<Mark>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub id: String,
}

impl Block {
    /// Every text leaf in document order, separated so words cannot fuse across blocks.
    ///
    /// This feeds the search index and the embedding chunker, so it must be stable: two
    /// documents that read identically must produce identical text.
    ///
    /// A **block** boundary is written as a space and runs of whitespace are then
    /// collapsed. Without the space the last word of one block fuses to the first of the
    /// next — a heading "…Maß" followed by a paragraph "Ein…" becomes the token "MaßEin",
    /// which is in the index and matches nothing anyone would ever search for.
    ///
    /// Adjacent **inline** leaves of one parent get no separator, because they are one run
    /// of prose that a mark boundary happened to split: `Siehe [das Handbuch](/h).` is
    /// three leaves and one sentence, and a space between them would put the full stop off
    /// the end of the word — "Siehe das Handbuch ." — in the search index, in the reader's
    /// table of contents, in a heading's anchor id, and in the seeder's exact comparison of
    /// a body heading against the page title.
    ///
    /// `web/src/lib/blocks/render.ts::plainText` is a deliberate mirror of this and must
    /// stay byte-identical; the shared cases in `PLAIN_TEXT_CASES` are duplicated in its
    /// test suite so a drift turns one of them red.
    pub fn plain_text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn collect_text(&self, out: &mut String) {
        if let Some(t) = &self.text {
            out.push_str(t);
        }
        let mut previous: Option<BlockKind> = None;
        for child in &self.content {
            // A boundary is where a block begins or ends. Two text leaves side by side in
            // one parent are neither, so nothing is written between them.
            if child.kind != BlockKind::Text || previous.is_some_and(|p| p != BlockKind::Text) {
                out.push(' ');
            }
            child.collect_text(out);
            previous = Some(child.kind);
        }
    }

    /// Extract the heading outline, with an ASCII anchor id for each.
    ///
    /// The id goes through `slugify`, so a German heading yields a fragment that needs no
    /// percent-encoding and survives being copied out of the address bar.
    pub fn headings(&self) -> Vec<Heading> {
        let mut out = Vec::new();
        self.collect_headings(&mut out);
        out
    }

    fn collect_headings(&self, out: &mut Vec<Heading>) {
        if self.kind == BlockKind::Heading {
            let level = self
                .attrs
                .get("level")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .clamp(1, 6) as u8;
            let text = self.plain_text();
            out.push(Heading {
                level,
                id: slugify(&text),
                text,
            });
            return; // headings do not nest
        }
        for child in &self.content {
            child.collect_headings(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::block::{Block, BlockKind, Mark};

    fn sample() -> Block {
        serde_json::from_str(
            r#"{
              "kind": "doc",
              "content": [
                {"kind": "heading", "attrs": {"level": 2},
                 "content": [{"kind": "text", "text": "Größe und Maß"}]},
                {"kind": "paragraph",
                 "content": [{"kind": "text", "text": "Ein Satz."},
                             {"kind": "text", "text": " Noch einer."}]}
              ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn deserialises_a_prosemirror_shaped_tree() {
        let doc = sample();
        assert_eq!(doc.kind, BlockKind::Doc);
        assert_eq!(doc.content.len(), 2);
        assert_eq!(doc.content[0].kind, BlockKind::Heading);
    }

    #[test]
    fn plain_text_concatenates_leaves_in_order() {
        assert_eq!(sample().plain_text(), "Größe und Maß Ein Satz. Noch einer.");
    }

    /// The cases `web/src/lib/blocks/render.test.ts` duplicates verbatim. The two
    /// implementations are deliberate mirrors — `plainText` feeds the reader's outline,
    /// its heading anchor ids and its table column labels, and this feeds the search index
    /// and the seeder's duplicate-title check, so a disagreement puts a different heading
    /// in the table of contents than the one the anchor points at. If they ever drift,
    /// one of the two suites goes red, which is the whole point of duplicating them.
    const PLAIN_TEXT_CASES: &[(&str, &str)] = &[
        // Adjacent inline leaves of ONE paragraph are one run of prose: a mark boundary
        // splits a sentence into leaves, and no space may appear where the split was.
        (
            r#"{"kind":"paragraph","content":[
                 {"kind":"text","text":"Siehe "},
                 {"kind":"text","text":"das Handbuch","marks":[{"kind":"link","attrs":{"href":"/h"}}]},
                 {"kind":"text","text":"."}]}"#,
            "Siehe das Handbuch.",
        ),
        (
            r#"{"kind":"paragraph","content":[
                 {"kind":"text","text":"Der "},
                 {"kind":"text","text":"Darm","marks":[{"kind":"strong"}]},
                 {"kind":"text","text":"-Trakt"}]}"#,
            "Der Darm-Trakt",
        ),
        // …and a BLOCK boundary still separates, or the last word of one block fuses to
        // the first of the next and the index holds a token nobody will ever search for.
        (
            r#"{"kind":"doc","content":[
                 {"kind":"heading","content":[{"kind":"text","text":"Maß"}]},
                 {"kind":"paragraph","content":[{"kind":"text","text":"Einheit"}]}]}"#,
            "Maß Einheit",
        ),
    ];

    #[test]
    fn adjacent_inline_leaves_are_one_run_of_prose_but_blocks_stay_separated() {
        for (json, expected) in PLAIN_TEXT_CASES {
            let block: Block = serde_json::from_str(json).unwrap();
            assert_eq!(&block.plain_text(), expected);
        }
    }

    #[test]
    fn headings_carry_a_transliterated_anchor_id() {
        let headings = sample().headings();
        assert_eq!(headings.len(), 1);
        assert_eq!(headings[0].level, 2);
        assert_eq!(headings[0].text, "Größe und Maß");
        // The anchor must be ASCII or the URL fragment needs percent-encoding.
        assert_eq!(headings[0].id, "groesse-und-mass");
    }

    #[test]
    fn heading_level_defaults_to_one_when_absent() {
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"heading","content":[{"kind":"text","text":"T"}]}]}"#,
        )
        .unwrap();
        assert_eq!(doc.headings()[0].level, 1);
    }

    #[test]
    fn table_cells_are_separated_in_plain_text() {
        // Written as JSON rather than built in Rust so the camelCase wire names are pinned
        // here too: `web/src/lib/blocks/render.ts` mirrors this enum and cannot see it.
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"table","content":[
                 {"kind":"tableRow","content":[
                   {"kind":"tableHeader","attrs":{"align":"right"},
                    "content":[{"kind":"paragraph","content":[{"kind":"text","text":"Länge"}]}]},
                   {"kind":"tableCell",
                    "content":[{"kind":"paragraph","content":[{"kind":"text","text":"Meter"}]}]}]}]}]}"#,
        )
        .unwrap();
        assert_eq!(doc.content[0].kind, BlockKind::Table);
        assert_eq!(doc.content[0].content[0].kind, BlockKind::TableRow);
        assert_eq!(
            doc.content[0].content[0].content[0].kind,
            BlockKind::TableHeader
        );
        // Exact, not `contains`: a substring check passes on the fused token "LängeMeter",
        // which is the failure this separation exists to prevent.
        assert_eq!(doc.plain_text(), "Länge Meter");
    }

    #[test]
    fn a_table_contributes_no_headings_to_the_outline() {
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"table","content":[
                 {"kind":"tableRow","content":[
                   {"kind":"tableHeader",
                    "content":[{"kind":"paragraph","content":[{"kind":"text","text":"Feld"}]}]}]}]}]}"#,
        )
        .unwrap();
        assert!(
            doc.headings().is_empty(),
            "a column title is not a section of the document"
        );
    }

    #[test]
    fn round_trips_through_json_unchanged() {
        let doc = sample();
        let again: Block = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(again.plain_text(), doc.plain_text());
        assert_eq!(again.headings().len(), doc.headings().len());
    }

    #[test]
    fn a_mark_round_trips_through_json_and_an_absent_marks_field_is_empty() {
        let b: Block = serde_json::from_str(
            r#"{"kind":"text","text":"hallo","marks":[{"kind":"strong"},
                 {"kind":"link","attrs":{"doc":"019ff0"}}]}"#,
        )
        .unwrap();
        assert_eq!(b.marks.len(), 2);
        assert_eq!(b.marks[1].target_doc(), Some("019ff0"));

        // A block written before marks existed must still parse, and must not grow a key.
        let old: Block = serde_json::from_str(r#"{"kind":"text","text":"hallo"}"#).unwrap();
        assert!(old.marks.is_empty());
        assert_eq!(
            serde_json::to_string(&old).unwrap(),
            r#"{"kind":"text","text":"hallo"}"#
        );
    }

    #[test]
    fn an_external_link_is_not_a_document_reference() {
        let m = Mark::link_to_url("https://example.org");
        assert_eq!(
            m.target_doc(),
            None,
            "an href must never be read as a document id"
        );
    }
}
