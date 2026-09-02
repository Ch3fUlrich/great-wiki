//! `Block` tree → markdown, and `great-wiki export --content <dir>`.
//!
//! The mirror image of [`crate::seed`], and held to the same two rules:
//!
//! * **Nothing is invented.** The file a document exports to is decided by the path the
//!   database already gave it, never by a title guessed back into a filename.
//! * **Nothing is quietly degraded.** Every document is re-imported *before it is written*
//!   and compared against what came out of the database. A document whose markdown does
//!   not come back as the same tree is **not written at all**, is named in the report, and
//!   fails the run — exactly as the seeder skips a file it cannot place.
//!
//! # What markdown can carry back, and what still cannot
//!
//! `Block` carries inline [`Mark`]s (bold, italic, strikethrough, inline code, and a link's
//! `href`), so this renderer writes them back out. A mark belongs to a *leaf* and markdown
//! writes it around a *span*, so the two do not line up on their own: the nesting order is
//! [`gw_core::MARK_ORDER`] — defined once, in the crate that also stamps it on import, so
//! that neither side can be edited into disagreeing with the other — and a mark covering
//! several leaves is opened once around the whole run rather than per leaf.
//!
//! A **file placed in the prose** (D-15) survives too, as
//! `![Beschreibung](anhang:datei.png)` on a line of its own. It is a reference and says
//! nothing about the file beyond its name — see [`Renderer::attachment`], and
//! [`gw_core::markdown`] for the syntax itself, which lives there so that the two directions
//! cannot drift apart.
//!
//! What a stored document still cannot hold is a picture from **somewhere else** (an
//! ordinary `![…](https://…)`, whose alt text is all the importer keeps), a link resolved to
//! another *document* rather than a URL (`Mark::attrs`' `doc` — nothing in this system writes
//! one yet; see `gw_store::links` for why an `href` naming a page in this wiki counts as an
//! internal link instead), and a horizontal rule — those are dropped on the way *in* (see
//! [`gw_core::markdown::Unsupported`]), long before anything reaches here, so an export
//! cannot contain them either. That is a true statement about the database, not a defect in
//! this converter, and it is the one thing an operator must not discover by accident:
//! exported markdown is **not** a replacement for hand-written source markdown.
//! [`ExportReport`] says so on every run, and [`FIDELITY_FILE`] leaves the same sentence in
//! the directory next to the files, where whoever opens it later will see it.

use anyhow::{Context, Result};
use gw_auth::{Action, Principal};
use gw_core::{markdown, slugify, split_frontmatter, Block, BlockKind, Mark, MarkKind, SeedMeta};
use gw_store::Store;
use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// Written at the root of an export directory.
///
/// Not a `.md` file, so [`crate::seed`]'s walk never tries to load it back — and so it
/// survives an `export` → `seed` round trip without becoming a page. It exists because a
/// warning printed to a terminal is gone the moment the window closes, and the person who
/// finds this directory in six months is not the person who ran the command.
pub const FIDELITY_FILE: &str = "EXPORT-README.txt";

/// A document that reached a file.
#[derive(Debug, Clone)]
pub struct Written {
    /// The document's path in the wiki, e.g. `/rundgang/tabellen`.
    pub path: String,
    /// The file, relative to the export directory.
    pub file: PathBuf,
}

/// A document that did **not** reach a file, and exactly why.
#[derive(Debug, Clone)]
pub struct Refused {
    pub path: String,
    /// Self-contained: names its own document, so a line lifted into a ticket still says
    /// which page it is about.
    pub reason: String,
}

/// A file already in the export directory that this run did not write.
///
/// Reported, never deleted. Removing a page from the wiki and removing its file are two
/// different decisions and only the operator gets to make the second one.
#[derive(Debug, Clone)]
pub struct Stale {
    pub file: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct ExportReport {
    pub written: Vec<Written>,
    pub refused: Vec<Refused>,
    pub stale: Vec<Stale>,
    /// Who the export ran as. Every page this principal may not read is simply absent,
    /// and no report can say how many those were without disclosing that they exist.
    pub principal: String,
}

impl ExportReport {
    /// Whether every document the principal can see was written faithfully. The caller
    /// turns this into an exit code.
    pub fn is_complete(&self) -> bool {
        self.refused.is_empty()
    }
}

/// The sentence an operator must not be able to miss. Printed by the CLI and written into
/// [`FIDELITY_FILE`], because a report nobody reads is not a warning.
pub const FIDELITY_WARNING: &str = "\
FIDELITY — read this before you overwrite anything.

great-wiki stores documents as a block tree. Inline formatting — bold, italic,
strikethrough, inline code and links — now exports and re-imports exactly, the same as
headings, paragraphs, lists and their nesting, blockquotes, code blocks with their
language, tables with their column alignment, and every character of the text itself.
A file placed in the prose comes back as `![Beschreibung](anhang:datei.png)`, which
names the file on its page and nothing else: the FILES THEMSELVES ARE NOT IN THIS
DIRECTORY, so an export is a copy of the wiki's words and not of its attachments.
What the tree still cannot hold is a picture from somewhere else, an internal link
resolved to another page rather than a URL, and a horizontal rule — those are dropped
when markdown is imported (the importer says so, page by page) and so they cannot come
back out. `export` verifies every other construct per document and refuses to write one
it cannot.

Therefore: this directory is a faithful copy of what the DATABASE holds. It is NOT a
faithful copy of hand-written markdown that was imported into it. Do not overwrite
your source files with these.";

impl fmt::Display for ExportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for w in &self.written {
            writeln!(f, "  exported  {:<40} {}", w.path, w.file.display())?;
        }
        for s in &self.stale {
            writeln!(
                f,
                "  stale     {} — no document exports here any more; delete it yourself if \
                 that is what you meant",
                s.file.display()
            )?;
        }
        for r in &self.refused {
            // `reason` already names the document — see `Refused::reason`.
            writeln!(f, "  REFUSED   {}", r.reason)?;
        }
        writeln!(
            f,
            "{} exported, {} refused, {} stale — as `{}`; pages this account cannot read \
             are not in this export",
            self.written.len(),
            self.refused.len(),
            self.stale.len(),
            self.principal
        )?;
        write!(f, "{FIDELITY_WARNING}")
    }
}

