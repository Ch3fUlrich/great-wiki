//! Synthetic block trees for the tests, and the deterministic generator behind the
//! round-trip property.
//!
//! Written rather than copied from the owner's pages on purpose: the live corpus is not in
//! this repository, and a fixture that only exists on one machine proves nothing on
//! another. The shapes here are modelled on it — twenty-one tables, the largest 27 rows by
//! 8 columns with per-column alignment, lists three levels deep, blockquotes and headings.

use gw_core::{Block, BlockKind};
use serde_json::{Map, Value};

/// A block as the JSON this project actually stores.
///
/// `gw_core::Block` derives neither `PartialEq` nor `Eq`, so a round trip is compared
/// through its canonical serialised form instead. That is not a workaround so much as the
/// stricter check: it is the exact bytes the revision table would hold.
pub fn json_of(block: &Block) -> Value {
    serde_json::to_value(block).expect("a Block always serialises")
}

pub fn block(kind: BlockKind) -> Block {
    Block {
        kind,
        attrs: Map::new(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

pub fn text(s: &str) -> Block {
    let mut b = block(BlockKind::Text);
    b.text = Some(s.to_string());
    b
}

pub fn paragraph(s: &str) -> Block {
    let mut b = block(BlockKind::Paragraph);
    b.content.push(text(s));
    b
}

fn with_attr(mut b: Block, key: &str, value: Value) -> Block {
    b.attrs.insert(key.to_string(), value);
    b
}

fn doc(children: Vec<Block>) -> Block {
    let mut d = block(BlockKind::Doc);
    d.content = children;
    d
}

/// A page with nothing in it.
pub fn empty_doc() -> Block {
    block(BlockKind::Doc)
}

/// A small, ordinary page: a heading and a paragraph.
pub fn simple_doc() -> Block {
    let mut heading = with_attr(block(BlockKind::Heading), "level", Value::from(2));
    heading.content.push(text("Größe und Maß"));
    doc(vec![heading, paragraph("Ein Satz.")])
}

/// One document per block kind `gw-core` defines today, each in a shape that exercises
/// what makes it different: attributes, nesting, and text.
///
/// `BlockKind` is `#[non_exhaustive]`, so no match in this crate can be made to fail to
/// compile when a variant is added. The conversion itself is kind-agnostic — the tag is
/// the serde name of whatever the kind is — so a new kind round-trips without a change to
/// `doc.rs`; this list is what proves the ones that exist today do.
pub fn one_per_kind() -> Vec<(&'static str, Block)> {
    let mut cases = vec![
        ("doc", simple_doc()),
        ("paragraph", doc(vec![paragraph("Ein Satz.")])),
        ("paragraph (empty)", doc(vec![block(BlockKind::Paragraph)])),
        ("text", doc(vec![text("nackter Text")])),
        ("text (empty)", doc(vec![text("")])),
    ];

    let mut headings = Vec::new();
    for level in 1..=6u64 {
        let mut h = with_attr(block(BlockKind::Heading), "level", Value::from(level));
        h.content.push(text(&format!("Überschrift {level}")));
        headings.push(h);
    }
    cases.push(("heading (levels 1-6)", doc(headings)));

    let mut item = block(BlockKind::ListItem);
    item.content.push(paragraph("eins"));
    let mut bullet = block(BlockKind::BulletList);
    bullet.content.push(item.clone());
    cases.push(("bulletList / listItem", doc(vec![bullet])));

    let mut ordered = with_attr(block(BlockKind::OrderedList), "start", Value::from(3));
    ordered.content.push(item);
    cases.push(("orderedList", doc(vec![ordered])));

    cases.push(("taskList / taskItem", checklist()));

    let mut quote = block(BlockKind::Blockquote);
    quote.content.push(paragraph("Zitat."));
    quote.content.push(paragraph("Noch eins."));
    cases.push(("blockquote", doc(vec![quote])));

    let mut code = with_attr(block(BlockKind::CodeBlock), "language", Value::from("rust"));
    code.content.push(text("fn main() {\n    let x = 1;\n}"));
    cases.push(("codeBlock", doc(vec![code])));

    cases.push(("table / row / header / cell", aligned_table()));

    cases
}

/// A checklist: an unticked line with a ticked one nested under it, then a ticked line.
///
/// Shaped around the two attributes whose loss is invisible rather than around the markup.
///
/// `checked` is written on every item including when it is `false`, because an unticked box
/// and no box at all are different documents — and `false` is both the commonest value and
/// what anything reading a missing attribute would assume, so a checklist that lost it
/// still renders and simply reads as nothing being done yet.
///
/// `id` is the uuid the *store* mints during reconciliation on publish (`gw_core::markdown`
/// mints none, and must not: the exporter re-imports its own output and compares). It is
/// what ties the line to its record — status, assignee, due date — and it appears nowhere
/// on the page, so losing it through this conversion would show up only as a card silently
/// leaving its board. One item carries one and the other does not, which is the honest
/// state of a document holding both an imported line and a reconciled one.
pub fn checklist() -> Block {
    fn task(checked: bool, id: Option<&str>, text: &str, nested: Vec<Block>) -> Block {
        let mut item = with_attr(block(BlockKind::TaskItem), "checked", Value::from(checked));
        if let Some(id) = id {
            item = with_attr(item, "id", Value::from(id));
        }
        item.content.push(paragraph(text));
        item.content.extend(nested);
        item
    }

    let inner = {
        let mut list = block(BlockKind::TaskList);
        list.content.push(task(true, None, "Vollmilch", Vec::new()));
        list
    };
    let mut list = block(BlockKind::TaskList);
    list.content.push(task(
        false,
        Some("0199c0de-0000-7000-8000-000000000001"),
        "Milch kaufen",
        vec![inner],
    ));
    list.content
        .push(task(true, None, "Brot geholt", Vec::new()));
    doc(vec![list])
}

fn cell(kind: BlockKind, align: Option<&str>, body: Option<&str>) -> Block {
    let mut c = block(kind);
    if let Some(align) = align {
        c.attrs.insert("align".to_string(), Value::from(align));
    }
    match body {
        // An empty cell holds an empty paragraph, which is what M1's importer produces:
        // the cell keeps its column even with nothing in it.
        None => c.content.push(block(BlockKind::Paragraph)),
        Some(body) => c.content.push(paragraph(body)),
    }
    c
}

fn row(cells: Vec<Block>) -> Block {
    let mut r = block(BlockKind::TableRow);
    r.content = cells;
    r
}

/// Four columns, three of them aligned and one not — the shape M1's converter produces
/// from `|:---|:---:|---:|---|`.
pub fn aligned_table() -> Block {
    let aligns = [Some("left"), Some("center"), Some("right"), None];
    let header = row(aligns
        .iter()
        .enumerate()
        .map(|(i, a)| cell(BlockKind::TableHeader, *a, Some(&format!("Spalte {i}"))))
        .collect());
    let body = row(aligns
        .iter()
        .enumerate()
        .map(|(i, a)| cell(BlockKind::TableCell, *a, Some(&format!("Wert {i}"))))
        .collect());
    let mut table = block(BlockKind::Table);
    table.content = vec![header, body];
    doc(vec![table])
}

/// A table whose rows disagree about how wide they are. Legal GFM, and the case a
/// conversion that rebuilds rows from the header's width silently pads or truncates.
pub fn ragged_table() -> Block {
    let header = row(vec![
        cell(BlockKind::TableHeader, Some("left"), Some("a")),
        cell(BlockKind::TableHeader, None, Some("b")),
        cell(BlockKind::TableHeader, Some("right"), Some("c")),
    ]);
    let narrow = row(vec![
        cell(BlockKind::TableCell, Some("left"), Some("1")),
        cell(BlockKind::TableCell, None, Some("2")),
    ]);
    let wide = row(vec![
        cell(BlockKind::TableCell, Some("left"), Some("x")),
        cell(BlockKind::TableCell, None, Some("y")),
        cell(BlockKind::TableCell, Some("right"), Some("z")),
        cell(BlockKind::TableCell, None, Some("w")),
    ]);
    let mut table = block(BlockKind::Table);
    table.content = vec![header, narrow, wide];
    doc(vec![table])
}

/// A body row whose middle cell is empty.
pub fn table_with_empty_cells() -> Block {
    let header = row(vec![
        cell(BlockKind::TableHeader, None, Some("a")),
        cell(BlockKind::TableHeader, None, Some("b")),
        cell(BlockKind::TableHeader, None, Some("c")),
    ]);
    let body = row(vec![
        cell(BlockKind::TableCell, None, Some("x")),
        cell(BlockKind::TableCell, None, None),
        cell(BlockKind::TableCell, None, Some("z")),
    ]);
    let mut table = block(BlockKind::Table);
    table.content = vec![header, body];
    doc(vec![table])
}

/// The largest table in the owner's live pages: 27 rows by 8 columns, every column aligned.
pub fn big_table(rows: usize, columns: usize) -> Block {
    let aligns = ["left", "center", "right"];
    let mut table = block(BlockKind::Table);
    table.content.push(row((0..columns)
        .map(|c| {
            cell(
                BlockKind::TableHeader,
                Some(aligns[c % aligns.len()]),
                Some(&format!("Spalte {c}")),
            )
        })
        .collect()));
    for r in 0..rows {
        table.content.push(row((0..columns)
            .map(|c| {
                cell(
                    BlockKind::TableCell,
                    Some(aligns[c % aligns.len()]),
                    Some(&format!("Zeile {r} Spalte {c}")),
                )
            })
            .collect()));
    }
    doc(vec![table])
}

/// A bullet list nested `depth` levels deep, each level carrying its own text.
pub fn nested_list(depth: usize) -> Block {
    fn level(remaining: usize, at: usize) -> Block {
        let mut item = block(BlockKind::ListItem);
        item.content.push(paragraph(&format!("Ebene {at}")));
        if remaining > 1 {
            item.content.push(level(remaining - 1, at + 1));
        }
        let mut list = block(BlockKind::BulletList);
        list.content.push(item);
        list
    }
    doc(vec![level(depth, 1)])
}

// ----- the generator -----------------------------------------------------------------

/// A deterministic xorshift, so a failure found in CI can be reproduced by hand.
///
/// `rand` is not a dependency of this crate and does not need to become one for this: a
/// property test that cannot be replayed from its seed is worse than no property test.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next_u64() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Kinds that become an XML element. `Text` is excluded because it is the one kind that
/// becomes something else, and `Doc` is included because a nested `doc` is a perfectly
/// legal thing for the conversion to have to name.
const ELEMENT_KINDS: &[BlockKind] = &[
    BlockKind::Doc,
    BlockKind::Paragraph,
    BlockKind::Heading,
    BlockKind::BulletList,
    BlockKind::OrderedList,
    BlockKind::ListItem,
    BlockKind::TaskList,
    BlockKind::TaskItem,
    BlockKind::Blockquote,
    BlockKind::CodeBlock,
    BlockKind::Table,
    BlockKind::TableRow,
    BlockKind::TableHeader,
    BlockKind::TableCell,
];

/// Strings chosen for the ways text breaks conversions: empty, markup, entities, quotes,
/// newlines and tabs, combining characters, and an emoji outside the basic plane.
const TEXTS: &[&str] = &[
    "",
    " ",
    "Ein Satz.",
    "Größe und Maß",
    "<div class=\"x\">roh & ungeparst</div>",
    "line one\nline two\n\ttabbed",
    "a\u{0301}mbiguous",
    "🧪 emoji außerhalb der BMP",
    "\"quoted\" and 'quoted'",
    "{\"json\": \"in text\"}",
];

const ATTR_KEYS: &[&str] = &[
    "level", "align", "language", "start", "colspan", "größe", "",
];

fn random_value(rng: &mut Rng, depth: usize) -> Value {
    // Whole-valued *floats* are deliberately absent: `2.0` and `2` are one value in the
    // JavaScript runtime this document is shared with, so the conversion normalises them
    // to the integer. That normalisation has its own named test rather than being smuggled
    // through the property.
    match rng.below(if depth >= 2 { 8 } else { 10 }) {
        0 => Value::Null,
        1 => Value::Bool(rng.below(2) == 0),
        2 => Value::from(rng.below(7) as i64),
        3 => Value::from(-(rng.below(100) as i64) - 1),
        4 => Value::from(rng.below(1000) as f64 + 0.5),
        5 => Value::from(*rng.pick(TEXTS)),
        6 => Value::from(9_007_199_254_740_993_i64),
        7 => Value::from(0),
        8 => Value::Array(
            (0..rng.below(3))
                .map(|_| random_value(rng, depth + 1))
                .collect(),
        ),
        _ => Value::Object(
            (0..rng.below(3))
                .map(|i| (format!("k{i}"), random_value(rng, depth + 1)))
                .collect(),
        ),
    }
}

fn random_attrs(rng: &mut Rng) -> Map<String, Value> {
    let mut attrs = Map::new();
    for _ in 0..rng.below(4) {
        attrs.insert(rng.pick(ATTR_KEYS).to_string(), random_value(rng, 0));
    }
    attrs
}

fn random_node(rng: &mut Rng, depth: usize) -> Block {
    if depth >= 4 || rng.below(4) == 0 {
        let mut leaf = text(rng.pick(TEXTS));
        leaf.attrs = random_attrs(rng);
        return leaf;
    }
    let mut node = block(*rng.pick(ELEMENT_KINDS));
    node.attrs = random_attrs(rng);
    for _ in 0..rng.below(4) {
        node.content.push(random_node(rng, depth + 1));
    }
    node
}

/// What the generator actually produced, so the property test can assert that its corpus
/// was varied rather than four hundred copies of the empty document.
#[derive(Default)]
pub struct Coverage {
    kinds: std::collections::BTreeSet<String>,
    value_types: std::collections::BTreeSet<&'static str>,
    max_depth: usize,
    nodes: usize,
    attributes: usize,
}

impl Coverage {
    pub fn observe(&mut self, block: &Block, depth: usize) {
        self.nodes += 1;
        self.max_depth = self.max_depth.max(depth);
        self.kinds
            .insert(format!("{:?}", block.kind).to_ascii_lowercase());
        for value in block.attrs.values() {
            self.attributes += 1;
            self.value_types.insert(match value {
                Value::Null => "null",
                Value::Bool(_) => "bool",
                Value::Number(n) if n.is_f64() => "float",
                Value::Number(_) => "integer",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            });
        }
        for child in &block.content {
            self.observe(child, depth + 1);
        }
    }

    #[track_caller]
    pub fn assert_broad(&self) {
        assert!(self.nodes > 2_000, "only {} nodes generated", self.nodes);
        assert!(
            self.attributes > 500,
            "only {} attributes generated",
            self.attributes
        );
        assert!(self.max_depth >= 5, "only reached depth {}", self.max_depth);
        // Every kind the generator can emit, plus `text`, plus the `doc` root.
        assert_eq!(
            self.kinds.len(),
            ELEMENT_KINDS.len() + 1,
            "kinds generated: {:?}",
            self.kinds
        );
        for expected in [
            "table",
            "tablerow",
            "tableheader",
            "tablecell",
            "tasklist",
            "taskitem",
            "text",
        ] {
            assert!(self.kinds.contains(expected), "never generated {expected}");
        }
        for expected in [
            "null", "bool", "integer", "float", "string", "array", "object",
        ] {
            assert!(
                self.value_types.contains(expected),
                "never generated a {expected} attribute: {:?}",
                self.value_types
            );
        }
    }
}

/// A random document, within the shapes this schema can express: a text leaf never carries
/// children, and a block that is not a text leaf never carries text. Both of those losses
/// are real and each has its own test saying so; generating them here would only prove the
/// same thing four hundred times.
pub fn random_doc(rng: &mut Rng) -> Block {
    let mut d = block(BlockKind::Doc);
    for _ in 0..rng.below(4) {
        d.content.push(random_node(rng, 0));
    }
    d
}
