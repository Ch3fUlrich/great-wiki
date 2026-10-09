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
   The answer is async, so the epoch is **looked at again once it is in** and the question is
   asked again if it moved (`vet_until_stable`); the look and the apply have no await between
   them. Without it a change landing between the answer and the apply let one lapsed update in.
3. **Churn is bounded, not amplified.** The epoch is global, so anyone whose write moves it
   (a logout, a move) would make every socket re-ask three queries on the one SQLite
   connection. A watch already collapses a burst into one wake-up; `vet_gap` (250 ms) bounds
   the rate: the first change after quiet is acted on at once, later ones wait out the gap, and
   an update waits too (it is never applied unvetted — churn costs latency, not safety). A
   hundred changes in a second cost a socket ≤ 8 answers (tested via `CollabState::vets`).
   Rejected: **per-document epochs** — a grant on an ancestor, a team, a group baseline or an
   instance admin reaches documents the writer does not name, so the store would have to
   compute the affected set at write time, which is the check itself; and a wrong set is a
   silent hole. Revisit if a deployment has thousands of sockets.
4. **Reads are gated too.** Every send of room data to the socket (the snapshot on connect,
   sync diffs, relayed updates, presence, a resync) and every relay of the socket's presence to
   the room goes through the same `vet_until_stable` first. Once the epoch has moved since the
   last answer nothing leaves or enters until the answer is in; on a no the session closes with
   none sent. Frames already queued for the socket before a revocation are held at the same
   gate. Cost: during the `vet_gap` wait the socket's outbound frames wait too (they are not
   dropped; a yes delivers them in order). The connect and resync sites are the likeliest read leak (a client reconnecting right after
   losing access), so they are tested too, through `CollabState::hold`: a test seam that parks
   the session task after the handshake or after the connect snapshot, so that a revocation can
   land in a window no request sequence reaches (and `CollabPolicy::update_buffer` makes a
   connection lag with a few frames). The seam is one cheap check when unset and nothing in the
   server sets it.
5. **No laundering, by prevention.** An update from a connection whose authorisation is older
   than the current epoch is never applied, so nothing lapsed is in the room for a publish (or
   a sweep) to snapshot. No publish-side rule is needed for committed revocations.
6. **View-as is asked by identity** (`Registry::is_viewing`): an open socket's cookies are
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

An access change costs one authorisation (three queries) per open socket, at most once per `vet_gap` per socket. Under sustained churn a writing socket's updates are delayed by up to that gap.
Changes that raise **no write** — a session or view-as record simply expiring by the clock —
are still found only by the interval check, so their window stays `reauth_interval`. A change
made by another process on the same database file (`seed`) is likewise not seen by the
in-memory epoch. The correctness of "announce at commit" relies on the pool holding one
connection (`Store::open`); with more, the bump must move to after the commit returns.
Revisit if sessions gain an expiry event, or the pool grows.
