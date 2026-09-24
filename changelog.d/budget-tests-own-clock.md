### Fixed

- **The web test suite no longer fails at random on a busy machine.** Three tests of the
  per-page limits on code highlighting and formula typesetting measured real time. Under load
  one ran past its timeout, and two reached the time limit before the size limit they were
  written to check, so they named the wrong limit. The budget code now takes its clock as an
  argument, real time by default, and each test states exactly how long every render takes.
  What a page is allowed to cost in production is unchanged.
