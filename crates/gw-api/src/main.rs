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
    /// Remove stored files that no page references any more.
    ///
    /// The second act after `endgültig löschen`. A purge destroys a page's attachment
    /// entries and reports `blobs_orphaned` — the stored files nothing points at any
    /// more — but it deliberately leaves the bytes on the media mount, because an
    /// `unlink` is not in the database transaction and no ordering of the two has a
    /// worst case better than a live page losing its file. See
    /// `docs/decisions/0013-what-a-purge-leaves-on-the-mount.md`.
    ///
    /// This is what takes them. It previews by default and destroys only with
    /// `--commit`, because a wiki that holds medical documents needs the operation that
    /// forgets them to be one somebody meant.
    ///
    /// A command rather than a button or a timer: it is instance-wide, so there is no
    /// page whose permissions could authorise it, and it holds the store's only
    /// connection for the whole of its transaction, so every other request waits behind
    /// it. If it should be periodic, something else calls it on a schedule — never host
    /// cron.
    Reclaim {
        /// Actually delete the files. Without it, nothing is destroyed and nothing is
        /// recorded: the report says what a `--commit` run would take.
        #[arg(long)]
        commit: bool,
        /// Recorded as `audit_log.principal_id`, exactly as `grant --actor` is, and for
        /// the same reason: a destruction with no actor is not a record.
        #[arg(long, default_value = "cli-reclaim")]
        actor: String,
    },
    /// Add an ACL grant on a path, from the command line.
    ///
    /// Exists for the case `seed --as` cannot reach: bootstrapping the FIRST grant on a
    /// fresh wiki, where no account yet holds enough permission to open the admin console
    /// at all — a freshly seeded wiki has zero rows in `acl`, so nobody can write anything
    /// until one of these runs. It calls the same `Store::add_grant_audited` the admin API
    /// route does, so this is not a second, weaker path to the same state: it writes to
    /// `audit_log` exactly as a grant made in the browser would, under `--actor` rather
    /// than a signed-in person's id.
    Grant {
        /// The path the grant applies to. A grant is inherited by every descendant that
        /// has no grants of its own (nearest ancestor wins; see
        /// `gw_store::Store::grants_for_path`) — granting on a top-level page therefore
        /// covers its whole subtree in one row.
        #[arg(long)]
        path: String,
        /// Who the grant is for: `principal:<username-or-id>`, `team:<slug>`,
        /// `group:<name>` (an Authelia group, matched against the verified `groups`
        /// claim), `anyone`, or `authenticated`.
        #[arg(long)]
        subject: String,
        /// What the grant confers: `read`, `comment`, `write` or `admin`. Each implies
        /// every weaker permission (`gw_auth::Permission::satisfies`).
        #[arg(long)]
        permission: String,
        /// Recorded as `audit_log.principal_id`. Not a real account — this command exists
        /// precisely because no account can be relied on to hold one yet — so it defaults
        /// to a value that reads as tooling, not as a person, in the audit trail.
        #[arg(long, default_value = "cli-grant")]
        actor: String,
    },
}

/// Parse `--subject`. `principal:<id>` matches by id OR username (see `gw_auth::can`), so
/// a username works here without a store lookup — deliberately: this command runs before
/// a wiki necessarily has any grants at all, and a lookup that could itself fail for "no
/// grant yet" reasons would be a strange way to bootstrap the first one.
fn parse_subject(raw: &str) -> Result<gw_auth::Subject> {
    if raw == "anyone" {
        return Ok(gw_auth::Subject::Anyone);
    }
    if raw == "authenticated" {
        return Ok(gw_auth::Subject::Authenticated);
    }
    let Some((kind, id)) = raw.split_once(':') else {
        bail!(
            "--subject `{raw}` is not understood — use `principal:<name>`, `team:<slug>`, \
             `group:<name>`, `anyone` or `authenticated`"
        );
    };
    if id.trim().is_empty() {
        bail!("--subject `{raw}` names no id after the `:`");
    }
    match kind {
        "principal" => Ok(gw_auth::Subject::Principal(id.to_string())),
        "team" => Ok(gw_auth::Subject::Team(id.to_string())),
        "group" => Ok(gw_auth::Subject::Group(id.to_string())),
        other => bail!(
            "--subject `{raw}` names an unknown kind `{other}` — use `principal`, `team` \
             or `group`, or the bare words `anyone` / `authenticated`"
        ),
    }
}

