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

# Deterministic Playwright behaviour checks against a running dev server: real assertions
# (focus, colour, table markup, dialog visibility), not screenshots — exits 0 only if
# every check passes. Requires `just dev` (or equivalent) already serving on SHOT_BASE.
#
# NOT part of `ci`, deliberately: it needs a running dev server, and wiring that into CI
# is a separate decision the owner has not made. The container is required for the same
# reason shots.mjs needs it — Playwright's own Chromium cannot start on this host because
# libnspr4.so is missing and installing it needs root.
behaviour:
    docker run --rm --network host --user "$(id -u):$(id -g)" \
      -v "$PWD/web/scripts:/scripts:ro" -e HOME=/tmp \
      -e SHOT_BASE=http://127.0.0.1:5173 -e PLAYWRIGHT_BROWSERS_PATH=/ms-playwright \
      mcr.microsoft.com/playwright:v1.56.0-noble \
      sh -c 'mkdir -p /tmp/pw && cd /tmp/pw && npm init -y >/dev/null && npm i playwright@1.56.0 >/dev/null && cp /scripts/behaviour.mjs . && node behaviour.mjs'

# Break the security-critical code on purpose and check the tests notice.
#
# About a minute for the whole set, which it was not: it had grown past ten minutes and
# timed out rather than finishing, and a gate too slow to run is a gate that stops being
# run. Run it by hand after touching gw-auth or gw-store — that is where a passing suite
# has three times now failed to notice a defence being removed entirely.
#
# NOTHING ELSE MAY TOUCH THE REPOSITORY WHILE THIS RUNS. It rewrites source files in place
# and restores them; a concurrent `cargo fmt` can write its copy back afterwards and
# reinstate a mutation. The script refuses to start while another cargo is running and
# checks every file it touched at the end, but the cheap answer is to run it alone.
#
# Still NOT part of `ci`: a minute per push is a minute, and the runner's image has no
# toolchain at all.
mutate:
    ./scripts/mutate.sh

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
