//! The field kinds (the spec's list has thirteen names).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldKind {
    Text,
    Number,
    Bool,
    Date,
    Select,
    MultiSelect,
    Tags,
    Url,
    Person,
    File,
    Relation,
    Rollup,
    Formula,
}

impl FieldKind {
    pub const ALL: [FieldKind; 13] = [
        Self::Text,
        Self::Number,
        Self::Bool,
        Self::Date,
        Self::Select,
        Self::MultiSelect,
        Self::Tags,
        Self::Url,
        Self::Person,
        Self::File,
        Self::Relation,
        Self::Rollup,
        Self::Formula,
    ];

    /// Snake_case name, as stored and sent.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::Select => "select",
            Self::MultiSelect => "multi_select",
            Self::Tags => "tags",
            Self::Url => "url",
            Self::Person => "person",
            Self::File => "file",
            Self::Relation => "relation",
            Self::Rollup => "rollup",
            Self::Formula => "formula",
        }
    }

    /// Computed kinds are evaluated at read and never stored in a row.
    pub fn is_computed(self) -> bool {
        matches!(self, Self::Rollup | Self::Formula)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_matches_as_str() {
        for k in FieldKind::ALL {
            let j = serde_json::to_string(&k).unwrap();
            assert_eq!(j, format!("\"{}\"", k.as_str()));
            assert_eq!(serde_json::from_str::<FieldKind>(&j).unwrap(), k);
        }
        assert!(serde_json::from_str::<FieldKind>("\"nope\"").is_err());
    }

    #[test]
    fn only_rollup_and_formula_are_computed() {
        let c: Vec<_> = FieldKind::ALL
            .into_iter()
            .filter(|k| k.is_computed())
            .collect();
        assert_eq!(c, [FieldKind::Rollup, FieldKind::Formula]);
    }
}
