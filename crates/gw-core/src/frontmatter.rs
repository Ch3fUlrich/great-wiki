//! YAML frontmatter: splitting it off a markdown file, and the metadata the seeder and
//! the M13 importer read out of it.
//!
//! Everything here fails closed. A file whose metadata cannot be understood produces an
//! error naming the file, never a document assembled from guesses — a guessed title
//! creates a page at the wrong path, and a guessed visibility publishes something.

use crate::document::{DocumentType, Visibility};
use serde::Deserialize;
use thiserror::Error;

/// The frontmatter keys this milestone understands. Anything else is reported rather than
/// rejected: `tags:` and `status:` are real columns in §4 that later milestones will read,
/// and failing on them now would make content unwritable ahead of the code.
const KNOWN_KEYS: &[&str] = &[
    "title",
    "type",
    "visibility",
    "slug",
    "language",
    "sort_key",
];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrontmatterError {
    #[error(
        "{file}: no YAML frontmatter — the file must start with a `---` line followed by \
         at least `title:`"
    )]
    Missing { file: String },

    #[error(
        "{file}: frontmatter has no `title` — every document needs one, and deriving it \
         from the filename would silently create a page at a path nobody chose"
    )]
    MissingTitle { file: String },

    #[error("{file}: frontmatter is not usable: {message}")]
    Invalid { file: String, message: String },
}

/// Split a leading `---` fenced YAML block off the body.
///
/// Returns the YAML (without either fence) and everything after it. Both slices borrow
/// `raw`, so this costs nothing and the caller keeps exact byte offsets.
///
/// An opening fence with no closing fence is treated as *not* frontmatter and the whole
/// input is returned as the body. The alternative — swallowing the rest of the file as
/// YAML — turns a typo into silent content loss, which is the one thing this pipeline
/// must never do. The missing `title` that follows is a clear, file-naming error.
pub fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    // A BOM before the fence is common from Windows editors and would otherwise make the
    // first line not match.
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);

    let Some(after_open) = strip_fence_line(raw) else {
        return (None, raw);
    };

    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if is_fence(line) {
            return (
                Some(&after_open[..offset]),
                &after_open[offset + line.len()..],
            );
        }
        offset += line.len();
    }
    (None, raw)
}

/// `true` when the line is exactly `---`, ignoring trailing whitespace and line ending.
fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
}

/// The remainder after a leading fence line, or `None` when the input does not open with one.
fn strip_fence_line(raw: &str) -> Option<&str> {
    let first = raw.split_inclusive('\n').next()?;
    is_fence(first).then(|| &raw[first.len()..])
}

/// Document metadata as a seed or import file states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedMeta {
    pub title: String,
    pub doc_type: DocumentType,
    /// Defaults to [`Visibility::Restricted`]. A file that forgets the key, or misspells
    /// it, gets the private setting — AGENTS.md rule 3, and the reason this is not an
    /// `Option` the caller has to remember to unwrap.
    pub visibility: Visibility,
    pub slug: Option<String>,
    pub language: String,
    pub sort_key: i64,
    /// Keys present in the file that this milestone does not read. Surfaced so a typo
    /// (`visibilty:`) is visible in the seeder's output instead of quietly failing closed.
    pub unknown_keys: Vec<String>,
}

/// The wire shape. Separate from `SeedMeta` so `title` can be optional here and required
/// there — the absence is turned into a message naming the file rather than serde's.
#[derive(Debug, Deserialize)]
struct Raw {
    title: Option<String>,
    #[serde(rename = "type", default = "default_type")]
    doc_type: DocumentType,
    #[serde(default)]
    visibility: Visibility,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default = "default_language")]
    language: String,
    #[serde(default)]
    sort_key: i64,
}

fn default_type() -> DocumentType {
    DocumentType::Page
}

/// German is the corpus language; an English page states `language: en` explicitly.
fn default_language() -> String {
    "de".to_string()
}

