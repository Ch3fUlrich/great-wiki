use serde::Serialize;

/// Who is making a request.
///
/// Deliberately independent of how it was established. OIDC produces one; the development
/// shim produces one; M2's local accounts will produce one. Handlers consume only this, so
/// adding an authentication method never touches a handler.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Identity {
    pub user: Option<String>,
    pub groups: Vec<String>,
}

impl Identity {
    pub fn anonymous() -> Self {
        Self::default()
    }

    pub fn dev(user: &str, groups: &[&str]) -> Self {
        Self {
            user: Some(user.to_string()),
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }

    /// A blank username is anonymous, not "a user called empty string".
    pub fn is_authenticated(&self) -> bool {
        self.user.as_deref().is_some_and(|u| !u.trim().is_empty())
    }

    pub fn in_group(&self, group: &str) -> bool {
        self.groups.iter().any(|g| g == group)
    }
}
