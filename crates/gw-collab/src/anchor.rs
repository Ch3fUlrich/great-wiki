//! Comment anchors: a character range in a page that follows edits and can be lost.
//!
//! # The flattened text
//!
//! A range is addressed in the page's *flattened text*: every `XmlText` leaf under the
//! content fragment, concatenated in document order (depth first), with no separator
//! between leaves. Offsets are UTF-16 code units, which is what a JavaScript string index
//! is. Formatting does not count; element boundaries do not count.
//!
//! # Wire format
//!
//! An anchor is two opaque byte strings, each a Yjs-encoded `RelativePosition` (v1). The
//! server can mint them itself ([`anchor_from_range`]), or the browser can send ones it
//! computed with `Y.createRelativePositionFromTypeIndex` ([`anchor_from_relative`], which
//! only checks that they decode). Start is `assoc >= 0` (sticks to the character after),
//! end is `assoc < 0` (sticks to the character before), so text typed at either edge falls
//! outside the passage and text typed inside falls within it.
//!
//! # Losing a passage
//!
//! A position whose item was deleted resolves to where it used to be, so deleting the whole
//! passage collapses the range to empty; that is [`passage_survives`] returning `false`.
//! Deleting part of it only shrinks it. A position that cannot be resolved at all counts as
//! lost. A position the server cannot place in the flattened text (a client anchored it on
//! an element rather than a text leaf) is not judged: it is assumed to survive.

use crate::doc::{CollabDoc, CONTENT_FIELD};
use std::ops::Range;
use yrs::branch::BranchPtr;
use yrs::types::text::YChange;
use yrs::types::xml::XmlOut;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::Encode;
use yrs::XmlTextRef;
use yrs::{Any, Assoc, IndexedSequence, Out, ReadTxn, StickyIndex, Text, Transact, XmlFragment};

/// The most characters of the passage kept as its quote.
const QUOTE_CHARS: usize = 120;

struct Leaf {
    text: XmlTextRef,
    start: u32,
    len: u32,
}

fn walk<T: ReadTxn, N: XmlFragment>(txn: &T, node: &N, out: &mut Vec<Leaf>, pos: &mut u32) {
    for child in node.children(txn) {
        match child {
            XmlOut::Text(text) => {
                let len = text.len(txn);
                out.push(Leaf {
                    text,
                    start: *pos,
                    len,
                });
                *pos += len;
            }
            XmlOut::Element(e) => walk(txn, &e, out, pos),
            XmlOut::Fragment(f) => walk(txn, &f, out, pos),
        }
    }
}

fn leaves<T: ReadTxn>(txn: &T) -> Vec<Leaf> {
    let mut out = Vec::new();
    if let Some(root) = txn.get_xml_fragment(CONTENT_FIELD) {
        walk(txn, &root, &mut out, &mut 0);
    }
    out
}

/// A leaf's visible text without formatting markup.
fn plain<T: ReadTxn>(txn: &T, text: &XmlTextRef) -> String {
    text.diff(txn, YChange::identity)
        .into_iter()
        .filter_map(|c| match c.insert {
            Out::Any(Any::String(s)) => Some(s.to_string()),
            _ => None,
        })
        .collect()
}

/// Mint an anchor for `from..to` of the flattened text, plus a short quote of it.
/// `None` for an empty or out-of-range selection.
pub fn anchor_from_range(
    doc: &CollabDoc,
    from: u32,
    to: u32,
) -> Option<(Vec<u8>, Vec<u8>, String)> {
    let txn = doc.inner().transact();
    let ls = leaves(&txn);
    let total = ls.last().map_or(0, |l| l.start + l.len);
    if from >= to || to > total {
        return None;
    }
    let first = ls.iter().find(|l| from < l.start + l.len)?;
    let last = ls.iter().find(|l| to > l.start && to <= l.start + l.len)?;
    let s = first
        .text
        .sticky_index(&txn, from - first.start, Assoc::After)?;
    let e = last
        .text
        .sticky_index(&txn, to - last.start, Assoc::Before)?;

    let mut units: Vec<u16> = Vec::new();
    for l in &ls {
        units.extend(plain(&txn, &l.text).encode_utf16());
    }
    let quote: String = String::from_utf16_lossy(units.get(from as usize..to as usize)?)
        .chars()
        .take(QUOTE_CHARS)
        .collect();
    Some((s.encode_v1(), e.encode_v1(), quote))
}

/// Accept client-computed Yjs relative positions: both must decode. Returns them unchanged.
pub fn anchor_from_relative(start: &[u8], end: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    StickyIndex::decode_v1(start).ok()?;
    StickyIndex::decode_v1(end).ok()?;
    Some((start.to_vec(), end.to_vec()))
}

enum Place {
    At(u32),
    Elsewhere,
}

