//! CommonMark → `Block` tree.
//!
//! This is one half of the export round-trip (M12) and the foundation of the Markdown
//! importer (M13), so it is written to be exhaustive rather than convenient. The rule it
//! obeys everywhere: **text is never dropped**. A construct the M1 block schema cannot
//! represent yet keeps its text in the nearest block that *can* hold it, and says so in
//! `Conversion::notes` — a silent loss is the one outcome that cannot be detected later.

use crate::block::{Block, BlockKind, Mark, MarkKind};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::BTreeMap;
use std::fmt;

/// A markdown construct the M1 block schema cannot represent, and what became of it.
///
/// Kept as an enum rather than a string so a caller can match on one, and so the
/// milestone that adds the missing block type deletes a variant and gets a compile error
/// at every site that still expects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Unsupported {
    /// An image. The alt text survives as text; the source URL does not.
    Image,
    /// A link. The link text survives; the destination does not.
    LinkTarget,
    /// Bold, italic, strikethrough or inline code. The text survives; the emphasis does not.
    InlineMarks,
    /// A thematic break (`---`). Carries no text.
    HorizontalRule,
    /// Raw HTML, inline or block. Kept verbatim as text rather than parsed or dropped.
    Html,
    /// A checkbox in a task list.
    TaskListMarker,
    /// An event this converter was not written against — future-proofing, not a known gap.
    Unrecognised,
}

impl Unsupported {
    /// A short, stable key. Used in messages and assertions rather than the `Debug` form,
    /// which is free to change.
    pub fn key(self) -> &'static str {
        match self {
            Unsupported::Image => "image",
            Unsupported::LinkTarget => "link",
            Unsupported::InlineMarks => "inline-marks",
            Unsupported::HorizontalRule => "horizontal-rule",
            Unsupported::Html => "html",
            Unsupported::TaskListMarker => "task-list-marker",
            Unsupported::Unrecognised => "unrecognised",
        }
    }

    /// What happened to the construct's content, and which milestone closes the gap.
    pub fn disposition(self) -> &'static str {
        match self {
            Unsupported::Image => "alt text kept, source URL dropped — M6 adds image blocks",
            Unsupported::LinkTarget => "link text kept, destination dropped — M4 adds inline marks",
            Unsupported::InlineMarks => "text kept, emphasis dropped — M4 adds inline marks",
            Unsupported::HorizontalRule => "dropped; it carries no text — M4 adds the rule block",
            Unsupported::Html => "kept verbatim as text, never parsed — M4 decides its fate",
            Unsupported::TaskListMarker => {
                "checkbox state dropped, item text kept — M4 adds task lists"
            }
            Unsupported::Unrecognised => {
                "skipped by a converter that predates it — report this, it is a bug"
            }
        }
    }
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({})", self.key(), self.disposition())
    }
}

/// How often one unsupported construct occurred in a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pub construct: Unsupported,
    pub count: usize,
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}× {}", self.count, self.construct)
    }
}

/// The result of a conversion: the document, plus everything that could not be modelled.
#[derive(Debug, Clone)]
pub struct Conversion {
    pub doc: Block,
    /// Sorted and de-duplicated, so a document with ninety links produces one line of
    /// output rather than ninety. Empty means the conversion was lossless.
    pub notes: Vec<Note>,
}

/// Convert markdown to a `Doc` block, discarding the report.
///
/// Prefer [`convert`] anywhere the caller can surface what was lost.
pub fn markdown_to_blocks(md: &str) -> Block {
    convert(md).doc
}

/// Convert markdown to a `Doc` block and report what the schema could not hold.
pub fn convert(md: &str) -> Conversion {
    let mut builder = Builder::new();
    let parser = Parser::new_ext(md, options());
    for event in parser {
        builder.handle(event);
    }
    builder.finish()
}

/// Tables and strikethrough are enabled deliberately. Tables have block kinds of their
/// own; strikethrough does not, but parsing it means its text is *seen* and can be
/// preserved, whereas leaving the extension off makes a `~~word~~` keep its tildes.
/// Seeing more than we can model is recoverable; not seeing it is not.
fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH
}

