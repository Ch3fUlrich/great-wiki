//! Identity and authorisation for great-wiki.
//!
//! Isolated in its own crate because a bug here is a disclosure rather than a defect:
//! the surface stays small enough to review in one sitting, and every rule is testable
//! without a web server or a database.

pub mod password;
pub mod permission;
pub mod principal;

pub use permission::{can, Action, Grant, Permission, Subject};
pub use principal::{Principal, PrincipalKind};
