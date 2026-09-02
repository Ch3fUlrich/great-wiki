//! Three ways of saying what changed between two versions of a page.
//!
//! **Why three and not one.** A word-level diff is what everybody means by "diff", and on
//! this content model it is silent about a whole class of edit that plainly changed the
//! page: demoting a heading from `h2` to `h4`, reordering two paragraphs, re-aligning a
//! table column, making a sentence bold. Every one of those leaves the text of the page
//! byte-identical, so a single diff mode answers "no changes" for an edit somebody can see
//! from across the room — and a history that says "no changes" about a change is worse than
//! no history, because it is believed.
//!
//! So the question is asked three times, of the same two trees:
//!
//! - [`diff_prose`] — the words, ignoring every structure they sit in.
//! - [`diff_structure`] — the blocks: added, removed, moved, replaced in place.
//! - [`diff_design`] — everything the other two throw away: block attributes (a heading's
//!   level, a cell's alignment, a code block's language) and inline formatting.
//!
//! **A reorder is ONE change.** [`diff_structure`] pairs a removed block with an added one
//! carrying the same fingerprint and reports a single [`ChangeKind::Moved`]. Reporting it as
//! an addition and a removal would make dragging a paragraph up two positions read exactly
//! like deleting it and writing a new one, which is the difference between "somebody tidied
//! this" and "somebody rewrote this".
//!
//! **Whitespace inside a fence is a Struktur change and not a Prosa one**, and that split is
//! a decision rather than an omission. Every mode used to read a block's text through
//! [`Block::plain_text`], which collapses every run of whitespace — the right answer for
//! prose, and the wrong one for a code block, where the whitespace IS the content:
//! ` ```mermaid ` needs its newlines to draw anything at all, and a revision that flattened
//! one changed no fingerprint and was therefore reported by no mode at all. So
//! [`diff_structure`]'s fingerprint reads [`Block::diff_text`], which is `plain_text`
//! everywhere except inside a code block, where the text is taken verbatim.
//!
//! [`diff_prose`] is deliberately left alone: it tokenises on whitespace by construction, so
//! a pure re-indentation adds and removes no word, and making it see one would mean inventing
//! a "word" that is a run of spaces. A preview is left alone for the same reason in reverse —
//! it is a one-line label in a list of changes, so it stays collapsed.
//!
//! **And [`diff_design`] is left alone on purpose too, which is less obvious**, because it
//! also aligns blocks by kind-plus-text. It keeps the collapsed text (`design_key`, as
//! against `fingerprint`): it compares only the pairs its alignment calls equal, so
//! sharpening its key would make a reflowed fence stop it descending — and an author who
//! retypes ` ```rust ` as ` ```mermaid ` while re-laying-out the source would lose the
//! language change from every tab. One tab per half is the answer: Struktur reports the
//! fence, Gestaltung reports the language.
//!
//! **Identical documents produce nothing in any mode.** No mode emits unchanged context, so
//! an empty result means an empty result — the three lists are what changed and nothing
//! else, and a caller can test them for emptiness without knowing how they were built.
//!
//! Pure functions over two trees, like the rest of this crate: no database, no revision, no
//! opinion about which of the two came first. `gw-api` decides that.

use crate::block::{Block, BlockKind};
use serde::Serialize;
use similar::{capture_diff_slices, Algorithm, DiffOp};
use std::collections::BTreeSet;

/// What happened to one thing, in whichever mode is asking.
///
/// Shared by all three modes so that an interface can render "hinzugefügt" and "entfernt"
/// once rather than three times, and so that a mode which grows a new kind of change cannot
/// spell it differently from the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Removed,
    /// The same block, somewhere else. Never an addition plus a removal — see the module
    /// comment.
    Moved,
    /// A block that stayed where it was and is no longer the same block: the paragraph at
    /// position three is still a paragraph, and now says something different.
    ///
    /// Not in the original design, and it earns its place by what it replaces: without it,
    /// correcting one word in a paragraph reports "Absatz entfernt" and "Absatz
    /// hinzugefügt", which is the same misreading [`ChangeKind::Moved`] exists to prevent.
    Changed,
}

/// A run of words that was added or removed.
///
/// Only [`ChangeKind::Added`] and [`ChangeKind::Removed`] ever appear here: words do not
/// move — the blocks holding them do, which is [`diff_structure`]'s question — and they do
/// not change without one run replacing another.
#[derive(Debug, Clone, Serialize)]
pub struct ProseChange {
    pub kind: ChangeKind,
    /// The words themselves, joined by single spaces, in the order they appeared.
    pub text: String,
}