/// A frame on the open-block stack.
struct Frame {
    block: Block,
    /// True when this block was opened by the converter rather than by a markdown tag —
    /// the paragraph a tight list item needs but does not declare. Implicit frames close
    /// themselves at the next block boundary.
    implicit: bool,
}

/// The table currently open: what the delimiter row said, and where in the row we are.
///
/// One is enough because GFM tables cannot nest — a cell's content is inline, so no table
/// can begin inside one.
struct Table {
    /// Column alignments, in column order. `Tag::Table` carries them once, but they belong
    /// on the cells, which are the only thing a renderer can act on.
    alignments: Vec<Alignment>,
    /// The column the next `TableCell` opens. Reset at every row.
    column: usize,
    /// Whether the open row is the header row, which decides `TableHeader` over `TableCell`.
    in_head: bool,
}

struct Builder {
    stack: Vec<Frame>,
    /// Fenced and indented code arrives as one `Text` event per line. They are joined here
    /// rather than becoming one text leaf each, because separate leaves lose the newlines
    /// and a code block without its line breaks is not the same code block.
    code: Option<String>,
    table: Option<Table>,
    losses: BTreeMap<Unsupported, usize>,
    /// The marks currently open, innermost last. `Start(Tag::Strong | Emphasis |
    /// Strikethrough | Link)` pushes, the matching `End` pops, and every text leaf is
    /// stamped with a clone of the whole stack — that is what lets `**bold *and
    /// italic***` land on one leaf carrying both marks.
    active: Vec<Mark>,
}

impl Builder {
    fn new() -> Self {
        Self {
            stack: vec![Frame {
                block: block(BlockKind::Doc),
                implicit: false,
            }],
            code: None,
            table: None,
            losses: BTreeMap::new(),
            active: Vec::new(),
        }
    }

    fn finish(mut self) -> Conversion {
        self.close_implicit();
        // Any frame still open means unbalanced input; close it rather than lose its text.
        while self.stack.len() > 1 {
            self.pop();
        }
        let notes = self
            .losses
            .into_iter()
            .map(|(construct, count)| Note { construct, count })
            .collect();
        Conversion {
            doc: self
                .stack
                .pop()
                .expect("the doc frame is never popped")
                .block,
            notes,
        }
    }

    fn note(&mut self, what: Unsupported) {
        *self.losses.entry(what).or_insert(0) += 1;
    }

    fn top(&mut self) -> &mut Block {
        &mut self
            .stack
            .last_mut()
            .expect("the doc frame is never popped")
            .block
    }

    fn push(&mut self, block: Block, implicit: bool) {
        self.stack.push(Frame { block, implicit });
    }

    fn pop(&mut self) {
        if self.stack.len() == 1 {
            return; // never pop the doc
        }
        let frame = self.stack.pop().expect("length checked above");
        self.top().content.push(frame.block);
    }

    fn close_implicit(&mut self) {
        while self.stack.last().is_some_and(|f| f.implicit) {
            self.pop();
        }
    }

    /// Open a block, first closing any paragraph the converter opened on its own.
    fn open(&mut self, block: Block) {
        self.close_implicit();
        self.push(block, false);
    }

    /// Close a block opened by `open`.
    fn close(&mut self) {
        self.close_implicit();
        self.pop();
    }

    /// Append inline text, opening a paragraph first if the innermost block cannot hold it.
    ///
    /// A tight list item (`- eins`) emits its text with no paragraph tag at all, and
    /// ProseMirror's `list_item` requires a paragraph. Without this the text would attach
    /// directly to the list item and the tree would be unrenderable.
    ///
    /// Stamps a clone of the active mark stack onto the leaf. `Event::Code` is the one
    /// caller that does not want the plain stack — it goes through [`Self::marked_text`]
    /// instead, with `MarkKind::Code` appended.
    fn text(&mut self, s: &str) {
        let marks = self.active.clone();
        self.marked_text(s, marks);
    }

