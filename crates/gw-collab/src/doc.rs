//! The CRDT document, and its conversion to and from the `Block` tree.
//!
//! Everything else in this crate is plumbing. This is the file where a document silently
//! loses content if it is wrong, so the mapping is stated once, here, and then proven as a
//! property rather than with examples:
//!
//! | `Block`                        | CRDT                                          |
//! |--------------------------------|-----------------------------------------------|
//! | the `doc` root                 | the XML fragment named [`CONTENT_FIELD`]      |
//! | any other block                | an `XmlElement` tagged with the kind's wire name |
//! | `attrs`                        | that element's XML attributes, JSON types kept |
//! | `content`                      | that element's children, in order              |
//! | a `text` leaf                  | an `XmlText`                                   |
//!
//! The element tag is the *serde* name of the [`BlockKind`] — `tableHeader`, not
//! `TableHeader` — which is the same name `web/src/lib/blocks/render.ts` and the editor
//! schema use. Deriving it from serde rather than a hand-written table means a kind added
//! to `gw-core` round-trips through here with no change to this file.

use crate::{CollabError, Result};
use gw_core::{Block, BlockKind, Mark, MarkKind};
use serde_json::{Map, Value};
use std::sync::Arc;
use yrs::types::text::YChange;
use yrs::types::xml::XmlOut;
use yrs::types::Attrs;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::{
    Any, Doc, Options, Out, ReadTxn, StateVector, Text, Transact, TransactionMut, Update, WriteTxn,
    Xml, XmlElementPrelim, XmlElementRef, XmlFragment, XmlFragmentRef, XmlTextPrelim, XmlTextRef,
};

/// The name of the shared XML fragment the document body lives in.
///
/// It is not a free choice: the browser must address the same structure, so TipTap's
/// collaboration extension has to be configured with `field: 'content'`. Its own default
/// is `'default'` — if this constant and that option ever disagree, both sides will work
/// perfectly on two different, permanently empty documents.
pub const CONTENT_FIELD: &str = "content";

/// The largest integer a JavaScript number holds exactly. Beyond it the browser could not
/// represent the value as a number at all, so it is the boundary between the two ways an
/// integer attribute can be stored.
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;

/// One page's live document.
///
/// Cheap to share: `yrs::Doc` is `Send + Sync` and guards its own store, so every method
/// here takes `&self` and blocks for as long as its transaction runs. Nothing in this type
/// holds a transaction across an await point, which is what keeps that safe.
#[derive(Debug)]
pub struct CollabDoc {
    doc: Doc,
}

impl Default for CollabDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl CollabDoc {
    /// An empty document.
    pub fn new() -> Self {
        // Yjs counts text offsets in UTF-16 code units because JavaScript strings do.
        // `yrs` defaults to bytes, which changes nothing on the wire — item clocks are
        // UTF-16 either way — but does change what an index means in its own API. Setting
        // it here means an index in this crate and an index in the browser are the same
        // number, on a corpus that is full of umlauts.
        let options = Options {
            offset_kind: yrs::OffsetKind::Utf16,
            ..Options::default()
        };
        let doc = Doc::with_options(options);
        // Materialise the fragment immediately, so an untouched document still has the
        // root type a client will ask to sync.
        let _ = doc.transact_mut().get_or_insert_xml_fragment(CONTENT_FIELD);
        Self { doc }
    }

    /// Seed a fresh document with a block tree.
    ///
    /// A root that is not a `doc` is nested inside one rather than unwrapped, because
    /// throwing away the caller's outermost block would be a silent loss.
    pub fn from_block(block: &Block) -> Self {
        let this = Self::new();
        {
            let mut txn = this.doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(CONTENT_FIELD);
            let roots: &[Block] = if block.kind == BlockKind::Doc {
                &block.content
            } else {
                std::slice::from_ref(block)
            };
            write_children(&mut txn, &fragment, roots);
        }
        this
    }

    /// Restore a document from [`CollabDoc::encode_state`].
    pub fn from_state(state: &[u8]) -> Result<Self> {
        let this = Self::new();
        this.apply_update(state)?;
        Ok(this)
    }

    /// The document as a block tree.
    ///
    /// Always a `doc` root. See the crate docs for what this view cannot express.
    pub fn to_block(&self) -> Block {
        let txn = self.doc.transact();
        let fragment = txn
            .get_xml_fragment(CONTENT_FIELD)
            .expect("the content fragment is created in `new`");
        read_fragment(&txn, &fragment)
    }

    /// Integrate an update produced by another replica.
    ///
    /// The bytes come off a WebSocket, so a decode failure is an error and never a panic.
    pub fn apply_update(&self, update: &[u8]) -> Result<()> {
        let update = Update::decode_v1(update).map_err(CollabError::MalformedUpdate)?;
        self.doc.transact_mut().apply_update(update)?;
        Ok(())
    }