/// One block that appeared, disappeared, moved or was rewritten in place.
#[derive(Debug, Clone, Serialize)]
pub struct StructureChange {
    pub kind: ChangeKind,
    /// What sort of block it is — `paragraph`, `heading`, `table` — so an interface can say
    /// "Absatz verschoben" rather than "Block verschoben".
    pub block: BlockKind,
    /// A short preview of its text, for recognising which block this is about.
    pub text: String,
    /// Its position among the old document's blocks, and among the new document's. Zero
    /// based. `None` on the side where it does not exist: an addition has no `from`, a
    /// removal has no `to`, and a move has both — which is what makes it a move.
    pub from_index: Option<usize>,
    pub to_index: Option<usize>,
}

/// One attribute that differs on a block both versions still have.
#[derive(Debug, Clone, Serialize)]
pub struct DesignChange {
    /// The block the attribute sits on.
    pub block: BlockKind,
    /// A short preview of that block's text, so the change can be located by reading
    /// rather than by counting.
    pub text: String,
    /// The attribute's name as the document model spells it — `level`, `language`,
    /// `alignment` — or [`MARKS_ATTRIBUTE`] for inline formatting.
    pub attribute: String,
    /// The values, rendered for a person. `None` means the attribute was not present on
    /// that side at all, which is a different statement from "present and empty".
    pub before: Option<String>,
    pub after: Option<String>,
}

/// The pseudo-attribute under which inline formatting is reported.
///
/// Bold, italic, a link's address: they live in a text leaf's `marks` rather than in any
/// block's `attrs`, and they are invisible to both other modes — the words are unchanged,
/// so [`diff_prose`] is silent, and a block's fingerprint is its kind and its text, so
/// [`diff_structure`] is too. Without this, emphasising a sentence would produce three
/// empty change lists.
pub const MARKS_ATTRIBUTE: &str = "marks";

/// How much of a block's text is enough to recognise it.
const PREVIEW_CHARS: usize = 120;

/// The words that changed, ignoring every structure they sit in.
///
/// Whitespace-separated tokens of each document's plain text, diffed with Myers. Text is
/// taken through [`Block::plain_text`], which is the same flattening the search index uses,
/// so two documents that read identically produce no changes here however differently they
/// are built.
pub fn diff_prose(a: &Block, b: &Block) -> Vec<ProseChange> {
    let before = a.plain_text();
    let after = b.plain_text();
    let old: Vec<&str> = before.split_whitespace().collect();
    let new: Vec<&str> = after.split_whitespace().collect();

    let mut out = Vec::new();
    // Every op is already a maximal run, so a run of three deleted words is one change
    // rather than three — which is what makes the result readable as prose.
    for op in capture_diff_slices(Algorithm::Myers, &old, &new) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete {
                old_index, old_len, ..
            } => push_run(
                &mut out,
                ChangeKind::Removed,
                &old[old_index..old_index + old_len],
            ),
            DiffOp::Insert {
                new_index, new_len, ..
            } => push_run(
                &mut out,
                ChangeKind::Added,
                &new[new_index..new_index + new_len],
            ),
            // Removed before added, so a replacement reads as "this became that".
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                push_run(
                    &mut out,
                    ChangeKind::Removed,
                    &old[old_index..old_index + old_len],
                );
                push_run(
                    &mut out,
                    ChangeKind::Added,
                    &new[new_index..new_index + new_len],
                );
            }
        }
    }
    out
}

fn push_run(out: &mut Vec<ProseChange>, kind: ChangeKind, words: &[&str]) {
    if words.is_empty() {
        return;
    }
    out.push(ProseChange {
        kind,
        text: words.join(" "),
    });
}