/// Write every document `principal` may read into `content_dir` as markdown.
///
/// This is the library seam: the CLI is a shell over it, and so is anything else — an HTTP
/// handler, the MCP server the owner has chosen — that has a [`Store`] and a
/// [`Principal`]. It takes the principal rather than deriving one, because reading the
/// tree is a permission-filtered operation (AGENTS.md rule 2) and there is no such thing
/// as "the whole tree" independent of who is asking.
///
/// Errors returned here are failures of the *run* (an unwritable directory). A document
/// that cannot be represented is a refusal inside the report, so one bad page never hides
/// the state of the other thirty-four.
pub async fn run(store: &Store, principal: &Principal, content_dir: &Path) -> Result<ExportReport> {
    std::fs::create_dir_all(content_dir)
        .with_context(|| format!("creating export directory {}", content_dir.display()))?;
    refuse_foreign_directory(content_dir)?;

    let mut report = ExportReport {
        principal: describe(principal),
        ..ExportReport::default()
    };
    let mut written: BTreeSet<PathBuf> = BTreeSet::new();

    for path in paths_in_order(store, principal).await? {
        // Re-read through the permission-checked accessor rather than trusting the tree:
        // `tree_for` proves readability of the *title*, and this is the body.
        let Some(doc) = store.document_for(principal, &path, Action::Read).await? else {
            continue;
        };
        let file = file_for(&doc.path);
        let body: Block = match serde_json::from_str(&doc.body) {
            Ok(body) => body,
            Err(e) => {
                report.refused.push(Refused {
                    path: path.clone(),
                    reason: format!("`{path}`: its stored body is not a block tree ({e})"),
                });
                continue;
            }
        };

        // Through the permission-checked accessor a second time, exactly as the body above
        // was: `store.document_topics_for` asks the same question about the same page, and
        // taking the topics from an unchecked lookup on the strength of the read that just
        // succeeded is the shortcut every aggregate view in this system refuses.
        let topics = store
            .document_topics_for(principal, &path)
            .await?
            .unwrap_or_default();
        let meta = FileMeta {
            title: doc.title.clone(),
            doc_type: doc.doc_type.clone(),
            visibility: doc.visibility.clone(),
            language: doc.language.clone(),
            sort_key: doc.sort_key,
            slug: doc.slug.clone(),
            tags: topics.into_iter().map(|t| t.display_path).collect(),
        };
        let rendered = match render_file(&meta, &body) {
            Ok(rendered) => rendered,
            Err(reason) => {
                report.refused.push(Refused {
                    path: path.clone(),
                    reason: format!("`{path}` was NOT written: {reason}"),
                });
                continue;
            }
        };

        let target = content_dir.join(&file);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&target, rendered)
            .with_context(|| format!("writing {}", target.display()))?;
        written.insert(file.clone());
        report.written.push(Written { path, file });
    }

    std::fs::write(
        content_dir.join(FIDELITY_FILE),
        fidelity_file_contents(&report),
    )
    .with_context(|| format!("writing {FIDELITY_FILE}"))?;

    for file in existing_markdown(content_dir)? {
        if !written.contains(&file) {
            report.stale.push(Stale { file });
        }
    }
    report.stale.sort_by(|a, b| a.file.cmp(&b.file));

    Ok(report)
}

/// A short, honest name for whoever the export ran as.
fn describe(principal: &Principal) -> String {
    if principal.is_authenticated() {
        principal.username.clone()
    } else {
        "anonymous (public pages only)".to_string()
    }
}

/// Refuse to write into a directory that holds markdown this tool did not put there.
///
/// The single most expensive mistake available here is exporting over the hand-written
/// corpus the wiki was seeded from: those files may hold pictures from elsewhere, raw HTML
/// or internal links the database never kept, and an export would replace them with a copy
/// that has none. A
/// directory carrying [`FIDELITY_FILE`] is a previous export and is fair game; anything else
/// is somebody's writing.
fn refuse_foreign_directory(dir: &Path) -> Result<()> {
    if dir.join(FIDELITY_FILE).exists() {
        return Ok(());
    }
    let existing = existing_markdown(dir)?;
    anyhow::ensure!(
        existing.is_empty(),
        "{} already contains {} markdown file(s) and no `{}`, so it is not a previous \
         export — refusing to overwrite it. An export holds no picture from elsewhere and \
         no raw HTML, because the database holds neither; writing it over hand-written \
         source markdown destroys them. Export to an empty directory instead.",
        dir.display(),
        existing.len(),
        FIDELITY_FILE
    );
    Ok(())
}

/// Every `.md` file under `dir`, relative to it. Empty when `dir` does not exist.
fn existing_markdown(dir: &Path) -> Result<BTreeSet<PathBuf>> {
    fn walk(root: &Path, rel: &Path, out: &mut BTreeSet<PathBuf>) -> Result<()> {
        let here = root.join(rel);
        let Ok(entries) = std::fs::read_dir(&here) else {
            return Ok(());
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let child = rel.join(name.as_ref());
            if entry.file_type()?.is_dir() {
                walk(root, &child, out)?;
            } else if child.extension().is_some_and(|e| e == "md") {
                out.insert(child);
            }
        }
        Ok(())
    }
    let mut out = BTreeSet::new();
    walk(dir, Path::new(""), &mut out)?;
    Ok(out)
}

