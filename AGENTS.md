# Agent Instructions — great-wiki

Self-hosted collaborative knowledge platform. Rust (Axum) backend + SvelteKit frontend,
SQLite, collaborative rich-text editing, permissioned search/RAG, and a derived knowledge
graph. **Keep this file a thin pointer — a skill's `SKILL.md` is the source of truth.**

## Start at the router

**Read `agent-skills/skills/repository-index/SKILL.md` first** (`~/code/agent-skills/…` on
coding.vm, `../agent-skills/…` where this repo sits beside it). It maps every MCP server and
skill to the trigger that loads it. A skill you do not know about is a skill you will not
load — and nothing tells you that you missed it; the work just quietly comes out worse.

| Skill | Load when |
|---|---|
| `coding-principles` | Any implementation, refactor or bugfix. The always-on floor. |
| `structured-memory` | Every session, at both ends. Recall before editing; persist before finishing. |
| `superpowers:test-driven-development` | Before writing implementation code. TDD is rigid here, not advisory. |
| `superpowers:systematic-debugging` | Any bug, test failure or surprise — before proposing a fix. |
| `superpowers:verification-before-completion` | Before claiming anything works. Evidence, then assertion. |
| `mcp-servers-setup` | A server misbehaves, is unregistered, or you are on a new machine. |
| `html-working-documents` | Output would exceed ~100 lines of markdown, or needs a diagram. Put it in `webpage/`. |
| `homelab-access` | **Before any command touching a VM, the firewall or the NAS** — including `DOCKER_HOST=ssh://`. |

## Memory — Omnigraph

**This repo's graph is `great-wiki`**, pinned by `OMNIGRAPH_GRAPH_ID` in [`.mcp.json`](./.mcp.json).
A bridge serves exactly one graph; no tool takes a graph argument. Never write this repo's
data to the shared `memory` graph — it holds two global `Preference`s and nothing else.

Write **typed** nodes (Decision/Rule/Convention/Component/Task) edged to `Project(great-wiki)`
**and** to the components they touch. A node whose only edge is to the `Project` is
under-linked; a node with *no* `Project` edge renders as "global", which is a bug.

| Var | Unset ⇒ | Get it from |
|---|---|---|
| `OMNIGRAPH_TOKEN` | empty bearer → memory **silently dead** | `agent-skills/infra/mcp-servers/.env.shared` |
| `OMNIGRAPH_NET` | wrong network → `fetch failed` (a network can exist but be empty) | `python3 agent-skills/infra/mcp-servers/scripts/_omni_env.py` |

> **Graph looks empty? Config bug until proven otherwise — do NOT rebuild.**
> `0 rows except 2 Preferences` **is** the `memory` graph, not a wipe. A same-named
> user-scope server silently outranks `.mcp.json`. Diagnose:
> `agent-skills/infra/mcp-servers/omnigraph-setup/setup-agent-memory.sh --check`

**Declared ≠ live.** Verify against the running server (`graphs_list`, `schema_get`), never
by reading a config file. An unapplied schema declaration once left five edge types
nonexistent and every write using them failed *silently*.

## Architecture rules — the ones that are not negotiable

1. **The database is the source of truth.** Markdown is an import/export format, not
   storage. Never add a second write path that bypasses the revision system.
2. **Every retrieval path filters by the caller's permissions at query time, in the
   retriever.** Search, RAG, graph, RSS, API, share links, analytics — all of them. Never a
   post-filter: once content reaches the model or the response body, filtering is too late.
   This corpus holds server runbooks; the failure mode is a signed-in guest asking the
   assistant a question and getting them back.
3. **Fail closed.** Unset secret → refuse to start. Unknown permission → deny. Missing
   visibility → private. A forgotten field must never publish something.
4. **LLM calls go to LiteLLM** (`http://192.168.178.159:4000/v1`, model `deepseek-v4-flash`,
   key `LITELLM_MASTER_KEY`) — never a provider SDK directly, never Ollama as primary.
   Embeddings use `nomic-embed-text` on `cloud.vm:11434` (768-dim).
5. **Storage split:** database, search index and vectors on NVMe (`$APPS_ROOT/great-wiki/`);
   blobs on `/mnt/cloud/great-wiki/media/`. **Never a database or object store on NFS.**
6. **This repository is PUBLIC.** No real credential, no private note, no runbook content
   in a tracked file. Only `.env.example`.

## Verification — run these, do not assume

```bash
cargo test --workspace && cargo clippy --all-targets -- -D warnings && cargo fmt --check
cd web && npm run check && npx vitest run && npm run build
cargo run -p gw-api -- seed --content content-example    # loads content; exits non-zero if any file was skipped
```

Every task ends green on all of the above before it is committed. A task that cannot end
green is not finished — say so rather than moving on.

**If you verified in an isolated worktree, delete its target directory when you are done.**
A separate `CARGO_TARGET_DIR` costs **5–6 GB**, it is not cleaned up by anything, and it does
not live in the repo where anyone would look for it. Three of them plus Docker's build cache
took this machine from 100 GB to **1.4 GB free** on 2026-08-24, and nothing reported it — it
surfaced only because an unrelated 2 GB pull would not fit. The failure that was coming next
is a build dying with an error that says nothing about disk. So:

```bash
rm -rf <your scratchpad>/*target*
git worktree remove <path> --force && git worktree prune
docker builder prune -f        # only if you built an image
```

`crates/target` in the repo is legitimately ~25 GB and must **not** be deleted while anything
is building. `df -h /` before you conclude a strange failure is something cleverer.

## Hard rules

- **Line endings.** `.gitattributes` forces LF on scripts and configs. Never "fix" a script
  by re-saving it as CRLF; a CRLF shebang makes the kernel look for `bash\r`.
- **Commits are small, revertible and state intent.** `CHANGELOG.md` in the same change,
  ADRs in `docs/decisions/NNNN-title.md` for non-obvious choices. Both are rigid, not advisory.

- **The changelog entry ships in the commit that earns it. No exceptions, and no orchestrator
  may grant one.** This rule has been broken more than any other here, always the same way and
  always for the same plausible reason: an orchestrator running several agents tells each of
  them "do not edit `CHANGELOG.md`, you will conflict — put the entry in your report and I
  will merge it". The entries then arrive in a batch days later, written by somebody who no
  longer remembers which change earned which line, or they do not arrive at all. Three
  separate reviews had to raise it before it was paid off, and one entry had gone stale in the
  meantime — it still described a limitation the very work it accompanied had removed.

  So:

  - **If you are implementing, write the entry.** It is part of the change, like a test.
    If an instruction tells you not to, that instruction is wrong; write it anyway and say
    so in your report.
  - **If you are orchestrating, do not issue that instruction.** Run implementers one at a
    time — which you should already be doing, because concurrent agents also collide over
    `Cargo.lock`, `scripts/mutate.sh` and each other's `cargo` builds — and the conflict you
    were avoiding does not exist.
  - **If agents genuinely must run in parallel**, each writes `changelog.d/<short-name>.md`
    instead, and the assembling commit folds them in. A fragment nobody can conflict over is
    the answer; silence is not.

  Write the *effect*, not the diff, and write it for somebody who was not here. `CHANGELOG.md`
  itself says so at the top.
- **Compatibility.** Give each agent its native instruction file; keep adapters short.
  `CLAUDE.md` is the Claude-specific delta only — nothing here is duplicated there.