    /// Append inline text carrying exactly `marks`, merging into the previous leaf only
    /// when its marks match — otherwise `**bold** plain` would fuse into one leaf and the
    /// mark boundary would be lost along with it.
    fn marked_text(&mut self, s: &str, marks: Vec<Mark>) {
        if let Some(code) = self.code.as_mut() {
            code.push_str(s);
            return;
        }
        if !matches!(self.top().kind, BlockKind::Paragraph | BlockKind::Heading) {
            self.push(block(BlockKind::Paragraph), true);
        }
        match self.top().content.last_mut() {
            // Merge into the previous leaf so two text events for the same run of marks
            // become one text node, not two.
            Some(prev) if prev.kind == BlockKind::Text && marks_equal(&prev.marks, &marks) => {
                prev.text.get_or_insert_with(String::new).push_str(s);
            }
            _ => {
                let mut leaf = block(BlockKind::Text);
                leaf.text = Some(s.to_string());
                leaf.marks = marks;
                self.top().content.push(leaf);
            }
        }
    }

    /// Open a table row, remembering whether its cells are header cells and restarting the
    /// column count — alignment is per column, so a row that miscounts bends the table.
    fn open_row(&mut self, in_head: bool) {
        if let Some(table) = &mut self.table {
            table.column = 0;
            table.in_head = in_head;
        }
        self.open(block(BlockKind::TableRow));
    }

