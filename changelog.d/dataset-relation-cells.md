<!-- Fold into CHANGELOG.md under [Unreleased], in the section named below. -->

### Added

- **A relation column can be filled and is read as names.** The row editor now offers a picker
  for `relation` fields: the target dataset's rows are fetched once when the cell opens (a single
  select, or checkboxes when the column is `multiple`) and a pick stores the row's **id**. The
  row's own label names it in the list — its first non-empty text or number value, else its id —
  and a link whose row has since disappeared is not quietly dropped: it stays listed as
  „(nicht mehr vorhanden)“ until the person decides. If the target cannot be loaded the picker
  says so in German and keeps the current selection; a failed fetch never empties a cell.
  In the table, linked rows show as chips of their labels. To keep that off an N+1 path, the
  page load fetches each visible relation column's target page **once** beside the rows
  themselves; when that fetch fails — or the column's target left the reader's sight — the cell
  falls back to counting ids („n verknüpft“), never to an error page.

- **Computed cells read as text.** A `formula` or `rollup` value shows through the table's normal
  shapes: numbers with the de-DE formatter, lists joined with a comma, nothing for an empty
  value — and an evaluation error shows its full `#ERR(reason)` as visible text in the cell, not
  behind a hover title any more. Computed columns stay non-editable: the row editor offers no
  control for one, and sorting or filtering them remains the server's answer as before.

Behaviour grows W4 and W5: an editor creates a second dataset page, links a row to
`/verweisbeispiel/tabelle` through the picker, reloads, and sees the target row's **label** and
the value of an `alter + 1` formula; and a formula dividing by zero visibly shows `#ERR(…)`.
The reader's-eye half of the story — a relation column hidden when its target is not readable —
is the store's omission rule and is covered there; the harness has only one identity and cannot
express it.
