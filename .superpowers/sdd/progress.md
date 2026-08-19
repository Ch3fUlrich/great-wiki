# SDD progress — marks, links, graph

Plan: docs/superpowers/plans/2026-08-15-marks-links-graph.md
Started 2026-08-19 on branch main (owner has directed all work on main this session).

Task 1: complete (commits 59b662b..7e3d6d6, review clean; Minor: no CHANGELOG entry — orchestrator writes those per piece)
Task 2: implemented (c08afe5) — workspace RED between T2 and T3 by plan flaw: the round-trip guard fails until the exporter emits marks. Reviewing T2+T3 as one unit.
Task 3: implemented (9e1eadf). Combined T2+T3 review: spec OK, quality NOT approved — 3 Important.
  Minor carried to final review: (5) exporter code_span/strike/doc-refusal untested; (7) seed.rs note assertions truthful but weaker than observed; (8) href not escaped, matters at Task 7.
Tasks 2+3: complete (commits 0832763..69cdca9, re-review APPROVED).
  Follow-ups (not blocking): (a) IMPORTANT render() returns problems:[] for some unexpressible output — pre-existing, cut ~80%, guard contains it, corpus unaffected; (b) duplicate same-kind marks on one leaf dropped (regression, degenerate input); (c) "!" before a link emits an image (pre-existing); (d) plain_text mirror differs on U+0085/U+FEFF (pre-existing).
