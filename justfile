# One definition of every gate.
#
# CI does NOT invoke `just` — the Forgejo runner's image has no toolchain at all and
# adding one more thing to install is one more thing to break. CI therefore MIRRORS
# these command lines rather than calling them, which means they can drift, and have:
# `clippy` here lacked `--workspace` while CI had it, so a warning in another crate
# failed CI and passed locally. Change a gate here and change it in BOTH
# `.forgejo/workflows/ci.yml` and `.github/workflows/ci.yml`.

# nvm is user-local and not on a non-interactive PATH, so every web recipe sources it.
node := "export NVM_DIR=\"$HOME/.nvm\" && . \"$NVM_DIR/nvm.sh\" && nvm use --silent &&"

default:
    @just --list

# Run the API and the web dev server together.
dev:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    cargo run -p gw-api -- serve &
    {{node}} cd web && npm run dev &
    wait

test:
    cargo test --workspace
    {{node}} cd web && npx vitest run

lint:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    {{node}} cd web && npm run check
    ./scripts/scan-secrets.sh --self-test
    ./scripts/scan-secrets.sh scan

# A production build catches what neither the type check nor the tests do: an adapter
# or SSR failure that only appears when the app is actually compiled. CI runs it, so
# this must too.
build:
    {{node}} cd web && npm run build

# The full gate. Every task must end with this passing.
ci: lint test build

# Rebuild the Graphify CODE graph. Seconds, no key, no network.
# --user is REQUIRED: without it the container writes root-owned files that the next
# rebuild cannot overwrite.
graph:
    docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo" -w /repo \
      --entrypoint python graphify-mcp:latest -m graphify update .

# Rebuild the FULL graph including prose. Minutes, and it costs a few cents through
# LiteLLM. Most of this repository is documentation, so the code-only pass above misses
# nearly all of it -- the semantic pass is what turns specs and ADRs into concept nodes
# with typed edges.
#
# Never run two of these at once: they race on graphify-out/. A timed-out foreground
# `docker run` leaves its container alive, so check `docker ps` first.
graph-full:
    #!/usr/bin/env bash
    set -euo pipefail
    key=$(grep -m1 '^LITELLM_MASTER_KEY=' ../Server/secrets-generated/server__cloud__ai__.env | cut -d= -f2-)
    run() {
      docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo" -w /repo \
        -e OPENAI_API_KEY="$key" \
        -e OPENAI_BASE_URL=http://192.168.178.159:4000/v1 \
        -e OPENAI_MODEL=deepseek-v4-flash \
        --entrypoint python graphify-mcp:latest "$@"
    }
    run -m graphify extract . --backend openai --model deepseek-v4-flash
    run -m graphify cluster-only . --backend=openai --model=deepseek-v4-flash
    run -m graphify label . --backend=openai --model=deepseek-v4-flash
