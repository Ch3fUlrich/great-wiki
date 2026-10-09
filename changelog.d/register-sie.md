<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **The interface says »Sie« everywhere.** The editor's connection and refusal messages,
  the history page's sign-in prompt, the access panel's empty state and the daily digest's
  subject and headings still said »du« — the register the invitation, sign-in and console
  pages had already left behind (invite walkthrough, 2026-09-17). They now say »Sie«.
- A test now reads every component and module and fails if one addresses the reader as
  »du«; it checks that it read over a hundred files, so a broken path cannot pass silently.
