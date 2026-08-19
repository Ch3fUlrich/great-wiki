//! Pure domain logic for great-wiki: the document model, conversions and validation.
//!
//! Deliberately free of I/O so every invariant here can be tested without a database,
//! a filesystem or a network. Round-trip fidelity of the export format is proven in
//! this crate.

pub mod block;
pub mod document;
pub mod frontmatter;
pub mod markdown;
pub mod slug;

pub use block::{Block, BlockKind, Heading, Mark, MarkKind, MARK_ORDER};
pub use document::{DocumentType, Visibility};
pub use frontmatter::{split_frontmatter, FrontmatterError, SeedMeta};
pub use markdown::{markdown_to_blocks, Conversion, Note, Unsupported};
pub use slug::slugify;