    /// Every `Event` variant is matched by name, with no catch-all arm. That is
    /// deliberate: the day a pulldown-cmark upgrade adds an event, this fails to compile
    /// instead of quietly discarding whatever the new event carried.
    fn handle(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                // Inline code carries no `Start`/`End` pair of its own — it arrives as one
                // leaf event — so the `Code` mark is appended here rather than pushed onto
                // `active`. Whatever marks were already open (`**`code`**`) still apply.
                let mut marks = self.active.clone();
                marks.push(mark(MarkKind::Code));
                self.marked_text(&t, marks);
            }
            // Raw HTML is kept as text rather than parsed. Parsing it here would create a
            // second, untrusted path into the block tree; dropping it would lose content.
            Event::Html(t) | Event::InlineHtml(t) => {
                self.note(Unsupported::Html);
                self.text(&t);
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => {
                // Not enabled in `options`, so unreachable today. Kept so enabling the
                // extension later cannot silently drop the formula.
                self.note(Unsupported::Unrecognised);
                self.text(&t);
            }
            Event::FootnoteReference(t) => {
                self.note(Unsupported::Unrecognised);
                self.text(&t);
            }
            // A soft break is a line wrap in the source and a space in the output; a hard
            // break is a `<br>`, which has no block kind yet, so it degrades to the same.
            Event::SoftBreak | Event::HardBreak => self.text(" "),
            Event::Rule => {
                self.close_implicit();
                self.note(Unsupported::HorizontalRule);
            }
            Event::TaskListMarker(_) => self.note(Unsupported::TaskListMarker),
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.open(block(BlockKind::Paragraph)),
            Tag::Heading { level, .. } => {
                let mut b = block(BlockKind::Heading);
                b.attrs
                    .insert("level".into(), serde_json::Value::from(level as u8));
                self.open(b);
            }
            Tag::BlockQuote(_) => self.open(block(BlockKind::Blockquote)),
            Tag::CodeBlock(kind) => {
                let mut b = block(BlockKind::CodeBlock);
                if let CodeBlockKind::Fenced(info) = &kind {
                    // ```rust,ignore — the info string's first word is the language, the
                    // rest is renderer-specific and has no home in the schema yet.
                    let lang = info.split(|c: char| c == ',' || c.is_whitespace()).next();
                    if let Some(lang) = lang.filter(|l| !l.is_empty()) {
                        b.attrs
                            .insert("language".into(), serde_json::Value::from(lang));
                    }
                }
                self.open(b);
                self.code = Some(String::new());
            }
            Tag::List(Some(start)) => {
                let mut b = block(BlockKind::OrderedList);
                // Only recorded when it is not the default, so the common case produces no
                // attrs and the JSON stays comparable across importers.
                if start != 1 {
                    b.attrs
                        .insert("start".into(), serde_json::Value::from(start));
                }
                self.open(b);
            }
            Tag::List(None) => self.open(block(BlockKind::BulletList)),
            Tag::Item => self.open(block(BlockKind::ListItem)),
            Tag::Table(alignments) => {
                self.table = Some(Table {
                    alignments,
                    column: 0,
                    in_head: false,
                });
                self.open(block(BlockKind::Table));
            }
            Tag::TableHead => self.open_row(true),
            Tag::TableRow => self.open_row(false),
            Tag::TableCell => {
                // Each cell is its own block, which is also what keeps `| Länge | Meter |`
                // from becoming the token "LängeMeter" in the search index: `plain_text`
                // separates blocks, and a cell boundary carries no character of its own.
                let (kind, align) = match &mut self.table {
                    Some(table) => {
                        // A row with more cells than the delimiter row declared is legal
                        // GFM; the surplus simply has no stated alignment.
                        let align = table
                            .alignments
                            .get(table.column)
                            .copied()
                            .unwrap_or(Alignment::None);
                        table.column += 1;
                        let kind = if table.in_head {
                            BlockKind::TableHeader
                        } else {
                            BlockKind::TableCell
                        };
                        (kind, align)
                    }
                    // Unreachable: a cell only ever arrives inside a table.
                    None => (BlockKind::TableCell, Alignment::None),
                };
                let mut b = block(kind);
                if let Some(name) = align_name(align) {
                    b.attrs
                        .insert("align".into(), serde_json::Value::from(name));
                }
                self.open(b);
            }
            Tag::Image { .. } => self.note(Unsupported::Image),
            // This crate has no store, so a markdown link can never be resolved to a
            // document id here — `[text](/darm/labor)` becomes an external href exactly
            // like `[text](https://example.org)`, regardless of how internal it looks.
            // Task 7 resolves internal-looking destinations against the store on publish;
            // that resolution does not belong in this crate and must not be guessed here.
            Tag::Link { dest_url, .. } => self.active.push(Mark::link_to_url(&dest_url)),
            Tag::Emphasis => self.active.push(mark(MarkKind::Em)),
            Tag::Strong => self.active.push(mark(MarkKind::Strong)),
            Tag::Strikethrough => self.active.push(mark(MarkKind::Strike)),
            Tag::HtmlBlock => self.note(Unsupported::Html),
            // Footnotes, definition lists and metadata blocks need extensions that
            // `options` does not enable, so these are unreachable today. Their text still
            // flows through `Event::Text` if one is ever switched on.
            _ => self.note(Unsupported::Unrecognised),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph
            | TagEnd::Heading(_)
            | TagEnd::BlockQuote(_)
            | TagEnd::List(_)
            | TagEnd::Item
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell => self.close(),
            TagEnd::Table => {
                self.table = None;
                self.close();
            }
            TagEnd::CodeBlock => {
                if let Some(code) = self.code.take() {
                    // The fence supplies the final newline, so storing it would grow the
                    // block by one blank line on every import/export cycle.
                    let code = code.strip_suffix('\n').unwrap_or(&code).to_string();
                    if !code.is_empty() {
                        let mut leaf = block(BlockKind::Text);
                        leaf.text = Some(code);
                        self.top().content.push(leaf);
                    }
                }
                self.close();
            }
            TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link => {
                self.active.pop();
            }
            // Images open no frame and push no mark, so they close neither.
            _ => {}
        }
    }
}

/// A mark with no attrs — every kind but `Link`, which carries its destination instead.
fn mark(kind: MarkKind) -> Mark {
    Mark {
        kind,
        attrs: serde_json::Map::new(),
    }
}

/// Whether two mark stacks are the same run, so [`Builder::marked_text`] knows whether the
/// next leaf can merge into the previous one. `Mark` has no `PartialEq` of its own — `kind`
/// and `attrs` each do, so this compares them field by field instead of deriving one.
fn marks_equal(a: &[Mark], b: &[Mark]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.kind == y.kind && x.attrs == y.attrs)
}

