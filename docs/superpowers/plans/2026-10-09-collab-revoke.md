# Collab revocation — plan (GW-COLLAB-REVOKE, 2026-10-09)

Defect: after a collaborator loses edit access their open socket keeps applying and
broadcasting updates for up to `reauth_interval`; a later publish snapshots the room and files
those updates in history. The pre-write re-check has no test that fails without it.
Choice and rejected alternatives: ADR 0027.

## Tasks (each ≤ 3 files + tests, ≤ 200 lines; branch pushed after each)

1. **Store access epoch** — `gw-store`: migration `0019_access_epoch.sql` (one-row counter +
   triggers on `acl`, `principals`, `team_members`, `group_roles`, `instance_admins`,
   `sessions`, `documents`), `access_epoch.rs` (watch channel; update hook marks dirty, commit
   hook publishes, rollback hook clears), `Store::access_epoch()` / `bump_access_epoch()`.
   Tests: every access-bearing write bumps; a rolled-back write and a body publish do not.
2. **Push + vet-before-apply** — `gw-api` `collab.rs`: socket subscribes to the epoch
   *before* the handshake check; a changed epoch re-authorises at once (select branch) and
   before the next update is applied; view-as entry bumps. `CollabPolicy::push_revocation`
   (default on) lets a test isolate the pre-write check. Tests: grant revoke, move out of
   reach, trash, deactivation, view-as entry, and the pre-write check biting.
3. **No laundering + mutations + docs** — a lapsed connection's update is never applied, so
   nothing lapsed reaches a snapshot; publish additionally refuses a room holding unvetted
   work from a connection that failed verification. `collab:` entries in `scripts/mutate.sh`,
   ADR 0027, changelog fragment, `collab.rs` doc comments.

Gates per task: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test --workspace`, web check + vitest (final), `./scripts/mutate.sh collab:`.
