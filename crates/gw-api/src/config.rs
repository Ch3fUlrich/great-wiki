use crate::identity::Identity;
use anyhow::{bail, Context, Result};
use std::net::SocketAddr;
use std::path::PathBuf;

/// The port `omnigraph-viewer` binds on coding.vm. Taking it would break the Omnigraph UI,
/// so it is refused rather than left as a footgun.
const RESERVED_PORT: u16 = 8090;

pub struct Config {
    pub database_url: String,
    pub media_dir: PathBuf,
    pub bind: SocketAddr,
    pub dev_identity: Option<Identity>,
    pub proxy_secret: Option<String>,
}

pub fn parse_dev_identity(raw: &str) -> Result<Identity> {
    let (user, groups) = raw.split_once(':').unwrap_or((raw, ""));
    if user.trim().is_empty() {
        bail!("GW_DEV_IDENTITY must name a user, e.g. `sergej:admins`");
    }
    Ok(Identity {
        user: Some(user.trim().to_string()),
        groups: groups
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(str::to_string)
            .collect(),
    })
}

/// Refuse to start on a configuration that would be an authentication bypass.
///
/// These checks are at startup rather than per request because a misconfiguration that
/// only fails on a request is a misconfiguration that reaches production.
pub fn validate(
    bind: SocketAddr,
    dev_identity: Option<&Identity>,
    proxy_secret: Option<&str>,
) -> Result<()> {
    if bind.port() == RESERVED_PORT {
        bail!("port 8090 is reserved by omnigraph-viewer; choose another (8092 is free)");
    }

    let loopback = bind.ip().is_loopback();

    if dev_identity.is_some() && !loopback {
        bail!(
            "GW_DEV_IDENTITY synthesises a signed-in user and must never be combined with a \
             non-loopback bind ({bind}). Unset it, or bind 127.0.0.1."
        );
    }

    if !loopback && proxy_secret.map(str::trim).unwrap_or("").is_empty() {
        bail!(
            "GW_PROXY_SECRET must be set when binding {bind}. Caddy runs on another host, so \
             this port is LAN-reachable and the shared secret is the only boundary left."
        );
    }

    Ok(())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let bind: SocketAddr = std::env::var("GW_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8092".into())
            .parse()
            .context("GW_BIND must be host:port, e.g. 127.0.0.1:8092")?;

        let dev_identity = match std::env::var("GW_DEV_IDENTITY") {
            Ok(raw) if !raw.trim().is_empty() => Some(parse_dev_identity(&raw)?),
            _ => None,
        };
        let proxy_secret = std::env::var("GW_PROXY_SECRET")
            .ok()
            .filter(|s| !s.trim().is_empty());

        validate(bind, dev_identity.as_ref(), proxy_secret.as_deref())?;

        Ok(Self {
            // Relative defaults: the application runs from a checkout with no arguments.
            // Container paths are supplied by compose in M18.
            database_url: std::env::var("GW_DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://./data/great-wiki.db".into()),
            media_dir: std::env::var("GW_MEDIA_DIR")
                .unwrap_or_else(|_| "./data/media".into())
                .into(),
            bind,
            dev_identity,
            proxy_secret,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{parse_dev_identity, validate};
    use crate::identity::Identity;
    use std::net::SocketAddr;

    #[test]
    fn dev_identity_parses_user_and_groups() {
        let id = parse_dev_identity("sergej:admins,editors").unwrap();
        assert_eq!(id.user.as_deref(), Some("sergej"));
        assert_eq!(id.groups, vec!["admins", "editors"]);
    }

    #[test]
    fn dev_identity_without_groups_is_allowed() {
        let id = parse_dev_identity("guest").unwrap();
        assert_eq!(id.user.as_deref(), Some("guest"));
        assert!(id.groups.is_empty());
    }

    #[test]
    fn dev_identity_with_empty_user_is_rejected() {
        assert!(parse_dev_identity(":admins").is_err());
    }

    #[test]
    fn dev_identity_on_a_loopback_bind_is_allowed() {
        let bind: SocketAddr = "127.0.0.1:8092".parse().unwrap();
        assert!(validate(bind, Some(&Identity::dev("s", &["admins"])), None).is_ok());
    }

    #[test]
    fn dev_identity_on_a_public_bind_refuses_to_start() {
        // This is the whole point: a synthesised identity is an authentication bypass,
        // so it must be impossible to combine with a reachable bind address.
        let bind: SocketAddr = "0.0.0.0:8092".parse().unwrap();
        let err = validate(bind, Some(&Identity::dev("s", &["admins"])), None).unwrap_err();
        assert!(err.to_string().contains("GW_DEV_IDENTITY"));
    }

    #[test]
    fn public_bind_without_a_proxy_secret_refuses_to_start() {
        // Binding 0.0.0.0 is required (Caddy is on another host), so the port is
        // LAN-reachable and proxy attestation is the only boundary left.
        let bind: SocketAddr = "0.0.0.0:8092".parse().unwrap();
        let err = validate(bind, None, None).unwrap_err();
        assert!(err.to_string().contains("GW_PROXY_SECRET"));
    }

    #[test]
    fn public_bind_with_a_proxy_secret_is_allowed() {
        let bind: SocketAddr = "0.0.0.0:8092".parse().unwrap();
        assert!(validate(bind, None, Some("not-a-real-secret")).is_ok());
    }

    #[test]
    fn port_8090_is_refused_because_omnigraph_viewer_owns_it() {
        let bind: SocketAddr = "0.0.0.0:8090".parse().unwrap();
        let err = validate(bind, None, Some("s")).unwrap_err();
        assert!(err.to_string().contains("8090"));
    }
}
