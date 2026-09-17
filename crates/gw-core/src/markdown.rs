//! CommonMark → `Block` tree.
//!
//! This is one half of the export round-trip (M12) and the foundation of the Markdown
//! importer (M13), so it is written to be exhaustive rather than convenient. The rule it
//! obeys everywhere: **text is never dropped**. A construct the M1 block schema cannot
//! represent yet keeps its text in the nearest block that *can* hold it, and says so in
//! `Conversion::notes` — a silent loss is the one outcome that cannot be detected later.
//!
//! # Placing a file in the prose (D-15)
//!
//! One construct is read as something other than what CommonMark calls it: an image whose
//! destination names a file on *this* page becomes a [`crate::BlockKind::Attachment`]. Both
//! halves of that syntax live here — [`attachment_destination`] writes it and
//! [`attachment_reference`] reads it — so the exporter cannot drift away from the importer,
//! and the two rules that decide when it applies are stated on
//! [`Builder::placement_is_possible`] and [`Builder::settle_placements`]: the reference must
//! stand **alone in its own paragraph** and that paragraph must be at the **top level** of
//! the document. Anywhere else it is an ordinary image and degrades exactly as one.
//!
//! # Pointing at another page (D-5, D-21)
//!
//! The same shape a second time: `[Titel](dok:<id>)` is a link to a page of this wiki *by
//! identity*, so that renaming or moving the target cannot break it. Both halves live here
//! too — [`document_destination`] writes it and [`document_reference`] reads it — and a
//! `dok:` destination that is not shaped like a document id stays an ordinary link rather
//! than becoming a broken reference. The destination may carry the target's path in
//! CommonMark's link-title slot ([`document_fallback`]) — what a database that has never
//! heard of that id falls back to, and nothing else. [ADR 0019](../../../docs/decisions/0019-how-a-document-reference-is-written-in-markdown.md)
//! has the reasoning and the cost.
//!
//! # Embedding another page, or one section of it (D-27)
//!
//! The first shape a third time: `![Beschriftung](einbettung:<ziel>#<abschnitt> "/pfad")` is a
//! live view of another page. [`transclusion_destination`] writes it and
//! [`transclusion_reference`] reads it, and it goes through the *image* path — not the link
//! path — because an embed is a top-level atom whose text is what to show when the thing
//! itself cannot be shown, which is what an image is and what a link is not. The
//! "alone in its own top-level paragraph" rule is therefore the same rule, unchanged.
//!
//! The `#<abschnitt>` names a heading by the **stable id** the store mints onto it on publish,
//! and that id is written into the file as well — `## Titel {#<uuid>}`, see
//! [`heading_anchor`]. It is read here rather than by turning on pulldown-cmark's heading
//! attributes, so that only a uuid-shaped suffix counts and no heading anybody has already
//! written changes meaning. [ADR 0020](../../../docs/decisions/0020-how-an-embedded-page-and-a-heading-s-anchor-are-written-in-markdown.md)
//! has the reasoning and the cost.

use crate::block::{Block, BlockKind, Mark, MarkKind};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::collections::BTreeMap;
use std::fmt;

/// What an image destination has to say to be a reference to a file on this page.
///
/// A scheme, not a shape. The obvious alternative — reading any destination with no slash
/// in it (`![x](befund.png)`) as an attachment — is a *guess*, and this project has already
/// written down why it does not make those: `gw_store::blobs` refuses to sniff a `.csv` by
/// looking for commas, because a guess dressed as a measurement is the same mistake as
/// trusting what an upload declared. Here the guess would be worse than useless: every
/// markdown file anybody has ever imported holding a relative image would silently acquire
/// a reference to a file that was never attached, and the page would then say so on screen.
///
/// So a placement is a *statement*. `anhang:` is German because this wiki is, the same way
/// its routes are (`/papierkorb`, `/themen`), and it is what somebody reading an exported
/// file sees rather than a bare filename that looks like a broken relative link.
pub const ATTACHMENT_SCHEME: &str = "anhang:";

/// The markdown destination that names `filename` as a file on this page.
///
/// The *writing* half of one agreement whose *reading* half is [`attachment_reference`],
/// and both live here rather than in `gw_api::export` for the reason [`crate::MARK_ORDER`]
/// does: two copies of one rule in two crates stop agreeing the day one of them is edited,
/// and what that costs is an export that refuses every page holding a picture — which is
/// the owner's backup path, failing on the first refusal.
///
/// `gw_store::attachments::canonical_filename` admits everything but `/`, `\`, `"` and
/// control characters, so a name can hold spaces, brackets and unbalanced parentheses —
/// none of which a bare CommonMark destination can carry. The angle-bracket form can carry
/// all of them, so it is used for anything that is not plainly safe without it, and `<`,
/// `>` and `\` inside it are backslash-escaped, which the parser undoes on the way back.
/// Nothing is percent-encoded: pulldown-cmark does not decode, so `%20` would come back as
/// three characters of the filename.
///
/// `None` for a name this syntax cannot carry back unchanged — one holding a character
/// [`attachment_reference`] refuses, or padded with spaces it would trim off. No name a
/// page can really give a file is in that set, so `None` means a hand-written body rather
/// than an ordinary page; the exporter turns it into a refusal that names the page, which
/// is this module's rule everywhere: nothing is quietly degraded.
pub fn attachment_destination(filename: &str) -> Option<String> {
    // Asked of the READER rather than restated as a second list of characters, so the two
    // halves cannot drift: this writes a destination only for a name that one would read
    // back. What it is asked about is the *unwrapped* name, because the angle brackets and
    // their escapes belong to the parser — that half is proved by
    // `every_name_a_page_can_give_a_file_survives_being_written_and_read_back`, which goes
    // through the real parser rather than through a second copy of its rules written here.
    if filename.trim() != filename
        || attachment_reference(&format!("{ATTACHMENT_SCHEME}{filename}")) != Some(filename)
    {
        return None;
    }
    let plain = !filename
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '<' | '>' | '(' | ')' | '\\'));
    let written = if plain {
        format!("{ATTACHMENT_SCHEME}{filename}")
    } else {
        let mut out = String::with_capacity(filename.len() + ATTACHMENT_SCHEME.len() + 2);
        out.push('<');
        out.push_str(ATTACHMENT_SCHEME);
        for c in filename.chars() {
            if matches!(c, '<' | '>' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('>');
        out
    };
    Some(written)
}

/// The file an image destination names, or `None` for a destination that names none.
///
/// The reading half of [`attachment_destination`]. It refuses exactly what
/// `gw_store::attachments::canonical_filename` refuses — `/`, `\`, `"`, control characters,
/// `.` and `..`, and a name that is empty or only spaces — so a destination this accepts is
/// a name a page could really give a file. It is deliberately **not** a permission check
/// and not an existence check: this crate has no store, exactly as it has none to resolve a
/// link's `doc` target with, and whether the file is actually attached is a question only
/// the page's `Anhänge` list can answer.
pub fn attachment_reference(dest: &str) -> Option<&str> {
    let name = dest.strip_prefix(ATTACHMENT_SCHEME)?.trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || c == '"' || c.is_control())
    {
        return None;
    }
    Some(name)
}

/// What a link destination has to say to be a reference to another page of this wiki.
///
/// [`ATTACHMENT_SCHEME`]'s argument, generalised to the other half of D-15's reasoning, and
/// it is D-5's: a link stores the target's document **id**, so that renaming or moving a
/// page cannot break an inbound link. A file on disk has to spell that id somehow, and the
/// obvious spelling — the resolved *path*, `[Titel](/darm/labor)` — is the one combination
/// that permanently breaks the backup. The importer below reads a path as
/// [`crate::Mark::link_to_url`] and can do nothing else (this crate has no store), so the
/// stored `{doc: id}` and the re-imported `{href: "/darm/labor"}` differ, and
/// `gw_api::export::render_file` refuses the page — for ever, since a page that cannot be
/// exported cannot be exported later either.
///
/// So a reference is a **statement**, exactly as a placement is. Nothing predating this
/// feature writes `dok:` and nothing writes it by accident; an import either finds that
/// statement or it does not. The cost is the same one ADR 0015 accepted: `dok:01J8…` is an
/// address no other renderer can follow, which is honest about a directory that
/// `EXPORT-README.txt` already calls a faithful copy of *the database*.
///
/// German, for [`ATTACHMENT_SCHEME`]'s reason — this wiki's routes are `/papierkorb` and
/// `/themen`, and somebody reading an exported file sees a German word rather than a bare
/// uuid that looks like a broken relative link.
pub const DOCUMENT_SCHEME: &str = "dok:";