/// Every readable document path, parents before children.
///
/// Depth order is not cosmetic: it is the order [`crate::seed`] must read them back in, so
/// producing it here means an export directory re-seeds without the seeder having to sort
/// around a missing parent.
async fn paths_in_order(store: &Store, principal: &Principal) -> Result<Vec<String>> {
    fn walk(nodes: &[gw_store::TreeNode], out: &mut Vec<String>) {
        for node in nodes {
            out.push(node.path.clone());
            walk(&node.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&store.tree_for(principal).await?, &mut out);
    // `tree_for` already yields parents before children; sorting by depth keeps that true
    // no matter how the tree is assembled later, and makes the report stable.
    out.sort_by_key(|p| p.matches('/').count());
    Ok(out)
}

/// The file a document at `path` exports to: its own path, with `.md` on the end.
///
/// `/rundgang/tabellen` → `rundgang/tabellen.md`. The seeder derives a parent from the
/// *directory* names and the slug from frontmatter, so laying the tree out this way is what
/// makes the round trip exact — every directory name is already a slug, and `slugify` of a
/// slug is that slug.
pub fn file_for(path: &str) -> PathBuf {
    let mut file = PathBuf::new();
    for segment in path.trim_start_matches('/').split('/') {
        file.push(segment);
    }
    file.set_extension("md");
    file
}

/// Everything about a document except its body: what the frontmatter states.
///
/// Public, with [`render_file`] and [`file_for`], so a caller that is not walking a
/// directory — an HTTP handler, the MCP server the owner has chosen — can produce the
/// **same bytes for one page**, with the same round-trip check, without a filesystem
/// anywhere in the call. The strings are the store's own spellings, so a handler passes a
/// [`gw_store::StoredDocument`]'s fields straight through and cannot mistranslate them.
#[derive(Debug, Clone)]
pub struct FileMeta {
    pub title: String,
    pub doc_type: String,
    pub visibility: String,
    pub language: String,
    pub sort_key: i64,
    pub slug: String,
    /// The topics this page is filed under, as [`gw_store::Topic::display_path`] spells
    /// them — `Medizin/Darm`, not `/medizin/darm`.
    ///
    /// Named `tags` because that is the frontmatter key, which is also the table's name;
    /// the domain word is *topic*, and the design's own data model draws that line the same
    /// way. [`gw_core::SeedMeta::tags`] is the other end of the same string.
    ///
    /// **The order is part of the contract**, and it is the store's order (canonical path)
    /// rather than anything decided here. [`render_file`] re-imports its own output and
    /// compares this list against what came back; a list rendered in a different order than
    /// it was read in would refuse the page, and one refused page fails the whole export.
    pub tags: Vec<String>,
}

/// The whole file — frontmatter and body — or the reason it cannot be written.
///
/// The unit of work, and the seam: [`run`] is this function plus a directory walk, and a
/// handler that wants one page as markdown calls it directly.
///
/// The check at the end is the point of this function and of the module: the markdown is
/// parsed back exactly the way [`crate::seed`] would parse it, and the resulting tree and
/// metadata are compared with what came out of the database. A mismatch means this file
/// would import as a different document, so it is not written.
pub fn render_file(meta: &FileMeta, body: &Block) -> Result<String, String> {
    let rendered = render(body);
    if !rendered.problems.is_empty() {
        return Err(rendered.problems.join("; "));
    }

    let file = format!("{}\n{}", frontmatter(meta)?, rendered.markdown);

    let (yaml, reimported_body) = split_frontmatter(&file);
    let reimported_meta = SeedMeta::parse(yaml, "the file just rendered")
        .map_err(|e| format!("its own frontmatter does not parse back ({e})"))?;
    let mut conversion = markdown::convert(reimported_body);
    crate::seed::drop_duplicate_title_heading(&mut conversion.doc, &reimported_meta.title);

    if comparable(&conversion.doc) != comparable(body) {
        return Err(format!(
            "the markdown for it re-imports as a DIFFERENT document, so exporting it would \
             hand you a file that quietly changes the page when you load it back. This is a \
             bug in the exporter, not in your content — please report it with the page path. \
             Rendered:\n{}",
            indent(&rendered.markdown)
        ));
    }
    if reimported_meta.title != meta.title
        || reimported_meta.doc_type.as_str() != meta.doc_type
        || reimported_meta.visibility.as_str() != meta.visibility
        || reimported_meta.language != meta.language
        || reimported_meta.sort_key != meta.sort_key
        || reimported_meta.slug.as_deref().map(slugify) != Some(meta.slug.clone())
        // Compared verbatim and IN ORDER. A topic decides which listings a page turns up in,
        // so a file that came back filed under something else is a file that quietly moves
        // the page in front of a different audience — the same class of change as
        // `visibility`, which is why it is checked in the same place and not more gently.
        || reimported_meta.tags != meta.tags
    {
        return Err(
            "its frontmatter re-imports as different metadata, which would move or \
             re-classify the page on the way back in"
                .to_string(),
        );
    }
    Ok(file)
}

/// The attributes of a `Link` mark that this system can actually mean something by.
///
/// `href` is the address markdown writes; `doc` is the internal target Task 7 resolves into
/// one. There is no third: `gw_core::Mark`'s own doc comment says a link carries either, and
/// nothing in the importer, the renderer below or `BlockView` reads any other key.
const LINK_ATTRS: [&str; 2] = ["href", "doc"];

/// The attributes of a `taskItem` that markdown has a spelling for.
///
/// Exactly one. `[x]` and `[ ]` say `checked` and say nothing else, and every other key a
/// task block carries is database identity rather than document content — the uuid that
/// ties the line to its record on the board, minted by the store during reconciliation on
/// publish and by the editor when somebody types a new checkbox line, never by
/// `gw_core::markdown`, which is a pure function and must stay one.
const TASK_ITEM_ATTRS: [&str; 1] = ["checked"];

/// A `Block` reduced to what markdown could ever state about it, for [`render_file`]'s
/// round-trip comparison.
///
/// The comparison asks one question — "would this file re-import as the same document?" —
/// and it has to ask it about the document, not about the editor's bookkeeping. TipTap's
/// `Link` mark declares `target`, `rel`, `class` and `title` beside `href` and ProseMirror
/// fills every one of them in with its default, so a link that has ever been through the
/// editor is stored as `{href, target, rel, class, title}`. `gw_core::markdown` never
/// produces those and `BlockView` never reads them — it renders its own fixed `rel` and no
/// `target` — so a byte-equal comparison refused every page containing such a link, and
/// `export` bails on the first refusal. One link written in the editor made the entire wiki
/// unexportable, on the one path that is the owner's backup.
///
/// `web/src/lib/editor/extensions.ts` now declares only `href`, so nothing new arrives this
/// way. That fix cannot reach backwards: the Y.Docs and the revisions already written hold
/// the full set and will keep holding it, and a document that cannot be exported cannot be
/// exported *later* either. So the exporter forgives them here, symmetrically — the same
/// reduction is applied to both sides, so it can never hide a difference in the text, the
/// structure, the mark kind, or the address itself.
///
/// Be precise about what it CAN hide, because the obvious reading is wrong in two ways and a
/// comment that overstates a security guarantee is how the next person widens it. This is an
/// **allow-list**: it keeps `href` and `doc` and discards *everything else* on a `Link` mark,
/// not merely TipTap's four. A difference in a key nobody has thought of is invisible here
/// too. And markdown is not silent about all four — `[text](url "titel")` is link-title
/// syntax, and `title` is one of the keys discarded. That is harmless only for as long as
/// `gw_core::markdown` keeps throwing titles away at import, which it does today
/// (`Tag::Link { dest_url, .. }` ignores the title) and TipTap's own `parseMarkdown` does
/// not. The day the importer preserves a title, this reduction stops the guard noticing that
/// an export dropped one — so that day, `title` must leave the allow-list.
///
/// Marks other than `Link` are compared whole; a `strong` carrying a stray attribute is
/// still refused. A link with no address is refused by [`Renderer::wrap`], loudly, before
/// this is reached.
///
/// # The same argument, for a task's identity
///
/// A `taskItem` gets the identical treatment and for the identical reason, arrived at from
/// the other end. Its `checked` is a real difference in the document — an unticked box and a
/// ticked one are two different pages — but the uuid beside it is not: it is what ties the
/// line to its record on the board, and markdown has no syntax for it and never will. The
/// converter is a pure function and mints none, so an exported checklist re-imports as
/// `{checked}` while the stored one says `{checked, id}`, and without this reduction every
/// page holding a reconciled task would be refused — permanently, since a document that
/// cannot be exported cannot be exported later either, and `export` fails the whole run on
/// the first refusal.
///
/// The same care about what it hides applies: this is an allow-list keeping `checked` and
/// discarding everything else, so a key nobody has thought of is invisible here too. That
/// is the right trade only while `checked` stays the sole thing GFM's `[x]` states. Block
/// kinds other than `taskItem` are compared with their attributes whole — a `heading` that
/// has grown a stray key is still refused.
pub(crate) fn comparable(block: &Block) -> serde_json::Value {
    let mut copy = block.clone();
    reduce(&mut copy);
    serde_json::to_value(&copy).expect("a Block always serialises")
}

fn reduce(block: &mut Block) {
    if block.kind == BlockKind::TaskItem {
        block
            .attrs
            .retain(|key, _| TASK_ITEM_ATTRS.contains(&key.as_str()));
    }
    for mark in &mut block.marks {
        if mark.kind == MarkKind::Link {
            mark.attrs
                .retain(|key, _| LINK_ATTRS.contains(&key.as_str()));
        }
    }
    for child in &mut block.content {
        reduce(child);
    }
}

fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("      | {l}\n"))
        .collect::<String>()
}

