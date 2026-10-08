<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **The event bus, comments and the daily digest are under the mutation gate.** `scripts/mutate.sh`
  gains 26 entries (`events:`, `comments:`, `digest:`), every one caught by the suite: delivery
  re-checking Read and admin on each row, the unread count, self-events, recipient-only
  mark-as-read, dedupe, comment page checks and the withheld-equals-absent answer, one-deep
  threads, orphaning, the absence of any delete path, and the digest's send-then-mark order,
  off-by-default flag and redacted password. One gap surfaced and is closed: a mention of a
  deactivated account was not tested and now is.