impl SeedMeta {
    /// Parse the YAML produced by [`split_frontmatter`].
    ///
    /// `file` appears in every error, because the caller is walking a directory and an
    /// error without a filename is an error nobody can act on.
    pub fn parse(yaml: Option<&str>, file: &str) -> Result<Self, FrontmatterError> {
        let Some(yaml) = yaml else {
            return Err(FrontmatterError::Missing {
                file: file.to_string(),
            });
        };

        let value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| FrontmatterError::Invalid {
                file: file.to_string(),
                message: e.to_string(),
            })?;

        if value.is_null() {
            // An empty `---`/`---` block. Not "invalid YAML", just no title.
            return Err(FrontmatterError::MissingTitle {
                file: file.to_string(),
            });
        }

        // Unknown keys are collected from the raw mapping rather than by
        // `deny_unknown_fields`, which would make the file unusable instead of reported.
        // Sorted so the seeder's output does not depend on the order someone typed them.
        let mut unknown_keys: Vec<String> = value
            .as_mapping()
            .map(|m| {
                m.keys()
                    .filter_map(|k| k.as_str())
                    .filter(|k| !KNOWN_KEYS.contains(k))
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        unknown_keys.sort();

        let raw: Raw = serde_yaml::from_value(value).map_err(|e| FrontmatterError::Invalid {
            file: file.to_string(),
            message: e.to_string(),
        })?;

        let title = raw
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .ok_or_else(|| FrontmatterError::MissingTitle {
                file: file.to_string(),
            })?;

        Ok(SeedMeta {
            title,
            doc_type: raw.doc_type,
            visibility: raw.visibility,
            slug: raw
                .slug
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            language: raw.language,
            sort_key: raw.sort_key,
            unknown_keys,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{DocumentType, Visibility};
    use crate::frontmatter::{split_frontmatter, FrontmatterError, SeedMeta};

    fn parse(md: &str) -> Result<SeedMeta, FrontmatterError> {
        let (yaml, _) = split_frontmatter(md);
        SeedMeta::parse(yaml, "handbuch/erste-schritte.md")
    }

    #[test]
    fn splits_the_yaml_block_from_the_body() {
        let (yaml, body) = split_frontmatter("---\ntitle: T\n---\n# Überschrift\n");
        assert_eq!(yaml, Some("title: T\n"));
        assert_eq!(body, "# Überschrift\n");
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let (yaml, body) = split_frontmatter("# Nur Text\n");
        assert_eq!(yaml, None);
        assert_eq!(body, "# Nur Text\n");
    }

    #[test]
    fn crlf_line_endings_are_handled() {
        let (yaml, body) = split_frontmatter("---\r\ntitle: T\r\n---\r\nHallo\r\n");
        assert_eq!(yaml, Some("title: T\r\n"));
        assert_eq!(body, "Hallo\r\n");
    }

    #[test]
    fn a_byte_order_mark_before_the_fence_is_tolerated() {
        let (yaml, _) = split_frontmatter("\u{feff}---\ntitle: T\n---\nHallo\n");
        assert_eq!(yaml, Some("title: T\n"));
    }

    #[test]
    fn an_unterminated_fence_is_body_not_swallowed_yaml() {
        // Treating the rest of the file as YAML would delete the content of a file whose
        // author merely forgot the closing fence.
        let raw = "---\ntitle: T\n\n# Überschrift\n";
        let (yaml, body) = split_frontmatter(raw);
        assert_eq!(yaml, None);
        assert_eq!(body, raw);
    }

    #[test]
    fn a_missing_title_is_an_error_that_names_the_file() {
        let err = parse("---\ntype: page\n---\nHallo\n").unwrap_err();
        assert!(matches!(err, FrontmatterError::MissingTitle { .. }));
        let message = err.to_string();
        assert!(
            message.contains("handbuch/erste-schritte.md"),
            "an error without the filename is unactionable: {message}"
        );
        assert!(message.contains("title"), "{message}");
    }

    #[test]
    fn an_empty_title_counts_as_missing() {
        assert!(matches!(
            parse("---\ntitle: \"   \"\n---\n").unwrap_err(),
            FrontmatterError::MissingTitle { .. }
        ));
    }

    #[test]
    fn a_file_with_no_frontmatter_at_all_names_the_file() {
        let err = parse("# Nur Text\n").unwrap_err();
        assert!(matches!(err, FrontmatterError::Missing { .. }));
        assert!(err.to_string().contains("handbuch/erste-schritte.md"));
    }

    #[test]
    fn visibility_defaults_to_restricted() {
        // AGENTS.md rule 3. A file that does not mention visibility must never be public.
        let meta = parse("---\ntitle: Geheim\n---\n").unwrap();
        assert_eq!(meta.visibility, Visibility::Restricted);
    }

    #[test]
    fn a_misspelled_visibility_key_still_fails_closed_and_is_reported() {
        let meta = parse("---\ntitle: T\nvisibilty: public\n---\n").unwrap();
        assert_eq!(meta.visibility, Visibility::Restricted);
        assert_eq!(meta.unknown_keys, vec!["visibilty"]);
    }

    #[test]
    fn an_unknown_visibility_value_is_an_error_not_a_default() {
        // Silently downgrading `visibility: word` to restricted would be safe but
        // confusing; the author asked for something the system does not have.
        let err = parse("---\ntitle: T\nvisibility: world\n---\n").unwrap_err();
        assert!(matches!(err, FrontmatterError::Invalid { .. }));
        assert!(err.to_string().contains("handbuch/erste-schritte.md"));
    }

    #[test]
    fn the_three_visibility_levels_parse() {
        for (raw, expected) in [
            ("public", Visibility::Public),
            ("internal", Visibility::Internal),
            ("restricted", Visibility::Restricted),
        ] {
            let meta = parse(&format!("---\ntitle: T\nvisibility: {raw}\n---\n")).unwrap();
            assert_eq!(meta.visibility, expected);
        }
    }

    #[test]
    fn defaults_are_page_german_and_sort_key_zero() {
        let meta = parse("---\ntitle: T\n---\n").unwrap();
        assert_eq!(meta.doc_type, DocumentType::Page);
        assert_eq!(meta.language, "de");
        assert_eq!(meta.sort_key, 0);
        assert_eq!(meta.slug, None);
        assert!(meta.unknown_keys.is_empty());
    }

    #[test]
    fn every_stated_field_is_read() {
        let meta = parse(
            "---\ntitle: Größe und Maß\ntype: research\nvisibility: internal\n\
             slug: groesse\nlanguage: en\nsort_key: 7\n---\n",
        )
        .unwrap();
        assert_eq!(meta.title, "Größe und Maß");
        assert_eq!(meta.doc_type, DocumentType::Research);
        assert_eq!(meta.visibility, Visibility::Internal);
        assert_eq!(meta.slug.as_deref(), Some("groesse"));
        assert_eq!(meta.language, "en");
        assert_eq!(meta.sort_key, 7);
    }

    #[test]
    fn future_keys_are_reported_but_do_not_reject_the_file() {
        // `tags` and `status` are real columns in the design's data model. Rejecting them
        // now would make content unwritable ahead of the code that reads them.
        let meta = parse("---\ntitle: T\ntags: [a, b]\nstatus: draft\n---\n").unwrap();
        assert_eq!(meta.unknown_keys, vec!["status", "tags"]);
    }

    #[test]
    fn malformed_yaml_is_an_error_that_names_the_file() {
        let err = parse("---\ntitle: [unclosed\n---\n").unwrap_err();
        assert!(matches!(err, FrontmatterError::Invalid { .. }));
        assert!(err.to_string().contains("handbuch/erste-schritte.md"));
    }
}
