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

/// Inline formatting on a text leaf, shaped exactly like a ProseMirror mark.
///
/// A link carries EITHER `doc` (an internal target, per D-5) or `href` (external, or an
/// internal one that could not be resolved). Never both: `target_doc` reading an `href`
/// as an id would turn a URL into a document reference.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Leaves are joined with a space and runs of whitespace are then collapsed, rather
    /// than concatenated directly. Direct concatenation fuses the last word of one block
    /// to the first word of the next — a heading "…Maß" followed by a paragraph "Ein…"
    /// becomes the token "MaßEin", which is in the index and matches nothing anyone would
    /// ever search for. The collapse is what keeps this identical to the TypeScript
    /// implementation, which joins the same way.
    pub fn plain_text(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        self.collect_text(&mut parts);
        parts
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn collect_text<'a>(&'a self, out: &mut Vec<&'a str>) {
        if let Some(t) = &self.text {
            out.push(t);
        }
        for child in &self.content {
            child.collect_text(out);
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
