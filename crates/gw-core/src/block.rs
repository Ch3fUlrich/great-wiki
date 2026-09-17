use crate::slugify;
use serde::{Deserialize, Serialize};

/// Every node kind this system can store. The block registry planned for M4 adds more.
///
/// `#[non_exhaustive]`, so adding one is not a breaking change for downstream matches —
/// **and that is the hazard, not the convenience.** Nothing fails to compile when a variant
/// is added, while four hand-maintained mirrors of this enum sit outside Rust's type system:
///
/// 1. **The editor's node list** (`web/src/lib/editor/extensions.ts`), which is the
///    dangerous one — TipTap *deletes* an element whose node name it does not know, and the
///    deletion is broadcast to every other editor and filed as a revision by the next
///    sweep. It also deletes any attribute the schema does not declare, so a kind's
///    attributes have to be declared there in the same change.
/// 2. **The reader's renderer** (`web/src/lib/blocks/render.ts`'s `BlockKind` union and
///    `BlockView.svelte`), which skips what it does not know — silent, but not destructive.
/// 3. **The CRDT fixtures** (`crates/gw-collab/src/fixtures.rs`), which are what prove a
///    kind survives the Y.Doc conversion at all.
/// 4. **The exporter** (`gw_api::export`), which at least refuses loudly — but a refusal
///    does **not** stop the run: `run()` records the page as refused and writes every other
///    one (`gw_api::export::run`), and only the CLI's exit code fails afterwards
///    (`gw-api/src/main.rs`, via `ExportReport::is_complete`). That is worse rather than
///    better — what the owner is left with is a directory that exists, is quietly missing
///    pages, and looks finished.
///
/// A fifth is softer and still worth doing: `web/src/lib/history.ts`'s `BLOCK_LABEL` names
/// every kind in German for the revision diff, and falls back to the raw name rather than
/// rendering nothing.
///
/// **This list understates it, and the correction is part of adding a kind.** Two more move
/// in practice, and both were missed by the count above until `Transclusion` was added:
///
/// 6. **`ATTRIBUTE_LABEL`**, in the same file as `BLOCK_LABEL` — the German names for the
///    attributes a *design* diff reports. A kind with attributes and no entries there shows
///    a reader `heading` and `label` where the rest of the page shows German words, and the
///    attachment commit edited it in the same hunk as `BLOCK_LABEL` without the list saying
///    it had to.
/// 7. **The editor surface's own `:global(.gw-ed-…)` CSS**, which has to mirror whatever
///    `BlockView.svelte` does for the kind, or one page looks like two different things in
///    the two places it is seen. Latent rather than overdue: both atom kinds give their
///    editor node a class (`gw-ed-datei`, `gw-ed-einbettung`) and neither styles it yet, so
///    they are consistently plain. The day one of them is styled, the other is the mirror
///    that was forgotten.
///
/// And **a mark's attributes have their own version of rule 1, with the opposite sign.**
/// y-tiptap null-filters a NODE's attributes — `createTypeFromElementNode` skips a null value
/// and `updateYFragment` removes one — and does **not** filter a mark's: `marksToAttributes`
/// writes `mark.attrs` back whole. So `TaskItem`'s `id: { default: null }` is free, and the
/// same shape on a mark mints `null` into every stored mark of that kind — which an
/// allow-list in `gw_api::export` keeps without inspecting, refusing the page from the
/// owner's backup. Both edits, or neither; see `Anchor` in `extensions.ts`.
///
/// Adding `TaskList` cost exactly this, adding `Attachment` cost it again, and adding
/// `Transclusion` cost all seven plus a store-side resolver.
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
    /// A file placed in the prose: an image where it belongs, or a card for everything
    /// else. D-15's other half, and a **reference** rather than a possession.
    ///
    /// It carries exactly two attributes and no third may be added without the editor's
    /// schema being widened in the same change (`web/src/lib/editor/extensions.ts`):
    ///
    /// * `filename` — the name the file has *on this page*. Not a path, not a URL and
    ///   above all **not a digest**: a download is authorised against the page it was
    ///   reached through (D-16), and an address built from a content hash is the one thing
    ///   that would bypass that check. The page half of the pair is where the block *is* —
    ///   this is a top-level block of one document's body, so "which page" is never in
    ///   question and never stored, which is also what stops a reference outliving a move.
    /// * `alt` — what the picture shows, written even when it is empty, for the reason
    ///   [`BlockKind::TaskItem`]'s `checked` is: an empty description and no description
    ///   are the same thing to a reader and two different documents to a comparison.
    ///
    /// **A block here does not attach anything and never has.** The `attachments` table is
    /// the authority on what a page carries (D-15), nothing derives a row from
    /// `documents.body`, and so cutting this block out of a paragraph leaves the file
    /// exactly where it was. The converse is a state this system genuinely has: a block
    /// naming a file that is not attached, which the reader states plainly rather than
    /// rendering as a broken picture. `gw_store::attachments`' header is the other end.
    ///
    /// **It contributes nothing to [`Block::plain_text`]**, and that is deliberate rather
    /// than an oversight: `alt` is an attribute, like a heading's `level`, and `plain_text`
    /// is a byte-for-byte contract with `web/src/lib/blocks/render.ts` that feeds the search
    /// index, the chunker and every anchor id. Two consequences worth knowing before somebody
    /// changes it. A description is not searchable; and [`crate::diff`] fingerprints a block
    /// by kind plus text, so two placements look alike to the structure diff and swapping one
    /// picture for another shows up as a *design* change (`filename: a.png → b.png`) instead.
    ///
    /// **Top-level only.** Markdown writes it as an image standing alone in its own
    /// paragraph, and the importer only reads one back at the root of the document; the
    /// editor's schema admits it in `doc` and nowhere else, so a list item, a table cell
    /// and a blockquote can none of them hold one. Both halves are stated in
    /// [`crate::markdown`] and in `extensions.ts`, and they have to agree: a placement the
    /// exporter writes somewhere the importer will not read one back is a page that can
    /// never be exported again.
    Attachment,
    /// A live view of another page, or of one section of it (D-27).
    ///
    /// A **reference**, exactly as [`BlockKind::Attachment`] is, and the difference between
    /// the two is only what is referenced. Nothing of the target is stored here: the block
    /// says *which* page and *which* section, and what is drawn is fetched when the page is
    /// **read**, through the same permission-checked accessor a page read itself ends in.
    /// That is not an optimisation — it is the whole disclosure property. A copy would go on
    /// showing a reader words they lost access to, and would go stale the moment the source
    /// was edited, which is the one thing an embed exists not to do.
    ///
    /// It carries four attributes and no fifth may be added without the editor's schema
    /// being widened in the same change (`web/src/lib/editor/extensions.ts`):
    ///
    /// * `doc` — the target's document id, when the target is known by identity (D-5). The
    ///   one that survives the target being renamed or moved.
    /// * `path` — the target's address, for a target whose id this database has never heard
    ///   of. Exactly one of the two, never both once `gw_store`'s `settle_embeds` has seen
    ///   the body: the pair exists only between `gw_core::markdown` and the store, for
    ///   [`Mark::FALLBACK_ATTR`]'s reason and with [`Mark::FALLBACK_ATTR`]'s life
    ///   expectancy.
    /// * `heading` — the stable id of the heading the embedded section begins at, or absent
    ///   for the whole page. It is a heading's `id` attribute (minted on publish), never its
    ///   slug and never its text: a heading's words change and its position certainly does,
    ///   and an anchor that moved with either would silently quote a different section.
    /// * `label` — the author's own words for what they embedded, written even when empty
    ///   for the reason [`BlockKind::Attachment`]'s `alt` is. It is what the reader is shown
    ///   when the target cannot be shown, and it is the **only** thing in the block that a
    ///   reader who may not read the target ever sees.
    ///
    /// **It has no children, and reconciliation depends on that.** A checklist inside an
    /// embedded section belongs to the page it is written on (D-30): one task, one record,
    /// one board. `gw_store::tasks::reconcile_tasks` walks `content`, so an embed that
    /// carried a copy of the target's tree would mint a second set of task records on the
    /// embedding page and shed the originals' cards on the next publish of the source.
    ///
    /// **It contributes nothing to [`Block::plain_text`]** — same as a placement, same
    /// reasons, plus a sharper one: `plain_text` is computed in this crate, which has no
    /// store and cannot ask a permission question, so feeding another page's words in would
    /// put content the reader may not see into this page's anchor ids and its search index.
    ///
    /// **Top-level only**, exactly as a placement is and for the identical reason: markdown
    /// writes it as an image standing alone in its own paragraph, and the importer only
    /// reads one back at the root of the document. A block the exporter writes somewhere the
    /// importer will not read one back is a page that can never be exported again.
    ///
    /// # `embed` on the wire, `Transclusion` in Rust — and the rename is the decision
    ///
    /// The wire name is what every mirror keys off: TipTap looks a node up by it, the
    /// reader's union is written in it, the CRDT stores it as the element tag. So it is
    /// pinned by this enum's guard test like every other one, and it is deliberately **not**
    /// `transclusion`. "Transclusion" is the name of the *idea* and the word the decision
    /// records use; `embed` is the word the editor's control, the reader's frame and every
    /// German label are about, it is four characters in every Y.Doc that holds one, and it
    /// is what a person reading a stored body will guess. The Rust variant keeps the precise
    /// word because a type name is read by people who want the precise word.
    #[serde(rename = "embed")]
    Transclusion,
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
/// # The one transient exception, and why it never reaches storage
///
/// A mark freshly imported from markdown may carry `doc` **and** [`Mark::FALLBACK_ATTR`] —
/// the target's path as it stood when the file was written. That is not a second address the
/// reader may follow and nothing outside the importer and `gw_store` ever sees it: the store
/// settles it on the way in, either by dropping it (the id is a live document here) or by
/// exchanging the whole mark for an `href` to that path (the id is unknown, which is what a
/// re-seed into a FRESH database produces — every id is minted anew there).
///
/// Without it, restoring a backup into an empty database would turn every internal link into
/// plain words: the text intact, the connection gone. With it, a fresh seed keeps every
/// connection whose target was seeded too, by path, exactly as a backup made before D-5
/// would have. What it must never do is win against a live id — a page RENAMED after the
/// export has a stale path in the file and a perfectly good identity, and identity is the
/// whole point.
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
    /// The key holding an imported reference's fallback path. See the type's own doc
    /// comment: transient, consumed by `gw_store`, never stored and never rendered.
    ///
    /// Deliberately NOT on `gw_api::export`'s `LINK_ATTRS`, so the round-trip comparison
    /// discards it from both sides — a stored `{doc}` and the `{doc, path}` its own markdown
    /// re-imports as are the same document, and a file that carried a path the exporter
    /// resolved afresh must not refuse the page.
    pub const FALLBACK_ATTR: &'static str = "path";

    pub fn link_to_doc(id: &str) -> Self {
        let mut attrs = serde_json::Map::new();
        attrs.insert("doc".into(), serde_json::Value::String(id.to_string()));
        Mark {
            kind: MarkKind::Link,
            attrs,
        }
    }

    /// A reference to `id`, carrying the path that page had when the file was written.
    ///
    /// `None` for `fallback` is [`Mark::link_to_doc`] exactly — an older export, or one whose
    /// target the exporting account could not read, carries no path to fall back to.
    pub fn link_to_doc_at(id: &str, fallback: Option<&str>) -> Self {
        let mut mark = Mark::link_to_doc(id);
        if let Some(path) = fallback {
            mark.attrs.insert(
                Mark::FALLBACK_ATTR.into(),
                serde_json::Value::String(path.to_string()),
            );
        }
        mark
    }

    /// The path this reference would fall back to if its id named nothing here.
    ///
    /// Only ever `Some` between `gw_core::markdown` and `gw_store`; see the type's doc
    /// comment.
    pub fn fallback_path(&self) -> Option<&str> {
        if self.kind != MarkKind::Link {
            return None;
        }
        self.attrs.get(Mark::FALLBACK_ATTR).and_then(|v| v.as_str())
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
    /// The anchor a reader's address bar shows: [`crate::slugify`] of the text.
    ///
    /// Human-readable, ASCII, and **derived** — so it changes when the heading's words do,
    /// which is exactly right for a fragment somebody copied out of the address bar an hour
    /// ago and wrong for anything that has to survive an edit. See [`Heading::anchor`].
    pub id: String,
    /// The heading's **stable** id: the uuid `gw_store` mints onto every heading on publish,
    /// or `None` for a heading that has not been published since that existed.
    ///
    /// This is what a transclusion names a section by (D-27), and the two ids are two
    /// different answers to two different questions on purpose. `id` follows the words; this
    /// one does not, so re-wording a heading moves its fragment and leaves every embed of it
    /// pointing at the same section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
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

    /// The same text, except that a **code block's is taken verbatim**.
    ///
    /// [`crate::diff`] fingerprints a block as its kind plus this string, and it cannot use
    /// [`Block::plain_text`] for that: `plain_text` ends by collapsing every run of
    /// whitespace, so a revision that re-indents a fence — or destroys the newlines a
    /// diagram is delimited by, which is the difference between a drawing and nothing at
    /// all — produces an identical fingerprint, and all three history tabs then report
    /// "Keine Änderungen" about the edit that broke the page.
    ///
    /// **A second function rather than a widening of the first**, because `plain_text` is a
    /// byte-for-byte contract with `web/src/lib/blocks/render.ts::plainText` and feeds the
    /// search index, the embedding chunker, every heading anchor id and the seeder's exact
    /// title comparison. All of those want the collapsed form, one of them lives in another
    /// language, and two test suites exist to keep the pair identical.
    ///
    /// Outside a fence this IS `plain_text`: prose is collapsed the same way and blocks are
    /// separated the same way, which `diff_text_is_plain_text_wherever_no_fence_is_involved`
    /// pins against the shared cases so the copy of the boundary rule below cannot drift
    /// from the original.
    ///
    /// **Exactly one caller: [`crate::diff::diff_structure`]'s fingerprint.** The other two
    /// modes stay on `plain_text`, and both refusals are decisions rather than oversights —
    /// [`crate::diff::diff_prose`] tokenises on whitespace and would have to invent a "word"
    /// that is a run of spaces, and [`crate::diff::diff_design`] compares only the pairs its
    /// alignment calls equal, so a sharper key there would make a reflowed fence hide its
    /// own `language` change and the `level` of a heading standing beside it. An earlier
    /// draft of this comment claimed the fingerprint was the only reader of *any* block
    /// text and lost that second effect; both are now pinned by tests, so before widening
    /// this to a third caller, read `design_key` in that module.
    pub fn diff_text(&self) -> String {
        let mut segments: Vec<String> = Vec::new();
        let mut prose = String::new();
        self.collect_diff_text(&mut prose, &mut segments);
        push_collapsed(&mut prose, &mut segments);
        segments.join(" ")
    }

    /// [`Block::collect_text`]'s walk, interrupted at every code block.
    ///
    /// Prose accumulates in `prose` and is collapsed as one segment whenever a fence
    /// interrupts it, so a fence's own text can be filed beside it untouched. With no fence
    /// anywhere there is exactly one segment and one collapse, which is what makes this
    /// equal to [`Block::plain_text`] for every document that holds no code.
    fn collect_diff_text(&self, prose: &mut String, out: &mut Vec<String>) {
        if self.kind == BlockKind::CodeBlock {
            push_collapsed(prose, out);
            let mut verbatim = String::new();
            self.collect_text(&mut verbatim);
            if !verbatim.is_empty() {
                out.push(verbatim);
            }
            return;
        }
        if let Some(t) = &self.text {
            prose.push_str(t);
        }
        let mut previous: Option<BlockKind> = None;
        for child in &self.content {
            // The same boundary rule as `collect_text`, and it has to stay the same one.
            if child.kind != BlockKind::Text || previous.is_some_and(|p| p != BlockKind::Text) {
                prose.push(' ');
            }
            child.collect_diff_text(prose, out);
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
                anchor: self
                    .attrs
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
            return; // headings do not nest
        }
        for child in &self.content {
            child.collect_headings(out);
        }
    }

    /// The part of this document that sits beneath the heading whose stable id is `anchor`:
    /// that heading and every block after it, up to the next heading of the same level or
    /// higher. `None` when no heading here carries that id.
    ///
    /// `None` is what D-29 is about, and it is the reason this returns an `Option` rather
    /// than an empty document: an embed whose anchor has been deleted or merged away keeps
    /// its frame and says the section is gone, with a link to the source. It must never fall
    /// back to the whole page — that quietly swaps a dosage table for an entire page the
    /// author never meant to quote — and it must never render as nothing, because nothing
    /// here vanishes silently (the rule a detached task follows, D-8).
    ///
    /// **Top level only.** A heading nested inside a blockquote or a list item is not a
    /// section of the document — [`Block::headings`] recurses because an outline wants every
    /// heading, and a *section* is a run of siblings, which only exists at the level the run
    /// is a run of. The store mints ids onto nested headings all the same; they simply
    /// cannot be anchored to, and an embed naming one is an orphan like any other.
    ///
    /// How one [`BlockKind::Transclusion`] is named in the map a reader is handed, or `None`
    /// for a block that names no target at all.
    ///
    /// `<ziel>` or `<ziel>#<abschnitt>`, where `<ziel>` is the target's document id or, for a
    /// target this database has not identified yet, its path. Not the markdown destination
    /// and not the block's position: the destination would put a scheme into a JSON key for
    /// no reason, and a position changes when somebody adds a paragraph above it, so a page
    /// holding two embeds of one section would resolve each of them twice.
    ///
    /// **`web/src/lib/blocks/render.ts::embedKey` is a deliberate mirror of this** and must
    /// stay byte-identical, exactly as `plainText` is: the server fills the map and the
    /// reader looks the entry up, so a disagreement renders every embed on the page as its
    /// label — the same thing an unreadable target renders as, which is precisely the state
    /// nobody would think to investigate.
    pub fn embed_key(&self) -> Option<String> {
        if self.kind != BlockKind::Transclusion {
            return None;
        }
        let attr = |key: &str| self.attrs.get(key).and_then(|v| v.as_str());
        let target = attr("doc").or_else(|| attr("path"))?;
        Some(match attr("heading") {
            Some(heading) => format!("{target}#{heading}"),
            None => target.to_string(),
        })
    }

    /// A pure function of the tree, here rather than in `gw_store` for [`crate::MARK_ORDER`]'s
    /// reason: the editor's section picker and the reader's expansion have to agree about
    /// where a section ends, and two copies of that rule would stop agreeing.
    pub fn section(&self, anchor: &str) -> Option<Block> {
        let start = self.content.iter().position(|b| {
            b.kind == BlockKind::Heading
                && b.attrs.get("id").and_then(|v| v.as_str()) == Some(anchor)
        })?;
        let level = heading_level(&self.content[start]);
        let mut section = Block {
            kind: BlockKind::Doc,
            attrs: serde_json::Map::new(),
            content: vec![self.content[start].clone()],
            text: None,
            marks: Vec::new(),
        };
        for block in &self.content[start + 1..] {
            if block.kind == BlockKind::Heading && heading_level(block) <= level {
                break;
            }
            section.content.push(block.clone());
        }
        Some(section)
    }
}

