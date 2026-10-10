//! Pure domain logic for great-wiki: the document model, conversions and validation.
//!
//! Deliberately free of I/O so every invariant here can be tested without a database,
//! a filesystem or a network. Round-trip fidelity of the export format is proven in
//! this crate.

pub mod block;
pub mod dataset;
pub mod diff;
pub mod document;
pub mod frontmatter;
pub mod markdown;
pub mod slug;
pub mod title;

pub use block::{body_plain_text, Block, BlockKind, Heading, Mark, MarkKind, MARK_ORDER};
pub use dataset::{FieldKey, FieldKind, KeyError};
pub use diff::{
    diff_design, diff_prose, diff_structure, ChangeKind, DesignChange, ProseChange, StructureChange,
};
pub use document::{DocumentType, Visibility};
pub use frontmatter::{split_frontmatter, FrontmatterError, SeedMeta};
pub use markdown::{markdown_to_blocks, Conversion, Note, Unsupported};
pub use slug::slugify;
pub use title::title_problem;