    /// This replica's state vector, encoded — what a peer needs to compute a diff for us.
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// Everything this replica knows, for persistence or for a client with nothing.
    pub fn encode_state(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// Only what the peer described by `state_vector` is missing.
    ///
    /// Fallible for the same reason [`CollabDoc::apply_update`] is: a state vector arrives
    /// from a client, and a client can send anything.
    pub fn encode_diff(&self, state_vector: &[u8]) -> Result<Vec<u8>> {
        let sv = StateVector::decode_v1(state_vector).map_err(CollabError::MalformedStateVector)?;
        Ok(self.doc.transact().encode_diff_v1(&sv))
    }

    /// Append a paragraph of plain text.
    ///
    /// Exists for tests and for the agent editing path: an agent that appends a note to a
    /// page must go through the CRDT like everyone else, or it is the second write path
    /// ADR 0001 forbids.
    pub fn append_paragraph(&self, text: &str) {
        let mut txn = self.doc.transact_mut();
        let fragment = txn.get_or_insert_xml_fragment(CONTENT_FIELD);
        let paragraph = fragment.push_back(&mut txn, XmlElementPrelim::empty(TAG_PARAGRAPH));
        paragraph.push_back(&mut txn, XmlTextPrelim::new(text));
    }

    /// This replica's client id, as Yjs uses it to break ties between concurrent inserts.
    pub fn client_id(&self) -> u64 {
        self.doc.client_id().get()
    }
}

/// The wire name of [`BlockKind::Paragraph`], resolved once rather than per call.
const TAG_PARAGRAPH: &str = "paragraph";

/// The element tag for a block kind: its serde name, so the tag, the JSON on the wire and
/// the editor schema cannot drift apart.
fn tag_of(kind: BlockKind) -> String {
    match serde_json::to_value(kind) {
        Ok(Value::String(tag)) => tag,
        // `BlockKind` is a unit-variant enum with `rename_all = "camelCase"`; it has no
        // other serialised form. If that ever stops being true, a paragraph keeps the text
        // where dropping the node would not.
        _ => TAG_PARAGRAPH.to_string(),
    }
}

/// The block kind an element tag names, or `None` for a tag this schema has no kind for.
fn kind_of_tag(tag: &str) -> Option<BlockKind> {
    serde_json::from_value(Value::String(tag.to_string())).ok()
}

fn new_block(kind: BlockKind) -> Block {
    Block {
        kind,
        attrs: Map::new(),
        content: Vec::new(),
        text: None,
        marks: Vec::new(),
    }
}

/// A place children can be appended to: the document root, or an element inside it.
#[derive(Clone)]
enum Parent {
    Fragment(XmlFragmentRef),
    Element(XmlElementRef),
}

impl Parent {
    fn push_element(&self, txn: &mut TransactionMut, tag: &str) -> XmlElementRef {
        let prelim = XmlElementPrelim::empty(tag.to_string());
        match self {
            Parent::Fragment(f) => f.push_back(txn, prelim),
            Parent::Element(e) => e.push_back(txn, prelim),
        }
    }

