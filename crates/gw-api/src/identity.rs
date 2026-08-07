use serde::Serialize;

/// What `GW_DEV_IDENTITY` says: a username and the Authelia groups to arrive with.
///
/// **Not an authorisation input.** M1 let handlers decide from this directly; M2 does not.
/// It is configuration for the development shim and nothing else — a name to look up. The
/// principal a request actually runs as is `gw_auth::Principal`, read from the store by
/// [`crate::routes::AppState::principal`], so the shim goes through the same permission
/// engine as a real sign-in instead of standing in for one.
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
