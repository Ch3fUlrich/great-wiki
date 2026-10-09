# Session handoff — 2026-10-09

Written for the next Claude Code session. Read this, then
[`AGENTS.md`](../../AGENTS.md), then the milestone plan you are working on.

## Where the project actually is

**Production runs `3128ece`** (Semaphore task 112569) at <https://wiki.ohje.ooguy.com>:
identity merge (ADR 0021), withheld pages read as absent (ADR 0022) and the `/admin` gate.
**`main` is far ahead and not deployed.** Since `3128ece` it gained rename and move
(ADR 0023, migration 0015), permission-aware full-text search (M7, ADR 0024, migration
0016), the event bus, comments, notifications and the daily digest (M6, ADRs 0025/0026,
migrations 0017/0018), the »Sie« register with a guard test, and a dependency-advisory
gate (`deny.toml`, `just advisories`, a CI job on both forges). About 1340 Rust tests,
1238 web tests and 122 behaviour checks, all green.

**Deploying needs the operator's own OK** — every release since `3128ece` carries forward
migrations — and runs through the `prox` session, because `scripts/build-images.sh` needs
`../Server/secrets-generated/server__cloud__harbor__.env`, which is not on coding.vm.
**Mail is a second, separate decision:** the digest sends nothing unless
`GW_DIGEST_ENABLED=1`, and uses the homelab's shared `SMTP_*` settings, never a credential
of the wiki's own.

**In flight** on branches, not merged: access-epoch revocation for open editing sockets
(`l2/collab-revoke`, ADR 0027) and page templates, which also brings the first way to
create a page from the interface (`l2/templates`, ADR 0028).

**Forgejo is the primary forge** — <https://forgejo.ohje.ooguy.com/Ch3fUlrich/great-wiki>
(private). GitHub is a public mirror and both carry CI; push to both. The Forgejo
pipeline is `.forgejo/workflows/ci.yml` and is shaped by one fact: the runner has nothing
preinstalled and runs `node:24-bookworm`. **`ubuntu-latest` matches no runner there and
queues forever rather than failing** — never use it in that file. Detail, including the
on-disk job-log path (there is no log API), is in `Server/docs/operations/ci-runner-vm.md`.

| Milestone | State |
|---|---|
| **M0–M5** Foundations, vertical slice, identity & access, editing core, blocks, media | Complete |
| **M6** Comments & notifications | Built on `main`; digest mail off |
| **M7** Search (no assistant — a separate decision) | Built on `main` |
| **M8+** | Outlined in the roadmap |

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

**Two gates exist that `just ci` deliberately does NOT run**, because each needs something
CI has not been given. Run both by hand after touching what they cover:

```bash
just mutate      # after any change to gw-auth or gw-store
just behaviour   # after any change to the reader or a component; needs `just dev` running
```

`just mutate` breaks the security-critical code on purpose and checks the tests notice. It
has found three vacuous tests so far, each of which looked completely reasonable. `just
behaviour` asserts real browser behaviour — focus, computed colour, dialog visibility — and
exits non-zero; it is not screenshots and needs no interpretation.

**Screenshots are still part of the loop for anything visual.** Three real defects shipped
past green tests and 200 responses — a title duplicated on every page, tables rendering as
flat paragraphs, and prose hugging the left edge of a too-wide column. None was visible to
any check that was passing.

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

**Verify an agent against the repository, and break something it did NOT choose.** An agent
asked to prove its own check can fail will pick the case it already had in mind. One
reported 13/13 checks passing and demonstrated failure detection correctly — but breaking a
*different* thing showed one check passed on the exact regression it was written to catch,
because it asserted "link colour differs from body text" when the requirement was "reads as
a link".

**Substituting one placeholder in a file containing several is silently destructive.** Any
deploy of `10-services.conf` must assert that *zero* value-carrying placeholders remain,
not that its own got replaced. That assertion caught a third secret and prevented breaking
a service.

**Spawned review sandboxes hold one commit and no history.** A reviewer told to read
`git diff A B` sees nothing and may answer "no findings". Paste the diff into the prompt,
or name the files to read at HEAD.

**Parallel worktrees share one build directory.** Set `CARGO_TARGET_DIR` to the main
checkout's `target/` and `CARGO_INCREMENTAL=0` in every worktree — coding.vm's disk has
filled more than once — and expect the occasional phantom "no field" error from a stale
artifact, cured by `cargo clean -p <crate>`.

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

- **Deploy `main`** — waits for the operator's OK, then `prox` builds and deploys
  (`docs/operations/running-in-production.md`, "Deploying a new version").
- **Enable the digest** (`GW_MAIL_ENABLE`) — waits for the operator; the control-character
  fix it depended on is merged.
- **The `dok:` SELECT on the production database** — read-only, needs production access.
- **Rotate the Cloudflare token** (`CF_TOKEN_KINDERTAGESPFLEGE`) — recorded 2026-08-08,
  not re-checked since; it belongs to the Server repository.
- Left for later from rename and move: sidebar dragging is built; forwarding of further
  sub-routes and a default template per content type are not.