/// The YAML block, with every field stated.
///
/// Nothing is omitted "because it is the default". `visibility` is the reason: the default
/// is `restricted`, so a public page whose visibility was left out of the file would come
/// back private — a silent, invisible demotion. The same argument makes `slug` explicit,
/// since a title that slugifies differently would otherwise land the page at a new path
/// and orphan every child under the old one.
fn frontmatter(meta: &FileMeta) -> Result<String, String> {
    let mut map = serde_yaml::Mapping::new();
    let mut put = |k: &str, v: serde_yaml::Value| {
        map.insert(serde_yaml::Value::from(k), v);
    };
    put("title", meta.title.clone().into());
    put("type", meta.doc_type.clone().into());
    put("visibility", meta.visibility.clone().into());
    put("slug", meta.slug.clone().into());
    put("language", meta.language.clone().into());
    put("sort_key", meta.sort_key.into());
    // Written even when it is empty, for the reason above one step smaller: `tags: []` is
    // how somebody reading an exported file learns the key exists at all. There is no
    // silent-demotion argument here — an absent `tags` and an empty one mean the same thing
    // — so this is about the format documenting itself, not about safety.
    put(
        "tags",
        serde_yaml::Value::Sequence(
            meta.tags
                .iter()
                .map(|tag| serde_yaml::Value::from(tag.as_str()))
                .collect(),
        ),
    );

    let yaml = serde_yaml::to_string(&serde_yaml::Value::Mapping(map))
        .map_err(|e| format!("its metadata cannot be written as YAML ({e})"))?;
    Ok(format!("---\n{yaml}---\n"))
}

fn fidelity_file_contents(report: &ExportReport) -> String {
    format!(
        "great-wiki export\n=================\n\n{FIDELITY_WARNING}\n\n\
         Written by `great-wiki export --content <this directory> --as {}`.\n\
         {} document(s) exported. Load it back with:\n\n    \
         great-wiki seed --content <this directory>\n\n\
         …into an EMPTY wiki, or add `--update --as <account>` to write changed pages over \
         the ones already there. `seed` never deletes: a page that exists in the wiki and \
         not here stays.\n",
        report.principal,
        report.written.len()
    )
}

// ---------------------------------------------------------------------------------------
// Block tree → markdown
// ---------------------------------------------------------------------------------------

/// Markdown, plus everything about the tree that markdown could not state.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub markdown: String,
    /// Empty means the tree was fully expressible. Non-empty is a refusal, not a warning:
    /// a document with a problem here is never written.
    pub problems: Vec<String>,
}

/// Render a `doc` block as markdown, discarding what could not be expressed.
///
/// Prefer [`render`] anywhere the caller can act on a problem.
pub fn blocks_to_markdown(doc: &Block) -> String {
    render(doc).markdown
}

/// Render a `doc` block as markdown and report what the format could not hold.
pub fn render(doc: &Block) -> Rendered {
    let mut r = Renderer::default();
    let markdown = r.blocks(&doc.content, Nesting::Block);
    let markdown = if markdown.is_empty() {
        markdown
    } else {
        format!("{markdown}\n")
    };
    Rendered {
        markdown,
        problems: r.problems,
    }
}

/// Where a run of sibling blocks sits, which decides how tightly they may be joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Nesting {
    /// Top level, or inside a blockquote: always a blank line between siblings.
    Block,
    /// Inside a list item, where a paragraph may sit directly above a nested list.
    Item,
}

#[derive(Default)]
struct Renderer {
    problems: Vec<String>,
}

/// The two bullet characters, alternated so two adjacent lists stay two lists.
const BULLETS: [char; 2] = ['-', '*'];
/// The two ordered-list delimiters, alternated for the same reason.
const DELIMITERS: [char; 2] = ['.', ')'];

impl Renderer {
    fn problem(&mut self, detail: impl Into<String>) {
        self.problems.push(detail.into());
    }

    /// A run of sibling blocks.
    ///
    /// The alternating markers are not decoration. Two bullet lists separated by a blank
    /// line are ONE loose list to every CommonMark parser, so a document holding two lists
    /// would come back holding one; changing the marker is what makes the second list a
    /// second list.
    fn blocks(&mut self, blocks: &[Block], nesting: Nesting) -> String {
        let mut out = String::new();
        let mut bullet = 0usize;
        let mut delimiter = 0usize;
        let mut previous: Option<BlockKind> = None;

        for block in blocks {
            match (previous, block.kind) {
                (Some(BlockKind::BulletList), BlockKind::BulletList) => bullet ^= 1,
                (Some(BlockKind::OrderedList), BlockKind::OrderedList) => delimiter ^= 1,
                // A checklist is written with the same bullet characters, and shares the
                // counter with `BulletList` on purpose. Two checklists in a row must be
                // separated the same way two bullet lists are, or they come back as one —
                // but a checklist NEXT TO a bullet list must NOT change marker, because the
                // importer's own split (`gw_core::markdown::split_task_runs`) is what puts
                // those two back apart, and it can only do that if they arrive as one
                // markdown list. `- [ ] a` / `- plain` is one list on the way in and has to
                // be one list on the way out.
                (Some(BlockKind::TaskList), BlockKind::TaskList) => bullet ^= 1,
                _ => {}
            }
            if let Some(previous) = previous {
                out.push('\n');
                if !tight(nesting, previous, block) {
                    out.push('\n');
                }
            }
            out.push_str(&self.block(block, BULLETS[bullet], DELIMITERS[delimiter]));
            previous = Some(block.kind);
        }
        out
    }