    fn push_text(&self, txn: &mut TransactionMut, text: &str) -> XmlTextRef {
        let prelim = XmlTextPrelim::new(text);
        match self {
            Parent::Fragment(f) => f.push_back(txn, prelim),
            Parent::Element(e) => e.push_back(txn, prelim),
        }
    }
}

/// Write a list of blocks as the children of `root`.
///
/// Iterative rather than recursive on purpose. Both directions of this conversion walk a
/// tree whose depth a client controls — nesting a list inside a list is a keystroke — and
/// a stack overflow in Rust aborts the process rather than unwinding. An explicit stack
/// costs a dozen lines and removes the ceiling entirely.
fn write_children(txn: &mut TransactionMut, root: &XmlFragmentRef, blocks: &[Block]) {
    let mut stack: Vec<(Parent, &Block)> = blocks
        .iter()
        .rev()
        .map(|b| (Parent::Fragment(root.clone()), b))
        .collect();

    while let Some((parent, block)) = stack.pop() {
        if block.kind == BlockKind::Text {
            // A text leaf's `content` has nowhere to go: ProseMirror text nodes are leaves.
            let text = parent.push_text(txn, block.text.as_deref().unwrap_or_default());
            write_attributes(txn, &text, &block.attrs);
            write_marks(txn, &text, &block.marks);
            continue;
        }
        let element = parent.push_element(txn, &tag_of(block.kind));
        write_attributes(txn, &element, &block.attrs);
        // Reversed, so that popping yields the children in document order.
        for child in block.content.iter().rev() {
            stack.push((Parent::Element(element.clone()), child));
        }
    }
}

fn write_attributes<X: Xml>(txn: &mut TransactionMut, node: &X, attrs: &Map<String, Value>) {
    for (key, value) in attrs {
        node.insert_attribute(txn, key.clone(), json_to_any(value));
    }
}

/// Apply a leaf's marks to the `XmlText` that holds it, as Yjs formatting over the whole
/// range.
///
/// Each `Block::Text` leaf is its own, separate `XmlTextRef` — `write_children` never merges
/// two leaves into one shared text — so "the whole range" and "this leaf's marks" are the
/// same thing. One `format` call carries every mark at once: `insert_format` merges the
/// attributes it is given with what is already there, so a single call and one per mark are
/// equivalent, and one call is cheaper.
///
/// An empty leaf is left unformatted. Yjs formatting wraps *content*; a zero-length range
/// has none for the attributes to attach to, so a mark on an empty leaf cannot survive this
/// representation. Nothing in this project writes one today.
fn write_marks(txn: &mut TransactionMut, node: &XmlTextRef, marks: &[Mark]) {
    if marks.is_empty() {
        return;
    }
    let len = node.len(txn);
    if len == 0 {
        return;
    }
    node.format(txn, 0, len, marks_to_attrs(marks));
}

/// A mark's Yjs attribute key: the *serde* name of its [`MarkKind`] — `strong`, not `Strong`
/// — the same name a real `y-prosemirror` client would format under, so a document this
/// crate writes and one the browser writes are shaped alike on the wire.
fn mark_key_of(kind: MarkKind) -> String {
    match serde_json::to_value(kind) {
        Ok(Value::String(key)) => key,
        // `MarkKind` is a unit-variant enum with `rename_all = "camelCase"`; it has no other
        // serialised form. If that ever stops being true, the mark is dropped rather than
        // panicking on a remote client's update — the same defensive shape as `tag_of`.
        _ => "strong".to_string(),
    }
}

/// The mark kind a Yjs attribute key names, or `None` for a key this schema has no kind for
/// — an attribute a real editor never wrote, or one from a mark kind this crate does not yet
/// know (`MarkKind` is `#[non_exhaustive]`).
fn kind_of_mark_key(key: &str) -> Option<MarkKind> {
    serde_json::from_value(Value::String(key.to_string())).ok()
}

/// A leaf's marks as one Yjs attribute map: one entry per mark, keyed by [`mark_key_of`].
///
/// A mark with no attrs (`Strong`, `Em`, `Code`, `Strike`) is written as `true` — there is
/// nothing else to say. A mark that carries attrs (`Link`'s `doc` or `href`) is written as a
/// nested map through the same [`json_to_any`] this file already uses for a block's own
/// `attrs`, so `doc` and `href` keep their JSON type and neither is flattened away.
fn marks_to_attrs(marks: &[Mark]) -> Attrs {
    let mut attrs = Attrs::new();
    for mark in marks {
        let value = if mark.attrs.is_empty() {
            Any::Bool(true)
        } else {
            json_to_any(&Value::Object(mark.attrs.clone()))
        };
        attrs.insert(mark_key_of(mark.kind).into(), value);
    }
    attrs
}

/// The inverse of [`marks_to_attrs`]: a Yjs attribute map as a leaf's `marks`, sorted into
/// [`gw_core::MARK_ORDER`].
///
/// Sorted, not merely collected: `Attrs` is a `HashMap`, so the order its entries come out in
/// is not the order they were formatted in and can change between two runs of the same
/// process. `Block::marks` is a nesting, and the markdown exporter trusts it to already be in
/// canonical order — an unsorted read here is a document that round-trips through the CRDT
/// and then fails to round-trip through markdown.
fn attrs_to_marks(attrs: Option<&Attrs>) -> Vec<Mark> {
    let Some(attrs) = attrs else {
        return Vec::new();
    };
    let mut marks: Vec<Mark> = attrs
        .iter()
        .filter_map(|(key, value)| {
            let kind = kind_of_mark_key(key)?;
            let attrs = match any_to_json(value) {
                Value::Object(fields) => fields,
                _ => Map::new(),
            };
            Some(Mark { kind, attrs })
        })
        .collect();
    marks.sort_by_key(|mark| mark.kind.nesting_rank());
    marks
}

/// A partially built block: the children still to visit, and what has been built so far.
struct Frame {
    children: Vec<XmlOut>,
    next: usize,
    block: Block,
    /// True for a node this schema cannot name. Its children are lifted into its parent
    /// when it closes, so the text survives even though the node does not.
    transparent: bool,
}

fn read_fragment<T: ReadTxn>(txn: &T, root: &XmlFragmentRef) -> Block {
    let mut stack = vec![Frame {
        children: root.children(txn).collect(),
        next: 0,
        block: new_block(BlockKind::Doc),
        transparent: false,
    }];

    loop {
        let child = {
            let top = stack.last_mut().expect("the root frame is never popped");
            if top.next < top.children.len() {
                let child = top.children[top.next].clone();
                top.next += 1;
                Some(child)
            } else {
                None
            }
        };

        let Some(child) = child else {
            let done = stack.pop().expect("the loop only runs with a frame open");
            match stack.last_mut() {
                Some(parent) if done.transparent => parent.block.content.extend(done.block.content),
                Some(parent) => parent.block.content.push(done.block),
                None => return done.block,
            }
            continue;
        };

        match child {
            XmlOut::Text(text) => {
                let leaves = read_text_leaves(txn, &text);
                stack
                    .last_mut()
                    .expect("the loop only runs with a frame open")
                    .block
                    .content
                    .extend(leaves);
            }
            XmlOut::Element(element) => {
                let children = element.children(txn).collect();
                match kind_of_tag(element.tag()) {
                    Some(kind) => {
                        let mut block = new_block(kind);
                        block.attrs = read_attributes(txn, &element);
                        stack.push(Frame {
                            children,
                            next: 0,
                            block,
                            transparent: false,
                        });
                    }
                    None => stack.push(Frame {
                        children,
                        next: 0,
                        block: new_block(BlockKind::Doc),
                        transparent: true,
                    }),
                }
            }
            // Yjs allows a fragment inside an element; `y-prosemirror` never makes one, so
            // there is no block kind for it. Treated as a group rather than dropped.
            XmlOut::Fragment(fragment) => stack.push(Frame {
                children: fragment.children(txn).collect(),
                next: 0,
                block: new_block(BlockKind::Doc),
                transparent: true,
            }),
        }
    }
}

/// One `XmlTextRef`, as one or more `Block::Text` leaves.
///
/// Usually one: `write_children` gives every `Block::Text` leaf its own `XmlTextRef`, so an
/// unformatted round trip stays one leaf in, one leaf out. It can come back as more than one
/// leaf here, though — a browser can select a run in the *middle* of one `XmlTextRef` and
/// format only that, which `Block` cannot express as a single leaf with a uniform `marks`.
/// `diff` already assembles maximal runs of uniform formatting (see its doc example), so each
/// chunk it yields becomes exactly one leaf and no further splitting or merging is needed
/// here.
///
/// Read through `diff` rather than `get_string`: `get_string` renders formatting back as
/// pseudo-XML (`<strong>x</strong>`), so a word someone made bold in the browser would arrive
/// in the block tree — and then in the search index — as literal markup. Taking the string
/// chunks keeps the text; mapping each chunk's attributes through [`attrs_to_marks`] is what
/// keeps the formatting too, as `Mark`s rather than markup.
fn read_text_leaves<T: ReadTxn>(txn: &T, text: &XmlTextRef) -> Vec<Block> {
    let attrs = read_attributes(txn, text);
    let mut leaves: Vec<Block> = text
        .diff(txn, YChange::identity)
        .into_iter()
        .filter_map(|chunk| {
            let Out::Any(Any::String(part)) = &chunk.insert else {
                return None;
            };
            let mut block = new_block(BlockKind::Text);
            block.attrs = attrs.clone();
            block.text = Some(part.to_string());
            block.marks = attrs_to_marks(chunk.attributes.as_deref());
            Some(block)
        })
        .collect();
    if leaves.is_empty() {
        // No diff chunk at all — an empty `XmlTextRef` — is the one case an absent `text`
        // field and an empty one cannot be told apart in a Y.Text; see the test for it.
        let mut block = new_block(BlockKind::Text);
        block.attrs = attrs;
        block.text = Some(String::new());
        leaves.push(block);
    }
    leaves
}

fn read_attributes<T: ReadTxn, X: Xml>(txn: &T, node: &X) -> Map<String, Value> {
    node.attributes(txn)
        .map(|(key, value)| (key.to_string(), out_to_json(&value)))
        .collect()
}

fn out_to_json(out: &Out) -> Value {
    match out {
        Out::Any(any) => any_to_json(any),
        // A shared type as an attribute value: nothing writes one, and `Block` could not
        // hold it. Null rather than a panic, because the source is a remote client.
        _ => Value::Null,
    }
}

/// JSON to the CRDT's value type.
///
/// Integers stay integers where JavaScript could hold them as numbers, and become bigints
/// where it could not — losing precision silently at 2^53 is how an id turns into a
/// different id.
fn json_to_any(value: &Value) -> Any {
    match value {
        Value::Null => Any::Null,
        Value::Bool(b) => Any::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) if i.abs() <= MAX_SAFE_INTEGER => Any::Number(i as f64),
            Some(i) => Any::BigInt(i),
            None => match n.as_u64() {
                Some(u) if u <= MAX_SAFE_INTEGER as u64 => Any::Number(u as f64),
                Some(u) if u <= i64::MAX as u64 => Any::BigInt(u as i64),
                // Beyond `i64::MAX` there is no exact representation on either side.
                _ => Any::Number(n.as_f64().unwrap_or_default()),
            },
        },
        Value::String(s) => Any::String(s.as_str().into()),
        Value::Array(items) => Any::Array(items.iter().map(json_to_any).collect()),
        Value::Object(fields) => Any::Map(Arc::new(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), json_to_any(v)))
                .collect(),
        )),
    }
}

