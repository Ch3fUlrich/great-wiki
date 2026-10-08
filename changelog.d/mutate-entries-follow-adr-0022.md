<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **The mutation gate is whole again.** Three entries in `scripts/mutate.sh` still described
  the pre-ADR-0022 code (a 404/403 split written at each call site) and found nothing to
  change, so `just mutate` exited non-zero. They now break what the code does today: the
  task list and the global board's `seite=` must refuse through `withheld_or_absent`, and
  `seite=` must authorise the page before it says which project is homed there.
