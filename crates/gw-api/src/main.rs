// The modules live in the library (`src/lib.rs`) and are used from there rather than
// re-declared here: declaring them again would compile a second, incompatible copy, so
// the integration tests would exercise a different `Identity` than the binary runs.
use anyhow::Result;
use clap::{Parser, Subcommand};
use gw_api::config;

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
        Command::Serve => {
            let store = std::sync::Arc::new(gw_store::Store::open(&cfg.database_url).await?);
            let state = gw_api::AppState {
                store,
                dev_identity: cfg.dev_identity.clone(),
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