fn any_to_json(any: &Any) -> Value {
    match any {
        Any::Null | Any::Undefined => Value::Null,
        Any::Bool(b) => Value::Bool(*b),
        Any::Number(n) => number_to_json(*n),
        Any::BigInt(i) => Value::from(*i),
        Any::String(s) => Value::String(s.to_string()),
        Any::Buffer(bytes) => Value::Array(bytes.iter().map(|b| Value::from(*b)).collect()),
        Any::Array(items) => Value::Array(items.iter().map(any_to_json).collect()),
        Any::Map(fields) => Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), any_to_json(v)))
                .collect(),
        ),
    }
}

/// A CRDT number as JSON.
///
/// A whole number comes back as an integer rather than as `2.0`, because the document is
/// shared with a JavaScript runtime where `2.0` and `2` are the same value. Normalising
/// here is what stops a heading level from alternating between the two representations
/// depending on which side wrote it last.
fn number_to_json(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() <= MAX_SAFE_INTEGER as f64 {
        Value::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
    }
}

/// Unused outside tests, but it is the one thing `get_string` is still good for: a quick
/// human-readable dump of the fragment.
#[cfg(test)]
fn debug_xml(doc: &CollabDoc) -> String {
    use yrs::GetString;
    let txn = doc.doc.transact();
    txn.get_xml_fragment(CONTENT_FIELD)
        .expect("the content fragment is created in `new`")
        .get_string(&txn)
}

#[cfg(test)]
mod tests {
    use super::{debug_xml, CollabDoc, CONTENT_FIELD};
    use crate::fixtures::{self, json_of, Rng};
    use gw_core::{Block, BlockKind, Mark, MarkKind};
    use serde_json::{json, Value};
    use yrs::updates::decoder::Decode;
    use yrs::{
        ReadTxn, Text, Transact, Update, WriteTxn, Xml, XmlElementPrelim, XmlFragment,
        XmlTextPrelim,
    };

