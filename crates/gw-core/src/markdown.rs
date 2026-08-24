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
    /// A link whose destination could not be carried.
    ///
    /// **Never produced any more, and kept on purpose.** `Block` gained marks, so a link's
    /// destination survives import. The variant stays because two tests assert this
    /// converter does *not* report one — a guard that needs something to name. Deleting it
    /// would delete the only statement that emphasis and link destinations are no longer
    /// lost; the day something starts emitting it again, those tests fail and say so.
    LinkTarget,
    /// Bold, italic, strikethrough or inline code that could not be carried.
    ///
    /// Never produced any more, for the same reason and kept for the same guard as
    /// [`Unsupported::LinkTarget`].
    InlineMarks,
    /// A thematic break (`---`). Carries no text.
    HorizontalRule,
    /// Raw HTML, inline or block. Kept verbatim as text rather than parsed or dropped.
    Html,
    /// A numbered list holding checkboxes (`1. [ ] etwas`). The checkboxes survive as a
    /// task list; the numbers do not, because a task list is unordered — TipTap's
    /// `taskList` has no `start` and renders a box where the number would be.
    ///
    /// Reported rather than silently accepted: a plan whose steps were numbered comes back
    /// as an unnumbered checklist, which is a visible change to the page. The plain lines
    /// of such a list keep their numbering (they stay an ordered list of their own), so
    /// what is lost is exactly the position of the ticked lines.
    OrderedTaskList,
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
            Unsupported::OrderedTaskList => "ordered-task-list",
            Unsupported::Unrecognised => "unrecognised",
        }
    }

    /// What happened to the construct's content, and where the gap gets closed.
    ///
    /// Two of these can no longer be reached at all — see [`Unsupported::LinkTarget`] — and
    /// say so rather than describing a loss that stopped happening.
    pub fn disposition(self) -> &'static str {
        match self {
            Unsupported::Image => "alt text kept, source URL dropped — M6 adds image blocks",
            Unsupported::LinkTarget => "NEVER REPORTED: link destinations survive as marks",
            Unsupported::InlineMarks => "NEVER REPORTED: emphasis survives as marks",
            Unsupported::HorizontalRule => "dropped; it carries no text — M4 adds the rule block",
            Unsupported::Html => "kept verbatim as text, never parsed — M4 decides its fate",
            Unsupported::OrderedTaskList => {
                "checkboxes kept as a task list, the numbering of those lines dropped"
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

/// Tables, strikethrough and task lists are enabled deliberately. Tables and task lists
/// have block kinds of their own; strikethrough does not, but parsing it means its text is
/// *seen* and can be preserved, whereas leaving the extension off makes a `~~word~~` keep
/// its tildes. Seeing more than we can model is recoverable; not seeing it is not.
///
/// Task lists are the case that proves the point. Without the extension pulldown-cmark
/// never emits `Event::TaskListMarker` at all — it emits the brackets as ordinary text —
/// so `- [ ] Stuhlprobe` imported as a bullet whose *words were* "[ ] Stuhlprobe", in the
/// page, in the search index and in every anchor derived from it. The converter's
/// `Unsupported::TaskListMarker` arm was unreachable and nothing said so.
fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
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
    /// The marks currently open, in the order the source opened them. `Start(Tag::Strong |
    /// Emphasis | Strikethrough | Link)` pushes, the matching `End` pops, and every text
    /// leaf is stamped with a clone of the whole stack — that is what lets `**bold *and
    /// italic***` land on one leaf carrying both marks. [`Builder::marked_text`] sorts that
    /// clone into [`crate::MARK_ORDER`] before it reaches the leaf, so what is stored is
    /// the canonical nesting and not the accident of which tag the author opened first.
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
    ///
    /// The marks are sorted into [`crate::MARK_ORDER`] first. The source is free to nest
    /// them either way round — `[**a**](url)` and `**[a](url)**` are the same document —
    /// and storing them in the order the tags happened to open would mean two spellings of
    /// one document became two different trees, only one of which the exporter can write
    /// back. The sort is stable, so marks of the same kind keep their source nesting.
    fn marked_text(&mut self, s: &str, mut marks: Vec<Mark>) {
        if let Some(code) = self.code.as_mut() {
            code.push_str(s);
            return;
        }
        marks.sort_by_key(|m| m.kind.nesting_rank());
        if !matches!(self.top().kind, BlockKind::Paragraph | BlockKind::Heading) {
            self.push(block(BlockKind::Paragraph), true);
        }
        match self.top().content.last_mut() {
            // Merge into the previous leaf so two text events for the same run of marks
            // become one text node, not two.
            Some(prev) if prev.kind == BlockKind::Text && prev.marks == marks => {
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

    /// Turn the open list item into a task item carrying `checked`.
    ///
    /// The checkbox arrives *after* the item it belongs to: pulldown-cmark emits
    /// `Start(Item)` — and, in a loose list, `Start(Paragraph)` as well — before
    /// `TaskListMarker`. So the item is already on the stack and the change is retroactive.
    /// The frame to change is therefore the innermost `ListItem`, which is not necessarily
    /// the top of the stack; reading the top alone would leave a loose list's checkboxes on
    /// the paragraph, where nothing looks for them.
    ///
    /// The enclosing list becomes a `TaskList` only once it closes, in [`Self::close_list`],
    /// because a list does not know until then whether *all* of its items were ticked.
    fn check_open_item(&mut self, checked: bool) {
        let Some(item) = self
            .stack
            .iter()
            .rposition(|f| f.block.kind == BlockKind::ListItem)
        else {
            // Unreachable: pulldown-cmark only emits the marker for a list item's first
            // inline. Noted rather than ignored, because a checkbox with no line to sit on
            // is a parser change, not a document.
            self.note(Unsupported::Unrecognised);
            return;
        };
        let item = &mut self.stack[item].block;
        item.kind = BlockKind::TaskItem;
        // Written even when false: an unticked box and no box at all are different
        // documents, and an attribute that vanishes at its default makes them one.
        item.attrs
            .insert("checked".into(), serde_json::Value::Bool(checked));
    }

    /// Close a list, splitting it wherever checkbox items and plain items meet.
    ///
    /// `- [ ] a` followed by `- plain` is one markdown list and two different things: a
    /// run of consecutive checkbox items becomes a `TaskList`, a run of plain items stays
    /// the list it was. The alternative — upgrading the whole list and stamping
    /// `checked: false` on the plain line — would put a to-do on somebody's board that
    /// nobody wrote, because in this system a checkbox line *is* a task (D-6).
    ///
    /// The split survives a round trip without needing the exporter's help: two adjacent
    /// lists re-import as the same two runs, and so does the single list they came from.
    fn close_list(&mut self) {
        self.close_implicit();
        if self.stack.len() == 1 {
            return; // never pop the doc
        }
        let list = self.stack.pop().expect("length checked above").block;
        if !list.content.iter().any(|c| c.kind == BlockKind::TaskItem) {
            self.top().content.push(list);
            return;
        }
        if list.kind == BlockKind::OrderedList {
            // Once per list, not once per checkbox: what was lost is one list's numbering.
            self.note(Unsupported::OrderedTaskList);
        }
        for run in split_task_runs(list) {
            self.top().content.push(run);
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
            Event::TaskListMarker(checked) => self.check_open_item(checked),
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
            | TagEnd::Item
            | TagEnd::TableHead
            | TagEnd::TableRow
            | TagEnd::TableCell => self.close(),
            // A list is the one frame that can close as more than one block: see
            // `close_list`, which splits a mixed list into task and plain runs.
            TagEnd::List(_) => self.close_list(),
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

/// One list in, one list per run of like items out.
///
/// Only called for a list that holds at least one `TaskItem`. A list with none returns from
/// [`Builder::close_list`] untouched, which is what keeps every list already in the wiki
/// byte-identical to what it was.
///
/// A plain run of a *numbered* list keeps the number it had. Splitting renumbers otherwise:
/// `1. [ ] a` / `2. plain` would come back reading "1. plain", a change to the page nobody
/// asked for and that no note explains, because what [`Builder::close_list`] reports is the
/// numbering of the *checkbox* lines — the ones that have no number left to keep.
fn split_task_runs(list: Block) -> Vec<Block> {
    let first = list
        .attrs
        .get("start")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);

    let mut runs: Vec<Block> = Vec::new();
    for (index, item) in list.content.into_iter().enumerate() {
        let kind = if item.kind == BlockKind::TaskItem {
            BlockKind::TaskList
        } else {
            list.kind
        };
        match runs.last_mut() {
            Some(run) if run.kind == kind => run.content.push(item),
            _ => {
                let mut run = block(kind);
                // The same rule the importer follows for a list as a whole: a run starting
                // at 1 states no `start`, so the common case grows no attrs and the JSON
                // stays comparable across importers.
                let number = first + index as u64;
                if kind == BlockKind::OrderedList && number != 1 {
                    run.attrs
                        .insert("start".into(), serde_json::Value::from(number));
                }
                run.content.push(item);
                runs.push(run);
            }
        }
    }
    runs
}

/// A mark with no attrs — every kind but `Link`, which carries its destination instead.
fn mark(kind: MarkKind) -> Mark {
    Mark {
        kind,
        attrs: serde_json::Map::new(),
    }
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
        // `nested_emphasis_stamps_both_marks_on_the_inner_text_in_canonical_order` for the
        // marks themselves), so nothing here is unsupported any more.
        let conversion = convert("Ein **fettes** und *kursives* Wort.\n");
        // Exact, not `contains`: emphasis splits the sentence into four text leaves, and a
        // substring check passes just as happily on "Ein fettes und kursives Wort ." —
        // where a leaf boundary has grown a space the source never had.
        assert_eq!(conversion.doc.plain_text(), "Ein fettes und kursives Wort.");
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
        // Exact, not `contains`: the link splits the sentence into three text leaves, and
        // `contains` passes on "Siehe das Handbuch ." — the full stop pushed off the word
        // by a leaf boundary, which is what the reader's outline would then show.
        assert_eq!(conversion.doc.plain_text(), "Siehe das Handbuch.");
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

    /// One list item: its kind, and what it says about its checkbox.
    ///
    /// `None` means the item states no `checked` at all, which is the distinction the whole
    /// split exists to keep: a plain line must not come back carrying `checked: false`,
    /// because in this system a checkbox line *is* a task and a fabricated `false` is a
    /// to-do nobody wrote.
    type Item = (BlockKind, Option<bool>);
    /// One list: its kind and its items.
    type List = (BlockKind, Vec<Item>);

    /// The doc's top-level lists — the whole shape a split is about, in one comparable value.
    fn lists_of(doc: &Block) -> Vec<List> {
        doc.content
            .iter()
            .map(|list| {
                let items = list
                    .content
                    .iter()
                    .map(|i| (i.kind, i.attrs.get("checked").and_then(|v| v.as_bool())))
                    .collect();
                (list.kind, items)
            })
            .collect()
    }

    #[test]
    fn a_checkbox_line_becomes_a_task_item_that_remembers_it_is_unticked() {
        let conversion = convert("- [ ] Stuhlprobe einschicken\n");

        let list = &conversion.doc.content[0];
        assert_eq!(list.kind, BlockKind::TaskList);
        let item = &list.content[0];
        assert_eq!(item.kind, BlockKind::TaskItem);
        assert_eq!(
            item.attrs.get("checked"),
            Some(&serde_json::Value::Bool(false))
        );
        // Block content, not bare text: a task can grow a second paragraph or a nested
        // list without changing kind, exactly as a list item can.
        assert_eq!(item.content[0].kind, BlockKind::Paragraph);

        // The brackets are syntax, not text. While the task-list extension was off they
        // arrived as literal text and the line read "[ ] Stuhlprobe einschicken" — in the
        // page, in the search index and in every heading anchor derived from it.
        assert_eq!(conversion.doc.plain_text(), "Stuhlprobe einschicken");
        assert!(
            conversion.notes.is_empty(),
            "a checkbox is modelled now, not lost: {:?}",
            conversion.notes
        );
    }

    #[test]
    fn a_checkbox_line_followed_by_a_plain_one_splits_into_two_lists() {
        assert_eq!(
            lists_of(&markdown_to_blocks("- [ ] a\n- plain\n")),
            vec![
                (
                    BlockKind::TaskList,
                    vec![(BlockKind::TaskItem, Some(false))]
                ),
                (BlockKind::BulletList, vec![(BlockKind::ListItem, None)]),
            ]
        );
    }

    #[test]
    fn a_plain_line_followed_by_a_checkbox_one_splits_into_two_lists() {
        assert_eq!(
            lists_of(&markdown_to_blocks("- plain\n- [x] a\n")),
            vec![
                (BlockKind::BulletList, vec![(BlockKind::ListItem, None)]),
                (BlockKind::TaskList, vec![(BlockKind::TaskItem, Some(true))]),
            ]
        );
    }

    #[test]
    fn a_plain_line_between_two_checkboxes_splits_the_list_into_three() {
        // The case that decides it: upgrading the whole list would put "plain" on a board
        // as an open task, which is exactly the cost D-6 was accepted on the grounds that
        // it does not happen.
        assert_eq!(
            lists_of(&markdown_to_blocks("- [ ] a\n- plain\n- [x] b\n")),
            vec![
                (
                    BlockKind::TaskList,
                    vec![(BlockKind::TaskItem, Some(false))]
                ),
                (BlockKind::BulletList, vec![(BlockKind::ListItem, None)]),
                (BlockKind::TaskList, vec![(BlockKind::TaskItem, Some(true))]),
            ]
        );
    }

    #[test]
    fn a_list_of_nothing_but_checkboxes_stays_one_task_list() {
        assert_eq!(
            lists_of(&markdown_to_blocks("- [ ] a\n- [x] b\n")),
            vec![(
                BlockKind::TaskList,
                vec![
                    (BlockKind::TaskItem, Some(false)),
                    (BlockKind::TaskItem, Some(true)),
                ]
            )]
        );
    }

    #[test]
    fn a_list_with_no_checkbox_at_all_is_untouched() {
        // The regression guard for every list already in the wiki: nothing about the split
        // may reach a list that has no task in it.
        assert_eq!(
            lists_of(&markdown_to_blocks("- eins\n- zwei\n")),
            vec![(
                BlockKind::BulletList,
                vec![(BlockKind::ListItem, None), (BlockKind::ListItem, None)]
            )]
        );
    }

    #[test]
    fn an_ordered_list_of_checkboxes_becomes_a_task_list() {
        // `1. [ ] a` is a checkbox line, and D-6 makes every checkbox line a task. A task
        // list is unordered — TipTap's `taskList` renders a box where the number was — so
        // the numbering is what gives way, not the checkbox.
        assert_eq!(
            lists_of(&markdown_to_blocks("1. [ ] a\n2. [x] b\n")),
            vec![(
                BlockKind::TaskList,
                vec![
                    (BlockKind::TaskItem, Some(false)),
                    (BlockKind::TaskItem, Some(true)),
                ]
            )]
        );
    }

    #[test]
    fn a_numbered_list_that_held_checkboxes_reports_its_lost_numbering() {
        let conversion = convert("1. [ ] a\n2. [x] b\n");
        assert_eq!(
            conversion
                .notes
                .iter()
                .map(|n| (n.construct.key(), n.count))
                .collect::<Vec<_>>(),
            vec![("ordered-task-list", 1)],
            "once per list, not once per checkbox: one list's numbering was lost"
        );
        // A bulleted task list loses nothing, so it must stay silent — a report that cries
        // wolf on the common case is a report nobody reads.
        assert!(convert("- [ ] a\n").notes.is_empty());
    }

    #[test]
    fn the_plain_run_of_a_split_numbered_list_keeps_the_number_it_had() {
        // Splitting must not renumber: without this the second line comes back as "1.
        // plain", a visible change to the page that no note explains, because what the
        // report names is the *checkbox* lines' numbering.
        let doc = markdown_to_blocks("1. [ ] a\n2. plain\n");
        assert_eq!(
            lists_of(&doc),
            vec![
                (
                    BlockKind::TaskList,
                    vec![(BlockKind::TaskItem, Some(false))]
                ),
                (BlockKind::OrderedList, vec![(BlockKind::ListItem, None)]),
            ]
        );
        assert_eq!(
            doc.content[1].attrs.get("start").and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn every_plain_run_of_a_split_numbered_list_counts_from_where_the_list_began() {
        // A list that does not start at 1, split into four runs. Each plain run has to know
        // its own position in the original numbering, not merely that it is not the first —
        // the arithmetic is where a split silently renumbers a page.
        let doc = markdown_to_blocks("5. [ ] a\n6. plain\n7. [x] b\n8. p2\n");
        let starts: Vec<Option<u64>> = doc
            .content
            .iter()
            .map(|b| b.attrs.get("start").and_then(|v| v.as_u64()))
            .collect();
        assert_eq!(starts, vec![None, Some(6), None, Some(8)]);
    }

    /// A source spelling and the markdown a split list is written back out as.
    ///
    /// The rendering half of the round trip lives in `gw_api::export`, which depends on
    /// this crate and therefore cannot be called from it. So the exported form is written
    /// here literally — `- [ ] ` / `- [x] ` for a task line, a blank line between the two
    /// lists the split produced — and the assertion is the one that matters either way:
    /// what the exporter writes must come back as the tree it was written from.
    ///
    /// Both bullet spellings of the join are listed on purpose. `- [ ] a` and `- plain`
    /// separated by a blank line are ONE loose list to every CommonMark parser, and `* plain`
    /// makes it two — so the pair proves the round trip survives whichever the exporter
    /// picks, and that the split is not quietly relying on the marker alternation.
    const TASK_ROUND_TRIPS: &[(&str, &[&str])] = &[
        (
            "- [ ] a\n- plain\n",
            &["- [ ] a\n\n- plain\n", "- [ ] a\n\n* plain\n"],
        ),
        (
            "- plain\n- [x] a\n",
            &["- plain\n\n- [x] a\n", "* plain\n\n- [x] a\n"],
        ),
        (
            "- [ ] a\n- plain\n- [x] b\n",
            &[
                "- [ ] a\n\n- plain\n\n- [x] b\n",
                "- [ ] a\n\n* plain\n\n- [x] b\n",
            ],
        ),
        ("- [ ] a\n- [x] b\n", &["- [ ] a\n- [x] b\n"]),
        // A numbered list that held checkboxes exports as a task list beside a numbered one
        // that kept its number. The loss was reported once, at the first import; the file
        // written from it no longer has anything left to lose, so re-importing it reports
        // nothing and must still land on the same tree.
        ("1. [ ] a\n2. plain\n", &["- [ ] a\n\n2. plain\n"]),
    ];

    #[test]
    fn a_split_list_re_imports_from_its_exported_form_as_the_very_same_tree() {
        for (source, exports) in TASK_ROUND_TRIPS {
            let imported = json(&markdown_to_blocks(source));
            for exported in *exports {
                assert_eq!(
                    json(&markdown_to_blocks(exported)),
                    imported,
                    "`{exported:?}` does not re-import as what `{source:?}` imported as"
                );
            }
        }
    }

    /// A tree as JSON. `Block` has no `PartialEq`, and this is the comparison
    /// `gw_api::export::render_file` makes on every export anyway.
    fn json(block: &Block) -> serde_json::Value {
        serde_json::to_value(block).unwrap()
    }

    #[test]
    fn a_task_item_carries_nothing_but_checked_because_the_store_mints_the_id() {
        // The data model gives a task a uuid, and the store mints it during reconciliation
        // on publish. Minting one here would break every export forever: `render_file`
        // re-imports its own output and compares it with the stored document, and a fresh
        // random id would differ every time. So this converter stays a pure function.
        let doc = markdown_to_blocks("- [ ] a\n- [x] b\n");
        for item in &doc.content[0].content {
            assert_eq!(
                item.attrs.keys().collect::<Vec<_>>(),
                vec!["checked"],
                "a task item grew an attribute this converter has no business minting"
            );
        }
        assert_eq!(
            json(&markdown_to_blocks("- [ ] a\n")),
            json(&markdown_to_blocks("- [ ] a\n")),
            "two conversions of one document must be the same document"
        );
    }

    #[test]
    fn a_task_list_serialises_under_the_editors_own_names() {
        // The editor and `web/src/lib/blocks/render.ts` mirror this enum and cannot see it,
        // so the wire names are pinned here: TipTap's extensions are `taskList`/`taskItem`.
        let doc = markdown_to_blocks("- [x] a\n");
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains(r#""kind":"taskList""#), "{json}");
        assert!(json.contains(r#""kind":"taskItem""#), "{json}");
        assert!(json.contains(r#""checked":true"#), "{json}");

        let again: Block = serde_json::from_str(&json).unwrap();
        assert_eq!(again.content[0].kind, BlockKind::TaskList);
        assert_eq!(again.content[0].content[0].kind, BlockKind::TaskItem);
    }

    #[test]
    fn a_checkbox_nested_under_a_checkbox_is_a_task_list_inside_a_task_item() {
        let doc = markdown_to_blocks("- [ ] a\n  - [x] b\n");
        let outer = &doc.content[0].content[0];
        assert_eq!(outer.kind, BlockKind::TaskItem);
        let inner = outer
            .content
            .iter()
            .find(|b| b.kind == BlockKind::TaskList)
            .expect("the nested checklist must be a child of the outer task");
        assert_eq!(inner.content[0].kind, BlockKind::TaskItem);
        assert_eq!(doc.plain_text(), "a b");
    }

    #[test]
    fn a_loose_checkbox_line_keeps_its_checkbox_and_its_second_paragraph() {
        // In a loose list the checkbox arrives *inside* the item's paragraph, one frame
        // deeper than in a tight one. Reading the top of the stack alone would leave the
        // `checked` attribute on the paragraph, where nothing looks for it.
        let doc = markdown_to_blocks("- [ ] a\n\n  noch etwas\n");
        let item = &doc.content[0].content[0];
        assert_eq!(item.kind, BlockKind::TaskItem);
        assert_eq!(
            item.attrs.get("checked"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(kinds_of(item), vec![BlockKind::Paragraph; 2]);
        assert_eq!(doc.plain_text(), "a noch etwas");
    }

    /// The kinds on the leaf whose text is `text`, in stored order.
    fn marks_on(doc: &Block, text: &str) -> Vec<MarkKind> {
        collect_text_leaves(doc)
            .iter()
            .find(|b| b.text.as_deref() == Some(text))
            .unwrap_or_else(|| panic!("no leaf holds `{text}`"))
            .marks
            .iter()
            .map(|m| m.kind)
            .collect()
    }

    #[test]
    fn nested_emphasis_stamps_both_marks_on_the_inner_text_in_canonical_order() {
        // The kinds AND their order, not just the count: the order is the nesting order,
        // outermost first, and `gw-api`'s exporter writes the marks back out in exactly
        // it. A count-only assertion pins nothing and lets the two sides disagree.
        let c = convert("*kursiv und **beides***");
        assert_eq!(marks_on(&c.doc, "beides"), [MarkKind::Strong, MarkKind::Em]);
        assert_eq!(marks_on(&c.doc, "kursiv und "), [MarkKind::Em]);
    }

    #[test]
    fn marks_are_stored_in_canonical_order_whichever_way_the_source_nested_them() {
        // `[**a**](url)` and `**[a](url)**` are the same document. Storing them as two
        // different mark arrays made the exporter — which writes one fixed nesting order —
        // refuse to export the first of them: its own output re-imported as a different
        // tree. One order, defined once in `MARK_ORDER`, is what closes that.
        for md in [
            "[**das Handbuch**](/handbuch)",
            "**[das Handbuch](/handbuch)**",
        ] {
            assert_eq!(
                marks_on(&convert(md).doc, "das Handbuch"),
                [MarkKind::Strong, MarkKind::Link],
                "`{md}` stored its marks out of canonical order"
            );
        }
        assert_eq!(
            marks_on(&convert("[*a*](/h)").doc, "a"),
            [MarkKind::Em, MarkKind::Link]
        );
        assert_eq!(
            marks_on(&convert("[~~a~~](/h)").doc, "a"),
            [MarkKind::Strike, MarkKind::Link]
        );
        // `Code` is last of all: a code span's content is literal, so nothing can nest
        // inside one — it is the leaf's base representation, never a wrapper.
        assert_eq!(
            marks_on(&convert("[`a`](/h)").doc, "a"),
            [MarkKind::Link, MarkKind::Code]
        );
        assert_eq!(
            marks_on(&convert("**`a`**").doc, "a"),
            [MarkKind::Strong, MarkKind::Code]
        );
    }

    #[test]
    fn the_canonical_order_places_every_mark_kind_exactly_once() {
        // The match is exhaustive on purpose. `MarkKind` is `#[non_exhaustive]`, but that
        // only binds OTHER crates — inside this one, adding a kind stops this test
        // compiling until someone decides where it nests. A kind left out of `MARK_ORDER`
        // would otherwise sort to the end by accident rather than by decision, and two
        // kinds sorting equal puts the stored order back at the mercy of the source.
        for kind in [
            MarkKind::Strong,
            MarkKind::Em,
            MarkKind::Code,
            MarkKind::Strike,
            MarkKind::Link,
        ] {
            match kind {
                MarkKind::Strong
                | MarkKind::Em
                | MarkKind::Code
                | MarkKind::Strike
                | MarkKind::Link => {}
            }
            assert!(
                crate::MARK_ORDER.contains(&kind),
                "{kind:?} has no place in the canonical nesting order"
            );
        }
        let mut ranks: Vec<usize> = crate::MARK_ORDER.iter().map(|k| k.nesting_rank()).collect();
        ranks.sort_unstable();
        assert_eq!(
            ranks,
            (0..crate::MARK_ORDER.len()).collect::<Vec<_>>(),
            "a kind is listed twice, so one of its ranks is unreachable"
        );
    }
}
