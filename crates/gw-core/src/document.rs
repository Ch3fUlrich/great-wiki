use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
#[error("unknown value `{0}`")]
pub struct ParseError(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentType {
    Page,
    Research,
    Project,
    Dataset,
}

impl DocumentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            DocumentType::Page => "page",
            DocumentType::Research => "research",
            DocumentType::Project => "project",
            DocumentType::Dataset => "dataset",
        }
    }
}

impl FromStr for DocumentType {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "page" => Ok(DocumentType::Page),
            "research" => Ok(DocumentType::Research),
            "project" => Ok(DocumentType::Project),
            "dataset" => Ok(DocumentType::Dataset),
            other => Err(ParseError(other.to_string())),
        }
    }
}

/// Who may read a document, before per-document ACLs are consulted.
///
/// `Restricted` is the Default deliberately. A document that arrives without a stated
/// visibility — from an importer, a migration, a bug — must never be world-readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Internal,
    #[default]
    Restricted,
}

impl Visibility {
    pub fn as_str(&self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Internal => "internal",
            Visibility::Restricted => "restricted",
        }
    }
}

impl FromStr for Visibility {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "public" => Ok(Visibility::Public),
            "internal" => Ok(Visibility::Internal),
            "restricted" => Ok(Visibility::Restricted),
            other => Err(ParseError(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{DocumentType, Visibility};
    use std::str::FromStr;

    #[test]
    fn document_type_round_trips_through_str() {
        for t in [
            DocumentType::Page,
            DocumentType::Research,
            DocumentType::Project,
            DocumentType::Dataset,
        ] {
            assert_eq!(DocumentType::from_str(t.as_str()).unwrap(), t);
        }
    }

    #[test]
    fn unknown_document_type_is_an_error() {
        assert!(DocumentType::from_str("wiki").is_err());
    }

    #[test]
    fn visibility_defaults_to_restricted() {
        // Fail closed: a document with no stated visibility must never be world-readable.
        assert_eq!(Visibility::default(), Visibility::Restricted);
    }

    #[test]
    fn visibility_parses_the_three_levels() {
        assert_eq!(Visibility::from_str("public").unwrap(), Visibility::Public);
        assert_eq!(
            Visibility::from_str("internal").unwrap(),
            Visibility::Internal
        );
        assert_eq!(
            Visibility::from_str("restricted").unwrap(),
            Visibility::Restricted
        );
        assert!(Visibility::from_str("world").is_err());
    }
}
