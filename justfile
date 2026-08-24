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

# The throwaway database `behaviour` and `behaviour-fixture` provision. Never
# `data/great-wiki.db` — see `behaviour-fixture`'s own comment for why — and named once
# here so the two recipes that touch it cannot drift apart on the path.
behaviour_db := "data/behaviour-fixture.db"
behaviour_media := "data/behaviour-fixture-media"

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

# The fixture `behaviour` tests against: a throwaway SQLite database, seeded fresh from
# `content-example` and granted write access, every time this runs.
#
# NEVER `data/great-wiki.db`. That file is gitignored working state a developer may have
# put real effort into, and destructively reseeding it on every `just behaviour` would be
# exactly the kind of silent mutation this recipe must not perform without consent — so it
# gets its own file instead ({{behaviour_db}}), deleted and recreated from scratch on every
# run rather than reused, which is what makes "same content, same grants, same result,
# every run" true instead of merely intended: a stale row left over from a previous run
# (E7 types into this database's one editable page) can never leak into the next one.
#
# `seed` runs with no `--as`: the operator path, which is also the only one that may create
# TOP-LEVEL pages at all (see `refuse_create` in gw-api/src/seed.rs). A freshly seeded wiki
# then has ZERO rows in `acl` — `seed` grants nothing and no migration inserts any — so
# without the write grants below, EVERY editing check in behaviour.mjs would 403 and Group E
# would prove nothing while still reporting "ok" for having correctly detected a refusal.
#
# The grant's subject is `group:editors`, deliberately NOT `group:admins` — and `behaviour`
# below runs the server with `GW_DEV_IDENTITY=sergej:editors` to match, NOT the `sergej:admins`
# .env.example documents for a developer's own use. `editors` has no row in `group_roles`, so
# it confers the D-M2-1 baseline of `Baseline::Public` — the SAME default reach a genuinely
# anonymous visitor has. `admins` maps to `Baseline::Admin`, which `Store`'s `permits()`
# widens to read EVERY `restricted` document regardless of any grant (see `gw-store/src/acl.rs`)
# — found the hard way, by running this exact recipe with `sergej:admins` and watching D5 see
# `/rundgang/nur-intern` (restricted on purpose, to prove the permission filter works) anyway.
# Write is still unaffected by any of this: an ACL grant is consulted before baseline, on its
# own, for any subject that matches — so `group:editors` gets exactly the write access these
# checks need at exactly the reach a real editor would have, no more.
#
# Granting the wiki's two top-level pages covers every descendant that has no grants of its
# own — a grant is inherited down the WHOLE subtree, unconditionally (nearest ancestor with
# ANY row wins outright — see `Store::grants_for_path`; grants are never unioned, which is
# what lets a closer, narrower grant override a broader one, but also means a broad grant
# reaches every descendant that has not been narrowed). That includes
# `/rundgang/nur-intern`, which content-example seeds with no `visibility:` specifically to
# be `restricted` by default — found the hard way, watching D5 see it in an anonymous
# visitor's subpage list anyway once `/rundgang` had a grant. The fourth grant below
# narrows it back: `group:admins` is deliberately a DIFFERENT group than the
# `group:editors` `GW_DEV_IDENTITY` below arrives with, so `/rundgang/nur-intern` gets its
# own (non-matching) grant row and never falls through to the broader one, and stays exactly
# as invisible to this fixture's identity as it is to a real anonymous reader.
#
# A sibling recipe rather than inlined into `behaviour`, so it is independently useful: run
# it on its own, then point a hand-started server at the same three variables it prints, to
# poke at the fixture interactively without the Playwright container at all.
behaviour-fixture:
    #!/usr/bin/env bash
    set -euo pipefail
    rm -f "{{behaviour_db}}" "{{behaviour_db}}-shm" "{{behaviour_db}}-wal"
    export GW_DATABASE_URL="sqlite://$PWD/{{behaviour_db}}"
    export GW_MEDIA_DIR="$PWD/{{behaviour_media}}"
    cargo run -q -p gw-api -- seed --content content-example
    cargo run -q -p gw-api -- grant --path /rundgang --subject group:editors \
      --permission write --actor behaviour-fixture
    cargo run -q -p gw-api -- grant --path /verweisbeispiel --subject group:editors \
      --permission write --actor behaviour-fixture
    cargo run -q -p gw-api -- grant --path /rundgang/nur-intern --subject group:admins \
      --permission read --actor behaviour-fixture
    echo "behaviour fixture ready:"
    echo "  GW_DATABASE_URL=$GW_DATABASE_URL"
    echo "  GW_MEDIA_DIR=$GW_MEDIA_DIR"
    echo "  GW_DEV_IDENTITY=sergej:editors"

