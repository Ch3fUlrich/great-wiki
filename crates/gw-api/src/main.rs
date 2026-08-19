// The modules live in the library (`src/lib.rs`) and are used from there rather than
// re-declared here: declaring them again would compile a second, incompatible copy, so
// the integration tests would exercise a different `Identity` than the binary runs.
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use gw_api::config;
use gw_auth::password::HashingCost;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "great-wiki",
    about = "Self-hosted collaborative knowledge platform"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the HTTP server.
    Serve,
    /// Validate configuration and exit. Non-zero on any problem.
    Check,
    /// Load a directory of markdown files into the database.
    ///
    /// Nothing is invented: a file with no title, or whose parent document does not
    /// exist, is skipped and named. Exits non-zero if anything was skipped, so a
    /// half-loaded corpus cannot pass for a loaded one in a script.
    ///
    /// Nothing is destroyed either. A page that already exists is an error unless
    /// `--update` says otherwise, and a page in the wiki that no file claims is reported
    /// and left exactly where it is — this command has no delete.
    Seed {
        /// Directory of `.md` files with YAML frontmatter.
        #[arg(long)]
        content: PathBuf,
        /// Run as this account, subject to its permissions: creating a page needs write on
        /// its parent, updating one needs write on the page, and pages the account cannot
        /// read stay invisible to the report.
        #[arg(long = "as", value_name = "USERNAME")]
        identity: Option<String>,
        /// Allow a file to replace the body of a page that already exists.
        ///
        /// Off by default: a slug collision is an error, not an overwrite. When on, the
        /// change is appended as a revision authored by `--as`, the previous body stays in
        /// the history, and the page's title, type, visibility, language and ordering are
        /// still refused — those move a page or change who can see it, and a file drop
        /// does not get to do either.
        #[arg(long, requires = "identity")]
        update: bool,
    },
    /// Write the page tree out as a directory of markdown files.
    ///
    /// The mirror of `seed`: folders are the page tree, frontmatter is the metadata, and
    /// `export` then `seed` into an empty wiki reproduces what was exported. Every
    /// document is re-imported and compared before its file is written; one that would
    /// come back different is NOT written, is named, and fails the run.
    Export {
        /// Directory to write into. Must be empty, or a previous export.
        #[arg(long)]
        content: PathBuf,
        /// Export as this account. Required, and not a formality: reading the tree is
        /// permission-filtered, so pages this account cannot read are simply not in the
        /// export.
        #[arg(long = "as", value_name = "USERNAME")]
        identity: String,
    },
}

/// Resolve `--as` to the principal the store holds for it.
///
/// Deliberately the same lookup a sign-in does, and deliberately not a constructed
/// `Principal`: an identity assembled in this process would carry whatever groups the
/// command line claimed, which is not an identity, it is an assertion.
async fn identity(store: &gw_store::Store, username: &str) -> Result<gw_auth::Principal> {
    let Some((principal, _)) = store.principal_by_username(username).await? else {
        bail!("no account named `{username}` — create it in the admin console first");
    };
    if !principal.active {
        bail!("the account `{username}` is deactivated");
    }
    Ok(principal)
}

