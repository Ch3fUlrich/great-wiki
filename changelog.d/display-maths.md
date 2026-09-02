<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Formeln werden gesetzt.** A code block whose language is `math` is no longer printed as
  TeX source — it is typeset, with real fractions, roots, sums and integrals, in the maths
  faces they were designed for. Write the formula between ` ```math ` fences the way you
  would write any other fenced block; everything else about the block is unchanged, so a page
  holding one still exports, re-imports and round-trips as the same file it always did.

  **It happens while the page is being built, not in your browser.** The formula is in the
  first response, so it is there with JavaScript switched off, there before anything has
  loaded, and there for a screen reader — the answer carries the maths twice, once drawn and
  once as MathML, so the formula is read out as a formula rather than skipped as a picture.
  And nothing of the maths library is downloaded: a page with formulas costs the reader 4 kB
  more stylesheet than one without, plus whichever of the typefaces its symbols actually use.

  The example tour gained one, under **Was schon geht**.

- **The maths typefaces are served from this wiki, like every other one.** Twenty faces,
  304 kB, under `web/static/fonts/katex/`. Nothing is fetched from a third party — which is
  the same rule the reading typefaces already follow, and here it is also the difference
  between a formula that is spaced correctly and one that quietly is not: the layout is
  computed from those exact files.

### Changed

- **A formula that cannot be set shows its own source and says why.** Four limits, all of
  them far above anything a page would really hold: 5 000 characters for one formula, 100
  formulas on one page, a ceiling on how much typeset maths one page may carry — that one
  because a short formula can expand into a great deal of markup — and a quarter of a second
  of typesetting per page, because how long a formula takes and how much it produces are not
  the same thing. Over any of them, the block prints the formula as written with one small
  line underneath naming the limit and the reason there is one: the setting happens on the
  server that every reader shares. The last two limits **stop** the page rather than skipping
  one formula: once a page has spent what it may, the rest of its formulas print as written
  instead of being set and then thrown away. A
  formula that is simply wrong prints as written too, in red, rather than taking the page
  down — and it is the page you would have to open to fix it.

- **The wiki still puts no stored text into a page as markup, with exactly one named
  exception.** Typeset maths is the first thing here that has to be markup, so it is confined
  to a single small component, and the check that enforces the rule
  (`scripts/check-html-sinks.sh`) was taught about **that one line** rather than being
  switched off for that file: any other construction in it, and the same construction
  anywhere else, still fails the build. Links, classes, ids, inline styles and images written
  inside a formula are all refused by the typesetter and shown as what they are.