    fn block(&mut self, block: &Block, bullet: char, delimiter: char) -> String {
        match block.kind {
            BlockKind::Paragraph => {
                let text = self.inline(block);
                self.line(text)
            }
            BlockKind::Heading => self.heading(block),
            BlockKind::BulletList => self.bullet_list(block, bullet),
            BlockKind::OrderedList => self.ordered_list(block, delimiter),
            BlockKind::TaskList => self.task_list(block, bullet),
            BlockKind::Blockquote => self.blockquote(block),
            BlockKind::CodeBlock => self.code(block),
            BlockKind::Table => self.table(block),
            BlockKind::Attachment => self.attachment(block),
            BlockKind::ListItem | BlockKind::TaskItem => {
                self.problem("a list item is outside any list");
                self.blocks(&block.content, Nesting::Item)
            }
            BlockKind::Doc
            | BlockKind::TableRow
            | BlockKind::TableHeader
            | BlockKind::TableCell => {
                self.problem(format!(
                    "a `{:?}` block sits where markdown has no way to write one",
                    block.kind
                ));
                String::new()
            }
            BlockKind::Text => {
                self.problem("bare text sits where a block should be");
                self.escape(block.text.as_deref().unwrap_or(""))
            }
            // `BlockKind` is `#[non_exhaustive]`: a kind added by a later milestone must
            // refuse loudly here rather than export as nothing.
            _ => {
                self.problem(format!(
                    "block kind `{:?}` is newer than this exporter — it would be dropped",
                    block.kind
                ));
                String::new()
            }
        }
    }

    /// Every text leaf under `block`, rendered with its marks and concatenated in order.
    fn inline(&mut self, block: &Block) -> String {
        let mut leaves = Vec::new();
        self.collect_inline(block, &mut leaves);
        self.runs(&leaves, &[])
    }