/// Which blocks appeared, disappeared, moved or were rewritten in place.
///
/// Compares the two documents' **direct children** as a sequence of fingerprints — the
/// block's kind and its plain text — rather than walking the whole tree. An edit inside a
/// list is therefore reported as "the list changed" rather than as an anonymous change five
/// levels down, which is both cheaper and closer to what somebody scanning a history wants
/// to be told; the prose mode already says which words it was.
///
/// The classification order is the whole design, and it is not interchangeable:
///
/// 1. **Moves first.** A removed fingerprint that reappears among the added ones is one
///    [`ChangeKind::Moved`], wherever the two sit. Doing this after step 2 would let a
///    reorder be swallowed as two in-place rewrites.
/// 2. **Then in-place rewrites**, but only for blocks the diff itself paired — a `Replace`
///    op — and only when the kind is unchanged. A paragraph that became a heading is a
///    genuine removal and addition and is reported as one.
/// 3. **Then what is left**, as plain additions and removals.
///
/// The result is in document order, so it can be read top to bottom against the page.
pub fn diff_structure(a: &Block, b: &Block) -> Vec<StructureChange> {
    let old = &a.content;
    let new = &b.content;
    let fps_old: Vec<String> = old.iter().map(fingerprint).collect();
    let fps_new: Vec<String> = new.iter().map(fingerprint).collect();

    // One slot per block the diff did not call equal, in document order. `Replace` ops also
    // record which slots they paired, which is the only honest basis for calling something
    // an in-place rewrite: it means the diff itself lined those two blocks up.
    let mut slots: Vec<Slot> = Vec::new();
    let mut paired: Vec<(usize, usize)> = Vec::new();
    for op in capture_diff_slices(Algorithm::Myers, &fps_old, &fps_new) {
        match op {
            DiffOp::Equal { .. } => {}
            DiffOp::Delete {
                old_index, old_len, ..
            } => slots.extend((old_index..old_index + old_len).map(Slot::Old)),
            DiffOp::Insert {
                new_index, new_len, ..
            } => slots.extend((new_index..new_index + new_len).map(Slot::New)),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                let common = old_len.min(new_len);
                for i in 0..common {
                    let from = slots.len();
                    slots.push(Slot::Old(old_index + i));
                    let to = slots.len();
                    slots.push(Slot::New(new_index + i));
                    paired.push((from, to));
                }
                slots.extend((old_index + common..old_index + old_len).map(Slot::Old));
                slots.extend((new_index + common..new_index + new_len).map(Slot::New));
            }
        }
    }

    let mut resolved: Vec<Option<StructureChange>> = vec![None; slots.len()];
    let mut taken = vec![false; slots.len()];

    // 1. Moves. Quadratic in the number of CHANGED blocks, which on a wiki page is a
    //    handful; the alternative — a map from fingerprint to slot — would have to handle
    //    repeated fingerprints anyway, and two identical paragraphs are not rare.
    for from in 0..slots.len() {
        let Slot::Old(oi) = slots[from] else { continue };
        if taken[from] {
            continue;
        }
        for to in 0..slots.len() {
            let Slot::New(ni) = slots[to] else { continue };
            if taken[to] || fps_old[oi] != fps_new[ni] {
                continue;
            }
            taken[from] = true;
            taken[to] = true;
            resolved[from.min(to)] = Some(StructureChange {
                kind: ChangeKind::Moved,
                block: new[ni].kind,
                text: preview(&new[ni]),
                from_index: Some(oi),
                to_index: Some(ni),
            });
            break;
        }
    }

    // 2. In-place rewrites.
    for (from, to) in paired {
        if taken[from] || taken[to] {
            continue;
        }
        let (Slot::Old(oi), Slot::New(ni)) = (slots[from], slots[to]) else {
            continue;
        };
        if old[oi].kind != new[ni].kind {
            continue;
        }
        taken[from] = true;
        taken[to] = true;
        resolved[from.min(to)] = Some(StructureChange {
            kind: ChangeKind::Changed,
            block: new[ni].kind,
            text: preview(&new[ni]),
            from_index: Some(oi),
            to_index: Some(ni),
        });
    }

    // 3. Everything else.
    for (at, slot) in slots.iter().enumerate() {
        if taken[at] {
            continue;
        }
        resolved[at] = Some(match *slot {
            Slot::Old(oi) => StructureChange {
                kind: ChangeKind::Removed,
                block: old[oi].kind,
                text: preview(&old[oi]),
                from_index: Some(oi),
                to_index: None,
            },
            Slot::New(ni) => StructureChange {
                kind: ChangeKind::Added,
                block: new[ni].kind,
                text: preview(&new[ni]),
                from_index: None,
                to_index: Some(ni),
            },
        });
    }

    resolved.into_iter().flatten().collect()
}

/// One block on one side of the comparison, before it has been classified.
#[derive(Debug, Clone, Copy)]
enum Slot {
    Old(usize),
    New(usize),
}

/// What makes two blocks "the same block" for [`diff_structure`]: kind plus text.
///
/// Deliberately NOT the whole serialised block. Attributes are excluded so that a heading
/// whose level changed stays the same heading — otherwise every design change would also be
/// reported as a structural rewrite, and the two modes would say the same thing twice.
///
/// The text is [`Block::diff_text`] rather than [`Block::plain_text`], which is the whole
/// of the difference between "a code block changed" and "Keine Änderungen": see this
/// module's docs.
fn fingerprint(block: &Block) -> String {
    alignment_key(block, &block.diff_text())
}

