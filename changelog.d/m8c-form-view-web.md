### Added

- **A dataset page can now show a saved form and file a row through it.** Picking a view of
  kind `form` (`?view=ID`) renders the form itself instead of the table: its heading (the
  config's `title`, or the view's name), one labelled editor per question using the same cell
  controls as the row editor, required questions marked with a visible `*` and
  `aria-required`, and an `Absenden` button. Submitting sends exactly the fields the form
  lists as one `POST /api/datasets/rows/{path}` with `{values, view}`; the draft is emptied
  and `Gespeichert.` is announced on 201. Refusals come back as words — the server's own
  sentence (`the form has no field "…"`, `… is required`) for a 4xx, a German explanation for
  nothing typed right or no answer at all — shown inline per field or under the button.
  A required field left empty, blank or absent is refused client-side first, so the ordinary
  round trip is only paid for submissions the form could accept; a key outside the form's
  subset never reaches the body either, mirroring the server's rule rather than replacing it.
  Without write on the dataset the page says `Zum Ausfüllen ist Schreibrecht nötig.` and
  renders no form at all. Submitting is script-driven — the server remains the authority on
  every promise the form makes.
