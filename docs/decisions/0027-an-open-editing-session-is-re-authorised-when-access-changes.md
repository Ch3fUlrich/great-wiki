# 0027 — An open editing session is re-authorised when access changes, before an update is applied

**Status:** accepted · **Date:** 2026-10-09 · Item: GW-COLLAB-REVOKE · Builds on 0006, 0023, 0025

## The defect

A collaborator who lost edit access kept an open socket that applied and broadcast updates for
up to `reauth_interval` (10 s). The sweep refused to *persist* them, but any other writer's
publish snapshots the whole room and checks only the publisher, so the lapsed updates were filed
in history under the publisher's name. The check before an update was applied had no test that
failed without it (its own comment said so).

## Decision

1. **The store owns an access epoch.** Migration 0019 adds a one-row counter and SQL triggers
   on every table that feeds `can()`: `acl`, `principals` (`active`, `groups`), `team_members`,
   `group_roles`, `instance_admins`, `sessions` (delete), `documents` (`path`, `parent_path`,
   `visibility`, `deleted_at`, delete). An update hook marks the connection dirty when the
   counter row changes; the **commit** hook publishes it on a `tokio::sync::watch`; rollback
   clears the mark. `Store::access_epoch()` subscribes. View-as lives in memory, so it calls
   `Store::bump_access_epoch()`.
2. **A socket subscribes before its handshake check** and is told of any later change. It
   re-authorises (a) immediately, even if it only listens (`push_revocation`), and (b) before
   applying any update, whenever the epoch moved or the interval passed. Check (b) has no
   switch and has its own test.
3. **No laundering, by prevention.** An update from a connection whose authorisation is older
   than the current epoch is never applied, so nothing lapsed is in the room for a publish (or
   a sweep) to snapshot. No publish-side rule is needed for committed revocations.
4. **View-as is asked by identity** (`Registry::is_viewing`): an open socket's cookies are
   frozen at the upgrade and cannot show that its administrator entered the mode since.

## Alternatives rejected

- **Reuse the M6 event bus (ADR 0025).** It is a per-recipient table read at delivery time —
  durable and pull-based. Revocation needs an in-process, unconditional wake-up of sockets;
  routing it through rows would add a poll, a recipient model (sockets are not recipients) and
  a write on every access change for no one to read.
- **Explicit bumps in each store method.** The failure is silent when the next writer
  forgets; triggers cannot be forgotten and are atomic with the change. Cost: the trigger list
  must grow with any new access-bearing table (the store test names each one).
- **Drop the lapsed client's pending updates / re-check each update's author at publish.** A
  CRDT has no un-apply; peers already received the updates and re-upload them on reconnect;
  and yrs authorship is a client id the client chooses. Both are repairs after the fact that
  cannot be made sound. Preventing the apply is.
- **Check on every frame.** Three SQLite queries per keystroke on a one-connection pool; the
  epoch makes the common case a memory read.

## Cost and residual

An access change costs one authorisation (three queries) per open socket, at admin rate.
Changes that raise **no write** — a session or view-as record simply expiring by the clock —
are still found only by the interval check, so their window stays `reauth_interval`. A change
made by another process on the same database file (`seed`) is likewise not seen by the
in-memory epoch. The correctness of "announce at commit" relies on the pool holding one
connection (`Store::open`); with more, the bump must move to after the commit returns.
Revisit if sessions gain an expiry event, or the pool grows.
