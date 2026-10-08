# 0025 — The event bus records who might hear, and asks again at delivery

**Status:** accepted · **Date:** 2026-10-08 · Roadmap: "M6 and M7, sized" (2026-09-17)

## Decision

One table, `events`, one row per (event, recipient). Four producers write it; two consumers
read it (the notifications view and the daily digest). Nothing else reads it.

1. **Emit records candidates, not entitlements.** A producer names the people an event might
   concern (the parent comment's author, the page's last editor, the assignee, the admins)
   and writes a row each. It performs **no** permission check and stores **no page words** —
   only ids (`doc_id`, `actor`, `subject`) and a `kind`.
2. **Delivery is the check.** Every read path (`notifications_for`, `unread_count_for`,
   `digest_for`) resolves each row's `doc_id` through `Store::document_for_id(…, Read)` — the
   one accessor — *at read time, for the reader*. A row whose page the reader may no longer
   read is skipped, not counted. Title and path are resolved from that same answer, never
   stored. Admin events (accepted invitation, grant change) additionally require the reader
   to still administer the path.
3. **Withheld = absent (ADR 0022).** A list, a badge count and a digest contain exactly what
   survives step 2; no total, no "N hidden". Marking a withheld row read answers 404.
4. **Dedupe by key.** `(recipient, dedupe_key)` is unique: "edited by someone else" coalesces
   into the recipient's unread row for that page; "task coming due" is keyed by (task, due).

## Why delivery, not emit

Grants change after the event. A relative removed from a page must stop hearing about it the
moment the grant goes; an emit-time check would keep a title in their inbox forever. Rejected:
checking at emit only (stale); copying page text into the row (a second, unfiltered copy of
the page's words — the exact failure rule 2 forbids).

## Cost / revisit

Every list costs one accessor call per row (tens of rows; rows older than 90 days are not
read). Revisit with a (reader, doc) cache — never an unfiltered count — if that is slow.
Producers never fail the user's action when an emit fails; a lost notification is logged.