fn locate<T: ReadTxn>(txn: &T, ls: &[Leaf], raw: &[u8]) -> Option<Place> {
    let off = StickyIndex::decode_v1(raw).ok()?.get_offset(txn)?;
    Some(
        match ls
            .iter()
            .find(|l| BranchPtr::from(AsRef::<yrs::branch::Branch>::as_ref(&l.text)) == off.branch)
        {
            Some(l) => Place::At(l.start + off.index.min(l.len)),
            None => Place::Elsewhere,
        },
    )
}

/// Where the anchored passage is now, in flattened-text offsets.
pub fn resolve_anchor(doc: &CollabDoc, start: &[u8], end: &[u8]) -> Option<Range<u32>> {
    let txn = doc.inner().transact();
    let ls = leaves(&txn);
    match (locate(&txn, &ls, start)?, locate(&txn, &ls, end)?) {
        (Place::At(a), Place::At(b)) => Some(a..b.max(a)),
        _ => None,
    }
}

/// Whether the passage still exists: both ends resolve and the range is non-empty.
pub fn passage_survives(doc: &CollabDoc, start: &[u8], end: &[u8]) -> bool {
    let txn = doc.inner().transact();
    let ls = leaves(&txn);
    match (locate(&txn, &ls, start), locate(&txn, &ls, end)) {
        (Some(Place::At(a)), Some(Place::At(b))) => b > a,
        (Some(_), Some(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_leaf(doc: &CollabDoc) -> XmlTextRef {
        let txn = doc.inner().transact();
        leaves(&txn).remove(0).text
    }

    fn doc() -> CollabDoc {
        let d = CollabDoc::new();
        d.append_paragraph("Hello brave world");
        d
    }

    fn anchor(d: &CollabDoc) -> (Vec<u8>, Vec<u8>) {
        let (s, e, q) = anchor_from_range(d, 6, 11).unwrap();
        assert_eq!(q, "brave");
        (s, e)
    }

    #[test]
    fn follows_an_insert_before_it() {
        let d = doc();
        let (s, e) = anchor(&d);
        let t = first_leaf(&d);
        t.insert(&mut d.inner().transact_mut(), 0, ">>> ");
        assert_eq!(resolve_anchor(&d, &s, &e), Some(10..15));
        assert!(passage_survives(&d, &s, &e));
    }

    #[test]
    fn survives_an_insert_inside_and_ignores_edge_typing() {
        let d = doc();
        let (s, e) = anchor(&d);
        let t = first_leaf(&d);
        t.insert(&mut d.inner().transact_mut(), 8, "XX");
        assert_eq!(resolve_anchor(&d, &s, &e), Some(6..13));
        t.insert(&mut d.inner().transact_mut(), 13, "!!");
        assert_eq!(resolve_anchor(&d, &s, &e), Some(6..13));
    }

    #[test]
    fn deleting_the_whole_passage_loses_it_but_a_part_does_not() {
        let d = doc();
        let (s, e) = anchor(&d);
        let t = first_leaf(&d);
        t.remove_range(&mut d.inner().transact_mut(), 6, 2);
        assert_eq!(resolve_anchor(&d, &s, &e), Some(6..9));
        assert!(passage_survives(&d, &s, &e));
        t.remove_range(&mut d.inner().transact_mut(), 6, 3);
        assert!(!passage_survives(&d, &s, &e));
        // Typing the same text again is new content, not the old passage.
        t.insert(&mut d.inner().transact_mut(), 6, "brave");
        assert!(!passage_survives(&d, &s, &e));
    }

    #[test]
    fn concurrent_edits_merge_and_the_anchor_follows() {
        let a = doc();
        let (s, e) = anchor(&a);
        let b = CollabDoc::from_state(&a.encode_state()).unwrap();
        first_leaf(&a).insert(&mut a.inner().transact_mut(), 0, "AA");
        first_leaf(&b).insert(&mut b.inner().transact_mut(), 17, "ZZ");
        first_leaf(&b).remove_range(&mut b.inner().transact_mut(), 0, 5);
        a.apply_update(&b.encode_diff(&a.state_vector()).unwrap())
            .unwrap();
        // "AA" + "brave worldZZ" minus "Hello": the passage starts after "AA" + " ".
        assert_eq!(resolve_anchor(&a, &s, &e), Some(3..8));
    }

    #[test]
    fn client_positions_are_validated_and_garbage_is_not_a_passage() {
        let d = doc();
        let (s, e) = anchor(&d);
        assert!(anchor_from_relative(&s, &e).is_some());
        assert!(anchor_from_relative(&[255, 1], &e).is_none());
        assert!(!passage_survives(&d, &[255, 1], &e));
        assert!(anchor_from_range(&d, 5, 5).is_none());
        assert!(anchor_from_range(&d, 0, 99).is_none());
    }
}
