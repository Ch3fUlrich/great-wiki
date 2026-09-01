//! `great-wiki seed --content <dir>` — load a directory of markdown files into the store.
//!
//! Exists so there is real content to develop against before the editor lands (M3), and so
//! bulk content can be moved in and out (M12). The database stays the source of truth
//! (AGENTS.md rule 1): this is an *import*, and it is not a second write path — it creates
//! through `Store::create_document`, which publishes revision 1 in the same transaction,
//! and updates through `Store::publish_revision`, exactly as a person editing in the
//! browser does. Every page this command produces has a history from its first line, and
//! the history says who wrote it — including when the honest answer is "no account did".
//!
//! Three rules shape the whole module:
//!
//! * **Nothing is invented.** No implicit parent, no title guessed from a filename, no
//!   visibility assumed. A file that cannot be placed exactly as written is skipped and
//!   named in the report.
//! * **Every skip is reported and the process exits non-zero**, so this is usable in a
//!   script and a half-loaded corpus cannot pass for a loaded one.
//! * **Nothing is destroyed.** A path that already exists is an error by default. Updating
//!   one is a separate, explicit opt-in ([`Options::update`]) that names the person doing
//!   it, appends a revision rather than replacing one, and still refuses to change a live
//!   page's title, type, visibility, language or ordering — the fields that decide where a
//!   page is and who may see it are not things a file drop gets to move. And nothing is
//!   ever deleted: a page in the wiki that no file claims is *reported*, never removed.
//!
//! # A note for whoever wires M3's collaborative editor to the store
//!
//! `gw-collab`'s CRDT can hold inline marks that `Block` cannot, which makes a `Block`
//! snapshot a lossy view of a live document (see that crate's header). The update path here
//! writes a `Block`. The moment a document has live CRDT state behind it, writing a `Block`
//! over it is the loss that crate warns about, and this importer will need to refuse a
//! document that has one. Nothing persists CRDT state today, so nothing here can hit it
//! yet — but the day it does, this is the paragraph that says so.

use crate::export;
use anyhow::{Context, Result};
use gw_auth::{Action, Principal};
use gw_core::{markdown, slugify, split_frontmatter, Block, BlockKind, SeedMeta};
use gw_store::{NewDocument, Store};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// How an import may act on the wiki it is loading into.
///
/// The default is the historical one and the safe one: no identity, no updating, and a
/// collision is an error.
#[derive(Debug, Clone, Copy, Default)]
pub struct Options<'a> {
    /// Who the import runs as.
    ///
    /// Supplying one does not merely label the run — it *subjects* it to that account's
    /// permissions: creating a page needs write on its parent (or instance admin at the
    /// root), updating one needs write on the page itself, and pages the account cannot
    /// read are invisible to the "in the wiki but not in this directory" report. Running
    /// without one is the operator-level path the seeder has always had, and it cannot
    /// update anything.
    pub principal: Option<&'a Principal>,
    /// Whether a file may replace the body of a page that already exists.
    ///
    /// Spelled `update` rather than `force` or `overwrite` because that is what it does and
    /// what the report then says: the verb in the flag and the verb in the output are the
    /// same word. `--force` would name an objection being ignored without naming the
    /// consequence, and `--overwrite` would claim more than happens — the previous body is
    /// not replaced, it stays in the revision history with the earlier author's name on it.
    ///
    /// Requires [`Options::principal`]; an update with no author is refused before any file
    /// is read.
    pub update: bool,
}

/// What an import did to one document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The page did not exist and now does.
    Created,
    /// The page existed and its body changed. A revision was appended, attributed to the
    /// principal the run named.
    Updated,
    /// The page existed and the file said exactly what it already held. Nothing was
    /// written — a no-op revision in the timeline is noise that hides the real edits.
    Unchanged,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Created => "created",
            Outcome::Updated => "updated",
            Outcome::Unchanged => "unchanged",
        }
    }
}

/// A document that reached the database, and what happened to it.
#[derive(Debug, Clone)]
pub struct Applied {
    /// Path relative to the content directory, so the report is readable regardless of
    /// where the corpus lives.
    pub file: PathBuf,
    pub path: String,
    pub outcome: Outcome,
}

