# CLAUDE.md — great-wiki

**Read [AGENTS.md](AGENTS.md) first** — skills, memory model, architecture rules, env vars,
verification commands. This file is *only* the Claude-specific delta. Start at the router:
`agent-skills/skills/repository-index/SKILL.md`.

## Code tools — Serena first, not Read/Grep

For any task touching Rust, YAML, TOML, JSON, Bash or Python in this repo, use Serena's
semantic tools instead of reading whole files:

- Discovery: `get_symbols_overview`, `find_symbol`
- Edits: `replace_symbol_body`, `insert_after_symbol`/`insert_before_symbol`, `replace_content`
- Call `initial_instructions` and `activate_project` once at session start.

**`find_referencing_symbols` does not work here — use `grep` to find references.** Verified
twice on 2026-08-19: it returns `{}` for `Block`, `BlockKind`, `Block::plain_text` and
`Mark`, all of which have references `grep` finds immediately (`Mark` is re-exported one
file away; `Block` is constructed in four crates). The failure is silent — an empty result
is indistinguishable from "nothing references this", so an agent told to rely on it will
conclude a symbol is unused and change it freely. `find_symbol`, `get_symbols_overview` and
every edit tool work normally.

**`web/` is covered too.** `typescript`, `html` and `scss` are configured and **verified
working in this container** (2026-08-07): both started in under 0.2 s and resolved
interfaces, classes, methods and hover types. Use `javascript` nowhere — it is not a
separate language here; `typescript` handles it. Same for `css`, which `scss` handles.

Two languages are deliberately absent:

- **`markdown`** — marksman *is* in the image, but it is a .NET binary and the image has no
  libicu, so it aborts with "Couldn't find a valid ICU package installed on the system".
- **`svelte`** — Serena's generated config notes it "subsumes typescript/javascript for
  .svelte projects; requires npm", so it may conflict with the `typescript` entry, and it
  has never been started in this container. Add it **alone**, restart `serena-mcp`, and
  read `docker logs serena-mcp | grep StartLS` before trusting it.

That last point is the general rule: one language server failing to start takes the *whole*
manager down with `The language server manager is not initialized`, naming neither the
language nor the cause. Never add a language speculatively.

Two more things about this container:

- **Serena rewrites `.serena/project.yml`** with its own documented template on activation,
  so hand-written comments there do not survive. Only the values persist.
- **It runs as root over a bind mount**, so it leaves root-owned files under `.serena/`.
  Fix with `docker exec serena-mcp chown -R $(id -u):$(id -g) /home/s/code/great-wiki/.serena`.

Serena also refuses gitignored files by design — use built-in Read/Edit for those.

## Graphify — blast radius, not symbols

Serena answers "who calls this function"; Graphify answers "what does this change reach".
Reach for it when a change spans crates. It needs `graphify-out/graph.json` to exist.

**Code only** — fast, no key, no network, a couple of seconds (`just graph`):
```bash
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo" -w /repo \
  --entrypoint python graphify-mcp:latest -m graphify update .
```

**Including docs** — this repo is mostly prose, so the code-only pass misses nearly all of
it. Semantic extraction needs an LLM, routed through LiteLLM per the rule in
[AGENTS.md](AGENTS.md#architecture-rules--the-ones-that-are-not-negotiable). Slow: minutes,
not seconds. Run it in the background.
```bash
LITELLM_MASTER_KEY=$(grep -m1 '^LITELLM_MASTER_KEY=' \
  ../Server/secrets-generated/server__cloud__ai__.env | cut -d= -f2-)
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo" -w /repo \
  -e OPENAI_API_KEY="$LITELLM_MASTER_KEY" \
  -e OPENAI_BASE_URL=http://192.168.178.159:4000/v1 \
  -e OPENAI_MODEL=deepseek-v4-flash \
  --entrypoint python graphify-mcp:latest \
  -m graphify extract . --backend openai --model deepseek-v4-flash
```

Three things that will waste your time:

- **`--user` is required.** Without it the container writes root-owned files that the next
  rebuild cannot overwrite.
- **Never run two extractions at once.** They race on `graphify-out/`. A timed-out
  foreground `docker run` leaves its container alive — check `docker ps` and kill it before
  starting another.
- **Graphify belongs in user scope only**, never this repo's `.mcp.json`: it is stdio and
  inherits its launch directory, so one cwd-relative entry serves every repo. Verify with
  `bash ~/code/agent-skills/infra/mcp-servers/scripts/linux/check-graphify-scope.sh --fix`

## Omnigraph — agent memory, and what belongs in it

ADR 0004 settled what this server is for: **agent memory**, not the wiki's own graph (that is
SQLite, and `links` is the wiki's graph). So what belongs here is what a *future session* will
wish it knew and cannot recover from the code — a decision and the alternatives it beat, a
failure mode and its cause, a constraint discovered the hard way.

**Record a change here as well as in `CHANGELOG.md`.** They are not the same audience and
neither substitutes for the other: the changelog tells a *reader of this project* what the
software now does, in terms of effect; Omnigraph tells the *next agent* why, so it does not
re-derive a decision or repeat a failure. A change that is in neither is a change nobody can
trace; a change in only the changelog leaves the reasoning to be rediscovered.

Write the reasoning, not the diff — git already has the diff. In particular: what was tried
and rejected, what the cost of the chosen option is, and what would have to change for the
decision to be revisited.

**Read `omnigraph://schema` before any query, mutation or load** — the server's own
instructions insist on it, and writing without it produces queries that lint-fail or silently
corrupt data. Verify a write landed: `commits_list` head before and after; identical heads
mean the write did not land, and a 504 does not mean failure.

**If the tools are absent**, the bridge is not connected to this session — the containers can
be healthy while the MCP server is not attached. `docker ps | grep omnigraph` tells them
apart. MCP servers load at startup, so reconnecting means restarting Claude Code; record what
should have been written and write it then rather than dropping it.

## MCP failure modes — all five are silent

| Symptom | Cause | Fix |
|---|---|---|
| Wrong/empty graph, `0 rows except 2 Preferences` | user-scope `omnigraph` overriding `.mcp.json` | remove it from `~/.claude.json` |
| MCP tool **absent entirely** — no prompt, no error | project server never approved | `.claude/settings.local.json` → `enabledMcpjsonServers` |
| Graph answers describe another repo | wrong cwd, or a stray hardcoded `graphify` entry | `check-graphify-scope.sh --fix` |
| `missing bearer token` / `fetch failed` | `OMNIGRAPH_TOKEN` / `OMNIGRAPH_NET` unset or wrong | `setup-agent-memory.sh --check` |
| `graph 'great-wiki' not found`, and `apply-cluster.sh` dies on `./.env.shared: No such file` | that repo's config was consolidated into one `.env`; the script still sources the two files it was split into | `.env.shared` and `.env.server` are now symlinks to `.env` — restore them if they vanish, rather than copying secrets into three files |

MCP servers load at startup — **restart Claude Code after editing `.mcp.json`.**

The graph must also exist server-side; naming it here does not create it:
`cd ~/code/agent-skills/infra/mcp-servers && ./scripts/add-project-graph.sh great-wiki && ./scripts/apply-cluster.sh`

Both halves are needed and the first one lies about it: `add-project-graph.sh` reports success
and writes the graph into `cluster/cluster.yaml`, but nothing exists server-side until
`apply-cluster.sh` pushes that config — so `schema_get` keeps answering **not found** with no
hint that a second step was ever required.
