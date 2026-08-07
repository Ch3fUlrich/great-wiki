use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    /// Authenticated by Authelia. Groups come from the verified `groups` claim.
    Oidc,
    /// A great-wiki account for someone with no homelab SSO account. Not an Authelia user
    /// — great-wiki never writes that database.
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: String,
    pub kind: PrincipalKind,
    pub username: String,
    pub display_name: String,
    pub email: Option<String>,
    /// From the OIDC `groups` claim. Empty for local accounts.
    pub groups: Vec<String>,
    /// great-wiki's own teams. Populated by the store on load.
    pub teams: Vec<String>,
    pub active: bool,
}

impl Principal {
    /// The absence of a principal, not a principal with no rights. `is_authenticated`
    /// is false, which is what every gate keys on.
    pub fn anonymous() -> Self {
        Self {
            id: String::new(),
            kind: PrincipalKind::Local,
            username: String::new(),
            display_name: "Anonymous".into(),
            email: None,
            groups: Vec::new(),
            teams: Vec::new(),
            active: true,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        !self.id.is_empty() && !self.username.trim().is_empty()
    }

    #[doc(hidden)]
    pub fn test(username: &str, groups: &[&str], teams: &[&str]) -> Self {
        Self {
            id: format!("test-{username}"),
            kind: PrincipalKind::Local,
            username: username.into(),
            display_name: username.into(),
            email: None,
            groups: groups.iter().map(|s| s.to_string()).collect(),
            teams: teams.iter().map(|s| s.to_string()).collect(),
            active: true,
        }
    }
}