/// The same key for [`compare_design`], and the one difference is [`Block::plain_text`].
///
/// **Not a duplicate of [`fingerprint`], and it must not be folded into it.** The two
/// functions answer different questions and a review caught the first draft of this file
/// conflating them:
///
/// - `fingerprint` asks *"is this the same block, down to the whitespace a diagram is
///   delimited by?"*. Sharpening it is the entire point of [`Block::diff_text`].
/// - `design_key` asks *"is this the same block, closely enough that comparing its
///   attributes tells the reader something?"*, and the answer there wants the **blunter**
///   key, because [`compare_design`] recurses only into the pairs its alignment calls
///   equal.
///
/// Give the design walk the sharp key and reflowing a fence stops it descending: an edit
/// that changes ` ```rust ` to ` ```mermaid ` *and* re-lays-out the source — one edit, and
/// the obvious one now that a language decides between a drawing, a formula and a listing —
/// loses the language change from every tab, and a fence re-indented beside a heading hides
/// that heading's `level`. Both are pinned by tests below.
///
/// The cost of the split is that one edit can now be reported twice, once per tab: Struktur
/// says the code block changed and Gestaltung says its language did. That is the correct
/// pair of statements about that edit — each tab answering for its own half — and it is
/// what the reader saw before [`Block::diff_text`] existed.
fn design_key(block: &Block) -> String {
    alignment_key(block, &block.plain_text())
}

/// Kind and text, joined by `\u{1}` so that a block whose text starts where another's kind
/// name ends cannot collide with it. Shared by both keys above so that the *shape* of the
/// two stays identical and only the text function differs.
fn alignment_key(block: &Block, text: &str) -> String {
    format!("{:?}\u{1}{text}", block.kind)
}

/// Enough of a block's text to recognise it, and no more.
fn preview(block: &Block) -> String {
    let text = block.plain_text();
    if text.chars().count() <= PREVIEW_CHARS {
        return text;
    }
    let head: String = text.chars().take(PREVIEW_CHARS).collect();
    format!("{head}…")
}

/// What changed about how the page looks rather than about what it says.
///
/// Walks both trees together and reports, for every pair of blocks the two documents agree
/// on, the attributes that differ and the inline formatting that differs.
///
/// **Only aligned pairs are compared**, and alignment is `design_key` — the same shape of
/// key [`diff_structure`] uses, deliberately reading [`Block::plain_text`] where the
/// structure mode reads [`Block::diff_text`]; `design_key`'s own docs say why the design
/// walk wants the blunter one. Zipping children by position instead would produce a cascade of
/// nonsense from a single insertion — insert a paragraph at the top and every block below it
/// pairs with its neighbour, so a page of alternating headings and paragraphs would report
/// dozens of attribute changes for one added line. A block that was added, removed or moved
/// is not a design change and is not reported here at all; that is the structure mode's
/// answer to give.
pub fn diff_design(a: &Block, b: &Block) -> Vec<DesignChange> {
    let mut out = Vec::new();
    compare_design(a, b, &mut out);
    out
}

fn compare_design(x: &Block, y: &Block, out: &mut Vec<DesignChange>) {
    let keys: BTreeSet<&str> = x
        .attrs
        .keys()
        .chain(y.attrs.keys())
        .map(String::as_str)
        .collect();
    for key in keys {
        let before = x.attrs.get(key);
        let after = y.attrs.get(key);
        if before == after {
            continue;
        }
        out.push(DesignChange {
            block: y.kind,
            text: preview(y),
            attribute: key.to_string(),
            before: before.map(render_value),
            after: after.map(render_value),
        });
    }

    // Inline formatting. Reported on the block that HOLDS the text rather than on the text
    // leaf, because emphasising part of a sentence re-splits it into different leaves and
    // no alignment of leaves would survive that — the paragraph, whose kind and text are
    // unchanged, is the last place both versions still agree.
    let (before, after) = (marks_profile(x), marks_profile(y));
    if before != after && x.plain_text() == y.plain_text() {
        out.push(DesignChange {
            block: y.kind,
            text: preview(y),
            attribute: MARKS_ATTRIBUTE.to_string(),
            before: (!before.is_empty()).then(|| before.join("; ")),
            after: (!after.is_empty()).then(|| after.join("; ")),
        });
    }

    let fps_x: Vec<String> = x.content.iter().map(design_key).collect();
    let fps_y: Vec<String> = y.content.iter().map(design_key).collect();
    for op in capture_diff_slices(Algorithm::Myers, &fps_x, &fps_y) {
        let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = op
        else {
            continue;
        };
        for i in 0..len {
            compare_design(&x.content[old_index + i], &y.content[new_index + i], out);
        }
    }
}

