# Session handoff — 2026-08-08

Written for the next Claude Code session. Read this, then
[`AGENTS.md`](../../AGENTS.md), then the milestone plan you are working on.

## Where the project actually is

**Live and working:** <https://wiki-dev.ohje.ooguy.com> — public pages readable with no
login, sign-in through Authelia OIDC, restricted content gated by the real permission
engine. **231 Rust tests, 31 web tests**, all gates green, everything pushed to
`github.com/Ch3fUlrich/great-wiki`.

| Milestone | State |
|---|---|
| **M0** Foundations | Complete |
| **M1** Vertical slice | Complete |
| **M2** Identity & access | Tasks 1–4 and 6 done. **Remaining: Task 5 (admin console), Task 7 (invites), Task 8 (view-as)** |
| **M3+** | Planned, not started |

## Start the dev servers

Nothing runs automatically. Two tmux windows:

```bash
tmux new-session -d -s gw -n api -c /home/s/code/great-wiki \
  /tmp/claude-1000/-home-s-code-great-wiki/*/scratchpad/run-api.sh
tmux new-window -t gw -n web -c /home/s/code/great-wiki/web \
  'export NVM_DIR=$HOME/.nvm && . $NVM_DIR/nvm.sh && npm run dev'
```

`run-api.sh` exports the OIDC settings and reads `GW_OIDC_CLIENT_SECRET` from
`../Server/secrets-generated/server__coding__great-wiki__.env`. **That script lives in a
session scratchpad and will not survive** — recreate it, or run the API with those four
`GW_OIDC_*` variables set. Without them `/auth/login` fails and only public content is
reachable.

Node is not on a non-interactive `PATH`; source nvm first in every shell.

## Verification — run all of it, every time

```bash
cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cd web && npm run check && npx vitest run
```

**Screenshots are part of the loop, not a final check.** Three real defects shipped past
green tests and 200 responses — a title duplicated on every page, tables rendering as flat
paragraphs, and prose hugging the left edge of a too-wide column. None was visible to any
check that was passing.

```bash
docker run --rm --network host --user "$(id -u):$(id -g)" \
  -v /home/s/code/great-wiki/web/scripts:/scripts:ro -v /tmp/shots:/out -e HOME=/tmp \
  -e SHOT_OUT=/out -e SHOT_BASE=http://127.0.0.1:5173 -e PLAYWRIGHT_BROWSERS_PATH=/ms-playwright \
  mcr.microsoft.com/playwright:v1.56.0-noble \
  sh -c 'mkdir -p /tmp/pw && cd /tmp/pw && npm init -y >/dev/null && npm i playwright@1.56.0 >/dev/null && cp /scripts/shots.mjs . && node shots.mjs'
```

The container is required. Playwright's own Chromium cannot start on this host —
`libnspr4.so` is missing and installing it needs root.

## What has repeatedly gone wrong

**Sub-agents claim completion without finishing.** Three of roughly fifteen did: one
implemented nothing, one waited for a notification that never came, one left a task
half-done. **Verify every agent's output against the repository, never against its
summary.** Every task here ended with the orchestrator running the gates personally.

**Mutation-test anything security-critical.** It found two genuinely vacuous tests —
`a_mismatched_state_is_refused` passed because the flow died at a *later* check, so
disabling the CSRF defence entirely failed no test; and a forged-session test whose store
held no principals, so any token would have failed. Both looked fine. Nothing but a
mutation exposes that.

**Do not `git add -A` while agents are running.** It swept an agent's in-flight work into
an unrelated commit. Stage explicit paths.

**Substituting one placeholder in a file containing several is silently destructive.** Any
deploy of `10-services.conf` must assert that *zero* value-carrying placeholders remain,
not that its own got replaced. That assertion caught a third secret and prevented breaking
a service.

## Owner decisions already settled — do not re-litigate

Recorded in the specification (§2) and ADRs 0001–0005. In short: database is the source of
truth with Markdown as import/export; OIDC rather than proxy headers; SQLite FTS5 behind a
swappable trait; own graph storage rather than Omnigraph; Ark UI with an own token layer.

Access follows the Authelia group (`admins` → everything, `users` → internal, anything else
→ public only) as a **table, not code**. Write is always an explicit grant. History is
visible to readers by default and configurable, with space-level defaults — which means
**deleting content does not hide it**, so M3 owes a purge operation and a warning at the
point of editing. View-as is instance admins only. Revocation is immediate and deactivation
ends sessions.

## Open items

- **Rotate the Cloudflare token** (`CF_TOKEN_KINDERTAGESPFLEGE`). It reached a transcript
  because the deploy substitutes it into an explanatory *comment* as well as the directive.
  The same pattern exists at `10-services.conf:631` for another service.
- **Server repo has unpushed commits** — the owner is handling those.
- **Production deployment is untried.** `wiki.ohje.ooguy.com` points at `cloud.vm:8100`,
  which serves nothing. Whatever routes `/api/*` there must also route **`/auth/*`**, or
  sign-in breaks.
- **Ark UI has never been used here.** ADR 0005 chose it; the current interface is
  hand-written CSS. Spike it before building the admin console on it.
- The owner has not seen a side-by-side visual comparison yet, and asked for one.