/// The markdown destination that names the document `id`, or `None` for an `id` this
/// syntax cannot carry back unchanged.
///
/// The *writing* half of one agreement whose *reading* half is [`document_reference`], both
/// here for [`attachment_destination`]'s reason: two copies of one rule in two crates stop
/// agreeing the day one of them is edited, and the cost is an export that refuses every
/// page holding a reference.
///
/// **`None` is the whole safety property, and it is not about aesthetics.** Nothing
/// validates a mark's attributes on the write path — `gw_collab`'s `attrs_to_marks` copies
/// whatever the Yjs attribute carries, so anyone with write access on one page can store
/// `{"doc": "x) [siehe](https://angreifer.example/"}` over the collaboration socket — and
/// `gw_api::export`'s renderer interpolates a link destination with no escaping at all. A
/// writer that emitted whatever it was handed would put attacker text straight into the
/// owner's backup file. Asking [`document_reference`] first means only a destination that
/// reads back as the same id is ever written, and a uuid cannot contain a space, a bracket
/// or a parenthesis — so no escaping is needed rather than merely omitted.
pub fn document_destination(id: &str) -> Option<String> {
    let written = format!("{DOCUMENT_SCHEME}{id}");
    (document_reference(&written) == Some(id)).then_some(written)
}

/// The document a link destination names, or `None` for a destination that names none.
///
/// The reading half of [`document_destination`]. It accepts exactly the shape
/// `Uuid::now_v7().to_string()` writes — eight, four, four, four and twelve hex digits,
/// hyphen-separated — because that is what `gw_store::documents` mints a `documents.id` as,
/// and nothing else in this system is a document id.
///
/// **A `dok:` destination that is not that shape is not a broken reference; it is an
/// ordinary link.** It has to be. `dok:etwas` is a string anybody with write access could
/// have typed into the old link control at any point in the past: it stores as an `href`,
/// exports as `[T](dok:etwas)` and re-imports as the same `href` — and the day this reader
/// learned to accept anything after the colon, that page would re-import as `{doc:
/// "etwas"}`, differ from the stored `{href: "dok:etwas"}`, and be refused from every
/// export from then on, without anybody having edited it. Degrading to an `href` keeps such
/// a page exactly as harmless as it was.
///
/// Deliberately **not** an existence check and **not** a permission check, exactly as
/// [`attachment_reference`] is neither: this crate has no store. Whether the id names a
/// live page the reader may see is `gw_store`'s question and is re-asked on every read.
pub fn document_reference(dest: &str) -> Option<&str> {
    let id = dest.strip_prefix(DOCUMENT_SCHEME)?;
    is_document_id(id).then_some(id)
}

/// Whether `s` is shaped like a `documents.id`: a hyphenated, lower- or upper-case hex uuid.
///
/// Case is not normalised and not refused — the string is handed back as it arrived, so
/// either case round-trips — because the question here is "could this be an id", and the
/// answer to "is it *this* wiki's id" belongs to the store.
fn is_document_id(s: &str) -> bool {
    let mut groups = s.split('-');
    for len in [8usize, 4, 4, 4, 12] {
        match groups.next() {
            Some(g) if g.len() == len && g.bytes().all(|b| b.is_ascii_hexdigit()) => {}
            _ => return false,
        }
    }
    groups.next().is_none()
}

/// The path a `dok:` reference falls back to, written in CommonMark's **link title** slot —
/// `[Titel](dok:<id> "/darm/labor")` — or `None` for a path this syntax cannot carry back
/// unchanged.
///
/// # Why a reference carries a path at all, when the id is the point
///
/// Because an export re-seeded into a **fresh** database mints every id anew (`SeedMeta` has
/// no `id` key), so every `dok:` in a restored corpus would point at nothing: the words
/// intact, the connection gone. That is a regression of the backup path, and it is avoidable.
/// The path is what the file already knew and what a pre-identity export would have carried.
///
/// **It is a fallback and never a second address.** `gw_store` settles it on the way in: the
/// id names a live document here, so the path is dropped; or it does not, so the mark becomes
/// an ordinary `href` to that path, which the next publish resolves to *this* database's id.
/// A page RENAMED after the export therefore resolves by identity — the file's path is stale
/// and the id is not — which is the whole reason identity is stored in the first place.
///
/// # Why the title slot
///
/// It is markdown's own place for a note about a destination, every CommonMark parser already
/// reads it, and `pulldown-cmark` hands it over unescaped. Nothing else in this system writes
/// a link title, and the importer throws one away everywhere else, so nothing is displaced.
///
/// `None` for anything the reader below would not give back unchanged. `documents.path` is
/// built from slugs and is always a plain, rooted, ASCII path — so `None` means a
/// hand-written or hostile value, and the exporter then writes the reference bare rather than
/// smuggling text into the file: `Renderer::wrap` escapes nothing at all.
pub fn document_fallback(path: &str) -> Option<String> {
    (document_fallback_path(path) == Some(path)).then(|| format!("\"{path}\""))
}

/// The fallback path a link title states, or `None` if it states none.
///
/// The reading half of [`document_fallback`], asked by it rather than restated, so the two
/// cannot drift. Accepts exactly the shape `Store::create_document`'s `resolved_path` builds:
/// rooted, non-empty, and made of slug characters and separators. It is deliberately **not**
/// an existence check and **not** a permission check — this crate has no store, exactly as it
/// has none to resolve a `doc` target with.
pub fn document_fallback_path(title: &str) -> Option<&str> {
    if !title.starts_with('/') || title.len() < 2 || title.ends_with('/') {
        return None;
    }
    if title.contains("//")
        || !title
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_' | '.'))
    {
        return None;
    }
    Some(title)
}

/// What an image destination has to say to be a **live view of another page** (D-27).
///
/// [`ATTACHMENT_SCHEME`]'s argument a third time, and the third time it needs no arguing:
/// a scheme rather than a shape, German because this wiki is, both halves in this crate
/// because two copies of one rule in two crates stop agreeing the day one of them is edited.
///
/// It is written as an **image** rather than as a link, and that is a decision rather than an
/// accident of reuse. A transclusion is a top-level atom with no inline content, which is
/// exactly what a placement is and exactly what a link is not: CommonMark renders `![…]`'s
/// content as a plain string, so the author's label cannot carry marks that would have to
/// round-trip, and the existing "alone in its own top-level paragraph" rule
/// ([`Builder::placement_is_possible`], [`Builder::settle_placements`]) is the same rule
/// unchanged. And the alt slot is *already* markdown's place for "what to show when the
/// thing itself cannot be shown", which is precisely what a label is for
/// ([`crate::BlockKind::Transclusion`]'s `label`).
pub const TRANSCLUSION_SCHEME: &str = "einbettung:";

/// What an [`crate::BlockKind::Transclusion`] destination names.
///
/// Exactly one of `doc` and `path` is `Some`, which is the same invariant
/// [`crate::Mark`] states about a link and for the same reason: a block carrying both can
/// disagree with itself the day the target moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transclusion<'a> {
    /// The target's document id, when the destination names one by identity (D-5).
    pub doc: Option<&'a str>,
    /// The target's address, when the destination names one by where it is.
    ///
    /// This is the restored-backup form and not a second way of writing a reference: a seed
    /// into a fresh database mints every document id anew, so no `einbettung:<uuid>` in a
    /// restored corpus can match anything, and an embed that could only be written by
    /// identity would come back as a frame around nothing. `gw_store`'s `settle_embeds`
    /// exchanges an unknown id for the path a file carried, and the next publish exchanges
    /// the path back for *this* database's id — the same two-step ADR 0019 built for `dok:`.
    pub path: Option<&'a str>,
    /// The stable id of the heading the embedded section starts at; `None` for a whole page.
    pub heading: Option<&'a str>,
}

