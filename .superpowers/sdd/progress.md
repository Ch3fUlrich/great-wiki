# SDD progress — marks, links, graph

Plan: docs/superpowers/plans/2026-08-15-marks-links-graph.md
Started 2026-08-19 on branch main (owner has directed all work on main this session).

Task 1: complete (commits 59b662b..7e3d6d6, review clean; Minor: no CHANGELOG entry — orchestrator writes those per piece)
Task 2: implemented (c08afe5) — workspace RED between T2 and T3 by plan flaw: the round-trip guard fails until the exporter emits marks. Reviewing T2+T3 as one unit.
Task 3: implemented (9e1eadf). Combined T2+T3 review: spec OK, quality NOT approved — 3 Important.
  Minor carried to final review: (5) exporter code_span/strike/doc-refusal untested; (7) seed.rs note assertions truthful but weaker than observed; (8) href not escaped, matters at Task 7.
Tasks 2+3: complete (commits 0832763..69cdca9, re-review APPROVED).
  Follow-ups (not blocking): (a) IMPORTANT render() returns problems:[] for some unexpressible output — pre-existing, cut ~80%, guard contains it, corpus unaffected; (b) duplicate same-kind marks on one leaf dropped (regression, degenerate input); (c) "!" before a link emits an image (pre-existing); (d) plain_text mirror differs on U+0085/U+FEFF (pre-existing).
Task 4: implemented (4fd2e67), under review.
Task 4: complete (4fd2e67, review PASS). Minor: marks_to_attrs collapses duplicate same-kind marks (same class as logged item b).
  OPEN RISK for Task 5: nobody has verified TipTap writes Yjs formatting under the SAME attribute keys mark_key_of expects (serde camelCase of MarkKind). If they differ, browser-authored marks are invisible to to_block.
Task 5: implemented (5ccf305). CONFIRMED: TipTap wrote bold/italic vs Rust strong/em — real bug, fixed via .extend({name}), pinned by a wire-key test.
Task 5 review: NOT APPROVED — C1 editor-authored AND edited markdown links become unexportable; I2 javascript: URLs reach the DOM (no scheme check, no CSP); I3 wire-key test pins keys not values; M4-M7.
Task 5 fix pass: C1/I2/I3/M4-M7 fixed (link attrs trimmed editor-side + exporter tolerates; javascript: URLs blocked at the render sink).
Task 5: complete (5ccf305 + fix 84adf2f, re-review APPROVED). Minor 1 (overstated comment) fixed by orchestrator.
  Follow-ups: (e) comparable() reduces attrs but adjacent link leaves with differing bookkeeping still refuse; (f) safeHref allows protocol-relative //evil.example — phishing legibility, not XSS; (g) IMPORTANT pre-existing: one refusable page kills the whole export run.
Task 6: complete (24e2866). Plan bug found: `gw-api -- check` never opens the DB, so the migration verification I specified was vacuous — corrected in the plan.
  Note: links is the first WITHOUT ROWID table here (team_members, login_attempts are similar and do not use it).