    fn sample() -> Block {
        serde_json::from_str(
            r#"{"kind":"doc","content":[
                 {"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Größe"}]},
                 {"kind":"paragraph","content":[{"kind":"text","text":"Ein Satz."}]}
               ]}"#,
        )
        .unwrap()
    }

    /// A doc containing one paragraph containing one text leaf, for tests that need to
    /// reach in and set marks on a single leaf directly.
    fn paragraph_with_text(text: &str) -> Block {
        serde_json::from_value(json!({
            "kind": "doc",
            "content": [{
                "kind": "paragraph",
                "content": [{"kind": "text", "text": text}]
            }]
        }))
        .unwrap()
    }

    /// The first text leaf in document order, for tests that do not care where it sits.
    fn first_text_leaf(block: &Block) -> &Block {
        if block.kind == BlockKind::Text {
            return block;
        }
        for child in &block.content {
            if child.kind == BlockKind::Text {
                return child;
            }
            if !child.content.is_empty() {
                return first_text_leaf(child);
            }
        }
        panic!("no text leaf found in {block:?}")
    }

    /// The fidelity assertion, in one place: what went in is what comes out, compared as
    /// JSON because `gw_core::Block` does not derive `PartialEq`.
    #[track_caller]
    fn assert_round_trips(block: &Block) {
        let doc = CollabDoc::from_block(block);
        assert_eq!(
            json_of(&doc.to_block()),
            json_of(block),
            "round trip changed the tree",
        );
        // And again through the bytes that get persisted and sent to the browser. An
        // attribute type that survives in memory but not through the v1 encoder — a large
        // integer collapsing to a float, say — would otherwise only break in production.
        let reloaded = CollabDoc::from_state(&doc.encode_state()).expect("its own state");
        assert_eq!(
            json_of(&reloaded.to_block()),
            json_of(block),
            "round trip through the encoded state changed the tree",
        );
    }

    // ----- fidelity ------------------------------------------------------------------

    #[test]
    fn a_block_tree_survives_a_round_trip_through_the_crdt() {
        // This is THE fidelity test. If it fails, editing loses content.
        let doc = CollabDoc::from_block(&sample());
        let back = doc.to_block();
        assert_eq!(back.plain_text(), sample().plain_text());
        assert_eq!(back.headings(), sample().headings());
        // Text and headings are not enough: they both survive a tree that has lost every
        // attribute and every level of nesting. The whole tree is compared as well.
        assert_round_trips(&sample());
    }

    #[test]
    fn every_block_kind_this_project_has_round_trips_exactly() {
        for (name, block) in fixtures::one_per_kind() {
            let back = CollabDoc::from_block(&block).to_block();
            assert_eq!(json_of(&back), json_of(&block), "{name} did not survive");
        }
    }

    #[test]
    fn a_table_keeps_its_per_column_alignment() {
        let table = fixtures::aligned_table();
        assert_round_trips(&table);
        // Asserted directly as well, so this test still means something if the fixture
        // ever loses its `align` attributes.
        let back = CollabDoc::from_block(&table).to_block();
        let header = &back.content[0].content[0];
        let aligns: Vec<_> = header
            .content
            .iter()
            .map(|c| c.attrs.get("align").and_then(|v| v.as_str()))
            .collect();
        assert_eq!(
            aligns,
            vec![Some("left"), Some("center"), Some("right"), None]
        );
    }

    #[test]
    fn a_table_whose_header_is_wider_than_its_rows_keeps_both_widths() {
        // Legal GFM, and present in the owner's pages. A conversion that rebuilds rows
        // from the header's width would quietly pad or truncate.
        let table = fixtures::ragged_table();
        assert_round_trips(&table);
        let back = CollabDoc::from_block(&table).to_block();
        let widths: Vec<usize> = back.content[0]
            .content
            .iter()
            .map(|row| row.content.len())
            .collect();
        assert_eq!(widths, vec![3, 2, 4]);
    }

    #[test]
    fn a_checklist_keeps_every_box_and_the_id_that_ties_it_to_its_record() {
        // Two attributes on one kind, and both are lost the same silent way — an attribute
        // that does not survive this conversion simply is not there afterwards.
        //
        // `checked` is the state of the box. `false` is its commonest value AND the value
        // anything reading a missing attribute would assume, so a checklist that lost it
        // still renders, still exports, and simply reads as nothing being done yet.
        //
        // `id` is the uuid the store mints during reconciliation on publish. It is what
        // ties the line to its record — status, assignee, due date — and it is invisible on
        // the page, so losing it shows up only as a card leaving the board.
        let list = fixtures::checklist();
        assert_round_trips(&list);

        let back = CollabDoc::from_block(&list).to_block();
        let items = &back.content[0].content;
        assert_eq!(
            items
                .iter()
                .map(|i| i.attrs.get("checked").and_then(Value::as_bool))
                .collect::<Vec<_>>(),
            vec![Some(false), Some(true)],
        );
        assert_eq!(
            items[0].attrs.get("id").and_then(Value::as_str),
            Some("0199c0de-0000-7000-8000-000000000001"),
        );
        // The nested checklist under the first item, at its own depth rather than flattened
        // into the one above it.
        let nested = items[0]
            .content
            .iter()
            .find(|b| b.kind == BlockKind::TaskList)
            .expect("the nested checklist must survive as a child of its task");
        assert_eq!(
            nested.content[0]
                .attrs
                .get("checked")
                .and_then(Value::as_bool),
            Some(true),
        );
        assert_eq!(back.plain_text(), "Milch kaufen Vollmilch Brot geholt");
    }

    #[test]
    fn an_empty_cell_keeps_its_place_rather_than_shifting_the_column() {
        let table = fixtures::table_with_empty_cells();
        assert_round_trips(&table);
        let back = CollabDoc::from_block(&table).to_block();
        assert_eq!(back.content[0].content[1].content.len(), 3);
        assert_eq!(back.plain_text(), "a b c x z");
    }

    #[test]
    fn a_big_table_survives_whole() {
        // 27 rows by 8 columns is the largest table in the owner's live pages.
        let table = fixtures::big_table(27, 8);
        assert_round_trips(&table);
    }

    #[test]
    fn lists_nested_three_deep_stay_nested() {
        let list = fixtures::nested_list(3);
        assert_round_trips(&list);
        let back = CollabDoc::from_block(&list).to_block();
        // Walk down and count, rather than trusting the equality above alone: a flattened
        // tree has the same text and would pass `plain_text` comparisons.
        let mut node = &back.content[0];
        let mut depth = 0;
        loop {
            assert_eq!(node.kind, BlockKind::BulletList);
            depth += 1;
            match node.content[0]
                .content
                .iter()
                .find(|b| b.kind == BlockKind::BulletList)
            {
                Some(inner) => node = inner,
                None => break,
            }
        }
        assert_eq!(depth, 3);
    }

    #[test]
    fn adjacent_text_leaves_stay_separate() {
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                 {"kind":"text","text":"Ein Satz."},
                 {"kind":"text","text":" Noch einer."}]}]}"#,
        )
        .unwrap();
        assert_round_trips(&doc);
        assert_eq!(
            CollabDoc::from_block(&doc).to_block().content[0]
                .content
                .len(),
            2,
            "two leaves merged into one"
        );
    }

    #[test]
    fn a_code_block_keeps_its_newlines_its_tabs_and_its_language() {
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"codeBlock","attrs":{"language":"sh"},
                 "content":[{"kind":"text","text":"eins\n\tzwei\n\nvier"}]}]}"#,
        )
        .unwrap();
        assert_round_trips(&doc);
    }

    #[test]
    fn raw_html_kept_as_text_is_neither_escaped_nor_parsed() {
        // M1 keeps raw HTML verbatim in a text leaf. If the CRDT escaped it, every import
        // would rewrite the page; if it parsed it, there would be a second path into the
        // tree. It must come back byte for byte.
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                 {"kind":"text","text":"<div class=\"x\">gefährlich & <b>fett</b></div>"}]}]}"#,
        )
        .unwrap();
        assert_round_trips(&doc);
        assert!(CollabDoc::from_block(&doc)
            .to_block()
            .plain_text()
            .contains('<'));
    }

    #[test]
    fn attribute_values_keep_their_json_type() {
        let doc: Block = serde_json::from_value(json!({
            "kind": "doc",
            "content": [{
                "kind": "paragraph",
                "attrs": {
                    "level": 3, "negative": -7, "zero": 0,
                    "ratio": 2.5, "small": -0.125,
                    "flag": true, "off": false, "nothing": null,
                    "name": "Größe & Maß", "empty": "",
                    "list": [1, "zwei", false, null],
                    "nested": {"a": 1, "b": {"c": "d"}},
                    "emptyList": [], "emptyMap": {},
                    "big": 9007199254740993_i64
                }
            }]
        }))
        .unwrap();
        assert_round_trips(&doc);
    }

    #[test]
    fn a_whole_float_attribute_comes_back_as_an_integer() {
        // Stated rather than hidden: the document is shared with a JavaScript runtime,
        // where `2.0` and `2` are one value. Normalising to the integer is what stops a
        // heading level from flipping representation depending on who wrote it last.
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"heading","attrs":{"level":2.0}}]}"#,
        )
        .unwrap();
        let back = CollabDoc::from_block(&doc).to_block();
        assert_eq!(back.content[0].attrs["level"], json!(2));
    }

    #[test]
    fn an_attribute_key_may_be_unicode_or_empty() {
        let doc: Block = serde_json::from_value(json!({
            "kind": "doc",
            "content": [{"kind": "paragraph", "attrs": {"": "leer", "größe": 1, "a b": "c"}}]
        }))
        .unwrap();
        assert_round_trips(&doc);
    }

    #[test]
    fn random_block_trees_round_trip_exactly() {
        // The property, not an example: every legal shape this schema permits, including
        // the ones nobody thought to write a case for. The seed is fixed, so a failure
        // here is reproducible rather than a story about a build that once went red.
        let mut rng = Rng::new(0x9E37_79B9_7F4A_7C15);
        let mut coverage = fixtures::Coverage::default();
        for i in 0..400 {
            let tree = fixtures::random_doc(&mut rng);
            coverage.observe(&tree, 0);
            let doc = CollabDoc::from_block(&tree);
            let reloaded = CollabDoc::from_state(&doc.encode_state()).expect("its own state");
            for (path, back) in [
                ("in memory", doc.to_block()),
                ("encoded", reloaded.to_block()),
            ] {
                assert_eq!(
                    json_of(&back),
                    json_of(&tree),
                    "tree {i} did not survive {path}:\n{}",
                    serde_json::to_string_pretty(&json_of(&tree)).unwrap()
                );
            }
        }
        // A generator that quietly settled on four hundred empty documents would pass the
        // loop above and prove nothing, so what it actually produced is asserted too.
        coverage.assert_broad();
    }

    #[test]
    fn a_mark_survives_a_round_trip_through_the_crdt() {
        let mut block = paragraph_with_text("fett");
        block.content[0].content[0].marks = vec![Mark {
            kind: MarkKind::Strong,
            attrs: Default::default(),
        }];
        let doc = CollabDoc::from_block(&block);
        let back = doc.to_block();
        let leaf = first_text_leaf(&back);
        assert_eq!(leaf.marks.len(), 1, "the CRDT dropped the mark");
        assert_eq!(leaf.marks[0].kind, MarkKind::Strong);
    }

    // ----- what a round trip cannot preserve, proven rather than assumed --------------

    #[test]
    fn a_text_leaf_without_a_text_field_comes_back_as_an_empty_string() {
        // There is no way to tell an absent string from an empty one inside a Y.Text, so
        // `None` normalises to `Some("")`. Nothing in this project writes the former.
        let doc: Block =
            serde_json::from_str(r#"{"kind":"doc","content":[{"kind":"text"}]}"#).unwrap();
        let back = CollabDoc::from_block(&doc).to_block();
        assert_eq!(back.content[0].text.as_deref(), Some(""));
    }

    #[test]
    fn text_on_a_block_that_is_not_a_leaf_is_dropped() {
        // ProseMirror has no such node — a paragraph holds text nodes, it does not carry
        // text. Asserted so the loss is known and not discovered in a diff one day.
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"paragraph","text":"verloren"}]}"#,
        )
        .unwrap();
        let back = CollabDoc::from_block(&doc).to_block();
        assert_eq!(back.content[0].text, None);
    }

    #[test]
    fn children_of_a_text_leaf_are_dropped() {
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"text","text":"a",
                 "content":[{"kind":"text","text":"b"}]}]}"#,
        )
        .unwrap();
        let back = CollabDoc::from_block(&doc).to_block();
        assert_eq!(back.content.len(), 1);
        assert!(back.content[0].content.is_empty());
    }

    #[test]
    fn attributes_on_the_document_root_are_dropped() {
        // The fragment *is* the doc node, and a Yjs fragment has no attributes. Nothing
        // in this project puts attrs on a doc; if M4 ever does, this test is the warning.
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","attrs":{"version":2},"content":[{"kind":"paragraph"}]}"#,
        )
        .unwrap();
        let back = CollabDoc::from_block(&doc).to_block();
        assert!(back.attrs.is_empty());
        assert_eq!(back.content.len(), 1, "the children still survive");
    }

    #[test]
    fn a_root_that_is_not_a_doc_is_nested_rather_than_discarded() {
        let paragraph: Block = serde_json::from_str(
            r#"{"kind":"paragraph","content":[{"kind":"text","text":"allein"}]}"#,
        )
        .unwrap();
        let back = CollabDoc::from_block(&paragraph).to_block();
        assert_eq!(back.kind, BlockKind::Doc);
        assert_eq!(json_of(&back.content[0]), json_of(&paragraph));
    }

    #[test]
    fn an_inline_mark_from_the_browser_keeps_its_text_and_its_emphasis() {
        // The M4 gap, closed. A browser bolding a word writes Yjs formatting under the
        // mark's canonical key (`mark_key_of`); `to_block` must now recover the same
        // `Mark` a real editor's write would have produced. The text must survive intact
        // and the formatted run must split into its own leaf, carrying the mark — *not*
        // reappear as literal `<strong>` markup in a leaf's text, which is what
        // `XmlText::get_string` would have produced.
        let doc = CollabDoc::from_block(&sample());
        {
            let mut txn = doc.doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(CONTENT_FIELD);
            let paragraph = fragment
                .push_back(&mut txn, XmlElementPrelim::empty("paragraph"))
                .clone();
            let text = paragraph.push_back(&mut txn, XmlTextPrelim::new("ein fettes Wort"));
            let mut attrs = yrs::types::Attrs::new();
            attrs.insert("strong".into(), true.into());
            text.format(&mut txn, 4, 6, attrs); // "fettes", the middle word
        }

        let back = doc.to_block();
        let last = back.content.last().unwrap();
        assert_eq!(last.kind, BlockKind::Paragraph);
        let recovered: String = last
            .content
            .iter()
            .filter_map(|t| t.text.clone())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(recovered, "ein fettes Wort");
        assert!(
            !recovered.contains("<strong>"),
            "formatting leaked into the text as markup: {recovered}"
        );
        // The CRDT still holds the formatting exactly as the browser wrote it...
        assert!(
            debug_xml(&doc).contains("<strong>"),
            "the CRDT lost the mark"
        );
        // ...and now the snapshot does too, as a `Mark` rather than markup: the formatted
        // run is its own leaf, and that leaf carries the `Strong` mark the formatting named.
        let bold_leaf = last
            .content
            .iter()
            .find(|leaf| leaf.text.as_deref() == Some("fettes"))
            .expect("the formatted run must split into its own leaf");
        assert_eq!(bold_leaf.marks.len(), 1);
        assert_eq!(bold_leaf.marks[0].kind, MarkKind::Strong);
        assert!(serde_json::to_string(&json_of(&back))
            .unwrap()
            .contains("strong"));
    }

    #[test]
    fn several_marks_on_one_leaf_come_back_in_mark_order_not_write_order() {
        // The actual risk in this task: `Attrs` is a `HashMap`, so nothing about *writing*
        // several marks in one order guarantees they come back in that order — or in any
        // consistent order at all. Written here deliberately out of `MARK_ORDER`
        // (`Link`, then `Em`, then `Strong`) so a read that merely preserved write order
        // would fail this assertion instead of accidentally passing it.
        let mut block = paragraph_with_text("fett und schräg und verlinkt");
        block.content[0].content[0].marks = vec![
            Mark::link_to_url("https://example.org"),
            Mark {
                kind: MarkKind::Em,
                attrs: Default::default(),
            },
            Mark {
                kind: MarkKind::Strong,
                attrs: Default::default(),
            },
        ];

        let doc = CollabDoc::from_block(&block);
        let reloaded = CollabDoc::from_state(&doc.encode_state()).expect("its own state");

        for (label, candidate) in [("in memory", &doc), ("through encoded state", &reloaded)] {
            let back = candidate.to_block();
            let leaf = first_text_leaf(&back);
            let kinds: Vec<MarkKind> = leaf.marks.iter().map(|m| m.kind).collect();
            assert_eq!(
                kinds,
                vec![MarkKind::Strong, MarkKind::Em, MarkKind::Link],
                "{label}: marks were not sorted into MARK_ORDER"
            );
        }
    }

    #[test]
    fn a_link_marks_doc_or_href_survives_and_the_two_are_never_confused() {
        // A link carries EITHER an internal `doc` id or an external `href`, never both —
        // and never flattened to a bare boolean, which is what a mark with no attrs would
        // look like on the wire. Both shapes go in on separate leaves of one paragraph and
        // must come back exactly as they went in, attrs and all.
        let doc_block: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                 {"kind":"text","text":"intern","marks":[{"kind":"link","attrs":{"doc":"019ff0"}}]},
                 {"kind":"text","text":"extern","marks":[{"kind":"link","attrs":{"href":"https://example.org"}}]}
               ]}]}"#,
        )
        .unwrap();

        let back = CollabDoc::from_block(&doc_block).to_block();
        let leaves = &back.content[0].content;
        assert_eq!(leaves[0].marks[0].target_doc(), Some("019ff0"));
        assert_eq!(
            leaves[0].marks[0].attrs.get("href"),
            None,
            "an internal link must not gain an href"
        );
        assert_eq!(
            leaves[1].marks[0]
                .attrs
                .get("href")
                .and_then(|v| v.as_str()),
            Some("https://example.org")
        );
        assert_eq!(
            leaves[1].marks[0].target_doc(),
            None,
            "an external link must not be read as a document reference"
        );
    }

    #[test]
    fn an_element_this_schema_cannot_name_keeps_its_text_and_loses_its_node() {
        // The editor gaining an image or a rule before `gw-core` gains the kind. The
        // element cannot be named in a `Block`, so its children are lifted into the parent
        // rather than thrown away with it.
        let doc = CollabDoc::new();
        {
            let mut txn = doc.doc.transact_mut();
            let fragment = txn.get_or_insert_xml_fragment(CONTENT_FIELD);
            let unknown = fragment
                .push_back(&mut txn, XmlElementPrelim::empty("figure"))
                .clone();
            unknown.insert_attribute(&mut txn, "src", "/media/a.png");
            let caption = unknown
                .push_back(&mut txn, XmlElementPrelim::empty("paragraph"))
                .clone();
            caption.push_back(&mut txn, XmlTextPrelim::new("Ein Diagramm"));
        }

        let back = doc.to_block();
        assert_eq!(back.content.len(), 1);
        assert_eq!(back.content[0].kind, BlockKind::Paragraph);
        assert_eq!(back.plain_text(), "Ein Diagramm");
    }

    #[test]
    fn the_element_tags_are_the_camel_case_wire_names() {
        // The tag is what the browser's schema matches on. `TableHeader` instead of
        // `tableHeader` would make every table render as an unknown node.
        let doc = CollabDoc::from_block(&fixtures::aligned_table());
        let xml = debug_xml(&doc);
        for tag in ["<table>", "<tableRow>", "<tableHeader>", "<tableCell>"] {
            assert!(xml.contains(tag), "missing {tag} in {xml}");
        }
    }

    #[test]
    fn the_body_lives_in_the_fragment_the_editor_is_configured_with() {
        let doc = CollabDoc::from_block(&sample());
        let txn = doc.doc.transact();
        assert_eq!(CONTENT_FIELD, "content");
        assert_eq!(
            txn.get_xml_fragment(CONTENT_FIELD).unwrap().len(&txn),
            2,
            "the body is not in the fragment the browser will subscribe to"
        );
    }

    // ----- the CRDT itself -----------------------------------------------------------

    #[test]
    fn two_documents_converge_after_exchanging_updates() {
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::new();

        b.apply_update(&a.encode_diff(&b.state_vector()).unwrap())
            .unwrap();
        assert_eq!(json_of(&b.to_block()), json_of(&a.to_block()));
    }

    #[test]
    fn concurrent_edits_from_two_replicas_both_survive() {
        // The requirement this whole milestone exists for: an agent editing while a
        // person types must not clobber them.
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::new();
        b.apply_update(&a.encode_diff(&b.state_vector()).unwrap())
            .unwrap();

        a.append_paragraph("von A");
        b.append_paragraph("von B");

        let a_update = a.encode_diff(&b.state_vector()).unwrap();
        let b_update = b.encode_diff(&a.state_vector()).unwrap();
        b.apply_update(&a_update).unwrap();
        a.apply_update(&b_update).unwrap();

        for doc in [&a, &b] {
            let text = doc.to_block().plain_text();
            assert!(text.contains("von A"), "A's edit was lost");
            assert!(text.contains("von B"), "B's edit was lost");
        }
        assert_eq!(
            json_of(&a.to_block()),
            json_of(&b.to_block()),
            "must converge"
        );
    }

    #[test]
    fn the_same_updates_in_different_orders_produce_the_same_document() {
        // The CRDT's whole promise. Three replicas edit a shared base concurrently; two
        // fresh documents receive the three updates in opposite orders and must agree
        // byte for byte, not merely hold the same words.
        let base = CollabDoc::from_block(&sample());
        let base_state = base.encode_state();

        let mut updates = Vec::new();
        for name in ["von A", "von B", "von C"] {
            let replica = CollabDoc::from_state(&base_state).unwrap();
            replica.append_paragraph(name);
            updates.push(replica.encode_diff(&base.state_vector()).unwrap());
        }

        let forwards = CollabDoc::from_state(&base_state).unwrap();
        for update in &updates {
            forwards.apply_update(update).unwrap();
        }
        let backwards = CollabDoc::from_state(&base_state).unwrap();
        for update in updates.iter().rev() {
            backwards.apply_update(update).unwrap();
        }

        let left = json_of(&forwards.to_block());
        let right = json_of(&backwards.to_block());
        assert_eq!(left, right, "order of delivery changed the document");
        let text = forwards.to_block().plain_text();
        for name in ["von A", "von B", "von C"] {
            assert!(text.contains(name), "{name} was lost: {text}");
        }
    }

    #[test]
    fn applying_the_same_update_twice_is_idempotent() {
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::new();
        let update = a.encode_diff(&b.state_vector()).unwrap();
        b.apply_update(&update).unwrap();
        b.apply_update(&update).unwrap();
        assert_eq!(json_of(&b.to_block()), json_of(&a.to_block()));
    }

    #[test]
    fn a_malformed_update_is_an_error_not_a_panic() {
        // Updates arrive over a WebSocket from a client. A corrupt frame must not take
        // the server down.
        let doc = CollabDoc::new();
        assert!(doc.apply_update(&[0xff, 0xff, 0xff, 0xff]).is_err());
        // A zero-length frame is not an empty update, it is a truncated one; the empty
        // update is two zero varints and must still be accepted.
        assert!(doc.apply_update(&[]).is_err());
        assert!(doc.apply_update(&[0x00, 0x00]).is_ok());
        // Truncated in the middle of a real update — the shape a dropped frame takes.
        let full = CollabDoc::from_block(&sample()).encode_state();
        assert!(doc.apply_update(&full[..full.len() / 2]).is_err());
        assert!(
            doc.to_block().content.is_empty(),
            "a rejected update must not half-apply"
        );
    }

    #[test]
    fn a_malformed_state_vector_is_an_error_not_a_panic() {
        // `encode_diff` takes a state vector straight off the socket during the sync
        // handshake; it is exactly as attacker-reachable as an update.
        let doc = CollabDoc::from_block(&sample());
        assert!(doc.encode_diff(&[0xff, 0xff, 0xff, 0xff]).is_err());
        assert!(doc.encode_diff(&doc.state_vector()).is_ok());
    }

    #[test]
    fn a_diff_against_an_identical_peer_carries_no_content() {
        let a = CollabDoc::from_block(&sample());
        let diff = a.encode_diff(&a.state_vector()).unwrap();
        let empty = CollabDoc::new();
        empty.apply_update(&diff).unwrap();
        assert!(empty.to_block().content.is_empty());
    }

    #[test]
    fn state_survives_encode_and_reload() {
        let a = CollabDoc::from_block(&fixtures::aligned_table());
        let b = CollabDoc::from_state(&a.encode_state()).unwrap();
        assert_eq!(json_of(&b.to_block()), json_of(&a.to_block()));
    }

    #[test]
    fn a_reloaded_document_is_a_different_replica() {
        // Two servers loading the same persisted state must not claim the same client id,
        // or their concurrent edits would be attributed to one replica and collide.
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::from_state(&a.encode_state()).unwrap();
        assert_ne!(a.client_id(), b.client_id());
    }

    #[test]
    fn seeding_twice_from_one_block_makes_two_documents_that_never_converge() {
        // A warning for whoever wires the socket up. `from_block` *creates* content: two
        // replicas that each seed from the same stored body have written the same words
        // twice, under two client ids, and the CRDT will faithfully keep both. Loading a
        // document must mean applying its persisted CRDT state — seeding is only ever for
        // a page that has none.
        let a = CollabDoc::from_block(&sample());
        let b = CollabDoc::from_block(&sample());
        b.apply_update(&a.encode_state()).unwrap();

        let text = b.to_block().plain_text();
        assert_eq!(
            text.matches("Ein Satz.").count(),
            2,
            "seeding twice must be visible, not silently deduplicated: {text}"
        );
    }

    #[test]
    fn an_update_from_this_crate_decodes_as_a_yjs_v1_update() {
        // The browser will decode these bytes with `yjs`, not with `yrs`. Decoding them
        // back through the v1 reader is the closest this crate can get to that without a
        // JavaScript runtime in the test suite.
        let a = CollabDoc::from_block(&sample());
        assert!(Update::decode_v1(&a.encode_state()).is_ok());
    }
}