/// The formatting on a block's own text, as a list a person can read.
///
/// Immediate text children only. A nested block carries its own profile and is compared on
/// its own, so nothing is reported twice; a run with no marks contributes nothing, so plain
/// text costs an empty list on both sides and reports nothing.
fn marks_profile(block: &Block) -> Vec<String> {
    block
        .content
        .iter()
        .filter(|child| child.kind == BlockKind::Text && !child.marks.is_empty())
        .map(|child| {
            let kinds: Vec<String> = child
                .marks
                .iter()
                .map(|mark| {
                    let name = format!("{:?}", mark.kind).to_lowercase();
                    // A link's address is the whole content of the mark: two links on the
                    // same words are the same formatting and different documents.
                    match mark.attrs.get("href").or_else(|| mark.attrs.get("doc")) {
                        Some(target) => format!("{name} → {}", render_value(target)),
                        None => name,
                    }
                })
                .collect();
            format!(
                "»{}«: {}",
                child.text.as_deref().unwrap_or_default(),
                kinds.join(", ")
            )
        })
        .collect()
}

/// A JSON value as a person would read it: `2`, not `2`; `links`, not `"links"`.
fn render_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use crate::block::Block;
    use crate::diff::{diff_design, diff_prose, diff_structure, ChangeKind};

    fn doc(json: &str) -> Block {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn prose_diff_reports_word_level_changes() {
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der schnelle Fuchs"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der langsame Fuchs"}]}]}"#,
        );
        let changes = diff_prose(&a, &b);
        assert!(changes
            .iter()
            .any(|c| c.kind == ChangeKind::Removed && c.text == "schnelle"));
        assert!(changes
            .iter()
            .any(|c| c.kind == ChangeKind::Added && c.text == "langsame"));
    }

    #[test]
    fn prose_diff_is_empty_when_only_the_design_changed() {
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Titel"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"heading","attrs":{"level":4},"content":[{"kind":"text","text":"Titel"}]}]}"#,
        );
        assert!(diff_prose(&a, &b).is_empty(), "no words changed");
        // ...but the page plainly changed, which is why the design diff exists.
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1);
        assert_eq!(design[0].attribute, "level");
        assert_eq!(design[0].before.as_deref(), Some("2"));
        assert_eq!(design[0].after.as_deref(), Some("4"));
    }

    #[test]
    fn structure_diff_reports_an_added_block() {
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins"}]},{"kind":"paragraph","content":[{"kind":"text","text":"zwei"}]}]}"#,
        );
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Added);
    }

    #[test]
    fn structure_diff_reports_a_move_rather_than_an_add_and_a_remove() {
        // Reordering two paragraphs is one change, not two. Reporting it as add+remove
        // makes a reorder look like a rewrite.
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"A"}]},{"kind":"paragraph","content":[{"kind":"text","text":"B"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"B"}]},{"kind":"paragraph","content":[{"kind":"text","text":"A"}]}]}"#,
        );
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Moved);
    }

    #[test]
    fn identical_documents_produce_no_changes_in_any_mode() {
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"gleich"}]}]}"#,
        );
        assert!(diff_prose(&a, &a).is_empty());
        assert!(diff_structure(&a, &a).is_empty());
        assert!(diff_design(&a, &a).is_empty());
    }

    // --- Beyond the five the plan named -------------------------------------------------

    #[test]
    fn a_move_names_both_positions_so_it_can_be_rendered_as_one() {
        let a = doc(r#"{"kind":"doc","content":[
                {"kind":"paragraph","content":[{"kind":"text","text":"A"}]},
                {"kind":"paragraph","content":[{"kind":"text","text":"B"}]},
                {"kind":"paragraph","content":[{"kind":"text","text":"C"}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[
                {"kind":"paragraph","content":[{"kind":"text","text":"B"}]},
                {"kind":"paragraph","content":[{"kind":"text","text":"C"}]},
                {"kind":"paragraph","content":[{"kind":"text","text":"A"}]}]}"#);
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Moved);
        assert_eq!(changes[0].from_index, Some(0));
        assert_eq!(changes[0].to_index, Some(2));
        assert_eq!(changes[0].text, "A");
    }

    #[test]
    fn a_removed_block_is_reported_once_and_names_where_it_was() {
        let a = doc(r#"{"kind":"doc","content":[
                {"kind":"paragraph","content":[{"kind":"text","text":"eins"}]},
                {"kind":"paragraph","content":[{"kind":"text","text":"zwei"}]}]}"#);
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins"}]}]}"#,
        );
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Removed);
        assert_eq!(changes[0].from_index, Some(1));
        assert_eq!(changes[0].to_index, None);
    }

    #[test]
    fn rewriting_a_paragraph_in_place_is_one_change_not_two() {
        // The same complaint as the move test: correcting a word must not read as
        // "paragraph deleted, paragraph written".
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der schnelle Fuchs"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der langsame Fuchs"}]}]}"#,
        );
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Changed);
    }

    #[test]
    fn a_paragraph_that_became_a_heading_is_a_removal_and_an_addition() {
        // Not `Changed`: the block is a different kind of thing now, and collapsing that
        // into one line would hide the only part of it that matters.
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Titel"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Titel"}]}]}"#,
        );
        let kinds: Vec<ChangeKind> = diff_structure(&a, &b).iter().map(|c| c.kind).collect();
        assert_eq!(kinds, vec![ChangeKind::Removed, ChangeKind::Added]);
    }

    #[test]
    fn design_diff_sees_a_table_alignment_that_no_other_mode_can() {
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"table","content":[{"kind":"tableRow","content":[
                {"kind":"tableCell","attrs":{"alignment":"left"},"content":[{"kind":"paragraph","content":[{"kind":"text","text":"Zelle"}]}]}]}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"table","content":[{"kind":"tableRow","content":[
                {"kind":"tableCell","attrs":{"alignment":"right"},"content":[{"kind":"paragraph","content":[{"kind":"text","text":"Zelle"}]}]}]}]}]}"#,
        );
        assert!(diff_prose(&a, &b).is_empty());
        assert!(diff_structure(&a, &b).is_empty());
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1);
        assert_eq!(design[0].attribute, "alignment");
        assert_eq!(design[0].before.as_deref(), Some("left"));
        assert_eq!(design[0].after.as_deref(), Some("right"));
    }

    #[test]
    fn design_diff_sees_formatting_added_to_words_that_did_not_change() {
        // Emphasising half a sentence re-splits one text leaf into three. The words are
        // identical, the block fingerprint is identical, and both other modes are silent —
        // so if this were not reported the whole history would answer "keine Änderungen"
        // for an edit anybody can see.
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Der schnelle Fuchs"}]}]}"#,
        );
        let b = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                {"kind":"text","text":"Der "},
                {"kind":"text","text":"schnelle","marks":[{"kind":"strong"}]},
                {"kind":"text","text":" Fuchs"}]}]}"#);
        assert!(diff_prose(&a, &b).is_empty());
        assert!(diff_structure(&a, &b).is_empty());
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1);
        assert_eq!(design[0].attribute, super::MARKS_ATTRIBUTE);
        assert_eq!(design[0].before, None);
        assert_eq!(design[0].after.as_deref(), Some("»schnelle«: strong"));
    }

    #[test]
    fn design_diff_reports_a_links_address_changing_under_the_same_words() {
        let a = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                {"kind":"text","text":"Handbuch","marks":[{"kind":"link","attrs":{"href":"/alt"}}]}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[{"kind":"paragraph","content":[
                {"kind":"text","text":"Handbuch","marks":[{"kind":"link","attrs":{"href":"/neu"}}]}]}]}"#);
        assert!(diff_prose(&a, &b).is_empty());
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1);
        assert_eq!(design[0].before.as_deref(), Some("»Handbuch«: link → /alt"));
        assert_eq!(design[0].after.as_deref(), Some("»Handbuch«: link → /neu"));
    }

    #[test]
    fn inserting_a_block_produces_no_design_changes_at_all() {
        // The failure this guards against is a cascade: pairing children by position means
        // one insertion misaligns everything below it, and a page of alternating headings
        // and paragraphs would report a design change on every block for one added line.
        let a = doc(r#"{"kind":"doc","content":[
                {"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Eins"}]},
                {"kind":"heading","attrs":{"level":3},"content":[{"kind":"text","text":"Zwei"}]}]}"#);
        let b = doc(r#"{"kind":"doc","content":[
                {"kind":"paragraph","content":[{"kind":"text","text":"Neu"}]},
                {"kind":"heading","attrs":{"level":2},"content":[{"kind":"text","text":"Eins"}]},
                {"kind":"heading","attrs":{"level":3},"content":[{"kind":"text","text":"Zwei"}]}]}"#);
        assert!(
            diff_design(&a, &b).is_empty(),
            "an insertion is a structural change and nothing else"
        );
        assert_eq!(diff_structure(&a, &b).len(), 1);
    }

    #[test]
    fn a_run_of_deleted_words_is_one_change_rather_than_one_per_word() {
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins zwei drei vier"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"eins vier"}]}]}"#,
        );
        let changes = diff_prose(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, ChangeKind::Removed);
        assert_eq!(changes[0].text, "zwei drei");
    }

    #[test]
    fn prose_ignores_which_block_a_word_sits_in() {
        // Splitting one paragraph into two changes no words. The structure mode is what
        // has something to say about it.
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"Ein Satz. Noch einer."}]}]}"#,
        );
        let b = doc(r#"{"kind":"doc","content":[
                {"kind":"paragraph","content":[{"kind":"text","text":"Ein Satz."}]},
                {"kind":"paragraph","content":[{"kind":"text","text":"Noch einer."}]}]}"#);
        assert!(diff_prose(&a, &b).is_empty());
        assert!(!diff_structure(&a, &b).is_empty());
    }

    #[test]
    fn an_empty_document_against_a_written_one_is_all_additions() {
        let a = doc(r#"{"kind":"doc"}"#);
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"paragraph","content":[{"kind":"text","text":"etwas"}]}]}"#,
        );
        let prose = diff_prose(&a, &b);
        assert_eq!(prose.len(), 1);
        assert_eq!(prose[0].kind, ChangeKind::Added);
        let structure = diff_structure(&a, &b);
        assert_eq!(structure.len(), 1);
        assert_eq!(structure[0].kind, ChangeKind::Added);
        assert!(diff_design(&a, &b).is_empty());
    }

    // --- fences, where the whitespace is the content ------------------------------------

    /// A document holding one fenced block with the given source.
    fn fenced(source: &str) -> Block {
        doc(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"codeBlock","attrs":{{"language":"mermaid"}},
                 "content":[{{"kind":"text","text":{}}}]}}]}}"#,
            serde_json::Value::String(source.to_string())
        ))
    }

    #[test]
    fn reindenting_a_fence_is_a_structural_change_rather_than_nothing_at_all() {
        // The revision that destroys a diagram. Both versions hold the same words in the
        // same order, so `plain_text` — and therefore the fingerprint, before this — could
        // not tell them apart, and all three tabs answered "Keine Änderungen" about the
        // edit that broke the page.
        let a = fenced("graph TD;\n  A-->B;\n  B-->C;");
        let b = fenced("graph TD; A-->B; B-->C;");
        let structure = diff_structure(&a, &b);
        assert_eq!(structure.len(), 1, "{structure:?}");
        assert_eq!(structure[0].kind, ChangeKind::Changed);
        assert_eq!(structure[0].block, crate::block::BlockKind::CodeBlock);
    }

    #[test]
    fn a_fence_that_did_not_change_is_still_reported_as_no_change() {
        // The other half: reading a fence verbatim must not make every unchanged one look
        // rewritten, which is what a fingerprint that carried, say, the whole serialised
        // block would do.
        let a = fenced("graph TD;\n  A-->B;");
        assert!(diff_structure(&a, &a).is_empty());
        assert!(diff_prose(&a, &a).is_empty());
        assert!(diff_design(&a, &a).is_empty());
    }

    #[test]
    fn reindenting_a_fence_is_deliberately_not_a_prose_change() {
        // Stated as a test rather than left to be discovered, because it looks like an
        // omission and is a decision: `diff_prose` tokenises on whitespace by construction,
        // so a pure re-indentation adds and removes no word at all. It is a Struktur
        // change, and the interface says so.
        let a = fenced("graph TD;\n  A-->B;");
        let b = fenced("graph TD;\n\tA-->B;");
        assert!(diff_prose(&a, &b).is_empty());
        assert_eq!(diff_structure(&a, &b).len(), 1);
    }

    #[test]
    fn changing_a_fences_language_is_still_a_design_change_and_not_a_rewrite() {
        // The fingerprint excludes attributes, and reading the text verbatim must not
        // change that: ```rust becoming ```mermaid is one line in the Gestaltung tab, not a
        // block removed and another added.
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"codeBlock","attrs":{"language":"rust"},
                 "content":[{"kind":"text","text":"let x = 1;\n"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"codeBlock","attrs":{"language":"mermaid"},
                 "content":[{"kind":"text","text":"let x = 1;\n"}]}]}"#,
        );
        assert!(diff_structure(&a, &b).is_empty());
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1);
        assert_eq!(design[0].attribute, "language");
        assert_eq!(design[0].before.as_deref(), Some("rust"));
    }

    #[test]
    fn a_fences_language_change_survives_a_reflow_in_the_same_edit() {
        // The test above holds the text identical, which is the easy half. This is the edit
        // somebody actually makes: ```rust becomes ```mermaid AND the source is reflowed
        // onto separate lines in the one revision. The two modes must each answer for their
        // own half — Struktur that the block changed, Gestaltung that the language did —
        // and they can only do that because `compare_design` aligns on `plain_text` while
        // `diff_structure` aligns on `diff_text`. Align them both on `diff_text` and this
        // returns `[]`: the language change is then reported by no tab at all, which is the
        // regression an adversarial review caught in this file's first draft.
        let a = doc(
            r#"{"kind":"doc","content":[{"kind":"codeBlock","attrs":{"language":"rust"},
                 "content":[{"kind":"text","text":"graph TD; A-->B;"}]}]}"#,
        );
        let b = doc(
            r#"{"kind":"doc","content":[{"kind":"codeBlock","attrs":{"language":"mermaid"},
                 "content":[{"kind":"text","text":"graph TD;\n  A-->B;"}]}]}"#,
        );
        let structure = diff_structure(&a, &b);
        assert_eq!(structure.len(), 1, "{structure:?}");
        assert_eq!(structure[0].kind, ChangeKind::Changed);
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1, "{design:?}");
        assert_eq!(design[0].block, crate::block::BlockKind::CodeBlock);
        assert_eq!(design[0].attribute, "language");
        assert_eq!(design[0].before.as_deref(), Some("rust"));
        assert_eq!(design[0].after.as_deref(), Some("mermaid"));
    }

    #[test]
    fn a_reflowed_fence_does_not_hide_a_design_change_in_the_block_beside_it() {
        // The same asymmetry one level down, and the more dangerous shape of it, because
        // here the block whose attribute changed is not the fence at all. `compare_design`
        // descends only into pairs its alignment calls equal, so a blockquote holding both
        // a re-indented fence and a heading that went 2→3 would hide the heading's level
        // change if the design walk read the fence verbatim. The control is the same edit
        // with a paragraph in place of the fence: it reports the level change either way,
        // because prose is collapsed on both alignment keys.
        let with_fence = |level: u8, source: &str| {
            doc(&format!(
                r#"{{"kind":"doc","content":[{{"kind":"blockquote","content":[
                     {{"kind":"heading","attrs":{{"level":{level}}},
                       "content":[{{"kind":"text","text":"Titel"}}]}},
                     {{"kind":"codeBlock","attrs":{{"language":"mermaid"}},
                       "content":[{{"kind":"text","text":{}}}]}}]}}]}}"#,
                serde_json::Value::String(source.to_string())
            ))
        };
        let a = with_fence(2, "graph TD;\n  A-->B;");
        let b = with_fence(3, "graph TD;\n\tA-->B;");
        let design = diff_design(&a, &b);
        assert_eq!(design.len(), 1, "{design:?}");
        assert_eq!(design[0].block, crate::block::BlockKind::Heading);
        assert_eq!(design[0].attribute, "level");
        assert_eq!(design[0].before.as_deref(), Some("2"));
        assert_eq!(design[0].after.as_deref(), Some("3"));
        // …and Struktur still reports the edit, so neither half of it is lost. It names the
        // BLOCKQUOTE, because `diff_structure` compares direct children only — "das Zitat
        // hat sich geändert" rather than an anonymous change two levels down. Without
        // `diff_text` that line would not exist at all: the quote's collapsed text is
        // identical on both sides.
        let structure = diff_structure(&a, &b);
        assert_eq!(structure.len(), 1, "{structure:?}");
        assert_eq!(structure[0].kind, ChangeKind::Changed);
        assert_eq!(structure[0].block, crate::block::BlockKind::Blockquote);
    }

    #[test]
    fn a_fences_preview_is_still_one_line() {
        // `preview` stays on `plain_text` deliberately: it is a label in a list of changes,
        // and a three-line one would break the layout of every row beside it.
        let a = doc(r#"{"kind":"doc"}"#);
        let b = fenced("graph TD;\n  A-->B;");
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].text, "graph TD; A-->B;");
    }

    #[test]
    fn a_preview_is_cut_at_a_character_boundary_and_says_so() {
        let long = "ä".repeat(400);
        let b = doc(&format!(
            r#"{{"kind":"doc","content":[{{"kind":"paragraph","content":[{{"kind":"text","text":"{long}"}}]}}]}}"#
        ));
        let a = doc(r#"{"kind":"doc"}"#);
        let changes = diff_structure(&a, &b);
        assert_eq!(changes.len(), 1);
        assert!(changes[0].text.ends_with('…'));
        assert_eq!(changes[0].text.chars().count(), super::PREVIEW_CHARS + 1);
    }
}
