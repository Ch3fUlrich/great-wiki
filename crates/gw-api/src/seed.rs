//! `great-wiki seed --content <dir>` — load a directory of markdown files into the store.
//!
//! Exists so there is real content to develop against before the editor lands (M3). The
//! database stays the source of truth (AGENTS.md rule 1): this is an *import*, it runs
//! once against an empty tree, and it is not a second write path — it inserts through
//! `Store::insert_document` like anything else.
//!
//! Two rules shape the whole module:
//!
//! * **Nothing is invented.** No implicit parent, no title guessed from a filename, no
//!   visibility assumed. A file that cannot be placed exactly as written is skipped and
//!   named in the report.
//! * **Every skip is reported and the process exits non-zero**, so this is usable in a
//!   script and a half-loaded corpus cannot pass for a loaded one.

use anyhow::{Context, Result};
use gw_core::{markdown, slugify, split_frontmatter, SeedMeta};
use gw_store::{NewDocument, Store};
use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

/// A document that reached the database.
#[derive(Debug, Clone)]
pub struct Inserted {
    /// Path relative to the content directory, so the report is readable regardless of
    /// where the corpus lives.
    pub file: PathBuf,
    pub path: String,
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
    pub inserted: Vec<Inserted>,
    pub skipped: Vec<Skipped>,
    pub notes: Vec<Note>,
}

impl SeedReport {
    /// Whether the run loaded the corpus completely. The caller turns this into an exit code.
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }
}

impl fmt::Display for SeedReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in &self.inserted {
            writeln!(f, "  inserted  {:<40} {}", i.path, i.file.display())?;
        }
        for n in &self.notes {
            writeln!(f, "  note      {}: {}", n.file.display(), n.detail)?;
        }
        for s in &self.skipped {
            // `reason` already names the file — see `Skipped::reason`.
            writeln!(f, "  SKIPPED   {}", s.reason)?;
        }
        write!(
            f,
            "{} inserted, {} skipped",
            self.inserted.len(),
            self.skipped.len()
        )
    }
}

/// Load every `.md` file under `content_dir`.
///
/// Errors returned here are failures of the *run* (an unreadable directory); a file that
/// cannot be loaded is a skip inside the report, not an error, so one bad file never
/// hides the state of the other ninety.
pub async fn run(store: &Store, content_dir: &Path) -> Result<SeedReport> {
    let files = collect_markdown(content_dir)
        .with_context(|| format!("reading content directory {}", content_dir.display()))?;

    let mut report = SeedReport::default();
    // Which file claimed which path during *this* run, so a collision names both sides
    // rather than only the loser.
    let mut claimed: HashMap<String, PathBuf> = HashMap::new();

    for rel in files {
        match load_one(store, content_dir, &rel, &claimed).await? {
            Loaded::Inserted { path, notes } => {
                claimed.insert(path.clone(), rel.clone());
                for detail in notes {
                    report.notes.push(Note {
                        file: rel.clone(),
                        detail,
                    });
                }
                report.inserted.push(Inserted { file: rel, path });
            }
            Loaded::Skipped(reason) => report.skipped.push(Skipped { file: rel, reason }),
        }
    }

    Ok(report)
}

enum Loaded {
    Inserted { path: String, notes: Vec<String> },
    Skipped(String),
}

async fn load_one(
    store: &Store,
    root: &Path,
    rel: &Path,
    claimed: &HashMap<String, PathBuf>,
) -> Result<Loaded> {
    let display = rel.display().to_string();
    // Every skip reason names its own file. `FrontmatterError` already does so itself, so
    // it is the one case that does not go through here.
    let skip = |message: String| Ok(Loaded::Skipped(format!("{display}: {message}")));

    let raw = match std::fs::read_to_string(root.join(rel)) {
        Ok(raw) => raw,
        // Not a hard error: a stray binary file named `.md` should not abort the corpus.
        Err(e) => return skip(format!("cannot be read ({e})")),
    };

    let (yaml, body) = split_frontmatter(&raw);
    let meta = match SeedMeta::parse(yaml, &display) {
        Ok(meta) => meta,
        Err(e) => return Ok(Loaded::Skipped(e.to_string())),
    };

    let parent_path = match parent_path_of(rel) {
        Ok(p) => p,
        Err(reason) => return skip(reason),
    };

    // No implicit parents. A directory is not a document — the document that owns
    // `handbuch/` is `handbuch.md` beside it, and if nobody wrote it, inventing one would
    // put an untitled, restricted page in the tree that nobody asked for.
    if let Some(parent) = &parent_path {
        if store.document_by_path(parent).await?.is_none() {
            return skip(format!(
                "parent document `{parent}` does not exist — create the file that owns it \
                 (`{}.md`, beside the directory); seeding never invents a parent",
                parent.trim_start_matches('/')
            ));
        }
    }

    let conversion = markdown::convert(body);
    let new = NewDocument {
        parent_path,
        doc_type: meta.doc_type,
        title: meta.title.clone(),
        slug: meta.slug.clone(),
        language: meta.language.clone(),
        visibility: meta.visibility,
        body: conversion.doc,
        sort_key: meta.sort_key,
    };

    let path = match new.resolved_path() {
        Ok(path) => path,
        Err(e) => return skip(e.to_string()),
    };

    // Checked before the insert so the message can say *what* collided. The UNIQUE
    // constraint is still the authority — see the fallback below.
    if store.document_by_path(&path).await?.is_some() {
        return skip(collision_reason(&path, claimed));
    }

    if let Err(e) = store.insert_document(&new).await {
        let message = e.to_string();
        // A collision that slipped past the check above — another writer, or a slug rule
        // this code got wrong. The UNIQUE constraint, not the pre-check, is the authority.
        if message.contains("UNIQUE") {
            return skip(collision_reason(&path, claimed));
        }
        return skip(format!("could not be inserted: {message}"));
    }

    let mut notes: Vec<String> = conversion.notes.iter().map(|n| n.to_string()).collect();
    if !meta.unknown_keys.is_empty() {
        notes.push(format!(
            "frontmatter keys not read by this milestone: {} — check for a typo, since an \
             unread key silently keeps the default",
            meta.unknown_keys.join(", ")
        ));
    }

    Ok(Loaded::Inserted { path, notes })
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
    use super::{collect_markdown, parent_path_of};
    use std::path::{Path, PathBuf};

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