/// The `align` attribute for a column, or `None` where the table states no alignment.
///
/// A column with no stated alignment writes no attribute at all, so the common case
/// produces no attrs and the JSON stays comparable across importers — the same rule the
/// list `start` attribute follows.
fn align_name(align: Alignment) -> Option<&'static str> {
    match align {
        Alignment::Left => Some("left"),
        Alignment::Center => Some("center"),
        Alignment::Right => Some("right"),
        Alignment::None => None,
    }
}

fn block(kind: BlockKind) -> Block {
    Block {
        kind,
        attrs: serde_json::Map::new(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::block::{Block, BlockKind, MarkKind};
    use crate::markdown::{convert, markdown_to_blocks, Unsupported};

    fn keys(md: &str) -> Vec<&'static str> {
        convert(md)
            .notes
            .iter()
            .map(|n| n.construct.key())
            .collect()
    }

    /// Every `Text` leaf in document order, so a test can find one by its content without
    /// caring which block it landed in.
    fn collect_text_leaves(block: &Block) -> Vec<&Block> {
        fn walk<'a>(b: &'a Block, out: &mut Vec<&'a Block>) {
            if b.kind == BlockKind::Text {
                out.push(b);
            }
            for child in &b.content {
                walk(child, out);
            }
        }
        let mut out = Vec::new();
        walk(block, &mut out);
        out
    }

    /// The kind of each child, so a row's cells can be asserted in one line.
    fn kinds_of(block: &Block) -> Vec<BlockKind> {
        block.content.iter().map(|c| c.kind).collect()
    }

    /// The `align` attribute of each child, `None` where the column states none.
    fn aligns_of(row: &Block) -> Vec<Option<&str>> {
        row.content
            .iter()
            .map(|c| c.attrs.get("align").and_then(|v| v.as_str()))
            .collect()
    }

    #[test]
    fn empty_input_is_an_empty_doc() {
        let doc = markdown_to_blocks("");
        assert_eq!(doc.kind, BlockKind::Doc);
        assert!(doc.content.is_empty());
    }

    #[test]
    fn a_paragraph_becomes_a_paragraph_with_a_text_leaf() {
        let doc = markdown_to_blocks("Ein Satz.");
        assert_eq!(doc.content.len(), 1);
        assert_eq!(doc.content[0].kind, BlockKind::Paragraph);
        assert_eq!(doc.content[0].content[0].kind, BlockKind::Text);
        assert_eq!(doc.content[0].content[0].text.as_deref(), Some("Ein Satz."));
    }

    #[test]
    fn atx_headings_carry_their_level() {
        let doc = markdown_to_blocks("# Eins\n\n### Drei\n");
        let headings = doc.headings();
        assert_eq!(headings.len(), 2);
        assert_eq!((headings[0].level, headings[0].text.as_str()), (1, "Eins"));
        assert_eq!((headings[1].level, headings[1].text.as_str()), (3, "Drei"));
        assert_eq!(
            doc.content[0].attrs.get("level").and_then(|v| v.as_u64()),
            Some(1)
        );
    }

    #[test]
    fn a_tight_bullet_list_wraps_each_item_in_a_paragraph() {
        // ProseMirror's list_item requires a paragraph. A tight markdown list declares
        // none, so the converter must supply it or the tree cannot be rendered.
        let doc = markdown_to_blocks("- eins\n- zwei\n");
        let list = &doc.content[0];
        assert_eq!(list.kind, BlockKind::BulletList);
        assert_eq!(list.content.len(), 2);
        assert_eq!(list.content[0].kind, BlockKind::ListItem);
        assert_eq!(list.content[0].content[0].kind, BlockKind::Paragraph);
        assert_eq!(doc.plain_text(), "eins zwei");
    }

    #[test]
    fn an_ordered_list_is_ordered_and_keeps_a_non_default_start() {
        let doc = markdown_to_blocks("1. eins\n2. zwei\n");
        assert_eq!(doc.content[0].kind, BlockKind::OrderedList);
        assert!(
            doc.content[0].attrs.is_empty(),
            "a list starting at 1 needs no attrs"
        );

        let doc = markdown_to_blocks("3. drei\n4. vier\n");
        assert_eq!(
            doc.content[0].attrs.get("start").and_then(|v| v.as_u64()),
            Some(3)
        );
    }

    #[test]
    fn nested_lists_nest() {
        let doc = markdown_to_blocks("- eins\n  - eins-a\n");
        let outer_item = &doc.content[0].content[0];
        let nested = outer_item
            .content
            .iter()
            .find(|b| b.kind == BlockKind::BulletList)
            .expect("the inner list must be a child of the outer item");
        assert_eq!(nested.content.len(), 1);
        assert_eq!(doc.plain_text(), "eins eins-a");
    }

    #[test]
    fn a_blockquote_holds_its_paragraphs() {
        let doc = markdown_to_blocks("> Zitat.\n");
        assert_eq!(doc.content[0].kind, BlockKind::Blockquote);
        assert_eq!(doc.content[0].content[0].kind, BlockKind::Paragraph);
        assert_eq!(doc.plain_text(), "Zitat.");
    }

    #[test]
    fn a_fenced_code_block_carries_its_language() {
        let doc = markdown_to_blocks("```rust\nfn main() {}\n```\n");
        let code = &doc.content[0];
        assert_eq!(code.kind, BlockKind::CodeBlock);
        assert_eq!(
            code.attrs.get("language").and_then(|v| v.as_str()),
            Some("rust")
        );
        assert_eq!(code.content[0].text.as_deref(), Some("fn main() {}"));
    }

    #[test]
    fn a_fenced_code_block_without_a_language_has_no_language_attr() {
        let doc = markdown_to_blocks("```\nplain\n```\n");
        assert_eq!(doc.content[0].kind, BlockKind::CodeBlock);
        assert!(doc.content[0].attrs.is_empty());
    }

    #[test]
    fn code_keeps_its_line_breaks() {
        // One text leaf per line would lose the newlines, and code without line breaks is
        // not the same code.
        let doc = markdown_to_blocks("```sh\neins\nzwei\n```\n");
        assert_eq!(doc.content[0].content.len(), 1);
        assert_eq!(
            doc.content[0].content[0].text.as_deref(),
            Some("eins\nzwei")
        );
    }

    #[test]
    fn a_table_becomes_a_table_block_with_a_header_row_and_body_rows() {
        let md = "| Feld | Wert |\n| --- | --- |\n| Größe | 42 |\n| Breite | 7 |\n";
        let conversion = convert(md);

        assert_eq!(
            conversion.doc.content.len(),
            1,
            "one table, not one block per row"
        );
        let table = &conversion.doc.content[0];
        assert_eq!(table.kind, BlockKind::Table);
        assert_eq!(table.content.len(), 3, "a header row and two body rows");
        assert!(table.content.iter().all(|r| r.kind == BlockKind::TableRow));

        // The header row is a row like any other; what makes it a header is that its cells
        // are `TableHeader`, exactly as ProseMirror models it. A renderer can therefore
        // decide `th` versus `td` from the cell alone, without knowing its ancestry.
        assert_eq!(
            kinds_of(&table.content[0]),
            vec![BlockKind::TableHeader, BlockKind::TableHeader]
        );
        assert_eq!(
            kinds_of(&table.content[1]),
            vec![BlockKind::TableCell, BlockKind::TableCell]
        );

        // A cell holds block content, not text: the text lives in a paragraph inside it,
        // so a cell can hold a list or a second paragraph the day the editor allows one.
        assert_eq!(
            table.content[0].content[0].content[0].kind,
            BlockKind::Paragraph
        );

        assert!(
            conversion.notes.is_empty(),
            "a table is no longer a lossy conversion: {:?}",
            conversion.notes
        );
    }

    #[test]
    fn every_cell_reaches_plain_text_separated_rather_than_fused() {
        // Asserted as exact text, not `contains`: a `contains("Feld")` check passes just
        // as happily on the fused token "FeldWert", which is the actual failure mode.
        let md = "| Feld | Wert |\n| --- | --- |\n| Größe | 42 |\n";
        assert_eq!(markdown_to_blocks(md).plain_text(), "Feld Wert Größe 42");
    }

    #[test]
    fn column_alignment_reaches_every_cell_and_a_default_column_carries_none() {
        // Dropping alignment silently is how a numeric column ends up ragged, so it rides
        // on the cells — the only place a renderer can act on it.
        let md = "| l | c | r | d |\n|:---|:---:|---:|---|\n| 1 | 2 | 3 | 4 |\n";
        let doc = markdown_to_blocks(md);
        let table = &doc.content[0];

        let expected = vec![Some("left"), Some("center"), Some("right"), None];
        assert_eq!(
            aligns_of(&table.content[0]),
            expected,
            "the header row is aligned too, or the column looks bent at its title"
        );
        assert_eq!(aligns_of(&table.content[1]), expected);
    }

    #[test]
    fn a_three_column_row_does_not_fuse_its_cells() {
        // Regression: a cell boundary carries no character of its own, so while cells
        // shared one block `| Länge | Meter |` produced "LängeMeter" — a token in the
        // search index that matches no query anyone would ever type.
        let md = "| Größe | Einheit | Symbol |\n| --- | --- | --- |\n| Länge | Meter | m |\n";
        let doc = markdown_to_blocks(md);
        assert_eq!(doc.content[0].content[1].plain_text(), "Länge Meter m");
    }

    #[test]
    fn an_empty_cell_neither_fuses_its_neighbours_nor_loses_its_column() {
        let md = "| a | b |\n| --- | --- |\n| x |  |\n";
        let doc = markdown_to_blocks(md);
        assert_eq!(doc.plain_text(), "a b x");
        assert_eq!(
            doc.content[0].content[1].content.len(),
            2,
            "an empty cell still holds its place, or every later cell shifts a column left"
        );
    }

    #[test]
    fn a_table_survives_a_json_round_trip_with_its_alignment() {
        let doc = markdown_to_blocks("| a |\n| ---: |\n| 1 |\n");
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("\"tableHeader\""), "{json}");
        assert!(json.contains("\"align\":\"right\""), "{json}");

        let again: crate::block::Block = serde_json::from_str(&json).unwrap();
        assert_eq!(again.content[0].kind, BlockKind::Table);
        assert_eq!(again.plain_text(), doc.plain_text());
    }

    #[test]
    fn inline_marks_keep_their_text_and_are_no_longer_reported_as_lost() {
        // Superseded by Task 2: bold and italic used to be flattened to plain text and
        // counted as a loss. They now land as `Mark`s on the text leaf (see
        // `emphasis_and_links_survive_import_and_are_no_longer_reported_as_lost` and
        // `nested_emphasis_stamps_both_marks_on_the_inner_text` for the marks themselves),
        // so nothing here is unsupported any more.
        let conversion = convert("Ein **fettes** und *kursives* Wort.\n");
        assert!(conversion.doc.plain_text().contains("fettes"));
        assert!(conversion.doc.plain_text().contains("kursives"));
        assert!(
            conversion.notes.is_empty(),
            "emphasis is modelled now, not lost: {:?}",
            conversion.notes
        );
    }

    #[test]
    fn strikethrough_is_parsed_rather_than_left_as_tildes() {
        let doc = markdown_to_blocks("~~weg~~\n");
        assert_eq!(doc.plain_text(), "weg");
    }

    #[test]
    fn a_link_keeps_its_text_and_its_destination_and_is_no_longer_reported_as_lost() {
        // Superseded by Task 2: the destination used to be dropped and counted as a loss.
        // It now lands as a `Mark::link_to_url` on the link text's leaf — this crate has no
        // store, so it cannot tell an internal destination from an external one, and Task 7
        // is where that resolution happens (see the comment on `Tag::Link` in `start`).
        let conversion = convert("Siehe [das Handbuch](/handbuch).\n");
        assert!(conversion.doc.plain_text().contains("das Handbuch"));
        let text = collect_text_leaves(&conversion.doc);
        let link = text
            .iter()
            .find(|b| b.text.as_deref() == Some("das Handbuch"))
            .unwrap();
        let m = link
            .marks
            .iter()
            .find(|m| m.kind == MarkKind::Link)
            .unwrap();
        assert_eq!(
            m.attrs.get("href").and_then(|v| v.as_str()),
            Some("/handbuch")
        );
        assert!(!conversion
            .notes
            .iter()
            .any(|n| n.construct == Unsupported::LinkTarget));
    }

    #[test]
    fn an_image_keeps_its_alt_text_and_is_reported() {
        let conversion = convert("![Ein Diagramm](/media/a.png)\n");
        assert!(conversion.doc.plain_text().contains("Ein Diagramm"));
        assert!(keys("![Ein Diagramm](/media/a.png)\n").contains(&"image"));
        let _ = conversion;
    }

    #[test]
    fn raw_html_is_kept_as_text_never_parsed() {
        // Parsing it would open a second, untrusted path into the block tree; dropping it
        // would lose content. Text is the only honest option in M1.
        let conversion = convert("<div>gefährlich</div>\n");
        assert!(conversion.doc.plain_text().contains("gefährlich"));
        assert!(conversion
            .notes
            .iter()
            .any(|n| n.construct == Unsupported::Html));
    }

    #[test]
    fn a_soft_break_becomes_a_space_not_a_fused_word() {
        let doc = markdown_to_blocks("Maß\nEinheit\n");
        assert_eq!(doc.plain_text(), "Maß Einheit");
    }

    #[test]
    fn german_text_survives_unchanged() {
        let doc = markdown_to_blocks("# Größe und Maß\n\nÜber Öl und Äpfel.\n");
        assert_eq!(doc.plain_text(), "Größe und Maß Über Öl und Äpfel.");
        assert_eq!(doc.headings()[0].id, "groesse-und-mass");
    }

    #[test]
    fn round_trip_sanity_heading_and_paragraph_both_reach_plain_text() {
        let text = markdown_to_blocks("# T\n\npara").plain_text();
        assert!(text.contains("T"), "{text}");
        assert!(text.contains("para"), "{text}");
    }

    #[test]
    fn the_result_serialises_as_a_prosemirror_tree() {
        // The store persists this as JSON and the editor loads it directly, so what comes
        // out here must survive a JSON round trip untouched.
        let doc = markdown_to_blocks("# T\n\n- a\n");
        let json = serde_json::to_string(&doc).unwrap();
        let again: crate::block::Block = serde_json::from_str(&json).unwrap();
        assert_eq!(again.plain_text(), doc.plain_text());
        assert!(json.contains("\"doc\""));
    }

    #[test]
    fn a_lossless_document_reports_nothing() {
        assert!(convert("# T\n\nEin Satz.\n\n- eins\n").notes.is_empty());
    }

    #[test]
    fn emphasis_and_links_survive_import_and_are_no_longer_reported_as_lost() {
        let c = convert("Ein **fetter** Satz mit [einem Link](https://example.org).");
        let text: Vec<_> = collect_text_leaves(&c.doc);
        let fett = text
            .iter()
            .find(|b| b.text.as_deref() == Some("fetter"))
            .unwrap();
        assert!(
            fett.marks.iter().any(|m| m.kind == MarkKind::Strong),
            "bold was dropped"
        );

        let link = text
            .iter()
            .find(|b| b.text.as_deref() == Some("einem Link"))
            .unwrap();
        let m = link
            .marks
            .iter()
            .find(|m| m.kind == MarkKind::Link)
            .unwrap();
        assert_eq!(
            m.attrs.get("href").and_then(|v| v.as_str()),
            Some("https://example.org")
        );

        assert!(
            !c.notes.iter().any(|n| matches!(
                n.construct,
                Unsupported::InlineMarks | Unsupported::LinkTarget
            )),
            "the converter still reports marks as lost: {:?}",
            c.notes
        );
    }

    #[test]
    fn nested_emphasis_stamps_both_marks_on_the_inner_text() {
        let c = convert("*kursiv und **beides***");
        let text: Vec<_> = collect_text_leaves(&c.doc);
        let both = text
            .iter()
            .find(|b| b.text.as_deref() == Some("beides"))
            .unwrap();
        assert_eq!(
            both.marks.len(),
            2,
            "an inner leaf carries every enclosing mark"
        );
    }
}