    /// Walk `block`'s children, collecting each `Text` leaf and recursing (with a refusal)
    /// into anything else — inline content holding a block is not something the schema is
    /// supposed to produce, but text nested inside it must still reach the page rather than
    /// vanish along with the refusal that names it.
    fn collect_inline<'a>(&mut self, block: &'a Block, out: &mut Vec<InlineLeaf<'a>>) {
        for child in &block.content {
            match child.kind {
                BlockKind::Text => out.push(InlineLeaf::of(child)),
                other => {
                    self.problem(format!(
                        "a `{other:?}` block is nested inside inline content, where markdown \
                         has no way to write one"
                    ));
                    self.collect_inline(child, out);
                }
            }
        }
    }

    /// A run of leaves, given that `applied` is already written around all of them.
    ///
    /// A mark is a property of a *leaf*, but markdown writes it as a delimiter *around a
    /// span*: `*kursiv und **beides***` is two leaves — one carrying em, one carrying em
    /// and strong — under one run of emphasis. Wrapping each leaf on its own closes the
    /// emphasis in the middle of the run and reopens it, which is not the same document and
    /// in general not even valid markdown (`*kursiv und ****beides***`). So each mark is
    /// opened once, around every consecutive leaf that carries it, and the leaves inside
    /// are rendered again with that mark taken as read.
    fn runs(&mut self, leaves: &[InlineLeaf<'_>], applied: &[&Mark]) -> String {
        let mut out = String::new();
        let mut i = 0;
        while i < leaves.len() {
            match choose(&leaves[i..], applied) {
                // Nothing left to open: this leaf is its own text.
                None => {
                    let base = self.base(&leaves[i]);
                    out.push_str(&base);
                    i += 1;
                }
                Some((mark, run)) => {
                    let mut nested: Vec<&Mark> = applied.to_vec();
                    nested.push(mark);
                    let inner = self.runs(&leaves[i..i + run], &nested);
                    out.push_str(&self.wrap(mark, inner));
                    i += run;
                }
            }
        }
        out
    }

    /// One leaf's own text, before any mark is wrapped around it.
    ///
    /// `Code` is the one mark that supplies a base representation instead of a wrap: its
    /// content is verbatim CommonMark, so it bypasses [`Self::escape`] entirely rather than
    /// double-escaping text that is never parsed as markup in the first place — and nothing
    /// can be nested inside it, which is why [`gw_core::MARK_ORDER`] puts it innermost.
    fn base(&mut self, leaf: &InlineLeaf<'_>) -> String {
        if !leaf.code {
            return self.escape(leaf.text);
        }
        if leaf.text.contains('\n') || leaf.text.contains('\r') {
            self.problem(
                "a paragraph holds a line break, and markdown cannot carry one back inside a \
                 paragraph — every break re-imports as a space",
            );
        }
        code_span(leaf.text)
    }

    /// Escape every character that would otherwise re-import as markup rather than text.
    ///
    /// A line break is the one thing that cannot be escaped: markdown has no way to put a
    /// newline inside a paragraph that survives a re-import — a soft break and a hard break
    /// both come back as a space — so it is a refusal, not a substitution.
    fn escape(&mut self, s: &str) -> String {
        if s.contains('\n') || s.contains('\r') {
            self.problem(
                "a paragraph holds a line break, and markdown cannot carry one back inside a \
                 paragraph — every break re-imports as a space",
            );
        }
        escape_inline(s)
    }

    /// One mark's markdown syntax, wrapped around its already-rendered inner content.
    fn wrap(&mut self, mark: &Mark, inner: String) -> String {
        match mark.kind {
            MarkKind::Strong => self.delimited("**", inner),
            MarkKind::Em => self.delimited("*", inner),
            MarkKind::Strike => self.delimited("~~", inner),
            // Applied as the leaf's base representation by [`Self::base`], not as a wrap
            // around it, and removed from a leaf's marks before any of this — so this arm
            // is unreachable and exists only to keep the match exhaustive.
            MarkKind::Code => inner,
            MarkKind::Link => match mark.attrs.get("href").and_then(|v| v.as_str()) {
                Some(href) => format!("[{inner}]({href})"),
                // `attrs` holds `doc` instead: an internal link this crate cannot resolve,
                // because `render` has no store. Task 7 resolves `doc` ids to a path in
                // `run`, before the tree ever reaches this renderer — nothing produces one
                // yet, so this arm exists only to refuse loudly instead of guessing.
                None => {
                    self.problem(
                        "a link has no `href` to write — an internal link is resolved to a \
                         path by Task 7, not by this renderer",
                    );
                    inner
                }
            },
            // `MarkKind` is `#[non_exhaustive]`: a kind added by a later milestone must
            // refuse loudly here rather than export as nothing.
            _ => {
                self.problem(format!(
                    "mark kind `{:?}` is newer than this exporter — it would be dropped",
                    mark.kind
                ));
                inner
            }
        }
    }

    /// Wrap `inner` in an emphasis delimiter, refusing what markdown cannot carry.
    ///
    /// CommonMark only *opens* a delimiter run that is followed by a non-space and only
    /// *closes* one that is preceded by a non-space, and an empty run is no run at all. So
    /// `**fett **und` is not bold — it is four literal asterisks, and a document that
    /// exported that way would come back saying something the author never wrote. Markdown
    /// import cannot produce such a leaf (text that does not parse as emphasis never
    /// becomes a mark in the first place), but an editor can, and a mark that cannot be
    /// written is a refusal rather than a warning: [`Rendered::problems`] is what
    /// [`render_file`] refuses on, and silence here would be a silent corruption.
    fn delimited(&mut self, delimiter: &str, inner: String) -> String {
        if inner.is_empty()
            || inner.starts_with(char::is_whitespace)
            || inner.ends_with(char::is_whitespace)
        {
            self.problem(format!(
                "a `{delimiter}` mark covers text that begins or ends with a space (or no \
                 text at all), and markdown cannot write one: `{delimiter}` only opens \
                 before a non-space and only closes after one, so it would re-import as \
                 literal punctuation"
            ));
        }
        format!("{delimiter}{inner}{delimiter}")
    }

    /// One paragraph, on one line. Deliberately not wrapped: every wrap point would be a
    /// soft break, every soft break re-imports as a space, and every new line start would
    /// need its own escaping pass. Editors wrap; markdown does not have to.
    fn line(&mut self, escaped: String) -> String {
        let escaped = escape_line_start(&escaped);
        if escaped.starts_with("    ") {
            self.problem(
                "a paragraph begins with four spaces, which re-imports as an indented code \
                 block",
            );
        }
        escaped
    }

    fn heading(&mut self, block: &Block) -> String {
        let level = block
            .attrs
            .get("level")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .clamp(1, 6) as usize;
        let text = self.inline(block);
        if text.is_empty() {
            return "#".repeat(level);
        }
        // A heading ending in `#` would have it read as a closing sequence and dropped.
        let text = match text.strip_suffix('#') {
            Some(head) => format!("{head}\\#"),
            None => text,
        };
        format!("{} {}", "#".repeat(level), text)
    }

    fn bullet_list(&mut self, block: &Block, bullet: char) -> String {
        let items: Vec<String> = block
            .content
            .iter()
            .map(|item| {
                let marker = format!("{bullet} ");
                let width = marker.chars().count();
                self.item(item, &marker, width, BlockKind::ListItem)
            })
            .collect();
        items.join("\n")
    }

    fn ordered_list(&mut self, block: &Block, delimiter: char) -> String {
        let start = block
            .attrs
            .get("start")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        let items: Vec<String> = block
            .content
            .iter()
            .enumerate()
            .map(|(i, item)| {
                let number = start.saturating_add(i as u64);
                let marker = format!("{number}{delimiter} ");
                let width = marker.chars().count();
                self.item(item, &marker, width, BlockKind::ListItem)
            })
            .collect();
        items.join("\n")
    }

    /// A checklist: a bullet list whose every item states a box.
    ///
    /// `[x]` / `[ ]` is GFM's task-list syntax and it belongs to the item's *content*, not
    /// to its marker — which is the whole reason the continuation width below is 2 and not
    /// 6. A checklist carries no `start` and no numbering: markdown has no numbered
    /// checklist, which is what `gw_core::markdown::Unsupported::OrderedTaskList` reports
    /// on the way in, so there is nothing here to lose that was not already lost.
    ///
    /// `checked` is read as a boolean and anything else is read as unticked. Nothing here
    /// may invent a completed task out of an attribute it could not parse.
    fn task_list(&mut self, block: &Block, bullet: char) -> String {
        let items: Vec<String> = block
            .content
            .iter()
            .map(|item| {
                let ticked = item
                    .attrs
                    .get("checked")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let marker = format!("{bullet} [{}] ", if ticked { 'x' } else { ' ' });
                // TWO, not `marker.chars().count()`. A task item's content column is the
                // one after `- `; the brackets are the first characters of its first
                // paragraph. Indenting a continuation by the marker's six columns puts it
                // four past the content, which CommonMark reads as an indented code block —
                // so a nested checklist would come back as literal text inside a fence, and
                // a second paragraph as code. Verified against the importer, not reasoned
                // about: `crates/gw-api/tests/export_markdown.rs` round-trips both.
                self.item(item, &marker, 2, BlockKind::TaskItem)
            })
            .collect();
        items.join("\n")
    }

    /// One list item: its first line carries the marker, every later line `continuation`
    /// spaces, so a continuation stays inside the item instead of ending the list.
    ///
    /// `continuation` is passed rather than derived from the marker's width because for a
    /// task item the two differ — see [`Self::task_list`].
    fn item(
        &mut self,
        item: &Block,
        marker: &str,
        continuation: usize,
        expected: BlockKind,
    ) -> String {
        if item.kind != expected {
            self.problem(format!(
                "a list holds a `{:?}` where only `{expected:?}`s may go",
                item.kind
            ));
        }
        let body = self.blocks(&item.content, Nesting::Item);
        let indent = " ".repeat(continuation);
        let mut out = String::new();
        for (i, line) in body.lines().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if line.is_empty() {
                continue;
            }
            out.push_str(if i == 0 { marker } else { &indent });
            out.push_str(line);
        }
        if out.is_empty() {
            // An empty item still has to occupy its place, or every later item renumbers.
            out.push_str(marker.trim_end());
        }
        out
    }

    /// A file placed in the prose (D-15): `![Beschreibung](anhang:datei.png)`, on a line of
    /// its own.
    ///
    /// The syntax is `gw_core::markdown`'s, not this module's, and that is deliberate:
    /// [`gw_core::markdown::attachment_destination`] writes the destination and
    /// `attachment_reference` reads it back, both in the crate that also imports it, for the
    /// reason [`gw_core::MARK_ORDER`] lives there. Two copies of one syntax in two crates
    /// stop agreeing the day either is edited, and what that costs here is total: a
    /// placement written in a form the importer does not read back as one re-imports as a
    /// paragraph of text, `render_file` refuses the page, and `export` fails the whole run.
    ///
    /// **It says nothing about what the file is.** No type, no size, no address — a
    /// placement is a *reference*, and everything else about the file is a fact about the
    /// `attachments` row, which is the authority (D-15). The reader resolves the name
    /// against the page's own list; a name that resolves to nothing is a reference to a file
    /// that is not attached, which is a real state and stays exactly as written. Nothing
    /// here is in a position to know, and nothing here may start guessing: this renderer has
    /// no store, exactly as it has none to resolve a link's `doc` target with.
    ///
    /// Two refusals rather than a guess, both of shapes nothing in this system produces —
    /// only a hand-written body could — and both loud, because a refusal names its page and
    /// a silent substitution does not.
    fn attachment(&mut self, block: &Block) -> String {
        let Some(filename) = block.attrs.get("filename").and_then(|v| v.as_str()) else {
            self.problem(
                "a placed file states no `filename`, and there is nothing else in a \
                 placement that could say which file it is",
            );
            return String::new();
        };
        let Some(destination) = markdown::attachment_destination(filename) else {
            self.problem(format!(
                "a placed file is called `{filename}`, which markdown has no way to write as \
                 a destination — no name a page can give a file looks like this, so the \
                 block was not written by this wiki"
            ));
            return String::new();
        };
        // Through the same escaping a paragraph's text takes, so brackets and asterisks in a
        // description come back as themselves instead of closing the image early — and so a
        // newline in one is refused rather than silently becoming a space.
        let alt = block
            .attrs
            .get("alt")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let alt = self.escape(alt);
        format!("![{alt}]({destination})")
    }

    fn blockquote(&mut self, block: &Block) -> String {
        let body = self.blocks(&block.content, Nesting::Block);
        let mut out = String::new();
        for (i, line) in body.lines().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if line.is_empty() {
                out.push('>');
            } else {
                out.push_str("> ");
                out.push_str(line);
            }
        }
        if out.is_empty() {
            out.push('>');
        }
        out
    }

    fn code(&mut self, block: &Block) -> String {
        let mut code = String::new();
        for child in &block.content {
            match (&child.text, child.kind) {
                (Some(text), BlockKind::Text) => code.push_str(text),
                _ => self.problem(format!(
                    "a code block holds a `{:?}`, and a fence can only hold text",
                    child.kind
                )),
            }
        }
        let language = block
            .attrs
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if language.contains(char::is_whitespace) || language.contains('`') {
            self.problem(format!(
                "a code block's language `{language}` cannot be written on a fence"
            ));
        }
        // Long enough to close nothing inside the code: a fence must be longer than the
        // longest run of backticks it contains, or the block ends early and the rest of the
        // code becomes prose.
        let fence = "`".repeat(longest_backtick_run(&code).max(2) + 1);
        let mut out = format!("{fence}{language}\n");
        out.push_str(&code);
        if !code.is_empty() && !code.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&fence);
        out
    }

    /// A GFM table, or a refusal.
    ///
    /// GFM tables are far narrower than the block tree: one header row, one paragraph per
    /// cell, one alignment per *column*. Every one of those is checked rather than assumed,
    /// because the alternative is a table that exports without complaint and re-imports
    /// with a column missing or a number column gone ragged.
    fn table(&mut self, block: &Block) -> String {
        let Some((header, body)) = block.content.split_first() else {
            self.problem("a table has no rows at all");
            return String::new();
        };
        for row in &block.content {
            if row.kind != BlockKind::TableRow {
                self.problem(format!(
                    "a table holds a `{:?}` where only rows may go",
                    row.kind
                ));
                return String::new();
            }
        }
        if header
            .content
            .iter()
            .any(|c| c.kind != BlockKind::TableHeader)
        {
            self.problem(
                "a table's first row is not a header row, and markdown has no way to write a \
                 table without one",
            );
            return String::new();
        }
        if body
            .iter()
            .any(|r| r.content.iter().any(|c| c.kind == BlockKind::TableHeader))
        {
            self.problem("a table has a header cell outside its first row");
            return String::new();
        }

        let alignments: Vec<Option<String>> = header
            .content
            .iter()
            .map(|c| {
                c.attrs
                    .get("align")
                    .and_then(|v| v.as_str())
                    .map(Into::into)
            })
            .collect();
        for row in body {
            if row.content.len() != alignments.len() {
                self.problem(format!(
                    "a table row has {} cell(s) where the header has {} — markdown pads or \
                     truncates the row, so a column would move",
                    row.content.len(),
                    alignments.len()
                ));
                return String::new();
            }
            for (cell, expected) in row.content.iter().zip(&alignments) {
                let actual = cell.attrs.get("align").and_then(|v| v.as_str());
                if actual.map(str::to_string).as_ref() != expected.as_ref() {
                    self.problem(
                        "a table cell is aligned differently from its column, and markdown \
                         states alignment per column only",
                    );
                    return String::new();
                }
            }
        }

        let mut out = String::new();
        out.push_str(&self.row(header));
        out.push('\n');
        out.push('|');
        for align in &alignments {
            out.push_str(match align.as_deref() {
                Some("left") => " :--- ",
                Some("center") => " :---: ",
                Some("right") => " ---: ",
                None => " --- ",
                Some(other) => {
                    self.problem(format!("a table column has an unknown alignment `{other}`"));
                    " --- "
                }
            });
            out.push('|');
        }
        for row in body {
            out.push('\n');
            out.push_str(&self.row(row));
        }
        out
    }

    fn row(&mut self, row: &Block) -> String {
        let mut out = String::from("|");
        for cell in &row.content {
            let text = match cell.content.as_slice() {
                [] => String::new(),
                [only] if only.kind == BlockKind::Paragraph => self.inline(only),
                _ => {
                    self.problem(
                        "a table cell holds something other than a single paragraph, and a \
                         markdown cell can hold nothing else",
                    );
                    String::new()
                }
            };
            out.push(' ');
            out.push_str(&text.replace('|', "\\|"));
            out.push(' ');
            out.push('|');
        }
        out
    }
}

