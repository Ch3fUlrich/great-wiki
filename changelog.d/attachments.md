### Added

- **Files can be attached to a page.** Until now the wiki was prose only: a scan, a lab
  report, a photograph of a rash had to live somewhere else and be described here. A page now
  carries an **Anhänge** list, and anyone who may write the page can put a file in it, take
  one out again, and hand somebody the address it is fetched from. Up to 250 MB per file —
  enough for a bundle of scans or a short video, and small enough that one accidental upload
  is noticeable rather than damaging.

  **A file is fetched through the page it is on, and only through it.** The address is
  `/api/attachment/<name>/<seite>`; there is no address that names the file itself. That is
  not a detail of the URL scheme, it is the whole rule: the same PDF attached to a public page
  and to a private one is stored once, and the answer to "may I have these bytes" is asked
  about the page you asked through, every time, before a single byte is read from disk. Two
  pages holding the same document are two statements about who may read *those pages*; the
  identical bytes underneath are how it happens to be stored, and nothing anywhere can
  authorise anything against them.

  Which means attaching a file to a private page does not quietly break the public page that
  already showed it, and being able to read one page never becomes a way to reach a file on
  another. It also means the file is stored once no matter how many pages carry it, so
  attaching the same 40 MB scan to four pages costs 40 MB.

- **What a file *is* is read from the file, never from what it was called.** A document named
  `befund.png` that is actually an HTML page is refused, and one named `.txt` that is actually
  a PDF is served as a PDF. The type the browser is told is the type the bytes are, because
  the browser is the thing being protected: it is what would run a script if it were told a
  page of markup was a picture.

  The cost is stated rather than hidden: **plain text, Markdown, CSV and SVG cannot be
  attached.** None of them has anything in its bytes that identifies it, so accepting them
  would mean guessing — and the two available guesses are "call it text and hope it is not
  markup" and "call it an image and hope it carries no script". Images, PDFs, ZIP archives
  (which is what a Word or LibreOffice document is), and MP4, WebM and Ogg media are what this
  stores. Text belongs in a page.

- **A picture or a PDF opens where you are; anything else is saved.** A download also arrives
  with the browser forbidden from second-guessing its type and with scripting switched off for
  it entirely, so a PDF that carries code — they can — renders with nothing of this wiki within
  its reach.

- **A purge now says what it took off a page's list, and what it left on the disk.**
  `endgültig löschen` destroys a page's attachment entries along with its history and its
  cards, and counts them. It does **not** delete the stored files, and says how many are now
  referenced by nothing at all, rather than letting an administrator assume they went. Why the
  bytes stay — and what has to be true before anything deletes them — is
  [ADR 0013](docs/decisions/0013-what-a-purge-leaves-on-the-mount.md). Until that exists,
  a purged file is still on the media mount and removing it is a manual job; the number in the
  report is what tells you there is one to do.

- **Attaching a file you already have tells you nothing about who else has it.** Uploading a
  document the wiki has already stored — on a page you cannot read — answers exactly as
  uploading something nobody has ever sent: same reply, same fields, same work done. Otherwise
  simply *possessing* a file would be a way to ask whether it is filed somewhere in this wiki,
  which is a question about a page, asked without naming one.

- **A file whose bytes have gone missing says so, and says it is a problem worth looking at.**
  The media directory is a network mount and can go stale while everything else is fine. A
  download then answers "not available right now" rather than "no such file" — the first sends
  you to the mount, the second would send you to the database, which is the one place the
  problem is not. The entry stays in the list, and uploading the file again puts it back.

### Changed

- **Request bodies are capped in the application rather than around it.** Everything except an
  attachment is limited to 2 MB as before; the limit now lives where the routes are, so an
  attachment route can be excepted from it and so the limit is exercised by the tests instead
  of only by the running binary. Nothing else changes about what any endpoint accepts.
