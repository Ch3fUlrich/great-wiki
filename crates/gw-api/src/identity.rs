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
    pub fn dev(user: &str, groups: &[&str]) -> Self {
        Self {
            user: Some(user.to_string()),
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }
}

// `anonymous()`, `is_authenticated()` and `in_group()` are gone with the visibility stub
// that used them. They were a second way to ask an authorisation question, and a second
// way is how the two answers eventually disagree: `Principal::is_authenticated` and
// `gw_auth::can` are now the only ones. There is no anonymous *shim* either — the absence
// of a shim is spelled `None`, and it resolves to `Principal::anonymous()`.