/// Whether two siblings may be joined by a single newline instead of a blank line.
///
/// Only inside a list item, and only when the second is a list that markdown allows to
/// interrupt a paragraph. An ordered list numbered from anything but 1 may **not** — it is
/// read as more of the paragraph above it, and the whole list disappears into the sentence.
fn tight(nesting: Nesting, previous: BlockKind, next: &Block) -> bool {
    if nesting != Nesting::Item || previous != BlockKind::Paragraph {
        return false;
    }
    match next.kind {
        // A checklist is written with a bullet marker and carries no `start`, so it
        // interrupts a paragraph exactly as a bullet list does.
        BlockKind::BulletList | BlockKind::TaskList => true,
        BlockKind::OrderedList => {
            next.attrs
                .get("start")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                == 1
        }
        _ => false,
    }
}

/// One text leaf on its way out: its text, whether it is a code span, and the marks that
/// have to be *written around* it.
struct InlineLeaf<'a> {
    text: &'a str,
    /// Carried a `Code` mark, which is a base representation rather than a wrap — see
    /// [`Renderer::base`]. It is therefore absent from `marks`.
    code: bool,
    /// Outermost first, exactly the order [`gw_core::MARK_ORDER`] defines.
    marks: Vec<&'a Mark>,
}

impl<'a> InlineLeaf<'a> {
    fn of(leaf: &'a Block) -> Self {
        let mut marks: Vec<&Mark> = leaf.marks.iter().collect();
        // Sorted rather than taken as given. `gw_core::markdown` already stores marks in
        // this order, so for anything imported from markdown this is a no-op — but the
        // DATABASE is the source of truth (AGENTS.md rule 1) and a tree can reach here
        // having been assembled some other way. [`Renderer::runs`] needs the outermost
        // mark first to see what a run of leaves shares, and this is what guarantees it.
        marks.sort_by_key(|m| m.kind.nesting_rank());
        let code = marks.iter().any(|m| m.kind == MarkKind::Code);
        marks.retain(|m| m.kind != MarkKind::Code);
        InlineLeaf {
            text: leaf.text.as_deref().unwrap_or(""),
            code,
            marks,
        }
    }