# Deterministic Playwright behaviour checks: real assertions (focus, colour, table markup,
# dialog visibility), not screenshots — exits 0 only if every check passes.
#
# Fully self-contained: provisions its own fixture ({{behaviour_db}}, via `behaviour-fixture`
# above) and starts its own backend and web dev server against it, rather than trusting
# whatever `just dev` a developer may already have pointed at their own database. That
# used to be the whole bug — the harness passed or failed depending on what state a
# developer's local database happened to be in, and a check that could not even run (no
# grant to edit with) silently reported "ok" instead of failing loudly. A fresh clone with
# nothing else running gets the same fixture, the same grants and the same result every
# time this recipe runs, with no README step to forget.
#
# NOT part of `ci`, deliberately: wiring a Docker-in-CI dependency into the Forgejo runner's
# bare image is a separate decision the owner has not made. The container is required for
# the same reason shots.mjs needs it — Playwright's own Chromium cannot start on this host
# because libnspr4.so is missing and installing it needs root.
#
# The image is ~2 GB and is NOT pulled automatically. That is deliberate: this box has run
# out of disk mid-task more than once, and a recipe that silently starts a 2 GB pull turns
# "run the checks" into a failed build somewhere else entirely. The preflight below says
# exactly what to run instead of failing with docker's own message, which names the image
# but not the fix — an agent hit precisely this and worked around it by hand-symlinking a
# different image's browser, which is not something to leave as folklore.
behaviour: behaviour-fixture
    #!/usr/bin/env bash
    set -euo pipefail
    image=mcr.microsoft.com/playwright:v1.56.0-noble
    if ! docker image inspect "$image" >/dev/null 2>&1; then
      echo "The behaviour harness needs $image, which is not pulled here." >&2
      echo "It is about 2 GB. Free space first ($(df -h / | awk 'NR==2{print $4}') available), then:" >&2
      echo "    docker pull $image" >&2
      exit 1
    fi

    export GW_DATABASE_URL="sqlite://$PWD/{{behaviour_db}}"
    export GW_MEDIA_DIR="$PWD/{{behaviour_media}}"
    export GW_DEV_IDENTITY="sergej:editors"
    export GW_BIND="127.0.0.1:8092"
    base="${SHOT_BASE:-http://127.0.0.1:5173}"

    # This recipe owns the dev stack for the run rather than sharing whatever is already on
    # these ports: the fixture above is only meaningful if the server under test is the one
    # actually pointed at it, and there is no way to ask an already-running process which
    # database it opened. Refuse loudly rather than test against a server that might be
    # serving a developer's own data/great-wiki.db under a passing-looking green run.
    if curl -sS -o /dev/null -m 2 "http://127.0.0.1:8092/"; then
      echo "Something is already answering on 127.0.0.1:8092 (likely \`just dev\` against" >&2
      echo "your own data/great-wiki.db). This recipe starts its own backend against the" >&2
      echo "behaviour fixture and cannot tell that one apart from a real dev server, so it" >&2
      echo "will not share the port. Stop the other one first, then re-run \`just behaviour\`." >&2
      exit 1
    fi
    if curl -sS -o /dev/null -m 2 "$base"; then
      echo "Something is already answering on $base for the same reason. Stop it first." >&2
      exit 1
    fi

    # `setsid` gives each process its own process group, so `kill -- -$pid` below can stop
    # BOTH it and whatever it forked (`cargo run` execs a child process for the built
    # binary; a plain `kill $pid` on the `cargo` pid alone leaves that child running) without
    # also signalling this script or `just` itself the way `kill 0` (as `just dev` uses,
    # where the whole point is to run until Ctrl-C) would — this script has real work left
    # to do (the docker run below) after starting these two, and needs its own exit code,
    # not a SIGTERM, to reach `just`.
    setsid cargo run -p gw-api -- serve &
    api_pid=$!
    setsid bash -c '{{node}} cd web && npm run dev' &
    web_pid=$!
    cleanup() {
      kill -- "-$api_pid" "-$web_pid" >/dev/null 2>&1 || true
      wait "$api_pid" "$web_pid" 2>/dev/null || true
    }
    trap cleanup EXIT

    echo "waiting for $base ..."
    up=""
    for _ in $(seq 1 60); do
      if curl -fsS -o /dev/null -m 2 "$base/"; then
        up=1
        break
      fi
      sleep 1
    done
    if [ -z "$up" ]; then
      echo "the fixture dev stack never answered 200 on $base — check the output above" >&2
      exit 1
    fi

    docker run --rm --network host --user "$(id -u):$(id -g)" \
      -v "$PWD/web/scripts:/scripts:ro" -e HOME=/tmp \
      -e SHOT_BASE="$base" -e PLAYWRIGHT_BROWSERS_PATH=/ms-playwright \
      "$image" \
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

# The full gate, from a git worktree, WITHOUT duplicating the target directory.
#
# A worktree that builds into its own target costs 5-6 GB and nothing cleans it up. Three of
# them, an incremental cache and Docker's layers took this machine from 100 GB to 1.4 GB free
# on 2026-08-24, and nothing reported it — it surfaced because an unrelated image pull would
# not fit, and the next symptom would have been a build failing with an error that says
# nothing about disk.
#
# `--git-common-dir` is what makes this portable: from a worktree it resolves to the MAIN
# checkout's `.git`, so its parent is the one target directory every worktree should share.
# No path is hardcoded and nothing needs configuring per machine.
#
# Cargo locks that directory, so two agents cannot build at once. That is a feature here —
# they serialise instead of duplicating, which is already the rule `scripts/mutate.sh`
# enforces. The cost: a worktree at a DIFFERENT commit invalidates artifacts the other just
# built, so keep worktrees at or near HEAD. If you truly need an old commit, take the 6 GB
# deliberately and delete it afterwards.
#
# Incremental is off because an agent runs a handful of full builds and never benefits from
# it. It stays ON for people — that is what makes an interactive edit-rebuild loop fast — which
# is why this lives here and not in a committed `.cargo/config.toml`.
#
# The full gate from a worktree, sharing the main checkout's target directory.
agent-ci:
    #!/usr/bin/env bash
    set -euo pipefail
    main="$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")"
    export CARGO_TARGET_DIR="$main/target"
    export CARGO_INCREMENTAL=0
    echo "building into $CARGO_TARGET_DIR (shared; incremental off)"
    just lint
    just test
    just build

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
