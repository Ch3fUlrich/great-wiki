// The modules live in the library (`src/lib.rs`) and are used from there rather than
// re-declared here: declaring them again would compile a second, incompatible copy, so
// the integration tests would exercise a different `Identity` than the binary runs.
use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use gw_api::config;
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
    Seed {
        /// Directory of `.md` files with YAML frontmatter.
        #[arg(long)]
        content: PathBuf,
    },
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
        Command::Seed { content } => {
            let store = gw_store::Store::open(&cfg.database_url).await?;
            let report = gw_api::seed::run(&store, &content).await?;
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
        Command::Serve => {
            let store = std::sync::Arc::new(gw_store::Store::open(&cfg.database_url).await?);
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
            let state = gw_api::AppState {
                store,
                dev_identity: cfg.dev_identity.clone(),
                proxy_guard: gw_api::ProxyGuard::from_config(&cfg),
                oidc,
                // The real corpus. If it cannot even be constructed the process still
                // starts: an unreachable corpus is already a handled, audited state
                // (D-M2-16), and refusing to boot over it would make an outside service
                // able to stop this one.
                corpus: match gw_api::auth::breach::HibpCorpus::new() {
                    Ok(corpus) => std::sync::Arc::new(corpus),
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "could not build the breach-corpus client; passwords will be \
                             accepted on length alone and each occurrence audited"
                        );
                        std::sync::Arc::new(gw_auth::breach::UnavailableCorpus)
                    }
                },
            };
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