/// The markdown destination naming `doc` or `path`, optionally anchored at `heading`, or
/// `None` for anything this syntax could not carry back unchanged.
///
/// The *writing* half of one agreement whose *reading* half is [`transclusion_reference`],
/// and it asks that half rather than restating its rules — [`document_destination`]'s
/// reason, and its safety property: `gw_api::export`'s renderer interpolates a destination
/// with no escaping at all, and every one of these values reached `documents.body` over the
/// collaboration socket without passing any validation.
pub fn transclusion_destination(
    doc: Option<&str>,
    path: Option<&str>,
    heading: Option<&str>,
) -> Option<String> {
    let target = match (doc, path) {
        (Some(doc), None) => doc,
        (None, Some(path)) => path,
        // Neither is a block that names nothing; both is a block that can disagree with
        // itself. The exporter turns either into a refusal that names the page.
        _ => return None,
    };
    let written = match heading {
        Some(heading) => format!("{TRANSCLUSION_SCHEME}{target}#{heading}"),
        None => format!("{TRANSCLUSION_SCHEME}{target}"),
    };
    let read = transclusion_reference(&written)?;
    (read.doc == doc && read.path == path && read.heading == heading).then_some(written)
}

/// What an image destination embeds, or `None` for a destination that embeds nothing.
///
/// The reading half of [`transclusion_destination`]. The target is a document id
/// ([`is_document_id`]) or a rooted wiki path ([`document_fallback_path`]'s shape, which is
/// what `Store::create_document` builds), and nothing else; the optional anchor after `#` is
/// a document id too, because that is what a heading's stable id is minted as.
///
/// **Anything else is an ordinary image**, and that degradation is load-bearing rather than
/// lenient for [`document_reference`]'s reason, run once more: `einbettung:etwas` is a string
/// anybody with write access could type into an image destination, it round-trips today as a
/// picture from somewhere this wiki does not store, and a reader that accepted it would make
/// that page re-import as a different tree — a page nobody edited becoming permanently
/// unexportable.
///
/// Deliberately **not** an existence check and **not** a permission check: this crate has no
/// store, exactly as it has none to resolve a link's `doc` with.
pub fn transclusion_reference(dest: &str) -> Option<Transclusion<'_>> {
    let target = dest.strip_prefix(TRANSCLUSION_SCHEME)?;
    // The anchor is split off first because a path may not contain `#` and an id may not
    // either, so the LAST `#` cannot belong to the target — and splitting on the first would
    // read `#` inside a malformed target as an anchor boundary.
    let (target, heading) = match target.rsplit_once('#') {
        Some((target, heading)) => (target, Some(heading)),
        None => (target, None),
    };
    if let Some(heading) = heading {
        if !is_document_id(heading) {
            return None;
        }
    }
    if is_document_id(target) {
        return Some(Transclusion {
            doc: Some(target),
            path: None,
            heading,
        });
    }
    document_fallback_path(target).map(|path| Transclusion {
        doc: None,
        path: Some(path),
        heading,
    })
}

/// The suffix that states a heading's **stable** id: `## Titel {#<uuid>}`.
///
/// # Why a heading needs one at all, and why it is in the file
///
/// A transclusion may name one section of a page (D-27), and a section needs an anchor that
/// survives an edit. A heading's words change and its position certainly does, so neither is
/// an anchor; the answer is the one a task block already uses — a uuid minted server-side on
/// publish, carried in the block's `attrs`.
///
/// It could have stopped there. `gw_api::export`'s comparison would simply reduce the id
/// away, exactly as it reduces a task's, and the file would not mention it. That was rejected
/// for ADR 0019's reason: a seed into a fresh database mints ids anew, so an export restored
/// into an empty store would keep every word, keep every page, and orphan **every section
/// embed in the corpus** — with no fallback available, because the anchor's whole point is
/// that it is not derivable from anything else in the file. Writing it costs one visible
/// suffix per heading and buys a restore that is complete rather than merely readable.
///
/// # Why it is parsed here rather than by pulldown-cmark
///
/// `Options::ENABLE_HEADING_ATTRIBUTES` exists and would read `{#id}` for us. Turning it on
/// is a **global re-parse of every heading in the corpus**: a heading whose text happens to
/// end in `{#…}` would come back as different text with an id, differ from what is stored,
/// and be refused from every export from then on — a page nobody edited becoming
/// unexportable, which this project treats as disqualifying. Reading the suffix here instead
/// means the rule can be as narrow as it needs to be: only a **uuid-shaped** id counts, which
/// is a thing no heading in this corpus contains and no author writes by accident.
///
/// `None` for an id this syntax could not carry back unchanged, which the exporter turns into
/// a refusal that names the page.
pub fn heading_anchor(id: &str) -> Option<String> {
    let written = format!(" {{#{id}}}");
    (heading_anchor_id(&format!("x{written}")) == Some(("x", id))).then_some(written)
}

/// A heading's text split from the stable id its last characters state, or `None` when they
/// state none.
///
/// The reading half of [`heading_anchor`], asked by it rather than restated. Returns the text
/// with the suffix **and the space before it** removed, so `## Titel {#…}` gives back exactly
/// the `Titel` that was stored.
///
/// The **last** such suffix wins, and that is what makes the pair total rather than merely
/// usually right: a heading whose own words end in `{#0199…}` exports as
/// `## Titel {#0199…} {#<real id>}` and reads back as the text it was, because the real id is
/// always written last. The one shape this cannot carry is a heading that has **no** id and
/// whose text ends in an anchor-shaped suffix; `gw_api::export` refuses that page loudly
/// rather than writing a file that would re-import as something else, and a single publish
/// (which mints the id) fixes it for good.
pub fn heading_anchor_id(text: &str) -> Option<(&str, &str)> {
    let head = text.strip_suffix('}')?;
    let (head, id) = head.rsplit_once("{#")?;
    if !is_document_id(id) {
        return None;
    }
    // The space is the separator the writer put there; without it, `Titel{#…}` would come
    // back as `Titel` and the file would not re-import as itself. An EMPTY head is the one
    // exception and it is a real document: a heading with no words and an id writes as
    // `## {#…}`, whose whole text is the suffix, with the separator absorbed by the `# `.
    let head = if head.is_empty() {
        head
    } else {
        head.strip_suffix(' ')?
    };
    Some((head, id))
}

/// What an image destination really names, or `None` for an ordinary picture.
///
/// The one place the two schemes are asked about together, so that "which schemes make an
/// image a block" is a single list rather than a condition repeated at each end of the
/// converter.
fn placed_by(dest: &str, title: &str) -> Option<Placed> {
    if let Some(filename) = attachment_reference(dest) {
        return Some(Placed::File {
            filename: filename.to_string(),
        });
    }
    transclusion_reference(dest).map(|t| Placed::Page {
        // The title slot carries the target's path as it stood when the file was written — a
        // FALLBACK for a database that does not know this id, never a second address, and
        // read only for an embed that names its target BY id. `gw_store`'s `settle_embeds`
        // ends the pair; see [`document_fallback`], whose reasoning this is.
        path: match (t.doc, t.path) {
            (Some(_), _) => document_fallback_path(title).map(str::to_string),
            (None, path) => path.map(str::to_string),
        },
        doc: t.doc.map(str::to_string),
        heading: t.heading.map(str::to_string),
    })
}

/// Whether this block is one an image destination produced — a placement or an embed.
///
/// Both are top-level atoms written as an image alone in its own paragraph, so both take the
/// same degradation when that paragraph turns out to hold something else as well.
fn is_placed(block: &Block) -> bool {
    matches!(block.kind, BlockKind::Attachment | BlockKind::Transclusion)
}