/// Refuse to answer a sign-in with anything but Authelia's parameters.
///
/// The cheap cost exists so the test suite does not spend a minute a run on argon2. It is
/// reachable only through `AppState::for_test*`, and it is not a build-time switch — there
/// is deliberately no Cargo feature, because features unify across a build graph and a
/// mistake in some other crate's dependency list would silently reach this binary with
/// nothing in the diff to show for it.
///
/// This is the backstop for the one remaining way it could arrive here: somebody editing
/// this file, or assigning to the public `hashing` field. Both are visible in a diff; this
/// makes them non-shippable as well, because the process exits rather than serving a login
/// form whose hashes cost eight kilobytes to compute.
fn refuse_weak_hashing(cost: HashingCost) -> Result<()> {
    if cost != HashingCost::PRODUCTION {
        bail!(
            "refusing to serve: passwords would be hashed at test parameters, which makes \
             every account here cheaper to attack than the same account on Authelia"
        );
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = config::Config::from_env()?;

    match cli.command {
        Command::Check => {
            println!(
                "configuration OK — bind {}, db {}",
                cfg.bind, cfg.database_url
            );
            Ok(())
        }
        Command::Seed {
            content,
            identity: username,
            update,
        } => {
            let store = gw_store::Store::open(&cfg.database_url)
                .await?
                .with_public_origin(cfg.public_origin.clone());
            let principal = match &username {
                Some(username) => Some(identity(&store, username).await?),
                None => None,
            };
            let report = gw_api::seed::run_as(
                &store,
                &content,
                gw_api::seed::Options {
                    principal: principal.as_ref(),
                    update,
                },
            )
            .await?;
            println!("seeding from {}", content.display());
            println!("{report}");
            if report.is_complete() {
                Ok(())
            } else {
                // Non-zero so `just seed && just serve` cannot start against a corpus that
                // is missing pages nobody noticed scrolling past.
                bail!(
                    "{} file(s) skipped — none of them were guessed at; fix them and run again",
                    report.skipped.len()
                )
            }
        }
        Command::Export {
            content,
            identity: username,
        } => {
            let store = gw_store::Store::open(&cfg.database_url)
                .await?
                .with_public_origin(cfg.public_origin.clone());
            let principal = identity(&store, &username).await?;
            let report = gw_api::export::run(&store, &principal, &content).await?;
            println!("exporting to {}", content.display());
            println!("{report}");
            if report.is_complete() {
                Ok(())
            } else {
                // Non-zero for the same reason `seed` is: a directory that is quietly
                // missing a page is worse than no directory at all, because it looks
                // finished.
                bail!(
                    "{} document(s) were NOT written — the export is incomplete and must not \
                     be treated as a backup",
                    report.refused.len()
                )
            }
        }
        Command::Serve => {
            let store = std::sync::Arc::new(
                gw_store::Store::open(&cfg.database_url)
                    .await?
                    .with_public_origin(cfg.public_origin.clone()),
            );
            let oidc = match cfg.oidc.clone() {
                Some(config) => {
                    tracing::info!(issuer = %config.issuer, client_id = %config.client_id,
                        "OpenID Connect login enabled");
                    Some(std::sync::Arc::new(gw_api::OidcClient::new(config)?))
                }
                None => {
                    tracing::warn!(
                        "no OpenID Connect provider configured — /auth/login will answer 503 \
                         and only local accounts can sign in"
                    );
                    None
                }
            };
            // The real corpus. If it cannot even be constructed the process still starts:
            // an unreachable corpus is already a handled, audited state (D-M2-16), and
            // refusing to boot over it would make an outside service able to stop this one.
            let corpus: std::sync::Arc<dyn gw_auth::breach::BreachRange> =
                match gw_api::auth::breach::HibpCorpus::new() {
                    Ok(corpus) => std::sync::Arc::new(corpus),
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "could not build the breach-corpus client; passwords will be \
                             accepted on length alone and each occurrence audited"
                        );
                        std::sync::Arc::new(gw_auth::breach::UnavailableCorpus)
                    }
                };
            // `serving` takes no hashing cost and offers none: a server hashes at
            // Authelia's parameters. The check below is what makes that true of the
            // *process* rather than only of this line, since `AppState`'s fields are
            // public and a future edit could set one directly.
            let state = gw_api::AppState::serving(
                store,
                cfg.dev_identity.clone(),
                gw_api::ProxyGuard::from_config(&cfg),
                oidc,
                corpus,
            );
            refuse_weak_hashing(state.hashing)?;
            let app = gw_api::build_router(state).layer(
                tower_http::limit::RequestBodyLimitLayer::new(2 * 1024 * 1024),
            );
            let listener = tokio::net::TcpListener::bind(cfg.bind).await?;
            tracing::info!(bind = %cfg.bind, "great-wiki listening");
            axum::serve(listener, app).await?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::refuse_weak_hashing;
    use gw_auth::password::HashingCost;

    #[test]
    fn a_server_may_only_start_at_authelia_s_parameters() {
        assert!(refuse_weak_hashing(HashingCost::PRODUCTION).is_ok());
        assert!(
            refuse_weak_hashing(HashingCost::CHEAP_FOR_TESTS).is_err(),
            "the cheap test cost reached a serving process and nothing stopped it"
        );
    }
}