/// A file that did not, and exactly why.
#[derive(Debug, Clone)]
pub struct Skipped {
    pub file: PathBuf,
    /// Self-contained: the reason always names its own file, so a line lifted out of the
    /// report into a ticket or a CI log is still actionable on its own.
    pub reason: String,
}

/// Something that was converted lossily but not lost. Informational: notes do not make
/// the command fail, because the content did reach the database.
#[derive(Debug, Clone)]
pub struct Note {
    pub file: PathBuf,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub struct SeedReport {
    pub applied: Vec<Applied>,
    pub skipped: Vec<Skipped>,
    pub notes: Vec<Note>,
    /// Paths that exist in the wiki and that no file in the directory claims.
    ///
    /// Listed, never acted on. Deleting is a different verb and this command does not have
    /// it: a directory that is missing a file — because somebody exported a subtree, or
    /// because a `git checkout` went sideways — must not be able to empty a wiki. Only
    /// populated when the run names a principal, and then only with pages that principal
    /// can see, because a list of paths is itself a disclosure.
    pub absent: Vec<String>,
}

impl SeedReport {
    /// Whether the run loaded the corpus completely. The caller turns this into an exit code.
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }

    /// How many documents had this outcome.
    pub fn count(&self, outcome: Outcome) -> usize {
        self.applied.iter().filter(|a| a.outcome == outcome).count()
    }
}

impl fmt::Display for SeedReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for a in &self.applied {
            writeln!(
                f,
                "  {:<9} {:<40} {}",
                a.outcome.as_str(),
                a.path,
                a.file.display()
            )?;
        }
        for n in &self.notes {
            writeln!(f, "  note      {}: {}", n.file.display(), n.detail)?;
        }
        for path in &self.absent {
            writeln!(
                f,
                "  absent    {path} — in the wiki, not in this directory; nothing was \
                 deleted, because deleting is a different command"
            )?;
        }
        for s in &self.skipped {
            // `reason` already names the file — see `Skipped::reason`.
            writeln!(f, "  SKIPPED   {}", s.reason)?;
        }
        write!(
            f,
            "{} created, {} updated, {} unchanged, {} skipped, {} in the wiki but not here",
            self.count(Outcome::Created),
            self.count(Outcome::Updated),
            self.count(Outcome::Unchanged),
            self.skipped.len(),
            self.absent.len()
        )
    }
}

/// Load every `.md` file under `content_dir` with the default options: no identity, and a
/// collision is an error.
pub async fn run(store: &Store, content_dir: &Path) -> Result<SeedReport> {
    run_as(store, content_dir, Options::default()).await
}

/// Load every `.md` file under `content_dir`.
///
/// This is the library seam: the CLI is a thin shell over it, and anything else holding a
/// [`Store`] — an HTTP handler, the MCP server the owner has chosen — calls the same
/// function with the same [`Options`] and gets the same [`SeedReport`] back. Nothing about
/// the rules lives in `main.rs`.
///
/// Errors returned here are failures of the *run* (an unreadable directory, or an update
/// with nobody to attribute it to); a file that cannot be loaded is a skip inside the
/// report, not an error, so one bad file never hides the state of the other ninety.
pub async fn run_as(store: &Store, content_dir: &Path, options: Options<'_>) -> Result<SeedReport> {
    // Refused before a single file is read, rather than per file: an update is a named
    // person's edit and appears in the history under their name, so a run that cannot name
    // one is a misconfiguration of the whole run.
    anyhow::ensure!(
        !options.update || options.principal.is_some(),
        "updating existing pages needs an account to attribute the edits to — pass the \
         identity as well, because every revision this writes carries an author and the \
         permission check that goes with it"
    );

    let files = collect_markdown(content_dir)
        .with_context(|| format!("reading content directory {}", content_dir.display()))?;

    let mut report = SeedReport::default();
    // Which file claimed which path during *this* run, so a collision names both sides
    // rather than only the loser.
    let mut claimed: HashMap<String, PathBuf> = HashMap::new();
    // Every path a file in this directory is *about*, whether or not it was applied. A
    // skipped file has still been seen, and listing its page as "in the wiki but not here"
    // would point at the wrong problem — the fix is the skip above it, not a missing file.
    let mut seen: HashSet<String> = HashSet::new();

    for rel in files {
        match load_one(store, content_dir, &rel, &claimed, options).await? {
            Loaded::Applied {
                path,
                outcome,
                notes,
            } => {
                claimed.insert(path.clone(), rel.clone());
                seen.insert(path.clone());
                for detail in notes {
                    report.notes.push(Note {
                        file: rel.clone(),
                        detail,
                    });
                }
                report.applied.push(Applied {
                    file: rel,
                    path,
                    outcome,
                });
            }
            Loaded::Skipped { reason, path } => {
                seen.extend(path);
                report.skipped.push(Skipped { file: rel, reason });
            }
        }
    }

    if let Some(principal) = options.principal {
        report.absent = readable_paths(store, principal)
            .await?
            .into_iter()
            .filter(|path| !seen.contains(path))
            .collect();
    }

    Ok(report)
}