/// A markdown construct the M1 block schema cannot represent, and what became of it.
///
/// Kept as an enum rather than a string so a caller can match on one, and so the
/// milestone that adds the missing block type deletes a variant and gets a compile error
/// at every site that still expects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Unsupported {
    /// An image that is **not** a placement: its alt text survives as text and its
    /// destination does not.
    ///
    /// Narrower than it was. An image whose destination names a file on this page —
    /// `![Befund](anhang:befund.png)`, see [`ATTACHMENT_SCHEME`] — and which stands alone
    /// in its own top-level paragraph is a [`crate::BlockKind::Attachment`] and no loss at
    /// all. What is still reported is a picture from somewhere this wiki does not store,
    /// and a reference standing where a placement cannot go.
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
            Unsupported::Image => {
                "alt text kept, source dropped — a file on this page is placed instead: \
                 `![Beschreibung](anhang:datei.png)` on a line of its own"
            }
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

/// An attachment reference between its `Start(Image)` and `End(Image)`.
struct Placing {
    /// What this image really is: a file on this page, or a live view of another page.
    ///
    /// One field rather than two `Option`s on the builder, because the two are mutually
    /// exclusive by construction — an image destination states one scheme — and two options
    /// would make "both open at once" a state somebody has to reason about.
    what: Placed,
    /// The alt text as it arrives. Plain: a description is what a screen reader is handed
    /// and what a card is labelled with, and neither has anywhere to put emphasis. For an
    /// embed it is the author's own label, and the same argument applies twice over: it is
    /// the only thing a reader who may not read the target is ever shown.
    alt: String,
}

/// The two things an image destination can name that are not a picture.
enum Placed {
    /// A file on this page ([`ATTACHMENT_SCHEME`]).
    File { filename: String },
    /// Another page, or one section of it ([`TRANSCLUSION_SCHEME`]).
    Page {
        doc: Option<String>,
        path: Option<String>,
        heading: Option<String>,
    },
}

