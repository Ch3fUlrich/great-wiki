# syntax=docker/dockerfile:1
#
# ===========================================================================
#  gw-api — the Rust API (crate `gw-api`, binary `great-wiki`, subcommand
#  `serve`). Built from the repository root:
#
#      docker build -f docker/gw-api.Dockerfile -t gw-api:dev .
#
#  ⚠️  THE BINARY IS CALLED `great-wiki`, NOT `gw-api`. `gw-api` is the crate;
#      `crates/gw-api/Cargo.toml` names the binary `great-wiki` and `main.rs`
#      names the command the same. `cargo build --bin gw-api` fails.
#
#  Two stages. The runtime carries the binary, a CA bundle and curl, and
#  nothing that could produce a binary — no cargo, no rustc, no gcc, no
#  libc6-dev.
# ===========================================================================

# Debian 13. The build and the runtime MUST share a suite: the binary links the
# builder's glibc, and running it against an older one fails at exec with an
# error that names a symbol rather than the cause.
ARG DEBIAN_SUITE=trixie
# Kept in step with rust-toolchain.toml, and the builder asserts it below rather
# than trusting this line.
ARG RUST_VERSION=1.97

# ---------------------------------------------------------------------------
#  Builder
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-slim-${DEBIAN_SUITE} AS builder

WORKDIR /src

# The pin is `rust-toolchain.toml`; this checks the base image agrees with it.
# The file is deliberately NOT left in the working directory: rustup reads it on
# every cargo invocation and would go and fetch the `rustfmt` and `clippy`
# components it lists, which this build has no use for. So the version is
# checked and the file is dropped.
COPY rust-toolchain.toml /tmp/rust-toolchain.toml
RUN set -eu; \
    want="$(sed -n 's/^ *channel *= *"\(.*\)".*/\1/p' /tmp/rust-toolchain.toml)"; \
    have="$(rustc --version | cut -d' ' -f2)"; \
    case "$have" in \
      "$want" | "$want".*) echo "rustc $have satisfies rust-toolchain.toml channel $want" ;; \
      *) echo "rust-toolchain.toml pins $want, base image has $have — bump ARG RUST_VERSION" >&2; exit 1 ;; \
    esac; \
    rm /tmp/rust-toolchain.toml

# --- Dependency layer ------------------------------------------------------
# Manifests first, with empty sources standing in for the real ones, so the ~200
# crates in the tree compile into a layer keyed on `Cargo.lock` alone. Editing a
# `.rs` file below then rebuilds four crates instead of all of them.
#
# Every workspace member is listed by hand rather than globbed, because
# `COPY crates/*/Cargo.toml` flattens the paths into one directory and cargo then
# cannot find the members at all.
#
# A NEW CRATE HAS TO BE ADDED HERE, IN BOTH PLACES — the COPY list and the loop
# below. This comment used to say that forgetting one made the dependency layer
# miss it and the real build compile it, "slower, never wrong". That was wrong,
# and `gw-collab` proved it: the workspace is `members = ["crates/*"]`, so cargo
# resolves every member from `Cargo.toml` and fails outright on the one whose
# manifest is absent —
#
#     failed to load manifest for workspace member `/src/crates/gw-api`
#       failed to read `/src/crates/gw-collab/Cargo.toml`
#
# It is a hard failure at image build time, not a slow path. That is the better
# of the two behaviours, but only if the comment says so.
COPY Cargo.toml Cargo.lock ./
COPY crates/gw-api/Cargo.toml    crates/gw-api/Cargo.toml
COPY crates/gw-auth/Cargo.toml   crates/gw-auth/Cargo.toml
COPY crates/gw-collab/Cargo.toml crates/gw-collab/Cargo.toml
COPY crates/gw-core/Cargo.toml   crates/gw-core/Cargo.toml
COPY crates/gw-store/Cargo.toml  crates/gw-store/Cargo.toml

# The stand-in artefacts are removed afterwards, fingerprints included. Without
# that, cargo considers the real crates already built and the image ships four
# empty libraries and an empty `main` — a container that starts, exits 0, and
# looks like a healthy deploy.
RUN set -eu; \
    for crate in gw-api gw-auth gw-collab gw-core gw-store; do \
      mkdir -p "crates/$crate/src"; \
      : > "crates/$crate/src/lib.rs"; \
    done; \
    printf 'fn main() {}\n' > crates/gw-api/src/main.rs; \
    cargo build --release --locked -p gw-api --bin great-wiki; \
    rm -rf crates \
           target/release/.fingerprint/gw-* \
           target/release/deps/gw_* \
           target/release/deps/great_wiki* \
           target/release/great-wiki

# --- The real build --------------------------------------------------------
# `crates/gw-store/migrations` arrives with this COPY and is required:
# `Store::open` runs `sqlx::migrate!("./migrations")`, which embeds the directory
# at COMPILE time. A build without it fails here rather than at first start.
COPY crates crates
RUN cargo build --release --locked -p gw-api --bin great-wiki

# ---------------------------------------------------------------------------
#  Runtime
# ---------------------------------------------------------------------------
FROM debian:${DEBIAN_SUITE}-slim AS runtime

# curl is here for the container healthcheck and nothing else. `/api/health` sits
# INSIDE the proxy-attestation layer like every other route — `build_router`
# applies the guard last, so it wraps even the 404 fallback — so the probe has to
# present `X-GW-Proxy`. The compose healthcheck reads the value out of this
# container's own environment, the one place it is legitimately already known.
#
# ca-certificates is not needed for OIDC: reqwest is built with `webpki-roots`,
# so the trust store is compiled into the binary. It is here so that a future
# outbound TLS call does not fail in a way that looks like a network fault.
#
# UID 1000 matches the owner of $APPS_ROOT on cloud.vm. A container writing to a
# bind mount is only as useful as its ability to write to it, and the
# alternative — chowning the host directory to a container-invented UID — is a
# step nobody remembers on the second host.
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates curl; \
    rm -rf /var/lib/apt/lists/*; \
    groupadd --gid 1000 gw; \
    useradd --uid 1000 --gid 1000 --shell /usr/sbin/nologin --no-create-home --home-dir /app gw; \
    mkdir -p /app /data /media; \
    chown gw:gw /app /data /media

COPY --from=builder /src/target/release/great-wiki /usr/local/bin/great-wiki

USER 1000:1000
WORKDIR /app

# Documentation only. Publishing is the compose file's business, and only the
# internal Caddy publishes anything.
EXPOSE 8092

# No `GW_*` defaults here, on purpose. `config.rs` already carries the ones a
# checkout needs, and the DEPLOYED values belong in the compose file, which is
# the authoritative spec and the thing a human reads. Two places to set `GW_BIND`
# is two places for it to be wrong.
#
# The default that remains is the safe one: an unset `GW_BIND` is `127.0.0.1:8092`,
# which inside a container is reachable by nothing.
ENTRYPOINT ["/usr/local/bin/great-wiki"]
CMD ["serve"]
