use crate::auth::OidcConfig;
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
    /// `None` means no identity provider is configured, which is a legitimate deployment
    /// — local accounts only. A *partly* configured one is not, and is refused at startup.
    pub oidc: Option<OidcConfig>,
    /// This deployment's own public origin (`GW_PUBLIC_URL`), or `None` when it is not
    /// configured — also a legitimate deployment: `gw_store::links::wiki_path` then treats
    /// every absolute URL as external, exactly as it always has. Handed to
    /// [`gw_store::Store::with_public_origin`] rather than read inside `gw-store` itself,
    /// which must not touch the environment — it is a library — and must never derive this
    /// from a request's `Host` header, which is attacker-controlled (see
    /// `routes::admin::deliver`'s doc comment for the same reasoning applied to invite
    /// links).
    pub public_origin: Option<url::Url>,
}

/// The four variables that make up an OIDC client, all or nothing.
///
/// All four or none. Three out of four is not "OIDC with a default for the fourth", it is
/// a deployment that will redirect somebody to a login it cannot finish — and it would
/// only be discovered by a person trying to sign in.
fn read_oidc() -> Result<Option<OidcConfig>> {
    let read = |name: &str| {
        std::env::var(name)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    oidc_from(
        read("GW_OIDC_ISSUER"),
        read("GW_OIDC_CLIENT_ID"),
        read("GW_OIDC_CLIENT_SECRET"),
        read("GW_OIDC_REDIRECT_URI"),
    )
}

/// The rule, separated from the environment so it can be tested without mutating process
/// globals that every other test in the binary shares.
pub fn oidc_from(
    issuer: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
    redirect_uri: Option<String>,
) -> Result<Option<OidcConfig>> {
    let present: Vec<&str> = [
        ("GW_OIDC_ISSUER", &issuer),
        ("GW_OIDC_CLIENT_ID", &client_id),
        ("GW_OIDC_CLIENT_SECRET", &client_secret),
        ("GW_OIDC_REDIRECT_URI", &redirect_uri),
    ]
    .iter()
    .filter(|(_, value)| value.is_some())
    .map(|(name, _)| *name)
    .collect();

    match (issuer, client_id, client_secret, redirect_uri) {
        (None, None, None, None) => Ok(None),
        (Some(issuer), Some(client_id), Some(client_secret), Some(redirect_uri)) => {
            if !redirect_uri.ends_with("/auth/callback") {
                bail!(
                    "GW_OIDC_REDIRECT_URI must end in /auth/callback (got `{redirect_uri}`) and \
                     must be one of the URIs registered with the provider — a mismatch is \
                     rejected by the provider, not by us."
                );
            }
            Ok(Some(OidcConfig {
                issuer,
                client_id,
                client_secret,
                redirect_uri,
            }))
        }
        _ => bail!(
            "OpenID Connect is half configured: {} set, the rest missing. All four of \
             GW_OIDC_ISSUER, GW_OIDC_CLIENT_ID, GW_OIDC_CLIENT_SECRET and \
             GW_OIDC_REDIRECT_URI are required, or none of them.",
            present.join(", ")
        ),
    }
}

/// The origin this deployment is publicly reachable at, or `None` when `GW_PUBLIC_URL` is
/// unset — separated from the environment for the same reason [`oidc_from`] is: so it can be
/// tested without mutating process globals every other test in this binary shares.
///
/// Unlike the OIDC group this is ONE variable, so there is no half-configured state to
/// refuse — only "set" and parses as an `http` or `https` URL, or "not set". A value that is
/// set but does NOT parse as a URL, or parses to some other scheme (`mailto:`, `file:`,
/// `data:`, …), is refused rather than silently treated as unset: a typo here should be loud
/// at startup, not a feature that is permanently missing and never explains why. The scheme
/// check is not pedantry on top of that — syntax validation alone leaves a real gap open:
/// `mailto:foo@bar.com` parses as a perfectly valid absolute URL, so it would sail through a
/// bare [`url::Url::parse`] and report "configuration OK". It would then never match a
/// single request for the deployment's whole life: its [`url::Url::origin`] is
/// [`url::Origin::Opaque`], which `gw_store::links::internal_path_from_absolute` (a private
/// function, hence no link — same crate boundary this module's own doc comment on
/// `public_origin` describes) can never find equal to a real request's `Tuple` origin. That
/// is exactly the silent, never-explained non-function this function exists to turn into a
/// startup error instead.
pub fn public_origin_from(raw: Option<String>) -> Result<Option<url::Url>> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let origin = url::Url::parse(&raw).with_context(|| {
        format!(
            "GW_PUBLIC_URL must be an absolute URL, e.g. https://wiki.example.com (got `{raw}`)"
        )
    })?;
    if origin.scheme() != "http" && origin.scheme() != "https" {
        bail!(
            "GW_PUBLIC_URL must be an http or https URL, e.g. https://wiki.example.com (got \
             `{raw}`, scheme `{}`) — no other scheme can be a wiki's origin, and one that \
             parses but never matches would fail silently instead of at startup.",
            origin.scheme()
        );
    }
    Ok(Some(origin))
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
            oidc: read_oidc()?,
            public_origin: public_origin_from(
                std::env::var("GW_PUBLIC_URL")
                    .ok()
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty()),
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{oidc_from, parse_dev_identity, public_origin_from, validate};
    use crate::identity::Identity;
    use std::net::SocketAddr;

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn no_oidc_variables_at_all_means_local_accounts_only() {
        assert!(oidc_from(None, None, None, None).unwrap().is_none());
    }

    #[test]
    fn a_complete_oidc_configuration_is_accepted() {
        let config = oidc_from(
            some("https://auth.ohje.ooguy.com"),
            some("great-wiki"),
            some("nicht-das-echte-geheimnis"),
            some("https://wiki.ohje.ooguy.com/auth/callback"),
        )
        .unwrap()
        .expect("all four present");
        assert_eq!(config.client_id, "great-wiki");
    }

    #[test]
    fn a_half_configured_provider_refuses_to_start() {
        // Otherwise the first person to click "sign in" discovers it, in production.
        let error = oidc_from(
            some("https://auth.ohje.ooguy.com"),
            some("great-wiki"),
            None,
            some("https://wiki.ohje.ooguy.com/auth/callback"),
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("half configured"), "{message}");
        assert!(
            message.contains("GW_OIDC_ISSUER") && message.contains("GW_OIDC_CLIENT_ID"),
            "the message must name what IS set, so the gap is obvious: {message}"
        );
    }

    #[test]
    fn a_redirect_uri_that_is_not_the_callback_path_refuses_to_start() {
        // The provider matches the redirect URI exactly. Getting it wrong here produces an
        // opaque `invalid_request` from Authelia at sign-in time instead of a message here.
        let error = oidc_from(
            some("https://auth.ohje.ooguy.com"),
            some("great-wiki"),
            some("nicht-das-echte-geheimnis"),
            some("https://wiki.ohje.ooguy.com/"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("/auth/callback"), "{error}");
    }

    #[test]
    fn unset_public_url_means_no_configured_origin() {
        // The safety property: `gw_store::Store::with_public_origin(None)` is what a
        // deployment with no `GW_PUBLIC_URL` gets, and that must behave exactly as it did
        // before this variable existed.
        assert!(public_origin_from(None).unwrap().is_none());
    }

    #[test]
    fn a_valid_public_url_is_parsed() {
        let origin = public_origin_from(some("https://wiki.ohje.ooguy.com"))
            .unwrap()
            .expect("a valid URL must parse");
        assert_eq!(origin.scheme(), "https");
        assert_eq!(origin.host_str(), Some("wiki.ohje.ooguy.com"));
    }

    #[test]
    fn an_unparseable_public_url_refuses_to_start() {
        // Fail loud rather than silently falling back to "not configured": a typo here
        // should be a startup error, not a feature that quietly never works.
        let error = public_origin_from(some("not a url")).unwrap_err();
        assert!(error.to_string().contains("GW_PUBLIC_URL"), "{error}");
    }

    #[test]
    fn a_public_url_with_a_non_http_scheme_refuses_to_start() {
        // Each of these is a syntactically valid absolute URL — `Url::parse` alone accepts
        // every one — but none of them can be a wiki's origin. Left unchecked, one pasted
        // by typo would report "configuration OK" and then never match a single request:
        // its `url::Origin` is `Opaque`, which can never equal the `Tuple` origin a real
        // request carries, so the deployment would behave as unconfigured, silently,
        // forever — exactly the failure mode this function exists to turn into a startup
        // error instead.
        for raw in [
            "mailto:foo@bar.com",
            "file:///etc/passwd",
            "data:text/plain,hello",
        ] {
            let error = public_origin_from(some(raw)).unwrap_err();
            assert!(
                error.to_string().contains("GW_PUBLIC_URL"),
                "{raw}: {error}"
            );
        }
    }

    #[test]
    fn http_and_https_public_urls_both_still_pass() {
        // The scheme check above must narrow to exactly these two, not further.
        for raw in ["https://wiki.ohje.ooguy.com", "http://wiki.ohje.ooguy.com"] {
            let origin = public_origin_from(some(raw))
                .unwrap()
                .unwrap_or_else(|| panic!("{raw} must still be accepted"));
            assert_eq!(origin.scheme(), &raw[..raw.find(':').unwrap()]);
        }
    }

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