struct Builder {
    stack: Vec<Frame>,
    /// Fenced and indented code arrives as one `Text` event per line. They are joined here
    /// rather than becoming one text leaf each, because separate leaves lose the newlines
    /// and a code block without its line breaks is not the same code block.
    code: Option<String>,
    /// The attachment reference currently open, and the description accumulating inside it.
    ///
    /// An image's alt text arrives as ordinary inline events between `Start(Image)` and
    /// `End(Image)`, so this diverts them the way [`Builder::code`] diverts a fence's lines
    /// — otherwise the description would land in the paragraph as prose, which is exactly
    /// what happens to an image that names no attachment and is exactly what must not
    /// happen to one that does.
    placing: Option<Placing>,
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
            placing: None,
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
        let block = self.settle_placements(frame.block);
        self.top().content.push(block);
    }

    /// Decide what the attachment references inside a closing paragraph actually are.
    ///
    /// A **placement** is an image reference standing alone in its own paragraph, and it
    /// becomes a block in that paragraph's place. A reference that shares its paragraph —
    /// `![x](anhang:a.png) und dann`, or two of them on adjacent lines, which a soft break
    /// puts a space between — is not a placement, and degrades to exactly what an ordinary
    /// image degrades to: its description as text, counted as an [`Unsupported::Image`].
    ///
    /// Only the "alone in its paragraph" half is decided here. The **top level** half is
    /// [`Self::placement_is_possible`]'s, asked before the description is diverted, and it is
    /// asked in exactly one place on purpose: two copies of one rule stop agreeing the day
    /// either is edited, and this one decides whether a page can ever be exported again. So
    /// nothing here re-checks the depth — an [`crate::BlockKind::Attachment`] can only exist
    /// in a top-level paragraph, because that is the only place one is ever built.
    ///
    /// **Alone in its paragraph** matters because markdown has no other way to say "this is a
    /// block": an image is inline, so a paragraph is what separates `![x](anhang:a.png)` from
    /// `siehe ![x](anhang:a.png) hier`. A reference that shared its paragraph would have to be
    /// an inline node, which `Block` has no room for and the exporter's mark machinery has no
    /// way to write.
    ///
    /// The degraded form is merged into its neighbours by the same rule
    /// [`Self::marked_text`] follows, so it is not merely *similar* to what an ordinary image
    /// produces — it is the same tree, leaf for leaf. It has to be: the exporter re-imports
    /// its own output and compares, so a paragraph that came back split into three leaves
    /// instead of one would refuse the page.
    fn settle_placements(&mut self, mut block: Block) -> Block {
        if block.kind != BlockKind::Paragraph || !block.content.iter().any(is_placed) {
            return block;
        }
        if block.content.len() == 1 {
            return block.content.pop().expect("length checked above");
        }

        let mut merged: Vec<Block> = Vec::with_capacity(block.content.len());
        for child in block.content.drain(..) {
            let child = if is_placed(&child) {
                self.note(Unsupported::Image);
                let mut leaf = self::block(BlockKind::Text);
                // The author's own words either way — a picture's description or an embed's
                // label — which is exactly what an ordinary image degrades to, leaf for leaf.
                let key = if child.kind == BlockKind::Transclusion {
                    "label"
                } else {
                    "alt"
                };
                leaf.text = Some(
                    child
                        .attrs
                        .get(key)
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                );
                leaf
            } else {
                child
            };
            match merged.last_mut() {
                Some(prev)
                    if prev.kind == BlockKind::Text
                        && child.kind == BlockKind::Text
                        && prev.marks == child.marks =>
                {
                    prev.text
                        .get_or_insert_with(String::new)
                        .push_str(child.text.as_deref().unwrap_or_default());
                }
                _ => merged.push(child),
            }
        }
        block.content = merged;
        block
    }

    fn close_implicit(&mut self) {
        while self.stack.last().is_some_and(|f| f.implicit) {
            self.pop();
        }
    }

    /// Whether an attachment reference starting here could be a placement at all.
    ///
    /// A placement is a top-level block written as an image alone in its own paragraph, so
    /// the stack has to be exactly the document and that paragraph, and nothing may have
    /// been written into the paragraph yet. Asked *before* the description is diverted, so
    /// a reference in a position that can never hold a placement — inside a sentence, a
    /// list item, a blockquote, a table cell — takes the ordinary image path untouched,
    /// keeping whatever emphasis its description carried.
    ///
    /// It cannot see what comes *after*, so `![x](anhang:a.png) und dann` still starts a
    /// placement that [`Self::settle_placements`] then degrades. That is the one shape
    /// where a description loses its emphasis, and it is reported as an image loss.
    fn placement_is_possible(&mut self) -> bool {
        self.stack.len() == 2 && {
            let top = self.top();
            top.kind == BlockKind::Paragraph && top.content.is_empty()
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
        // An image's description is plain text — CommonMark renders the inline content of
        // `![…]` as its plain string — so the marks are dropped here rather than carried.
        // That is the format's own rule and not a loss this converter invents; the only
        // place it can be *observed* as one is a reference that started a paragraph and had
        // something follow it, which degrades (see `settle_placements`) and would then have
        // been emphasised text before. Reported as an image either way.
        if let Some(placing) = self.placing.as_mut() {
            placing.alt.push_str(s);
            return;
        }
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

    /// Move a `{#<uuid>}` suffix off the open heading's last leaf and into its `id`.
    ///
    /// The **last unmarked text leaf**, and only that one: the writer appends the suffix
    /// after everything the heading says, so a space separates it from whatever came before
    /// and CommonMark gives it back as a leaf of its own whenever the heading ends in a mark.
    /// A leaf that carries marks is therefore never the suffix, and a heading whose own words
    /// end in braces inside a `**…**` keeps them.
    ///
    /// The leaf is dropped entirely when nothing is left of it, which is what makes a heading
    /// with no words and an id (`## {#…}`) come back as the empty heading it was rather than
    /// as one holding an empty text node.
    fn take_heading_anchor(&mut self) {
        let heading = self.top();
        if heading.kind != BlockKind::Heading {
            return;
        }
        let Some(last) = heading.content.last_mut() else {
            return;
        };
        if last.kind != BlockKind::Text || !last.marks.is_empty() {
            return;
        }
        let Some((head, id)) = last
            .text
            .as_deref()
            .and_then(heading_anchor_id)
            .map(|(head, id)| (head.to_string(), id.to_string()))
        else {
            return;
        };
        if head.is_empty() {
            heading.content.pop();
        } else {
            last.text = Some(head);
        }
        heading
            .attrs
            .insert("id".into(), serde_json::Value::from(id));
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
            // An image is a file placed in the prose (D-15) when its destination names one
            // and it can stand where a placement may stand; anything else is a picture from
            // somewhere this wiki does not store, and keeps the behaviour it always had.
            //
            // A `einbettung:` destination is the same shape a third time and a live view of
            // another page (D-27): an atom at the top level of the document, written as an
            // image because an image is markdown's only way to say "this stands alone and
            // its text is what to show when the thing itself cannot be shown".
            Tag::Image {
                dest_url, title, ..
            } => match placed_by(&dest_url, &title) {
                Some(what) if self.placement_is_possible() => {
                    self.placing = Some(Placing {
                        what,
                        alt: String::new(),
                    })
                }
                _ => self.note(Unsupported::Image),
            },
            // A `dok:` destination names a document by id and becomes an internal
            // reference (D-5); everything else becomes an href, including a destination
            // that merely LOOKS internal. This crate has no store, so `[text](/darm/labor)`
            // becomes an external href exactly like `[text](https://example.org)` —
            // resolving a path to an id is `gw_store`'s job on publish and must not be
            // guessed here. A `dok:` that is not shaped like an id is an href too; see
            // [`document_reference`] for why that degradation is load-bearing rather than
            // lenient.
            Tag::Link {
                dest_url, title, ..
            } => self.active.push(match document_reference(&dest_url) {
                // The title slot carries the target's path as it stood when the file was
                // written — a FALLBACK for a database that does not know this id, never a
                // second address. `gw_store` settles it; see [`document_fallback`].
                Some(id) => Mark::link_to_doc_at(id, document_fallback_path(&title)),
                // A title on any other link is still thrown away, as it always has been:
                // nothing in this system stores one and `BlockView` renders none.
                None => Mark::link_to_url(&dest_url),
            }),
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
            // A heading may end with the stable id a transclusion anchors to — see
            // `heading_anchor`. It is taken off here, before the frame closes, so nothing
            // downstream ever sees it as part of the heading's words: it is not in
            // `plain_text`, not in the search index, not in the slug the reader's fragment
            // is built from, and not in the seeder's exact comparison of a body heading
            // against the page title.
            TagEnd::Heading(_) => {
                self.take_heading_anchor();
                self.close();
            }
            TagEnd::Paragraph
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
            // An image that named an attachment becomes a block here; one that did not
            // opened no frame and pushed no mark, so it closes neither.
            TagEnd::Image => {
                if let Some(placing) = self.placing.take() {
                    let mut placement = match placing.what {
                        Placed::File { filename } => {
                            let mut b = block(BlockKind::Attachment);
                            b.attrs
                                .insert("filename".into(), serde_json::Value::from(filename));
                            b
                        }
                        // An embed's own two-or-three attributes. `doc` and `path` are never
                        // both written — `transclusion_reference` guarantees exactly one —
                        // and `heading` is absent for a whole page rather than null, because
                        // an absent key and a null one must be the same document and the
                        // editor's schema drops a null node attribute on the way to the CRDT.
                        Placed::Page { doc, path, heading } => {
                            let mut b = block(BlockKind::Transclusion);
                            if let Some(doc) = doc {
                                b.attrs.insert("doc".into(), serde_json::Value::from(doc));
                            }
                            if let Some(path) = path {
                                b.attrs.insert("path".into(), serde_json::Value::from(path));
                            }
                            if let Some(heading) = heading {
                                b.attrs
                                    .insert("heading".into(), serde_json::Value::from(heading));
                            }
                            b
                        }
                    };
                    // Written even when it is empty, for the reason `checked` is: an empty
                    // description and no description are the same thing to a reader and two
                    // different documents to `render_file`'s comparison — and the editor's
                    // schema fills a missing one in with `''`, so leaving it out here would
                    // make every placement the editor touched differ from the imported one.
                    // `alt` for a placement, `label` for an embed: two names because they
                    // are two different things a reader is shown — a description of a picture
                    // and the author's own words for what they quoted — and one key for both
                    // would make the editor's two schemas share an attribute they each mean
                    // something else by.
                    let key = if placement.kind == BlockKind::Transclusion {
                        "label"
                    } else {
                        "alt"
                    };
                    placement
                        .attrs
                        .insert(key.into(), serde_json::Value::from(placing.alt));
                    self.top().content.push(placement);
                }
            }
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
    use crate::markdown::{
        attachment_destination, attachment_reference, convert, document_destination,
        document_fallback, document_fallback_path, document_reference, heading_anchor,
        heading_anchor_id, markdown_to_blocks, transclusion_destination, Unsupported,
    };

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

    // --- inline attachments (D-15) -------------------------------------------------------

    /// The block an attachment placement produces, or `None` if the doc holds no placement
    /// at that position.
    fn placement(doc: &Block, at: usize) -> Option<&Block> {
        doc.content
            .get(at)
            .filter(|b| b.kind == BlockKind::Attachment)
    }

    #[test]
    fn an_image_naming_an_attachment_becomes_a_block_of_its_own() {
        let doc = markdown_to_blocks("![Befund vom März](anhang:befund.png)\n");
        let block = placement(&doc, 0).expect("a lone attachment reference is a placement");
        assert_eq!(
            block.attrs.get("filename").and_then(|v| v.as_str()),
            Some("befund.png")
        );
        assert_eq!(
            block.attrs.get("alt").and_then(|v| v.as_str()),
            Some("Befund vom März")
        );
        // A reference, not a possession: it is a top-level block, not text in a paragraph.
        assert_eq!(doc.content.len(), 1);
        assert!(block.content.is_empty(), "a placement holds no content");
    }

    #[test]
    fn a_placement_states_an_empty_description_rather_than_leaving_it_out() {
        // The same rule `checked` follows: an attribute that vanishes at its default makes
        // two different documents into one, and here it would also make the editor's
        // schema (which fills `alt` in with `''`) disagree with this converter's output —
        // which `gw_api::export::render_file` turns into a page that can never be exported.
        let doc = markdown_to_blocks("![](anhang:befund.png)\n");
        let block = placement(&doc, 0).expect("a description is not what makes it a placement");
        assert_eq!(block.attrs.get("alt").and_then(|v| v.as_str()), Some(""));
        assert_eq!(
            block.attrs.keys().collect::<Vec<_>>(),
            vec!["alt", "filename"],
            "a placement grew an attribute the editor's schema does not declare"
        );
    }

    #[test]
    fn a_placement_is_no_loss_and_reports_none() {
        assert!(keys("![a](anhang:befund.png)\n").is_empty());
    }

    #[test]
    fn an_ordinary_image_is_still_dropped_to_its_alt_text_and_reported() {
        // Unchanged, deliberately. A destination that does not say `anhang:` is a picture
        // from somewhere else, and this wiki stores no such thing — so guessing that
        // `![x](bild.png)` meant an attachment would invent a reference the author never
        // wrote, on every markdown file anybody has ever imported.
        let doc = markdown_to_blocks("![Ein Diagramm](/media/a.png)\n");
        assert_eq!(doc.content[0].kind, BlockKind::Paragraph);
        assert_eq!(doc.plain_text(), "Ein Diagramm");
        assert!(keys("![Ein Diagramm](/media/a.png)\n").contains(&"image"));
    }

    #[test]
    fn a_reference_inside_a_sentence_degrades_to_exactly_what_an_image_degrades_to() {
        // A placement is a paragraph of its own. Anywhere else the reference is not a
        // placement at all and falls back to the behaviour every other image has — which
        // has to be the SAME tree, leaf for leaf, or a page holding one would export as
        // something that re-imports differently.
        let placed = markdown_to_blocks("Text ![a](anhang:x.png) mehr\n");
        let plain = markdown_to_blocks("Text ![a](/media/x.png) mehr\n");
        assert_eq!(json(&placed), json(&plain));
        assert_eq!(placed.plain_text(), "Text a mehr");
        assert!(keys("Text ![a](anhang:x.png) mehr\n").contains(&"image"));
    }

    #[test]
    fn two_references_on_adjacent_lines_are_one_paragraph_and_therefore_neither_is_placed() {
        // A soft break puts a space between them, so the paragraph holds three children and
        // not one. Both degrade, and to exactly what two ordinary images degrade to.
        let placed = markdown_to_blocks("![a](anhang:x.png)\n![b](anhang:y.png)\n");
        let plain = markdown_to_blocks("![a](/x.png)\n![b](/y.png)\n");
        assert_eq!(json(&placed), json(&plain));
        assert_eq!(placed.plain_text(), "a b");
        // And BOTH are reported, exactly as two ordinary images are. A reference that
        // degrades has lost its destination, and a loss nobody is told about is the one
        // outcome this converter's header says cannot be detected later.
        //
        // The COUNT, not just the key. One of these two takes the ordinary image path at
        // `Start` (the second one, which is not the first thing in its paragraph) and the
        // other is reported by `settle_placements` when it degrades — so `keys` alone says
        // "image" whichever of the two reports it, and would pass with one of them silent.
        assert_eq!(
            convert("![a](anhang:x.png)\n![b](anhang:y.png)\n").notes,
            convert("![a](/x.png)\n![b](/y.png)\n").notes
        );
        assert_eq!(
            convert("![a](anhang:x.png)\n![b](anhang:y.png)\n")
                .notes
                .iter()
                .find(|n| n.construct == Unsupported::Image)
                .map(|n| n.count),
            Some(2),
            "a reference that degraded was not counted as the image it fell back to"
        );
    }

    #[test]
    fn a_reference_in_a_sentence_keeps_the_emphasis_in_its_description() {
        // A reference standing where a placement can never go never becomes one, so its
        // description flows into the paragraph as ordinary inline events and keeps whatever
        // marks it carried — exactly as an ordinary image's does. That is what
        // `placement_is_possible` buys by asking BEFORE the description is diverted rather
        // than unwinding a placement afterwards: CommonMark renders an image's description as
        // plain text, so a placement that had to be degraded can only give back plain text.
        let placed = markdown_to_blocks("Text ![**fett**](anhang:x.png) mehr\n");
        let plain = markdown_to_blocks("Text ![**fett**](/media/x.png) mehr\n");
        assert_eq!(json(&placed), json(&plain));
        let marked: Vec<&Block> = collect_text_leaves(&placed)
            .into_iter()
            .filter(|leaf| !leaf.marks.is_empty())
            .collect();
        assert_eq!(
            marked.len(),
            1,
            "the emphasis in the description was flattened"
        );
        assert_eq!(marked[0].text.as_deref(), Some("fett"));
    }

    #[test]
    fn a_degraded_reference_merges_into_its_neighbours_only_where_the_marks_agree() {
        // Degrading has to produce the SAME tree an ordinary image produces, leaf for leaf,
        // or the exporter's round-trip comparison refuses the page — and "the same tree"
        // includes where the leaf boundaries are. `marked_text` merges two neighbouring
        // leaves only when their marks match, so this must too: merging across a mark
        // boundary would swallow the emphasis of the words beside the picture.
        let placed = markdown_to_blocks("![a](anhang:x.png) **fett**\n");
        let plain = markdown_to_blocks("![a](/media/x.png) **fett**\n");
        assert_eq!(json(&placed), json(&plain));
        let leaves = collect_text_leaves(&placed);
        assert_eq!(leaves.len(), 2, "the two runs were fused into one");
        assert!(leaves[0].marks.is_empty());
        assert_eq!(leaves[1].text.as_deref(), Some("fett"));
        assert!(!leaves[1].marks.is_empty(), "the bold run lost its mark");
    }

    // --- a live view of another page (D-27) ------------------------------------------------

    const EMBED_ID: &str = "0199c0de-0000-7000-8000-000000000001";
    const EMBED_ANCHOR: &str = "0199c0de-0000-7000-8000-00000000000a";

    #[test]
    fn an_image_naming_another_page_becomes_an_embed_of_it() {
        let doc = markdown_to_blocks(&format!("![Laborwerte](einbettung:{EMBED_ID})\n"));
        assert_eq!(doc.content.len(), 1);
        let embed = &doc.content[0];
        assert_eq!(embed.kind, BlockKind::Transclusion);
        assert_eq!(embed.attrs.get("doc").unwrap(), EMBED_ID);
        assert_eq!(embed.attrs.get("label").unwrap(), "Laborwerte");
        // No anchor means the whole page, and the key is ABSENT rather than null: the
        // editor's schema drops a null node attribute on the way to the CRDT, so a null here
        // would differ from what comes back and refuse the page from every export.
        assert!(!embed.attrs.contains_key("heading"));
        assert!(!embed.attrs.contains_key("path"));
        // The wire name IS the CRDT element tag and the TipTap node name.
        assert!(serde_json::to_string(&doc)
            .unwrap()
            .contains(r#""kind":"embed""#));
    }

    #[test]
    fn an_embed_may_name_one_section_of_the_page() {
        let doc = markdown_to_blocks(&format!(
            "![Dosierung](einbettung:{EMBED_ID}#{EMBED_ANCHOR})\n"
        ));
        let embed = &doc.content[0];
        assert_eq!(embed.attrs.get("doc").unwrap(), EMBED_ID);
        assert_eq!(embed.attrs.get("heading").unwrap(), EMBED_ANCHOR);
    }

    #[test]
    fn an_embed_may_name_its_target_by_address_for_a_database_that_never_heard_of_the_id() {
        // What a restored backup produces, and the reason the path form exists at all: a
        // seed into a fresh database mints every id anew.
        let doc = markdown_to_blocks(&format!(
            "![Dosierung](einbettung:/darm/labor#{EMBED_ANCHOR})\n"
        ));
        let embed = &doc.content[0];
        assert_eq!(embed.kind, BlockKind::Transclusion);
        assert_eq!(embed.attrs.get("path").unwrap(), "/darm/labor");
        assert_eq!(embed.attrs.get("heading").unwrap(), EMBED_ANCHOR);
        assert!(!embed.attrs.contains_key("doc"), "both addresses at once");
    }

    #[test]
    fn a_destination_that_is_not_shaped_like_a_target_stays_an_ordinary_image() {
        // The degradation is load-bearing, exactly as `dok:etwas` is: anybody with write
        // access could have typed one of these into an image destination at any point in the
        // past, and a reader that accepted them would make that page re-import as a different
        // tree — a page nobody edited becoming permanently unexportable.
        for dest in [
            "einbettung:etwas",
            "einbettung:",
            "einbettung:darm/labor",            // not rooted
            "einbettung:/darm/labor#abschnitt", // the anchor is not an id
            "einbettung:/darm/labor#",
        ] {
            let doc = markdown_to_blocks(&format!("![a]({dest})\n"));
            assert!(
                doc.content
                    .iter()
                    .all(|b| b.kind != BlockKind::Transclusion),
                "{dest:?} became an embed"
            );
            assert_eq!(doc.plain_text(), "a", "{dest:?} lost its description too");
        }
    }

    #[test]
    fn an_embed_below_the_top_level_degrades_to_its_label() {
        // Its own group in the editor's schema admits it in `doc` and nowhere else, so an
        // embed read back inside a list item or a table cell is a tree TipTap deletes on
        // open. The importer never writes one there, and the label survives as text.
        for md in [
            &format!("- ![a](einbettung:{EMBED_ID})\n"),
            &format!("> ![a](einbettung:{EMBED_ID})\n"),
            &format!("| Kopf |\n|---|\n| ![a](einbettung:{EMBED_ID}) |\n"),
            &format!("siehe ![a](einbettung:{EMBED_ID}) hier\n"),
        ] {
            let doc = markdown_to_blocks(md);
            let mut kinds = Vec::new();
            fn walk(b: &Block, out: &mut Vec<BlockKind>) {
                out.push(b.kind);
                for c in &b.content {
                    walk(c, out);
                }
            }
            walk(&doc, &mut kinds);
            assert!(
                !kinds.contains(&BlockKind::Transclusion),
                "{md:?} embedded somewhere the editor cannot hold one"
            );
            assert!(
                doc.plain_text().contains('a'),
                "{md:?} lost the label as well: {:?}",
                doc.plain_text()
            );
        }
    }

    #[test]
    fn the_two_halves_of_the_embed_syntax_are_one_rule() {
        // The writer asks the reader rather than restating its rules, so this pins the pair
        // rather than either side: `Renderer::wrap` and `Renderer::embed` escape nothing, and
        // every one of these values reached `documents.body` over the collaboration socket.
        assert_eq!(
            transclusion_destination(Some(EMBED_ID), None, None).as_deref(),
            Some(&*format!("einbettung:{EMBED_ID}"))
        );
        assert_eq!(
            transclusion_destination(Some(EMBED_ID), None, Some(EMBED_ANCHOR)).as_deref(),
            Some(&*format!("einbettung:{EMBED_ID}#{EMBED_ANCHOR}"))
        );
        assert_eq!(
            transclusion_destination(None, Some("/darm/labor"), None).as_deref(),
            Some("einbettung:/darm/labor")
        );
        // Neither names nothing; both can disagree with itself the day the target moves.
        assert_eq!(transclusion_destination(None, None, None), None);
        assert_eq!(
            transclusion_destination(Some(EMBED_ID), Some("/darm/labor"), None),
            None
        );
        // And nothing that would not read back unchanged is ever written.
        assert_eq!(transclusion_destination(Some("x) [a](b"), None, None), None);
        assert_eq!(
            transclusion_destination(Some(EMBED_ID), None, Some("kopf")),
            None
        );
        assert_eq!(
            transclusion_destination(None, Some("/pfad mit leerzeichen"), None),
            None
        );

        // And the case the `?` above does NOT cover, which is the one the final comparison
        // exists for: a destination that reads back perfectly and reads back as something
        // ELSE. Both attributes are arbitrary strings that reached `documents.body` over the
        // collaboration socket, so a `doc` holding a path is a thing a page can hold — and
        // writing it would export a block that re-imports as an ADDRESS-named embed, a
        // different document, which `render_file` then refuses for ever.
        assert_eq!(
            transclusion_destination(Some("/darm/labor"), None, None),
            None
        );
        assert_eq!(transclusion_destination(None, Some(EMBED_ID), None), None);
    }

    // --- a heading's stable id, which is what a section embed anchors to --------------------

    #[test]
    fn a_heading_carries_a_stable_id_written_after_its_words() {
        let doc = markdown_to_blocks(&format!("## Dosierung {{#{EMBED_ANCHOR}}}\n"));
        let heading = &doc.content[0];
        assert_eq!(heading.kind, BlockKind::Heading);
        assert_eq!(heading.attrs.get("id").unwrap(), EMBED_ANCHOR);
        // The id is NOT part of the heading's words: not in the search index, not in the
        // slug the reader's fragment is built from, not in the seeder's title comparison.
        assert_eq!(heading.plain_text(), "Dosierung");
        assert_eq!(doc.headings()[0].id, "dosierung");
        assert_eq!(doc.headings()[0].anchor.as_deref(), Some(EMBED_ANCHOR));
    }

    #[test]
    fn a_heading_with_no_words_and_an_id_comes_back_as_the_empty_heading_it_was() {
        let doc = markdown_to_blocks(&format!("## {{#{EMBED_ANCHOR}}}\n"));
        let heading = &doc.content[0];
        assert!(
            heading.content.is_empty(),
            "an empty text leaf was left behind"
        );
        assert_eq!(heading.attrs.get("id").unwrap(), EMBED_ANCHOR);
    }

    #[test]
    fn only_an_id_shaped_suffix_is_an_anchor_and_the_last_one_wins() {
        // Narrow on purpose. `Options::ENABLE_HEADING_ATTRIBUTES` would read ANY `{…}`, which
        // is a global re-parse of every heading in the corpus and would refuse from every
        // export a page nobody had edited.
        let doc = markdown_to_blocks("## Dosis {#2}\n");
        assert!(!doc.content[0].attrs.contains_key("id"));
        assert_eq!(doc.content[0].plain_text(), "Dosis {#2}");

        // …and a heading whose own words end in one keeps them, because the real id is always
        // written last. This is what makes the pair total rather than usually right.
        let doc = markdown_to_blocks(&format!("## Dosis {{#{EMBED_ID}}} {{#{EMBED_ANCHOR}}}\n"));
        assert_eq!(doc.content[0].attrs.get("id").unwrap(), EMBED_ANCHOR);
        assert_eq!(
            doc.content[0].plain_text(),
            format!("Dosis {{#{EMBED_ID}}}")
        );
    }

    #[test]
    fn a_heading_anchor_needs_the_space_its_writer_puts_there() {
        assert_eq!(
            heading_anchor(EMBED_ANCHOR).as_deref(),
            Some(&*format!(" {{#{EMBED_ANCHOR}}}"))
        );
        assert_eq!(heading_anchor("kopf"), None);
        assert_eq!(
            heading_anchor_id(&format!("Dosierung {{#{EMBED_ANCHOR}}}")),
            Some(("Dosierung", EMBED_ANCHOR))
        );
        // Without the separator the file would not re-import as itself.
        assert_eq!(
            heading_anchor_id(&format!("Dosierung{{#{EMBED_ANCHOR}}}")),
            None
        );
    }

    #[test]
    fn a_reference_below_the_top_level_degrades_rather_than_placing() {
        // The editor's schema admits `attachment` in `doc` and nowhere else, so a placement
        // read back inside a list item, a table cell or a blockquote would be a tree the
        // editor deletes on open. Both halves of that rule live here and in
        // `extensions.ts`, and this is the half that keeps the importer from writing one.
        for md in [
            "- ![a](anhang:x.png)\n",
            "> ![a](anhang:x.png)\n",
            "| Kopf |\n|---|\n| ![a](anhang:x.png) |\n",
            "1. ![a](anhang:x.png)\n",
        ] {
            let doc = markdown_to_blocks(md);
            let mut kinds = Vec::new();
            fn walk(b: &Block, out: &mut Vec<BlockKind>) {
                out.push(b.kind);
                for c in &b.content {
                    walk(c, out);
                }
            }
            walk(&doc, &mut kinds);
            assert!(
                !kinds.contains(&BlockKind::Attachment),
                "{md:?} placed an attachment somewhere the editor cannot hold one"
            );
            assert!(
                doc.plain_text().ends_with('a'),
                "{md:?} lost the description as well: {:?}",
                doc.plain_text()
            );
        }
    }

    #[test]
    fn a_placement_serialises_under_the_editors_own_name() {
        // The wire name IS the CRDT element tag and the TipTap node name, so this is the
        // string `extensions.ts` has to register. Asserted rather than assumed, the same
        // way `taskList` is.
        let doc = markdown_to_blocks("![a](anhang:x.png)\n");
        assert_eq!(
            json(&doc)["content"][0]["type"],
            serde_json::Value::Null,
            "a Block serialises `kind`, not ProseMirror's `type`"
        );
        assert_eq!(json(&doc)["content"][0]["kind"], "attachment");
    }

    #[test]
    fn every_name_a_page_can_give_a_file_survives_being_written_and_read_back() {
        // The two halves of one agreement, in the crate that owns both, for the reason
        // `MARK_ORDER` lives here rather than in the exporter: a destination this writes and
        // a destination this reads back are the same statement, and two copies of it in two
        // crates stop agreeing the day one is edited. What that costs is an export that
        // refuses every page holding a picture.
        //
        // The names are what `gw_store::attachments::canonical_filename` actually admits —
        // it refuses only `/`, `\`, `"` and control characters — so spaces, brackets,
        // parentheses and umlauts all have to go through a markdown destination and come
        // back unchanged.
        for name in [
            "befund.png",
            "Befund 2024.pdf",
            "a(b).png",
            "a)b.png",
            "a<b>c.svg",
            "Röntgen Größe.png",
            "a b (2).csv",
            "100%.png",
            "a'b.png",
            "#tag.png",
        ] {
            let dest = attachment_destination(name)
                .unwrap_or_else(|| panic!("`{name}` is a name a page can really give a file"));
            let md = format!("![x]({dest})\n");
            let doc = markdown_to_blocks(&md);
            let block = placement(&doc, 0)
                .unwrap_or_else(|| panic!("{md:?} did not come back as a placement"));
            assert_eq!(
                block.attrs.get("filename").and_then(|v| v.as_str()),
                Some(name),
                "{md:?} came back naming a different file"
            );
        }
    }

    /// The `doc` mark on the single text leaf `md` produces, or `None`.
    fn reference_in(md: &str) -> Option<String> {
        let doc = markdown_to_blocks(md);
        collect_text_leaves(&doc)
            .iter()
            .flat_map(|leaf| leaf.marks.clone())
            .find(|m| m.kind == MarkKind::Link)
            .and_then(|m| {
                m.attrs
                    .get("doc")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
    }

    /// The `href` on the single link mark `md` produces, or `None`.
    fn href_in(md: &str) -> Option<String> {
        let doc = markdown_to_blocks(md);
        collect_text_leaves(&doc)
            .iter()
            .flat_map(|leaf| leaf.marks.clone())
            .find(|m| m.kind == MarkKind::Link)
            .and_then(|m| {
                m.attrs
                    .get("href")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            })
    }

    #[test]
    fn every_id_a_document_can_have_survives_being_written_and_read_back() {
        // `every_name_a_page_can_give_a_file_survives_being_written_and_read_back`'s twin,
        // and it exists for that test's reason: the writer and the reader are one agreement
        // and two copies of it stop agreeing the day one is edited. What that costs here is
        // an export that refuses every page holding a reference.
        for id in [
            // What `Uuid::now_v7().to_string()` writes, which is what `documents.id` holds.
            "01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f",
            "00000000-0000-0000-0000-000000000000",
            "ffffffff-ffff-ffff-ffff-ffffffffffff",
            // Upper case is not this system's spelling and is still a uuid; the string is
            // handed back as it arrived, so it round-trips either way.
            "01936C9E-7F3A-7C21-9A7E-2F0B1D4C5E6F",
        ] {
            let dest = document_destination(id)
                .unwrap_or_else(|| panic!("`{id}` is a shape `documents.id` really holds"));
            // No escaping, and none needed: a uuid holds nothing markdown reads as markup.
            assert_eq!(dest, format!("dok:{id}"));
            assert_eq!(
                reference_in(&format!("Siehe [Titel]({dest}).\n")).as_deref(),
                Some(id),
                "{dest} did not come back as a reference to the same document"
            );
        }
    }

    #[test]
    fn a_destination_that_names_no_document_is_an_ordinary_link() {
        // The degradation D-21g is about, and it is the whole reason the reader checks the
        // SHAPE rather than accepting anything after the colon. `dok:etwas` is a string
        // anybody could have typed into the link control before this feature existed: it is
        // stored as an href, exported verbatim, and must keep re-importing as the same href
        // — otherwise a page nobody edited becomes unexportable for ever.
        for dest in [
            "dok:",
            "dok:etwas",
            "dok:01936c9e7f3a7c219a7e2f0b1d4c5e6f",
            "dok:01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6",
            "dok:01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f7",
            "dok:01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6g",
            "dok:01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f-0",
            "dok:x) [siehe](https://angreifer.example/",
            "DOK:01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f",
            // The scheme is stripped ONCE. `trim_start_matches` would strip it repeatedly
            // and read this as a reference to the uuid — the one destination for which
            // "does it say so" and "does it look like it" give different answers, and
            // therefore the one that pins the difference. `scripts/mutate.sh` makes exactly
            // that substitution.
            "dok:dok:01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f",
            "/darm/labor",
            "https://example.org",
        ] {
            assert_eq!(
                document_reference(dest),
                None,
                "{dest} must not read as a document reference"
            );
        }
        assert_eq!(
            reference_in("[T](dok:etwas)\n"),
            None,
            "a `dok:` that is not an id must not become a reference"
        );
        assert_eq!(
            href_in("[T](dok:etwas)\n").as_deref(),
            Some("dok:etwas"),
            "it degrades to the ordinary link it has always been"
        );
    }

    #[test]
    fn a_destination_a_writer_cannot_be_read_back_is_refused_rather_than_written() {
        // D-21f, and the model is `attachment_destination`. `gw_api::export`'s renderer
        // interpolates a destination with no escaping at all, and a `doc` value is an
        // arbitrary attacker-controlled string: nothing between the collaboration socket
        // and `documents.body` validates it. A writer that emitted what it was handed would
        // put attacker text into the owner's backup file.
        for id in [
            "",
            "etwas",
            "x) [siehe](https://angreifer.example/",
            "01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f ",
            " 01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f",
        ] {
            assert_eq!(
                document_destination(id),
                None,
                "`{id}` is not an id and must not be written as a destination"
            );
        }
    }

    #[test]
    fn a_reference_carries_the_path_its_target_had_when_the_file_was_written() {
        // The fallback, and the only thing that stops a restored backup losing every internal
        // connection: a seed mints new ids, so no `dok:` in a restored corpus can ever match.
        // `gw_store` decides which of the two wins; this is only the carrying.
        let id = "01936c9e-7f3a-7c21-9a7e-2f0b1d4c5e6f";
        let md = format!("Siehe [Blutbild](dok:{id} \"/darm/blutbild\").\n");
        let doc = markdown_to_blocks(&md);
        let mark = collect_text_leaves(&doc)
            .iter()
            .flat_map(|leaf| leaf.marks.clone())
            .find(|m| m.kind == MarkKind::Link)
            .expect("the reference went missing entirely");
        assert_eq!(mark.target_doc(), Some(id));
        assert_eq!(mark.fallback_path(), Some("/darm/blutbild"));
    }

    #[test]
    fn a_link_title_that_is_not_a_wiki_path_carries_nothing() {
        // The writer's rule, asked of the reader so the two cannot drift. `documents.path` is
        // built from slugs and is always plain, rooted ASCII — so anything else is a
        // hand-written or hostile value, and `Renderer::wrap` escapes NOTHING.
        for title in [
            "",
            "kein Pfad",
            "darm/labor",
            "/darm/labor/",
            "/darm//labor",
            "/",
            "/a b",
            "/a\"b",
            "/a)b",
            "/ümlaut",
        ] {
            assert_eq!(
                document_fallback_path(title),
                None,
                "`{title}` must not be read as a wiki path"
            );
            assert_eq!(
                document_fallback(title),
                None,
                "`{title}` must not be written as one either"
            );
        }
        assert_eq!(document_fallback_path("/darm/labor"), Some("/darm/labor"));
        assert_eq!(
            document_fallback("/darm/labor"),
            Some("\"/darm/labor\"".into())
        );
    }

    #[test]
    fn a_link_title_on_anything_but_a_reference_is_still_thrown_away() {
        // Unchanged, and stated because `comparable()`'s allow-list depends on it: nothing in
        // this system stores a link title and `BlockView` renders none, so an ordinary link
        // with one must keep coming back without it.
        let doc = markdown_to_blocks("[T](https://example.org \"ein Titel\")\n");
        let mark = collect_text_leaves(&doc)
            .iter()
            .flat_map(|leaf| leaf.marks.clone())
            .find(|m| m.kind == MarkKind::Link)
            .unwrap();
        assert_eq!(
            mark.attrs.keys().collect::<Vec<_>>(),
            vec!["href"],
            "a link title reached the document: {:?}",
            mark.attrs
        );
    }

    #[test]
    fn a_path_is_still_an_href_and_never_a_reference() {
        // Restated as a test because it is the one combination that would break the backup
        // permanently: this crate has no store, so it cannot resolve a path to an id, and
        // an exporter that wrote the path instead of the scheme would produce a file whose
        // re-import differs from what is stored.
        assert_eq!(reference_in("[T](/darm/labor)\n"), None);
        assert_eq!(
            href_in("[T](/darm/labor)\n").as_deref(),
            Some("/darm/labor")
        );
    }

    #[test]
    fn a_destination_that_names_no_attachment_is_not_one() {
        for dest in [
            "bild.png",
            "/media/a.png",
            "https://example.org/a.png",
            "anhang:",
            "anhang:  ",
            "anhang:.",
            "anhang:..",
            "anhang:a/b.png",
            "anhang:a\\b.png",
            "anhang:a\"b.png",
        ] {
            assert_eq!(
                attachment_reference(dest),
                None,
                "`{dest}` was read as a reference to a file"
            );
        }
    }
}
