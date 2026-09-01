### Added

- **Text, Markdown, CSV and SVG can be attached.** Until now a file had to carry a recognisable
  magic number in its first few bytes, which every one of those four lacks — so the CSV a lab
  result came from, the notes exported from another tool, and the diagram somebody drew could
  not go on a page at all. They can now, and the check that lets them in is a different kind of
  question rather than a longer list: not "does this begin like a PNG" but **"is every byte of
  this valid UTF-8, with no control characters beyond tab, newline and carriage return"** —
  asked over the whole file as it arrives, so a document whose first page is a licence header
  and whose remainder is a binary payload is still refused.

  **Text, Markdown and CSV all arrive as plain text, and the wiki says so rather than
  guessing.** Nothing in the bytes tells them apart; a `.csv` and a `.md` differ by convention
  and by what you do with them, not by anything that can be measured, and looking for commas
  and calling it a spreadsheet would be exactly the guess this wiki refuses to make about a
  file's type. The name on the page is where "this is a spreadsheet" is written down.

  A byte order mark is accepted and left exactly where it is — nothing here ever alters the
  bytes of a file, because the file is stored under a fingerprint of its own contents and
  altering it would mean handing back something the uploader cannot recognise. A UTF-16
  document is refused rather than stored as text nobody could read.

- **An SVG is stored exactly as given, and never opened where it was reached.** SVG is the one
  picture format that is also a program: it can carry scripts, event handlers and references to
  other sites. Nothing strips those out, deliberately — half-cleaning a file is worse than
  leaving it plainly quarantined, because a half-cleaned file invites being trusted — and
  instead every SVG is **saved rather than displayed**, unlike every other image. It also
  arrives with the browser forbidden from re-typing it and with scripting switched off for it
  entirely, so even a browser that displayed it anyway would run nothing.

- **The wiki can now forget a file completely.** `endgültig löschen` destroys a page's
  attachment entries and says how many stored files are left referenced by nothing at all; the
  files themselves stay, because deleting them in the same breath has no safe ordering — every
  arrangement of "destroy the entries" and "delete the files" ends, in its worst case, with a
  page nobody deleted losing its file. `great-wiki reclaim` is the second, separate act that
  clears them, and it is what turns "purged" into "gone".

  It says what it would take and takes nothing until you add `--commit`, it names how much disk
  it freed, and the number matches what the purge told you it had left behind — so the report
  you confirmed and the thing that happened are the same figure. It cannot touch a file any
  page still carries, and it cannot take a file from an upload that is in flight while it runs.
  Files on the mount that the wiki has no record of are left alone: finding those would mean
  searching the disk, and a search of the disk cannot tell a stray file from one being uploaded
  a moment ago.

  It is a command rather than a button or a timer, for the reason emptying the Papierkorb is
  two presses: this is the operation that exists to forget a medical document, and it should be
  one somebody meant. Why the bytes wait for it, and what it is built on, is
  [ADR 0013](docs/decisions/0013-what-a-purge-leaves-on-the-mount.md); what a file has to be to
  be attached at all is [ADR 0014](docs/decisions/0014-what-a-file-has-to-be-to-be-attached.md).

### Changed

- **A file that is really markup is now stored, as text.** An HTML document uploaded under any
  name used to be refused; it is now kept and served as plain text, because Markdown may
  legitimately contain HTML and there is no line between the two to draw. What makes that safe
  is unchanged and is the whole point: the wiki never calls it HTML, forbids the browser from
  deciding otherwise, and hands it over to be saved rather than opened. What is refused is now
  the smaller, stranger set of files that are neither a format this recognises nor text at
  all — a sound file, a compiled program, a document that stops mid-character — and the refusal
  says so.
