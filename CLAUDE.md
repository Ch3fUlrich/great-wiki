# CLAUDE.md — great-wiki

**Read [AGENTS.md](AGENTS.md) first** — skills, memory model, architecture rules, env vars,
verification commands. This file is *only* the Claude-specific delta. Start at the router:
`agent-skills/skills/repository-index/SKILL.md`.

## Code tools — Serena first, not Read/Grep

For any task touching Rust, YAML, TOML, JSON, Bash or Python in this repo, use Serena's
semantic tools instead of reading whole files:

- Discovery: `get_symbols_overview`, `find_symbol`, `find_referencing_symbols`
- Edits: `replace_symbol_body`, `insert_after_symbol`/`insert_before_symbol`, `replace_content`
- Call `initial_instructions` and `activate_project` once at session start.

**`web/` is not covered.** `.serena/project.yml` deliberately omits `typescript` — adding a
language whose server is absent from the image does not degrade gracefully, it takes the
*entire* server down with `The language server manager is not initialized`, naming neither
the language nor the cause. Verify against the running image before adding it, then restart
`serena-mcp`. Until then use built-in Read/Grep for the frontend.

Serena also refuses gitignored files by design — use built-in Read/Edit for those.

## Graphify — blast radius, not symbols

Serena answers "who calls this function"; Graphify answers "what does this change reach".
Reach for it when a change spans crates. It needs `graphify-out/graph.json` to exist:

```bash
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo" -w /repo \
  --entrypoint python graphify-mcp:latest -m graphify update .
```

`--user` is required — without it the container writes root-owned files the next rebuild
cannot overwrite. Graphify belongs in **user scope only**, never this repo's `.mcp.json`:
it is stdio and inherits its launch directory, so one cwd-relative entry serves every repo.
Verify: `bash ~/code/agent-skills/infra/mcp-servers/scripts/linux/check-graphify-scope.sh --fix`

## MCP failure modes — all four are silent

| Symptom | Cause | Fix |
|---|---|---|
| Wrong/empty graph, `0 rows except 2 Preferences` | user-scope `omnigraph` overriding `.mcp.json` | remove it from `~/.claude.json` |
| MCP tool **absent entirely** — no prompt, no error | project server never approved | `.claude/settings.local.json` → `enabledMcpjsonServers` |
| Graph answers describe another repo | wrong cwd, or a stray hardcoded `graphify` entry | `check-graphify-scope.sh --fix` |
| `missing bearer token` / `fetch failed` | `OMNIGRAPH_TOKEN` / `OMNIGRAPH_NET` unset or wrong | `setup-agent-memory.sh --check` |

MCP servers load at startup — **restart Claude Code after editing `.mcp.json`.**

The graph must also exist server-side; naming it here does not create it:
`cd ~/code/agent-skills/infra/mcp-servers && ./scripts/add-project-graph.sh great-wiki && ./scripts/apply-cluster.sh`