fn parse_permission(raw: &str) -> Result<gw_auth::Permission> {
    match raw {
        "read" => Ok(gw_auth::Permission::Read),
        "comment" => Ok(gw_auth::Permission::Comment),
        "write" => Ok(gw_auth::Permission::Write),
        "admin" => Ok(gw_auth::Permission::Admin),
        other => bail!("--permission `{other}` is not one of read, comment, write, admin"),
    }
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
                "configuration OK — bind {}, db {}, media {}",
                cfg.bind,
                cfg.database_url,
                cfg.media_dir.display()
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
        Command::Reclaim { commit, actor } => {
            let store = gw_store::Store::open(&cfg.database_url)
                .await?
                .with_public_origin(cfg.public_origin.clone());
            // Opened rather than assumed, for the reason `serve` opens it before binding a
            // listener: a sweep pointed at a media directory that is not there would
            // report having reclaimed files it never touched.
            let blobs = gw_store::BlobStore::open(&cfg.media_dir)?;
            let mode = if commit {
                gw_store::Reclaim::Commit
            } else {
                gw_store::Reclaim::Preview
            };
            let report = store.reclaim_blobs(&blobs, &actor, mode).await?;
            println!("media {}", cfg.media_dir.display());
            println!("{report}");
            if !commit && report.blobs > 0 {
                println!(
                    "nothing was deleted — run again with --commit to take them off the mount"
                );
            }
            Ok(())
        }
        Command::Grant {
            path,
            subject,
            permission,
            actor,
        } => {
            let store = gw_store::Store::open(&cfg.database_url)
                .await?
                .with_public_origin(cfg.public_origin.clone());
            let subject = parse_subject(&subject)?;
            let permission = parse_permission(&permission)?;
            let changed = store
                .add_grant_audited(&actor, &path, &subject, permission)
                .await?;
            if changed {
                println!("granted {permission:?} on {path} to {subject:?}");
            } else {
                // Idempotent success (see `gw_store::admin`'s module doc): the exact same
                // grant already existed, so running this twice is safe in a script.
                println!("{path} already grants {permission:?} to {subject:?} — nothing to do");
            }
            Ok(())
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
            // The media mount, opened BEFORE a listener is bound: a server that starts
            // without a usable one would accept an upload and lose it (AGENTS.md rule 3).
            // It is a directory rather than a connection, so this both creates it and
            // proves it is writable.
            let blobs = std::sync::Arc::new(gw_store::BlobStore::open(&cfg.media_dir)?);
            tracing::info!(media = %cfg.media_dir.display(), "media directory ready");
            // `serving` takes no hashing cost and offers none: a server hashes at
            // Authelia's parameters. The check below is what makes that true of the
            // *process* rather than only of this line, since `AppState`'s fields are
            // public and a future edit could set one directly.
            let state = gw_api::AppState::serving(
                store,
                blobs,
                cfg.dev_identity.clone(),
                gw_api::ProxyGuard::from_config(&cfg),
                oidc,
                corpus,
            );
            refuse_weak_hashing(state.hashing)?;
            // The request-body limits live in `build_router` — one for the ordinary routes
            // and a larger one for attachments (D-17). They used to be one layer here, which
            // meant no test ever saw them and no route inside the crate could be excepted
            // from them; see `gw_api::routes::REQUEST_BODY_LIMIT`.
            let app = gw_api::build_router(state);
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
