# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); newest first.
Entries describe the *effect* of a change, not the diff.

## [Unreleased]

### Added

- Repository foundations: MIT licence, public-repo-safe `.gitignore`, `.gitattributes`
  enforcing LF on scripts and configs, and `.graphifyignore`.
- Agent instruction files — `AGENTS.md` (hub: skills, memory, architecture rules,
  verification commands) and `CLAUDE.md` (Claude-specific delta only).
- MCP wiring: project-scoped `.mcp.json` pinning the Omnigraph graph to `great-wiki`, with
  the approval gate in untracked `.claude/settings.local.json`. Graphify is deliberately
  left to user scope; Serena is configured by `.serena/project.yml`.
- `.serena/project.yml` with a conservative language list. TypeScript is omitted on purpose
  until the running image is verified to carry its language server — an absent server takes
  the entire Serena instance down rather than degrading.