/// Every path `principal` may read, parents first.
async fn readable_paths(store: &Store, principal: &Principal) -> Result<Vec<String>> {
    fn walk(nodes: &[gw_store::TreeNode], out: &mut Vec<String>) {
        for node in nodes {
            out.push(node.path.clone());
            walk(&node.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&store.tree_for(principal).await?, &mut out);
    Ok(out)
}

enum Loaded {
    Applied {
        path: String,
        outcome: Outcome,
        notes: Vec<String>,
    },
    Skipped {
        reason: String,
        /// The page the file was about, where the skip happened late enough to know it. A
        /// file with no title never gets one, and inventing a path for it would be the
        /// exact guess this module refuses everywhere else.
        path: Option<String>,
    },
}

async fn load_one(
    store: &Store,
    root: &Path,
    rel: &Path,
    claimed: &HashMap<String, PathBuf>,
    options: Options<'_>,
) -> Result<Loaded> {
    let display = rel.display().to_string();
    // Every skip reason names its own file. `FrontmatterError` already does so itself, so
    // it is the one case that does not go through here.
    // Every skip carries the page it was about when that is known, so the "in the wiki but
    // not in this directory" list does not accuse a file that IS here of being missing.
    let skip = |message: String| {
        Ok(Loaded::Skipped {
            reason: format!("{display}: {message}"),
            path: None,
        })
    };
    let skip_at = |path: &str, message: String| {
        Ok(Loaded::Skipped {
            reason: format!("{display}: {message}"),
            path: Some(path.to_string()),
        })
    };

    let raw = match std::fs::read_to_string(root.join(rel)) {
        Ok(raw) => raw,
        // Not a hard error: a stray binary file named `.md` should not abort the corpus.
        Err(e) => return skip(format!("cannot be read ({e})")),
    };

    let (yaml, body) = split_frontmatter(&raw);
    let meta = match SeedMeta::parse(yaml, &display) {
        Ok(meta) => meta,
        Err(e) => {
            return Ok(Loaded::Skipped {
                reason: e.to_string(),
                path: None,
            })
        }
    };

    let parent_path = match parent_path_of(rel) {
        Ok(p) => p,
        Err(reason) => return skip(reason),
    };

    // No implicit parents. A directory is not a document — the document that owns
    // `handbuch/` is `handbuch.md` beside it, and if nobody wrote it, inventing one would
    // put an untitled, restricted page in the tree that nobody asked for.
    if let Some(parent) = &parent_path {
        if !store.document_exists(parent).await? {
            return skip(format!(
                "parent document `{parent}` does not exist — create the file that owns it \
                 (`{}.md`, beside the directory); seeding never invents a parent",
                parent.trim_start_matches('/')
            ));
        }
    }

    // Checked here, before the create/update split, so that "🧬 cannot be a topic" is the
    // skip reason rather than an insert failure with the same words wrapped in another
    // sentence — and so that the update path below has the canonical set it needs to tell a
    // re-filing from a no-op without writing anything.
    let mut canonical_topics = HashSet::new();
    for stated in &meta.tags {
        match gw_store::canonical_topic(stated) {
            Ok(path) => {
                canonical_topics.insert(path);
            }
            Err(reason) => return skip(reason),
        }
    }

    let mut conversion = markdown::convert(body);
    drop_duplicate_title_heading(&mut conversion.doc, &meta.title);
    let new = NewDocument {
        parent_path,
        doc_type: meta.doc_type,
        title: meta.title.clone(),
        slug: meta.slug.clone(),
        language: meta.language.clone(),
        visibility: meta.visibility,
        body: conversion.doc,
        sort_key: meta.sort_key,
        // Verbatim, as the file spells them: `gw_store` resolves them, and a topic that is
        // new is created by the same transaction that creates the page.
        topics: meta.tags.clone(),
    };

    let path = match new.resolved_path() {
        Ok(path) => path,
        Err(e) => return skip(e.to_string()),
    };

    let mut notes: Vec<String> = conversion.notes.iter().map(|n| n.to_string()).collect();
    if !meta.unknown_keys.is_empty() {
        notes.push(format!(
            "frontmatter keys not read by this milestone: {} — check for a typo, since an \
             unread key silently keeps the default",
            meta.unknown_keys.join(", ")
        ));
    }

    // Checked before writing so the message can say *what* collided. The UNIQUE constraint
    // is still the authority — see the fallback below.
    let outcome = if store.document_exists(&path).await? {
        if !options.update {
            return skip_at(&path, collision_reason(&path, claimed));
        }
        // `run_as` refuses an update with no principal before any file is opened, so this
        // cannot be `None` here. Expressed as a skip rather than an `expect` all the same:
        // a future caller constructing `Options` by hand should get a message, not a panic.
        let Some(principal) = options.principal else {
            return skip_at(
                &path,
                "updating needs an account to attribute the edit to".to_string(),
            );
        };
        match update_one(
            store,
            principal,
            &path,
            &new,
            &canonical_topics,
            rel,
            &mut notes,
        )
        .await?
        {
            Ok(outcome) => outcome,
            Err(reason) => return skip_at(&path, reason),
        }
    } else {
        // Naming an identity means being subject to it. Without one this is the operator
        // path the seeder has always been, and the store's own insert is the only gate.
        if let Some(principal) = options.principal {
            if let Some(reason) = refuse_create(store, principal, &new, &path).await? {
                return skip_at(&path, reason);
            }
        }
        // Who revision 1 is filed under. A named account authors its own imports, exactly
        // as it authors its own updates. A run with no `--as` has nobody to name, so it says
        // so — `Author::Import` is not an account and cannot become one — rather than
        // attributing the corpus to whoever was at the keyboard.
        let author = match options.principal {
            Some(principal) => gw_store::Author::Account(principal),
            None => gw_store::Author::Import,
        };
        // The same summary an update writes, from the first revision onward, so a page's
        // history says where it came from rather than starting with a row that says nothing.
        let summary = format!("importiert aus {}", rel.display());
        if let Err(e) = store.create_document(author, &new, Some(&summary)).await {
            let message = e.to_string();
            // A collision that slipped past the check above — another writer, or a slug
            // rule this code got wrong. The UNIQUE constraint, not the pre-check, is the
            // authority.
            if message.contains("UNIQUE") {
                return skip_at(&path, collision_reason(&path, claimed));
            }
            return skip_at(&path, format!("could not be inserted: {message}"));
        }
        Outcome::Created
    };

    Ok(Loaded::Applied {
        path,
        outcome,
        notes,
    })
}

/// Replace the body of a page that already exists, or say why not.
///
/// `Err(reason)` is a skip, not a failure of the run. The write goes through
/// [`Store::publish_revision`], which is the same call the editor makes: it checks the
/// author may write *this* page, appends a revision rather than replacing one, and leaves
/// the previous body in the history under whoever wrote it. There is deliberately no path
/// here that touches `documents.body` directly — a second write path is how a wiki loses
/// the ability to say who changed what.
async fn update_one(
    store: &Store,
    principal: &Principal,
    path: &str,
    new: &NewDocument,
    canonical_topics: &HashSet<String>,
    rel: &Path,
    notes: &mut Vec<String>,
) -> Result<std::result::Result<Outcome, String>> {
    let Some(existing) = store.document_for(principal, path, Action::Read).await? else {
        return Ok(Err(format!(
            "`{path}` already exists but `{}` cannot read it, so this run cannot tell \
             whether the file would change it — grant access or run as somebody who has it",
            principal.username
        )));
    };

    // The fields that decide where a page lives and who may see it are NOT importable.
    // Changing them is reported and refused, because the failure mode is unrecoverable in
    // exactly the wrong direction: a stray `visibility: public` in a file drop would
    // publish a restricted page, and nothing in a bulk run is watching closely enough to
    // catch it. Moving a page is worse still — it orphans every child under the old path.
    for (field, in_file, in_wiki) in [
        ("title", new.title.as_str(), existing.title.as_str()),
        ("type", new.doc_type.as_str(), existing.doc_type.as_str()),
        (
            "visibility",
            new.visibility.as_str(),
            existing.visibility.as_str(),
        ),
        (
            "language",
            new.language.as_str(),
            existing.language.as_str(),
        ),
    ] {
        if in_file != in_wiki {
            notes.push(metadata_refusal(path, field, in_file, in_wiki));
        }
    }
    if new.sort_key != existing.sort_key {
        notes.push(metadata_refusal(
            path,
            "sort_key",
            &new.sort_key.to_string(),
            &existing.sort_key.to_string(),
        ));
    }

    let stored: Block = match serde_json::from_str(&existing.body) {
        Ok(body) => body,
        Err(e) => {
            return Ok(Err(format!(
                "`{path}` already exists but its stored body is not a block tree ({e}), so \
                 this run cannot tell an update from a no-op"
            )))
        }
    };
    // Compared as JSON because that is what the store holds and what the editor loads;
    // comparing extracted text instead would call a table turned into paragraphs
    // "unchanged". An unchanged page writes NOTHING: a no-op revision per file per run
    // buries the real edits in a history nobody can then read.
    //
    // Through [`crate::export::comparable`], and it must be — a raw comparison asks
    // "are these trees identical?" when the question is "does this FILE still say what
    // the database says?". A stored tree carries things a markdown file cannot: the uuid
    // the store mints into every task block on publish, and the `target`/`rel`/`class`
    // that ProseMirror fills in on every link the editor has touched. Compared raw, a
    // page holding one checkbox is republished on every single run — measured: four
    // revisions after three runs — which is precisely the no-op revision the paragraph
    // above forbids. Sharing the reduction with the exporter rather than writing a second
    // one keeps one answer to "what can markdown state"; two would drift.
    // Compared as CANONICAL paths, never as the spellings either side happens to hold: a
    // file saying `darm` where the wiki holds `Darm` states the same topic, and calling that
    // a change would re-file every page on every run and report an edit nobody made.
    let filed_now: HashSet<String> = store
        .document_topics_for(principal, path)
        .await?
        .unwrap_or_default()
        .into_iter()
        .map(|topic| topic.path)
        .collect();
    let refiled = &filed_now != canonical_topics;
    let rewritten = export::comparable(&stored) != export::comparable(&new.body);

    if !rewritten && !refiled {
        return Ok(Ok(Outcome::Unchanged));
    }

    // The topics first, and separately, because they are not part of the revision: a page
    // whose words are unchanged and whose filing is not writes no version of itself (see
    // `Store::set_document_topics`). Refused on the same terms a body would be — which
    // listings a page appears in is an edit of that page — and reported as a skip rather
    // than applied by halves.
    if refiled {
        match store
            .set_document_topics(principal, path, &new.topics)
            .await?
        {
            gw_store::TopicOutcome::Done(_) => {}
            gw_store::TopicOutcome::Refused => {
                return Ok(Err(format!(
                    "`{path}` exists and this file would file it under different topics, but \
                     `{}` may not write it — which listings a page appears in is an edit of \
                     that page, and writing is only ever an explicit grant",
                    principal.username
                )))
            }
            // Unreachable: `load_one` canonicalises every stated topic before this function
            // is called, and a topic that parses there parses here. Written out rather than
            // unwrapped all the same, because an `expect` in an importer is a panic over
            // somebody else's file.
            gw_store::TopicOutcome::Rejected(reason) => return Ok(Err(reason)),
        }
    }

    if !rewritten {
        // Only the filing changed. `Updated` is still the honest word — the wiki now says
        // something different about this page — and the history correctly holds no new
        // version of it, because nobody wrote one.
        return Ok(Ok(Outcome::Updated));
    }

    let summary = format!("importiert aus {}", rel.display());
    match store
        .publish_revision(principal, &existing.id, &new.body, Some(&summary))
        .await?
    {
        Some(_) => Ok(Ok(Outcome::Updated)),
        None => Ok(Err(format!(
            "`{path}` exists and this file would change it, but `{}` may not write it — an \
             import edits through the same permission check a person does, and writing is \
             only ever an explicit grant",
            principal.username
        ))),
    }
}

fn metadata_refusal(path: &str, field: &str, in_file: &str, in_wiki: &str) -> String {
    format!(
        "`{path}`: the file says {field} `{in_file}`, the wiki says `{in_wiki}` — the BODY \
         was updated and this was NOT. A file drop does not get to move a page or change \
         who can see it; do that in the admin console, where it is one deliberate act with \
         a name on it"
    )
}

/// Whether `principal` may create the document `new` describes, or the reason they may not.
///
/// **This system has no permission-checked "create a document" yet.** `create_document` takes
/// an *author* — who revision 1 is filed under — and deliberately no permission decision, and
/// there is no HTTP route that creates a document at all, so there is no existing rule to
/// call — and inventing an authorisation rule here would make this the *second* place that
/// decides access, which is always the one that gets it wrong when the rules change.
///
/// So exactly one question is asked, and it is one the store already answers: may this
/// account write the page the new one would hang under? Adding a child is an edit of the
/// branch, and `document_for(.., Write)` is the same check `publish_revision` makes.
///
/// At the root there is no parent to ask about and therefore no answer that is not a guess,
/// so creating a top-level page is **refused** whenever a principal is named. That is not a
/// gap being papered over: bootstrapping a wiki is the identity-less operator path (`seed`
/// with no account), which is unchanged and still unchecked, exactly as it was before this
/// milestone. When a permission-checked create lands with the editor, this function is where
/// it plugs in.
async fn refuse_create(
    store: &Store,
    principal: &Principal,
    new: &NewDocument,
    path: &str,
) -> Result<Option<String>> {
    if !principal.is_authenticated() || !principal.active {
        return Ok(Some(format!(
            "`{path}` would be created, but the run named no active, signed-in account to \
             create it as"
        )));
    }
    let Some(parent) = new.parent_path.as_deref() else {
        return Ok(Some(format!(
            "`{path}` would be a new TOP-LEVEL page, and nothing in this system yet says who \
             may create one — there is no parent to check write access on, and guessing a \
             rule here would put a second, weaker answer next to the real one. Create \
             top-level pages by running without an account (the operator path), then use an \
             account for everything below them"
        )));
    };
    if store
        .document_for(principal, parent, Action::Write)
        .await?
        .is_some()
    {
        return Ok(None);
    }
    Ok(Some(format!(
        "`{path}` would be created, but `{}` may not write its parent `{parent}` — adding a \
         page under a branch is an edit of that branch",
        principal.username
    )))
}

/// Drop a leading `# Title` that only repeats the frontmatter title.
///
/// A content file states its title twice — once in frontmatter, once as the first line of
/// the body — and the reader renders the frontmatter title as the page's `h1`. Keeping
/// both puts the same heading on the page twice and in the outline twice.
///
/// The comparison is **exact** (trimmed): not case-insensitive, not by prefix. A page
/// whose first heading genuinely differs from its title keeps both, because only the
/// author knows whether "Größe" under a page titled "Größe und Maß" is a repetition or a
/// section. Guessing wrong here deletes writing, which is worse than a duplicated line.
pub(crate) fn drop_duplicate_title_heading(doc: &mut Block, title: &str) {
    let Some(first) = doc.content.first() else {
        return;
    };
    if first.kind != BlockKind::Heading {
        return;
    }
    // An absent level means 1, as `Block::headings` reads it.
    let level = first
        .attrs
        .get("level")
        .and_then(|v| v.as_u64())
        .unwrap_or(1);
    if level != 1 || first.plain_text().trim() != title.trim() {
        return;
    }
    doc.content.remove(0);
}

fn collision_reason(path: &str, claimed: &HashMap<String, PathBuf>) -> String {
    match claimed.get(path) {
        Some(other) => format!(
            "path `{path}` was already taken by `{}` earlier in this run — two files \
             cannot own one path; give one of them a different `title` or `slug`",
            other.display()
        ),
        None => format!(
            "path `{path}` already exists in the database — seeding never overwrites; \
             remove the existing document or change this file's `title` or `slug`"
        ),
    }
}

/// The parent document's path, derived from the file's directory.
///
/// `handbuch/erste-schritte.md` → `/handbuch`. Directory names go through `slugify`, so
/// a folder called `Handbuch` and a document titled `Handbuch` agree without the author
/// having to think about it.
fn parent_path_of(rel: &Path) -> Result<Option<String>, String> {
    let Some(dir) = rel.parent() else {
        return Ok(None);
    };

    let mut segments = Vec::new();
    for component in dir.components() {
        let Component::Normal(name) = component else {
            // `..` or an absolute root would escape the content directory.
            return Err(format!(
                "path component `{}` is not a plain directory name",
                component.as_os_str().to_string_lossy()
            ));
        };
        let slug = slugify(&name.to_string_lossy());
        if slug.is_empty() {
            return Err(format!(
                "directory `{}` has no URL-safe name — rename it",
                name.to_string_lossy()
            ));
        }
        segments.push(slug);
    }

    if segments.is_empty() {
        Ok(None)
    } else {
        Ok(Some(format!("/{}", segments.join("/"))))
    }
}

/// Every `.md` file under `root`, relative to it, **shallowest first**.
///
/// Depth ordering is what makes "no implicit parents" workable: a parent is always
/// inserted before any of its children are considered, so a missing parent means the file
/// really is absent rather than merely later in the walk. Within a depth the order is
/// lexicographic, so two runs over the same corpus produce identical reports.
fn collect_markdown(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    walk(root, Path::new(""), &mut out)?;
    out.sort_by(|a, b| {
        a.components()
            .count()
            .cmp(&b.components().count())
            .then_with(|| a.cmp(b))
    });
    Ok(out)
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let dir = root.join(rel);
    let entries = std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // `.git`, `.obsidian` and editor backups are not content.
        if name.starts_with('.') {
            continue;
        }
        let child = rel.join(name.as_ref());
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk(root, &child, out)?;
        } else if child.extension().is_some_and(|e| e == "md") {
            out.push(child);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{collect_markdown, drop_duplicate_title_heading, parent_path_of};
    use gw_core::markdown;
    use std::path::{Path, PathBuf};

    /// Convert `body` and apply the rule, then describe what is left — one
    /// `kind: text` line per top-level block, so a test asserts the whole document
    /// rather than only the part it expected to change.
    fn after(title: &str, body: &str) -> Vec<String> {
        let mut doc = markdown::convert(body).doc;
        drop_duplicate_title_heading(&mut doc, title);
        doc.content
            .iter()
            .map(|b| format!("{:?}: {}", b.kind, b.plain_text()))
            .collect()
    }

    #[test]
    fn a_leading_h1_that_repeats_the_title_is_dropped() {
        // The reader renders the frontmatter title as the page's `h1`. Keeping the body's
        // copy puts the same heading on screen twice, and in the outline twice.
        assert_eq!(
            after("Größe und Maß", "# Größe und Maß\n\nEin Satz.\n"),
            vec!["Paragraph: Ein Satz."]
        );
    }

    #[test]
    fn a_leading_h1_that_differs_from_the_title_is_kept() {
        // Only the author knows the difference is meaningful, so it is never guessed away.
        assert_eq!(
            after("Handbuch", "# Erste Schritte\n\nEin Satz.\n"),
            vec!["Heading: Erste Schritte", "Paragraph: Ein Satz."]
        );
    }

    #[test]
    fn a_heading_matching_only_in_case_or_as_a_prefix_is_kept() {
        // The comparison is exact. A case-insensitive or prefix match would delete a
        // heading the author wrote deliberately — "Größe" under a page titled "Größe und
        // Maß" is a section, not a repetition.
        assert_eq!(
            after("Größe und Maß", "# größe und maß\n\nText.\n").len(),
            2
        );
        assert_eq!(after("Größe und Maß", "# Größe\n\nText.\n").len(), 2);
        assert_eq!(after("Größe", "# Größe und Maß\n\nText.\n").len(), 2);
    }

    #[test]
    fn surrounding_whitespace_does_not_decide_it() {
        assert_eq!(after("  Notiz  ", "#   Notiz  \n\nText.\n").len(), 1);
    }

    #[test]
    fn a_deeper_heading_that_repeats_the_title_is_kept() {
        // Only an `h1` collides with the title the reader renders; an `h2` of the same
        // name is a section that happens to share the page's name.
        assert_eq!(
            after("Tabellen", "## Tabellen\n\nText.\n"),
            vec!["Heading: Tabellen", "Paragraph: Text."]
        );
    }

    #[test]
    fn only_the_first_block_is_considered() {
        // A repetition further down is prose the author placed there, not the artefact of
        // writing the title twice at the top of the file.
        assert_eq!(
            after("Notiz", "Ein Satz.\n\n# Notiz\n\nMehr.\n"),
            vec!["Paragraph: Ein Satz.", "Heading: Notiz", "Paragraph: Mehr."]
        );
    }

    #[test]
    fn a_document_with_no_leading_heading_is_untouched() {
        assert_eq!(after("Notiz", "Ein Satz.\n"), vec!["Paragraph: Ein Satz."]);
        assert!(after("Notiz", "").is_empty(), "an empty body stays empty");
    }

    #[test]
    fn the_document_is_not_emptied_when_the_title_is_all_it_holds() {
        // A page whose body is only its own title becomes an empty document, not a
        // document with a heading nobody wanted. The reader still shows the title.
        let doc = after("Notiz", "# Notiz\n");
        assert!(doc.is_empty(), "{doc:?}");
    }

    #[test]
    fn a_root_file_has_no_parent() {
        assert_eq!(parent_path_of(Path::new("handbuch.md")), Ok(None));
    }

    #[test]
    fn a_nested_file_takes_its_directory_as_parent() {
        assert_eq!(
            parent_path_of(Path::new("handbuch/erste-schritte.md")),
            Ok(Some("/handbuch".to_string()))
        );
    }

    #[test]
    fn directory_names_are_slugified_like_titles() {
        assert_eq!(
            parent_path_of(Path::new("Größe/maß.md")),
            Ok(Some("/groesse".to_string()))
        );
    }

    #[test]
    fn deeply_nested_files_keep_the_whole_chain() {
        assert_eq!(
            parent_path_of(Path::new("a/b/c.md")),
            Ok(Some("/a/b".to_string()))
        );
    }

    #[test]
    fn a_traversal_component_is_refused() {
        assert!(parent_path_of(Path::new("../geheim/a.md")).is_err());
    }

    #[test]
    fn the_walk_is_shallowest_first_then_alphabetical() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("handbuch/tief")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        for f in [
            "zuletzt.md",
            "handbuch.md",
            "handbuch/erste-schritte.md",
            "handbuch/tief/tiefer.md",
            "nicht-markdown.txt",
            ".git/config.md",
        ] {
            std::fs::write(root.join(f), "x").unwrap();
        }

        let found = collect_markdown(root).unwrap();
        assert_eq!(
            found,
            vec![
                PathBuf::from("handbuch.md"),
                PathBuf::from("zuletzt.md"),
                PathBuf::from("handbuch/erste-schritte.md"),
                PathBuf::from("handbuch/tief/tiefer.md"),
            ],
            "parents must be inserted before children, and dotfiles are not content"
        );
    }
}
