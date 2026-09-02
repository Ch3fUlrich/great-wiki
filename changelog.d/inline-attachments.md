<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **A file can now sit *in* the text, where it belongs.** Attaching a picture put it in the
  `Anhänge` list under the page and nowhere else; a scan of a lab result had to be described
  in a sentence and then hunted for at the bottom. A file can now be placed in the prose, and
  what it looks like there depends on what it is: **a picture is shown where it was put, and
  everything else — a PDF, a text file, a table of measurements — becomes a labelled card**
  giving its name, its type and its size, which downloads when you press it. That division is
  deliberate rather than a limitation. A picture beside the paragraph that explains it is most
  of what a picture is for in a medical reference; a document viewer opened in the middle of
  somebody's prose is not.

  Whether a file is shown or offered is decided from **what the bytes are**, read from the
  file itself when it was uploaded — never from its name. Renaming a PDF to `.png` changes
  nothing.

- **Placing a file is choosing one from the list.** Open the editor, put the cursor where the
  file should go, and press its name in the row under the toolbar; you are asked how you would
  describe it to somebody who cannot see it, and that description becomes the picture's
  alternative text. There is no field to type a filename into, and that is the point: only a
  file the page actually carries can be placed, so a reference to something that was never
  uploaded cannot be written by accident.

- **Taking a file off a page does not rewrite the page.** The `Anhänge` list is what says a
  file is attached; a placement in the text is a *reference* to an entry in it. So detaching a
  file leaves every paragraph exactly as it was, and the place the picture stood says, in
  words, that this page no longer carries a file of that name and that it was either removed
  or never uploaded. It is never a broken-image icon: that reads as "the network failed", and
  sends whoever goes looking to the one place the problem is not. The same sentence appears
  for a page imported from markdown that names a file nobody has uploaded yet.

- **A placed file survives export and re-import.** `great-wiki export` writes it as
  `![Beschreibung](anhang:datei.png)` on a line of its own, and reading that directory back in
  produces the same page. The exporter proves it per page rather than trusting it — it
  re-imports every file it is about to write and compares — so a placement that would come
  back as something else is refused by name instead of quietly changing the page. **The files
  themselves are not in an export**, which the fidelity warning beside them now says: an export
  is a copy of the wiki's words, and the attachments live on the media mount.

- **An SVG is shown as a picture and never as a program.** It is displayed through an `<img>`
  element and through nothing else — never an `<object>`, an `<embed>`, an `<iframe>`, or by
  putting its markup into this wiki's own page, which is the one that would let a drawing run
  code with your session in reach. Nothing sanitises an uploaded SVG, deliberately; not
  executing it is the defence, and it is now checked in the reader as well as in the server's
  own response headers.

### Changed

- **The revision history names a placed file.** Swapping one picture for another shows as
  »Datei — Datei: befund.png → nachher.png«, because a placement carries no words for the
  prose diff to compare.

### Known limitations

- **Dragging a file into the editor still does nothing.** Placing one is a button per attached
  file; drag-and-drop, resizing, captions and alignment are not here.

- **A placement is a block of the page, not something inside a sentence.** It stands between
  paragraphs and cannot be put inside a list item, a quotation or a table cell — markdown has
  no way to write one there that reads back the same, and a page that cannot be read back is a
  page that cannot be exported.

- **A file placed on one page always means that page's copy of it.** There is no way to show a
  file that is attached to a different page; a download is authorised against the page it was
  reached through, and an address that named a file without naming a page would go around that.