/// A heading's level, clamped the way every reader of one clamps it.
fn heading_level(block: &Block) -> u8 {
    block
        .attrs
        .get("level")
        .and_then(|v| v.as_u64())
        .unwrap_or(1)
        .clamp(1, 6) as u8
}

/// Collapse the prose gathered so far into one segment, exactly as [`Block::plain_text`]
/// collapses a whole document, and file it. Nothing is filed for an empty run, so a fence
/// at the start or the end of a block never produces a leading or trailing separator.
fn push_collapsed(prose: &mut String, out: &mut Vec<String>) {
    let collapsed = prose.split_whitespace().collect::<Vec<_>>().join(" ");
    prose.clear();
    if !collapsed.is_empty() {
        out.push(collapsed);
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

    // --- the diff's text, which is `plain_text` everywhere except inside a fence ----------

    /// A fenced block as the importer stores one: the info string's first word, and text
    /// whose newlines and indentation are the whole of its meaning.
    fn fence(text: &str) -> Block {
        serde_json::from_str(&format!(
            r#"{{"kind":"codeBlock","attrs":{{"language":"mermaid"}},
                 "content":[{{"kind":"text","text":{}}}]}}"#,
            serde_json::Value::String(text.to_string())
        ))
        .unwrap()
    }

    #[test]
    fn diff_text_is_plain_text_wherever_no_fence_is_involved() {
        // The mirror, pinned. `diff_text` repeats `collect_text`'s boundary rule so that it
        // can interrupt it at a code block, and this is what stops the copy drifting from
        // the original — which would put a different string in front of the structure diff
        // than in front of the search index, for prose that has nothing to do with code.
        for (json, expected) in PLAIN_TEXT_CASES {
            let block: Block = serde_json::from_str(json).unwrap();
            assert_eq!(&block.diff_text(), expected);
        }
        let doc = sample();
        assert_eq!(doc.diff_text(), doc.plain_text());
    }

    #[test]
    fn a_fence_keeps_its_whitespace_in_diff_text_and_loses_it_in_plain_text() {
        // Both halves asserted together, because the split is the decision: the search
        // index and every anchor id want the collapsed form, and the structural diff wants
        // the typed one. `graph TD; A-->B;` on one line parses as nothing.
        let block = fence("graph TD;\n  A-->B;");
        assert_eq!(block.diff_text(), "graph TD;\n  A-->B;");
        assert_eq!(block.plain_text(), "graph TD; A-->B;");
    }

    #[test]
    fn a_fence_inside_a_quote_keeps_its_whitespace_too() {
        // A code block is not a top-level-only kind — the editor's schema admits one inside
        // a blockquote or a list item — so the verbatim rule has to survive the recursion,
        // and the prose around it still has to collapse the way it always did.
        let doc = Block {
            kind: BlockKind::Doc,
            attrs: serde_json::Map::new(),
            content: vec![Block {
                kind: BlockKind::Blockquote,
                attrs: serde_json::Map::new(),
                content: vec![
                    serde_json::from_str(
                        r#"{"kind":"paragraph","content":[{"kind":"text","text":"So   geht   es"}]}"#,
                    )
                    .unwrap(),
                    fence("a\n  b"),
                ],
                text: None,
                marks: Vec::new(),
            }],
            text: None,
            marks: Vec::new(),
        };
        assert_eq!(doc.diff_text(), "So geht es a\n  b");
    }

    #[test]
    fn an_empty_fence_reads_as_empty_rather_than_as_a_stray_separator() {
        let empty: Block =
            serde_json::from_str(r#"{"kind":"codeBlock","attrs":{"language":"rust"}}"#).unwrap();
        assert_eq!(empty.diff_text(), "");
        assert_eq!(empty.diff_text(), empty.plain_text());
    }

    /// The `BlockKind` twin of `markdown.rs`'s
    /// `the_canonical_order_places_every_mark_kind_exactly_once`, and the only thing in Rust
    /// that notices a variant being added at all.
    #[test]
    fn adding_a_block_kind_trips_this_test_and_names_what_else_must_change() {
        // The match is exhaustive on purpose. `BlockKind` is `#[non_exhaustive]`, but that
        // only binds OTHER crates — inside this one, adding a kind stops this test
        // compiling until somebody has read the list below. Nothing else in the workspace
        // does: there is no exhaustive match on this enum anywhere, and the exporter's
        // dispatch carries a wildcard arm by design, so a Rust-only addition otherwise
        // passes `cargo test --workspace`, `npm run check` and `npx vitest run` green while
        // the editor's deletion path is live on the kind nobody taught it about.
        const EVERY_KIND: &[BlockKind] = &[
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
            BlockKind::Attachment,
            BlockKind::Transclusion,
            BlockKind::Text,
        ];
        for kind in EVERY_KIND {
            match kind {
                BlockKind::Doc
                | BlockKind::Paragraph
                | BlockKind::Heading
                | BlockKind::BulletList
                | BlockKind::OrderedList
                | BlockKind::ListItem
                | BlockKind::TaskList
                | BlockKind::TaskItem
                | BlockKind::Blockquote
                | BlockKind::CodeBlock
                | BlockKind::Table
                | BlockKind::TableRow
                | BlockKind::TableHeader
                | BlockKind::TableCell
                | BlockKind::Attachment
                | BlockKind::Transclusion
                | BlockKind::Text => {}
            }
        }

        // The WIRE name, not the Rust one: `camelCase` is what every mirror keys off — the
        // editor looks a node up by it, the reader's union is written in it, the CRDT
        // stores it — so renaming a variant without renaming the serialisation is the same
        // bug as adding one.
        let names: Vec<String> = EVERY_KIND
            .iter()
            .map(|kind| {
                serde_json::to_value(kind)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_string()
            })
            .collect();
        assert_eq!(
            names,
            [
                "doc",
                "paragraph",
                "heading",
                "bulletList",
                "orderedList",
                "listItem",
                "taskList",
                "taskItem",
                "blockquote",
                "codeBlock",
                "table",
                "tableRow",
                "tableHeader",
                "tableCell",
                "attachment",
                "embed",
                "text",
            ],
            "A `BlockKind` was added, removed or renamed. It has four mirrors outside Rust's \
             type system and they must move in the SAME change (see this enum's doc \
             comment): 1. the editor's node list, `web/src/lib/editor/extensions.ts` — the \
             dangerous one, because TipTap DELETES an element whose name it does not know, \
             broadcasts the deletion to everyone else editing and files it as a revision, \
             and it deletes any attribute the schema does not declare; 2. the reader, \
             `web/src/lib/blocks/render.ts`'s union and `BlockView.svelte`, which skip what \
             they do not know; 3. the CRDT fixtures, `crates/gw-collab/src/fixtures.rs`, \
             which are what prove a kind survives the Y.Doc at all; 4. the exporter, \
             `gw_api::export`, which refuses loudly — and a refusal is a page missing from \
             the owner's backup. A fifth is softer: `web/src/lib/history.ts`'s \
             `BLOCK_LABEL`, the German name in the revision diff. Then add the new wire \
             name here."
        );
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

    // --- a section, which is what a transclusion may name half of a page by ---------------

    /// Two sections under one `##`, with a `###` inside the first so the walk has something
    /// to *not* stop at, and a trailing `##` so it has something to stop at.
    fn sectioned() -> Block {
        serde_json::from_str(
            r#"{"kind":"doc","content":[
                 {"kind":"paragraph","content":[{"kind":"text","text":"Vorspann"}]},
                 {"kind":"heading","attrs":{"level":2,"id":"0199c0de-0000-7000-8000-00000000000a"},
                  "content":[{"kind":"text","text":"Dosierung"}]},
                 {"kind":"paragraph","content":[{"kind":"text","text":"5 mg"}]},
                 {"kind":"heading","attrs":{"level":3,"id":"0199c0de-0000-7000-8000-00000000000b"},
                  "content":[{"kind":"text","text":"Kinder"}]},
                 {"kind":"paragraph","content":[{"kind":"text","text":"2 mg"}]},
                 {"kind":"heading","attrs":{"level":2,"id":"0199c0de-0000-7000-8000-00000000000c"},
                  "content":[{"kind":"text","text":"Nebenwirkungen"}]},
                 {"kind":"paragraph","content":[{"kind":"text","text":"Selten"}]}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_section_runs_to_the_next_heading_of_its_own_level_or_higher() {
        let section = sectioned()
            .section("0199c0de-0000-7000-8000-00000000000a")
            .expect("the anchor is in the document");
        // The heading itself, its paragraph, the `###` under it and that one's paragraph —
        // and NOT the `##` that follows, nor the prose before it.
        assert_eq!(section.plain_text(), "Dosierung 5 mg Kinder 2 mg");
        assert_eq!(section.kind, BlockKind::Doc);
    }

    #[test]
    fn a_deeper_section_stops_at_the_next_heading_of_any_higher_level() {
        let section = sectioned()
            .section("0199c0de-0000-7000-8000-00000000000b")
            .expect("the anchor is in the document");
        assert_eq!(section.plain_text(), "Kinder 2 mg");
    }

    #[test]
    fn a_section_whose_anchor_is_gone_is_none_and_never_the_whole_page() {
        // D-29: the embedding page keeps its frame and says the section no longer exists.
        // Falling back to the whole page would quietly swap a dosage table for a page the
        // author never meant to quote, so the absence has to be expressible.
        assert!(sectioned()
            .section("0199c0de-0000-7000-8000-ffffffffffff")
            .is_none());
    }

    #[test]
    fn a_heading_carries_both_its_slug_and_its_stable_anchor() {
        let headings = sectioned().headings();
        assert_eq!(headings[0].id, "dosierung");
        assert_eq!(
            headings[0].anchor.as_deref(),
            Some("0199c0de-0000-7000-8000-00000000000a")
        );
        // A heading that has never been published since ids existed has no stable anchor,
        // and says so rather than pretending its slug is one.
        assert_eq!(sample().headings()[0].anchor, None);
    }

    #[test]
    fn an_embed_has_no_children_and_contributes_no_text() {
        // D-30's reconciliation half, asserted where the shape is decided rather than only
        // where it is relied on: an embed is a REFERENCE, so the embedding page's body holds
        // none of the target's blocks and `reconcile_tasks` — which walks `content` — has
        // nothing of somebody else's checklist to mint records for.
        let doc: Block = serde_json::from_str(
            r#"{"kind":"doc","content":[
                 {"kind":"paragraph","content":[{"kind":"text","text":"Siehe:"}]},
                 {"kind":"embed","attrs":{"doc":"0199c0de-0000-7000-8000-000000000001",
                                          "heading":"0199c0de-0000-7000-8000-00000000000a",
                                          "label":"Dosierung"}}]}"#,
        )
        .unwrap();
        assert_eq!(doc.content[1].kind, BlockKind::Transclusion);
        assert!(doc.content[1].content.is_empty());
        // The label is an attribute, like a placement's `alt`, so it is not in the search
        // index, not in an anchor id and not in the chunker.
        assert_eq!(doc.plain_text(), "Siehe:");
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