    fn carries(&self, mark: &Mark) -> bool {
        self.marks.contains(&mark)
    }
}

/// The mark to open next at the start of `leaves`, and how many leaves it covers.
///
/// Of the marks the first leaf still carries, the one covering the longest run from here —
/// so `[a **b**](url)`, whose two leaves share only the link, opens the link around both
/// rather than writing the link twice. Ties go to the outermost kind by
/// [`gw_core::MARK_ORDER`], which is what makes the choice a decision rather than an
/// accident of iteration order, and therefore what makes two exports of one document
/// identical.
fn choose<'a>(leaves: &[InlineLeaf<'a>], applied: &[&Mark]) -> Option<(&'a Mark, usize)> {
    let first = leaves.first()?;
    let mut best: Option<(&'a Mark, usize)> = None;
    for mark in first.marks.iter().copied().filter(|m| !applied.contains(m)) {
        let run = leaves.iter().take_while(|l| l.carries(mark)).count();
        let better = match best {
            None => true,
            Some((best_mark, best_run)) => {
                run > best_run
                    || (run == best_run && mark.kind.nesting_rank() < best_mark.kind.nesting_rank())
            }
        };
        if better {
            best = Some((mark, run));
        }
    }
    best
}

/// An inline code span: verbatim, never escaped — CommonMark reads everything between the
/// backticks literally, so running it through [`escape_inline`] would hand the reader a
/// backslash the source text never had. The fence must still be longer than the longest run
/// of backticks the content contains, or the span closes early and the rest reads as prose;
/// and content that starts or ends with a backtick — or is only spaces — gets a padding
/// space on each side, exactly what keeps that edge character from reading as part of the
/// fence instead of the content, per CommonMark's own rule for stripping such padding back
/// out on the way in.
fn code_span(text: &str) -> String {
    let fence = "`".repeat(longest_backtick_run(text) + 1);
    let pad = text.starts_with('`')
        || text.ends_with('`')
        || (!text.is_empty() && text.chars().all(|c| c == ' '));
    if pad {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}

fn longest_backtick_run(s: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in s.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

/// Backslash-escape everything that would re-import as markup rather than as itself.
///
/// Deliberately narrow rather than escaping all punctuation: these files are meant to be
/// read and edited by a person, and `\.` after every abbreviation is how a format becomes
/// unusable. Narrowness is affordable *because* `render_file` re-imports every document and
/// compares — an escape this misses is a refusal, never a silent corruption.
fn escape_inline(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (i, &c) in chars.iter().enumerate() {
        let previous = i.checked_sub(1).map(|j| chars[j]);
        let next = chars.get(i + 1).copied();
        let escape = match c {
            // Always: a stray one of these opens emphasis, code or a link.
            '\\' | '`' | '*' | '[' | ']' => true,
            // `snake_case` is not emphasis in CommonMark — underscores only mark up at a
            // word boundary — so escaping them everywhere would make identifiers unreadable.
            '_' => {
                !previous.is_some_and(char::is_alphanumeric)
                    || !next.is_some_and(char::is_alphanumeric)
            }
            // `<0,5 %` is text; `<div>` and `<https://…>` are not. Escaping only what could
            // open a tag or an autolink keeps German measurements readable.
            '<' => next.is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '!' | '/' | '?')),
            // `&amp;` decodes to `&` on the way back, so the five characters the author
            // wrote would come back as one.
            '&' => next.is_some_and(|n| n.is_ascii_alphanumeric() || n == '#'),
            // Strikethrough is enabled in the importer, so a doubled tilde marks up.
            '~' => next == Some('~') || previous == Some('~'),
            _ => false,
        };
        if escape {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escape the characters that are only structural at the start of a line.
///
/// `- ` in the middle of a sentence is a dash; at the start of one it is a list, and the
/// paragraph the author wrote becomes a bullet.
fn escape_line_start(line: &str) -> String {
    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return line.to_string();
    };
    let rest: String = chars.collect();

    // `#` through `######` followed by a space or nothing is a heading.
    if first == '#' {
        let hashes = 1 + rest.chars().take_while(|&c| c == '#').count();
        let after = line.chars().nth(hashes);
        if hashes <= 6 && (after.is_none() || after == Some(' ')) {
            return format!("\\{line}");
        }
    }
    // A blockquote, a bullet, or a thematic break. `-` and `+` are escaped whenever they
    // lead the line: the cases where they are harmless are not worth the risk of getting
    // the "followed by a space" rule subtly wrong.
    if matches!(first, '>' | '-' | '+') {
        return format!("\\{line}");
    }
    // `1.` or `1)` — an ordered list. The digits are fine; the delimiter is not.
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let mut after = line.chars().skip(digits);
        if let Some(delimiter @ ('.' | ')')) = after.next() {
            let following = after.next();
            if following.is_none() || following == Some(' ') {
                let (head, tail) = line.split_at(digits);
                return format!("{head}\\{delimiter}{}", &tail[delimiter.len_utf8()..]);
            }
        }
    }
    line.to_string()
}
