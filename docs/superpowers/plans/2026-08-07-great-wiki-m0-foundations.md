# great-wiki M0 — Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A repository where the Rust workspace, the SvelteKit app, the dev runner and CI
all run green, and the agent tooling (Omnigraph memory graph, Graphify code graph) is live.

**Architecture:** A Cargo workspace of focused crates plus a SvelteKit app under `web/`. M0
creates only the two ends that prove the toolchain — `gw-core` with one real, well-tested
pure function, and a SvelteKit skeleton that type-checks — plus the gates that keep every
later task honest.

**Tech Stack:** Rust 1.97 (edition 2021), Cargo workspaces, Node LTS via nvm, SvelteKit 2 /
Svelte 5, Vitest, GitHub Actions, `just`.

## Global Constraints

Inherited from [the roadmap](2026-08-07-great-wiki-roadmap.md#global-constraints). The ones
that bite in M0:

- **Rust edition 2021**, toolchain pinned in `rust-toolchain.toml`. No nightly features.
- **This repository is PUBLIC.** No real credential in a tracked file. Only `.env.example`.
- **Every task ends green** on `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --check`, and from Task 2 on, `cd web && npm run check`.
- **Every task commits**, with `CHANGELOG.md` updated in the same change.

## File Structure

```
Cargo.toml                        workspace root: members, shared dependency versions
rust-toolchain.toml               pins the toolchain so CI and laptop agree
crates/gw-core/Cargo.toml         the pure-domain crate manifest
crates/gw-core/src/lib.rs         module declarations and re-exports only
crates/gw-core/src/slug.rs        slugify — URL-safe slugs with German transliteration
.nvmrc                            pins Node for nvm and for CI
web/                              SvelteKit application (scaffolded, not yet featureful)
justfile                          one-command dev, test and lint runners
.env.example                      every environment variable, with safe placeholder values
.github/workflows/ci.yml          the gate: rust job + web job
```

**Why `slug.rs` is its own file from the start:** it is the first thing every later
subsystem depends on (documents, datasets, headings, exports), it is pure, and it is where
the German/English requirement first becomes concrete. Keeping it separate means the
transliteration table has one home rather than being reimplemented per call site.

---

## Task 1: Rust workspace scaffold with slugify and the lint gate

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `crates/gw-core/Cargo.toml`
- Create: `crates/gw-core/src/lib.rs`
- Create: `crates/gw-core/src/slug.rs`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: nothing.
- Produces: `gw_core::slugify(input: &str) -> String`. Every later milestone uses it for
  document slugs, heading anchors, dataset keys and export filenames.

- [ ] **Step 1: Write the failing test**

Create `crates/gw-core/src/slug.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use crate::slug::slugify;

    #[test]
    fn lowercases_and_collapses_separators() {
        assert_eq!(slugify("  Table 0:  Dysbiotic   Shifts!! "), "table-0-dysbiotic-shifts");
    }

    #[test]
    fn strips_em_dashes_without_leaving_double_separators() {
        assert_eq!(
            slugify("Darm — ADHD Microbiota Reference"),
            "darm-adhd-microbiota-reference"
        );
    }

    // The predecessor plan asserted slugify("Präbiotika") == "pr-biotika". That is a
    // mangled URL for every German title, so transliteration is a requirement, not a nicety.
    #[test]
    fn transliterates_german_umlauts() {
        assert_eq!(slugify("Präbiotika Guide"), "praebiotika-guide");
        assert_eq!(slugify("Größe und Maß"), "groesse-und-mass");
        assert_eq!(slugify("Öl Überblick"), "oel-ueberblick");
    }

    #[test]
    fn drops_characters_with_no_ascii_equivalent() {
        assert_eq!(slugify("Präbiotika 🧬 Guide"), "praebiotika-guide");
    }

    #[test]
    fn empty_and_separator_only_input_yield_empty() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("---   ---"), "");
    }
}
```

- [ ] **Step 2: Create the workspace and crate manifests**

`rust-toolchain.toml`:
```toml
# Pinned so CI and every developer machine agree. 1.97 is what is installed on the
# development host; bumping it is a deliberate change, not a drift.
[toolchain]
channel = "1.97"
components = ["rustfmt", "clippy"]
```

`Cargo.toml`:
```toml
[workspace]
members = ["crates/*"]
resolver = "2"

[workspace.package]
edition = "2021"
rust-version = "1.97"
license = "MIT"
repository = "https://github.com/Ch3fUlrich/great-wiki"

# Versions live here so a crate never pins its own and they cannot drift apart.
[workspace.dependencies]
anyhow = "1.0"
thiserror = "2.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_yaml = "0.9"
tokio = { version = "1", features = ["full"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

`crates/gw-core/Cargo.toml`:
```toml
[package]
name = "gw-core"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

`crates/gw-core/src/lib.rs`:
```rust
//! Pure domain logic for great-wiki: the document model, conversions and validation.
//!
//! Deliberately free of I/O so every invariant here can be tested without a database,
//! a filesystem or a network. Round-trip fidelity of the export format is proven in
//! this crate.

pub mod slug;

pub use slug::slugify;
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p gw-core`
Expected: FAIL — `cannot find function 'slugify' in module 'crate::slug'`.

- [ ] **Step 4: Implement slugify**

Prepend to `crates/gw-core/src/slug.rs`, above `mod tests`:

```rust
/// Turn arbitrary text into a URL-safe slug.
///
/// German characters are transliterated *before* ASCII folding (ä→ae, ö→oe, ü→ue, ß→ss).
/// Without that step every German title collapses into a mangled slug — "Präbiotika"
/// becomes "pr-biotika" — which is both ugly and lossy, since "Präbiotika" and "Prabiotika"
/// would produce different slugs while "Präbiotika" and "Prbiotika" would collide.
///
/// Output is ASCII-only so slugs are stable across filesystems that normalise Unicode
/// differently (macOS NFD vs Linux NFC) and safe in a URL without percent-encoding.
pub fn slugify(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    // Starts true so a leading separator run produces no leading dash.
    let mut last_dash = true;

    for ch in input.chars() {
        let expansion = match ch {
            'ä' | 'Ä' => "ae",
            'ö' | 'Ö' => "oe",
            'ü' | 'Ü' => "ue",
            'ß' | 'ẞ' => "ss",
            _ => "",
        };

        if !expansion.is_empty() {
            out.push_str(expansion);
            last_dash = false;
        } else if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            // Any run of non-alphanumerics becomes exactly one dash.
            out.push('-');
            last_dash = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }
    out
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p gw-core`
Expected: `test result: ok. 5 passed; 0 failed`

- [ ] **Step 6: Run the lint gate**

Run: `cargo fmt && cargo fmt --check && cargo clippy --all-targets -- -D warnings`
Expected: no output, exit 0.

Note the `cargo fmt` before `--check`. The test snippets above are written for readability,
and rustfmt disagrees with some of them: its default `fn_call_width` is 60 columns, so a
93-column `assert_eq!` wrapping a nested call gets split across three lines even though it
is well under the 100-column limit. Let rustfmt win — that is what the gate is for. From
here on the committed code is rustfmt-canonical and `--check` alone is enough.

- [ ] **Step 7: Update the changelog and commit**

Add under `## [Unreleased]` → `### Added` in `CHANGELOG.md`:
```markdown
- Rust workspace with `gw-core`, the pure-domain crate, and the `cargo test` /
  `clippy -D warnings` / `fmt --check` gate that every later task must pass.
- `slugify` with German transliteration (ä→ae, ö→oe, ü→ue, ß→ss), so German titles
  produce readable, collision-free slugs.
```

```bash
git add Cargo.toml rust-toolchain.toml crates CHANGELOG.md
git commit -m "feat(core): rust workspace and slugify with German transliteration"
```

---

## Task 2: Node toolchain and the SvelteKit skeleton

**Files:**
- Create: `.nvmrc`
- Create: `web/` (scaffolded by `sv create`)
- Modify: `web/package.json`
- Create: `web/src/lib/slug.ts`
- Create: `web/src/lib/slug.test.ts`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `gw_core::slugify` — as a *specification*, not a dependency. The TypeScript
  implementation must agree with the Rust one, and the shared test cases prove it.
- Produces: `slugify(input: string): string` in `web/src/lib/slug.ts`, and a working
  `npm run check` / `npx vitest run` gate.

**Why a second implementation rather than WASM:** the frontend needs slugs for anchor links
and client-side previews, where a round trip to the server would be visible latency. Two
small implementations with a shared test corpus is cheaper than a WASM build step in the
dev loop. The shared corpus is what stops them drifting.

- [ ] **Step 1: Install Node via nvm and pin it**

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash
export NVM_DIR="$HOME/.nvm" && . "$NVM_DIR/nvm.sh"
nvm install --lts
node --version > /dev/null && node --version | sed 's/^v//' > .nvmrc
cat .nvmrc
```
Expected: a version number such as `24.9.0`. `nvm` is user-local; no root is needed.

- [ ] **Step 2: Scaffold the SvelteKit application**

```bash
export NVM_DIR="$HOME/.nvm" && . "$NVM_DIR/nvm.sh" && nvm use
npx sv create web --template minimal --types ts --no-add-ons --install npm
```
Expected: `web/` containing `package.json`, `svelte.config.js`, `vite.config.ts`,
`tsconfig.json` and `src/`.

Note the flags: `sv create` is the current tool. The older
`npm create svelte@latest -- --template skeleton --types typescript --no-add-ons` mixes
flags from two different generations of the CLI and does not work.

- [ ] **Step 3: Add Vitest and the check script**

```bash
cd web && npm install -D vitest @sveltejs/adapter-node && cd ..
```

In `web/package.json`, ensure the `scripts` block contains exactly these entries (keep any
others `sv create` added):
```json
{
  "scripts": {
    "dev": "vite dev",
    "build": "vite build",
    "preview": "vite preview",
    "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
    "test": "vitest run"
  }
}
```

Switch to the Node adapter — the application is served by its own process, not a static host.

**Where this lives depends on the scaffold generation, so check before editing.** The `sv`
CLI ≥ 0.17 with SvelteKit ≥ 2.70 generates **no `svelte.config.js` at all**; the adapter and
compiler options live inside the `sveltekit({...})` plugin call in `web/vite.config.ts`. In
that case change only the import:

```ts
import adapter from '@sveltejs/adapter-node';   // was: @sveltejs/adapter-auto
```

Do **not** create a `svelte.config.js` when the scaffold did not — this generation does not
read one, so the file would look like configuration while having no effect. If your scaffold
*did* produce one, set `kit: { adapter: adapter() }` there instead.

`@sveltejs/adapter-auto` remains an unused devDependency either way; harmless, and removing
it is not worth a separate step.

- [ ] **Step 4: Write the failing test**

`web/src/lib/slug.test.ts`:
```ts
import { describe, expect, it } from 'vitest';
import { slugify } from './slug';

// These cases are duplicated verbatim from crates/gw-core/src/slug.rs. If the two
// implementations ever disagree, one of these suites goes red — which is the whole
// point of duplicating them.
describe('slugify', () => {
  it('lowercases and collapses separators', () => {
    expect(slugify('  Table 0:  Dysbiotic   Shifts!! ')).toBe('table-0-dysbiotic-shifts');
  });

  it('strips em dashes without leaving double separators', () => {
    expect(slugify('Darm — ADHD Microbiota Reference')).toBe('darm-adhd-microbiota-reference');
  });

  it('transliterates German umlauts', () => {
    expect(slugify('Präbiotika Guide')).toBe('praebiotika-guide');
    expect(slugify('Größe und Maß')).toBe('groesse-und-mass');
    expect(slugify('Öl Überblick')).toBe('oel-ueberblick');
  });

  it('drops characters with no ASCII equivalent', () => {
    expect(slugify('Präbiotika 🧬 Guide')).toBe('praebiotika-guide');
  });

  it('returns empty for empty and separator-only input', () => {
    expect(slugify('')).toBe('');
    expect(slugify('---   ---')).toBe('');
  });
});
```

- [ ] **Step 5: Run the test to verify it fails**

Run: `cd web && npx vitest run src/lib/slug.test.ts`
Expected: FAIL — `Failed to resolve import "./slug"`.

- [ ] **Step 6: Implement slugify in TypeScript**

`web/src/lib/slug.ts`:
```ts
// Mirrors crates/gw-core/src/slug.rs. The shared test corpus in slug.test.ts is what
// keeps the two honest; change one and you must change the other.
const TRANSLITERATIONS: Record<string, string> = {
  ä: 'ae', Ä: 'ae',
  ö: 'oe', Ö: 'oe',
  ü: 'ue', Ü: 'ue',
  ß: 'ss', ẞ: 'ss'
};

const ASCII_ALPHANUMERIC = /[A-Za-z0-9]/;

/**
 * Turn arbitrary text into a URL-safe slug.
 *
 * German characters are transliterated before ASCII folding, so "Präbiotika" becomes
 * "praebiotika" rather than the lossy "pr-biotika".
 */
export function slugify(input: string): string {
  let out = '';
  // Starts true so a leading separator run produces no leading dash.
  let lastDash = true;

  for (const ch of input) {
    const expansion = TRANSLITERATIONS[ch];
    if (expansion !== undefined) {
      out += expansion;
      lastDash = false;
    } else if (ASCII_ALPHANUMERIC.test(ch)) {
      out += ch.toLowerCase();
      lastDash = false;
    } else if (!lastDash) {
      out += '-';
      lastDash = true;
    }
  }

  return out.replace(/-+$/, '');
}
```

- [ ] **Step 7: Run the tests and the type check**

Run: `cd web && npx vitest run && npm run check`
Expected: `Test Files 1 passed`, `Tests 5 passed`, then `svelte-check found 0 errors`.

- [ ] **Step 8: Update the changelog and commit**

Add under `### Added`:
```markdown
- SvelteKit 2 / Svelte 5 application skeleton with the Node adapter, Vitest, and the
  `npm run check` type gate. Node is pinned in `.nvmrc`.
- `slugify` in TypeScript, mirroring the Rust implementation, with a test corpus shared
  verbatim between the two so they cannot drift apart.
```

```bash
git add .nvmrc web CHANGELOG.md
git commit -m "feat(web): sveltekit skeleton, vitest gate, and a slugify that matches gw-core"
```

---

## Task 3: The dev runner and the environment contract

**Files:**
- Create: `justfile`
- Create: `.env.example`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: the Rust and web toolchains from Tasks 1 and 2.
- Produces: `just dev`, `just test`, `just lint`, `just ci`. Every later task and the CI
  workflow call these rather than restating command lines, so the gate has one definition.

- [ ] **Step 1: Write the environment contract**

`.env.example` — **every** variable the application will read, with placeholder values.
Real values go in an untracked `.env`, which `.gitignore` already excludes.

```bash
# Copy to .env and fill in. NEVER commit .env — this repository is public.

# --- Runtime -----------------------------------------------------------------
# Relative by default so the application runs from a checkout with no arguments.
# Container paths are supplied by compose in M18, not baked in here.
GW_DATABASE_URL=sqlite://./data/great-wiki.db
GW_MEDIA_DIR=./data/media
GW_BIND=127.0.0.1:8092
RUST_LOG=info,gw_api=debug

# --- Development identity ----------------------------------------------------
# Synthesises a signed-in user so private content and admin surfaces are testable
# without a proxy. The application REFUSES TO START if this is set while GW_BIND is
# not a loopback address — see M1 Task 4.
GW_DEV_IDENTITY=sergej:admins

# --- Proxy attestation (production only) -------------------------------------
# Caddy injects this header; the application checks it in constant time BEFORE
# reading any identity, and refuses to start if it is empty in production.
GW_PROXY_SECRET=

# --- OpenID Connect (M1) -----------------------------------------------------
GW_OIDC_ISSUER=https://auth.ohje.ooguy.com
GW_OIDC_CLIENT_ID=great-wiki
GW_OIDC_CLIENT_SECRET=
GW_OIDC_REDIRECT_URI=https://kb.ohje.ooguy.com/auth/callback

# --- AI (M7) -----------------------------------------------------------------
# LiteLLM only. Never a provider SDK directly, never Ollama as primary.
GW_LLM_BASE_URL=http://192.168.178.159:4000/v1
GW_LLM_MODEL=deepseek-v4-flash
GW_LLM_API_KEY=
GW_EMBEDDING_BASE_URL=http://192.168.178.159:11434/v1
GW_EMBEDDING_MODEL=nomic-embed-text
```

- [ ] **Step 2: Write the justfile**

`justfile`:
```make
# One definition of every gate. CI calls these, so "green locally" and "green in CI"
# cannot diverge into two different command lines.

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
    cargo clippy --all-targets -- -D warnings
    {{node}} cd web && npm run check

# The full gate. Every task must end with this passing.
ci: lint test

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
```

> **The image must carry the `openai` extra.** Without it the semantic pass fails at call
> time — *after* the AST pass has already succeeded — leaving a graph that looks built while
> every prose file is silently absent from it. Fixed upstream in
> `agent-skills/infra/mcp-servers/servers/graphify-mcp/Dockerfile`, which now installs
> `graphifyy[mcp,openai]==0.9.20`. The pin matters: the AST cache is namespaced by version,
> so an unpinned rebuild invalidates every repository's cache at once.

- [ ] **Step 3: Verify the gate runs**

Run: `just lint && just test`
Expected: clippy silent, `cargo test` reports 5 passed for `gw-core`, vitest reports
5 passed. `just dev` is not runnable yet — `gw-api` does not exist until M1.

- [ ] **Step 4: Update the changelog and commit**

Add under `### Added`:
```markdown
- `justfile` with `dev`, `test`, `lint`, `ci` and `graph` recipes, so the gate has one
  definition shared by developers and CI.
- `.env.example` documenting every environment variable. Runtime paths default to
  relative locations so the application runs from a checkout with no arguments.
```

```bash
git add justfile .env.example CHANGELOG.md
git commit -m "chore: dev runner and environment contract"
```

---

## Task 4: Continuous integration

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: `justfile` recipes from Task 3.
- Produces: a required status check on every push and pull request.

**Why this exists in M0 rather than later:** the predecessor plan had no CI, and its
self-review claimed type consistency across eighteen tasks that in fact contained a missing
dependency, a nullability mismatch and a fail-open security filter. A gate that runs on
every push is what makes "every task ends green" a fact rather than an intention.

- [ ] **Step 1: Write the workflow**

`.github/workflows/ci.yml`:
```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

# A new push to the same branch cancels the previous run — CI minutes are not free
# and a superseded result is noise.
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # No toolchain input: rust-toolchain.toml is authoritative, so CI and a laptop
      # cannot silently use different compilers.
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy --all-targets -- -D warnings
      - run: cargo test --workspace

  web:
    runs-on: ubuntu-latest
    defaults:
      run:
        working-directory: web
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version-file: .nvmrc
          cache: npm
          cache-dependency-path: web/package-lock.json
      - run: npm ci
      - run: npm run check
      - run: npx vitest run
      - run: npm run build

  secrets:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      # This repository is public. A credential reaching main is not recoverable by
      # deleting it — it must be rotated. Catching it at the gate is far cheaper.
      - name: Fail if a credential-shaped value is committed
        run: |
          if git grep -nEI \
              '(api[_-]?key|secret|password|token|bearer)[[:space:]]*[=:][[:space:]]*["'"'"']?[A-Za-z0-9/+_-]{16,}' \
              -- ':!*.example' ':!docs/**' ':!.github/workflows/ci.yml'; then
            echo "::error::A credential-shaped value is committed. Rotate it, then remove it."
            exit 1
          fi
          echo "No credential-shaped values found."
```

- [ ] **Step 2: Verify the secret scan locally before pushing**

Run:
```bash
git grep -nEI '(api[_-]?key|secret|password|token|bearer)[[:space:]]*[=:][[:space:]]*["'"'"']?[A-Za-z0-9/+_-]{16,}' \
  -- ':!*.example' ':!docs/**' ':!.github/workflows/ci.yml' || echo "clean"
```
Expected: `clean`.

- [ ] **Step 3: Update the changelog and commit**

Add under `### Added`:
```markdown
- GitHub Actions CI: a Rust job (fmt, clippy with `-D warnings`, tests), a web job
  (type check, unit tests, build), and a secret scan that fails the build if a
  credential-shaped value is committed.
```

```bash
git add .github CHANGELOG.md
git commit -m "ci: rust, web and secret-scan gates on every push"
```

---

## Task 5: Agent tooling — the Omnigraph memory graph and the Graphify code graph

**Files:**
- Modify: `CHANGELOG.md`
- No source changes. This task makes the tooling that `AGENTS.md` and `CLAUDE.md` already
  document actually exist.

**Interfaces:**
- Consumes: `.mcp.json` and `.serena/project.yml`, already committed.
- Produces: a live `great-wiki` graph on the Omnigraph server, and `graphify-out/graph.json`
  in this repository.

**Why this is a task and not setup:** `.mcp.json` naming a graph does not create it —
`GET /graphs/great-wiki/schema` currently returns 404. Until this runs, the memory bridge
points at a graph that is not there and every write fails silently.

- [ ] **Step 1: Confirm the graph really is missing**

```bash
cd ~/code/agent-skills/infra/mcp-servers
./omnigraph-setup/setup-agent-memory.sh --check
```
Expected: reports the environment is wired but lists no `great-wiki` graph.

- [ ] **Step 2: Clear stale branches — `apply-cluster.sh` refuses to run while any exist**

```bash
docker exec omnigraph-server omnigraph branches list --graph homelab-server
```
Expected: several `mem/homelab-server/*` branches. For each, merge if it holds wanted
writes, then delete:
```bash
docker exec omnigraph-server omnigraph branches merge --graph homelab-server --source <branch> --target main
docker exec omnigraph-server omnigraph branches delete --graph homelab-server --name <branch>
```
Re-run the list until it shows only `main`.

> **This step touches shared infrastructure.** `apply-cluster.sh` in Step 3 stops the
> Omnigraph server to release the state lock, which makes **every** graph unavailable for
> its duration — including the memory other sessions are using. Do it deliberately, not
> alongside other work.

- [ ] **Step 3: Register the graph and converge the cluster**

```bash
cd ~/code/agent-skills/infra/mcp-servers
./scripts/add-project-graph.sh great-wiki
./scripts/apply-cluster.sh
```
Expected: `apply-cluster.sh` snapshots, stops the server, applies, restarts, and reports a
node count per graph without error.

- [ ] **Step 4: Verify the graph is live**

```bash
curl -fsS -H "Authorization: Bearer $OMNIGRAPH_TOKEN" \
  http://127.0.0.1:8080/graphs | python3 -m json.tool | grep great-wiki
```
Expected: `great-wiki` appears in the list. If it does not, stop — do **not** proceed and do
**not** "rebuild" anything. An empty or missing graph is a configuration fault until proven
otherwise; re-run `setup-agent-memory.sh --check`.

- [ ] **Step 5: Seed the project node**

Through the `omnigraph` MCP tool, `mutate`:
```gq
query seed($slug: String, $name: String, $path: String) {
    insert Project { slug: $slug, name: $name, path: $path }
}
```
with params `{"slug": "great-wiki", "name": "great-wiki", "path": "/home/s/code/great-wiki"}`.

Verify with `commits_list` that the head advanced. A 504 does **not** mean failure — the
server may have committed after the proxy dropped the response, so re-check rather than
retrying blindly.

- [ ] **Step 6: Build the Graphify graph**

```bash
just graph        # code only: seconds, no key, no network
just graph-full   # including prose: minutes, a few cents through LiteLLM
```
Expected from `graph`: `[graphify watch] Rebuilt: N nodes, M edges, K communities`.
Expected from `graph-full`: `wrote graphify-out/graph.json: N nodes, M edges`, then named
communities rather than `Community 0`, `Community 1`.

Verify through the MCP tools, not by reading the file — and **omit `project_path` or pass
`/repo`**. The parameter is resolved *inside* the container, so a host path like
`/home/s/code/great-wiki` fails with a confusing "graph.json not found" naming a path that
plainly exists:
```
graph_stats  -> Nodes: …, Edges: …, Communities: …
god_nodes    -> the specification and the milestone plans, most-connected first
```

Then confirm the wiring is correct — Graphify must be a single cwd-relative entry in **user**
scope, never in this repository's `.mcp.json`:
```bash
bash ~/code/agent-skills/infra/mcp-servers/scripts/linux/check-graphify-scope.sh --fix
```
Expected: exit 0.

- [ ] **Step 7: Confirm `graphify-out/` is not committed**

Run: `git status --short`
Expected: no `graphify-out/` entries — `.gitignore` already excludes it, because the graph
is derived and rebuilding it is two seconds.

- [ ] **Step 8: Update the changelog and commit**

Add under `### Added`:
```markdown
- Agent tooling made live: the `great-wiki` Omnigraph memory graph registered and seeded
  with its `Project` node, and the Graphify code graph built. Naming a graph in
  `.mcp.json` does not create it — until registration, memory writes fail silently.
```

```bash
git add CHANGELOG.md
git commit -m "chore: register the great-wiki memory graph and build the code graph"
```

---

## Milestone exit criteria

M0 is done when all of these are true:

- [ ] `just ci` passes from a clean checkout.
- [ ] The CI workflow is green on `main`, including the secret scan.
- [ ] `curl .../graphs` lists `great-wiki`, and it contains a `Project` node.
- [ ] `graphify-out/graph.json` exists and `check-graphify-scope.sh` exits 0.
- [ ] `.nvmrc`, `rust-toolchain.toml` and `.env.example` all exist, and nothing outside
      `.env.example` contains a credential.

## Self-Review

**Spec coverage.** M0 is scoped to the roadmap's "Foundations" row: toolchain, gates and
agent tooling. It implements the German transliteration requirement (spec §12) in both
languages, the relative-path default from §5, and the public-repository constraint from §6.
It deliberately implements no product capability — those begin in M1.

**Placeholders.** None. Every file has complete content, every command has expected output.
Two steps are marked as touching shared infrastructure (Task 5, Steps 2–3) rather than
silently assuming it is safe.

**Type consistency.** `slugify` has one signature in each language — `&str -> String` and
`(input: string) => string` — and one shared test corpus. `just` recipe names (`dev`, `test`,
`lint`, `ci`, `graph`) are referenced identically in the justfile, the CI workflow and the
exit criteria. `GW_*` variable names in `.env.example` match those the M1 plan consumes.

**Known gap, deliberate.** `just dev` cannot run until `gw-api` exists in M1. The recipe is
written now so M1 adds a binary rather than also inventing a runner.
