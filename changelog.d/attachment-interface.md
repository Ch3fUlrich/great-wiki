<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Anhänge**, on the page itself, below what is written there: every file the page carries,
  what it is, how big it is, who put it there and when. The store, the upload and the download
  existed and nothing in the interface could reach any of them — a file could be attached only
  by making an HTTP request by hand. The list sits under the document rather than beside it,
  because it is a fact you want once you have read the page and because a 40 MB scan and the
  control that adds one do not belong in a column of chrome.

  **The list is what says a file is attached** — not the text. A picture cut out of a
  paragraph is still on the page, and this is the only place that shows it. Nothing in the
  section is derived from the words of the document, so nothing about editing a page can
  quietly detach a file from it.

- **»Hochladen«, offered where the wiki would actually accept it.** The control appears where
  the page says you may write it *and* you are signed in — the same pair the API checks, and
  it checks the account first, because an attachment records who put it there and "nobody" is
  not an answer. Where it is withheld a sentence takes its place saying which of the two is
  missing: a control that is silently not there reads as a fault rather than as an answer.

- **A refusal is a sentence somebody can act on, in German, carrying what the server said.**
  A type this wiki does not store, a file past the size it accepts, a name already taken on
  the page: each comes back as a German explanation with the API's own words inside it and a
  promise that nothing was attached — never as "Fehler 415". The interface deliberately keeps
  **no list of acceptable file types and no size limit of its own**. Both belong to the server,
  both change, and a copy in the browser would refuse a file the wiki would have taken —
  before the request was made, with nothing in any log to say why.

- **A download is a link.** It works before any script has loaded, it opens in a new tab, and
  it can be saved with a right-click. The address is the one the API hands over, which names
  the page and never the file's content address: fetching a file is authorised against the page
  you reached it through, and the interface has no way to assemble any other kind of address.

- **The whole section works with JavaScript switched off.** The list is markup in the first
  response; the upload is an ordinary form submission. Afterwards the page comes back with the
  cursor on a sentence naming the file that arrived — read out, not merely drawn — and that
  sentence is taken from the list itself, so it can never name a file the list does not show.

### Known limitations

- **Files are still not placed *inside* the text.** The decision puts a file inline in the
  prose as well as in the `Anhänge` list; only the list and the upload are here. The inline
  placement needs a new kind of block in the document model, which is a separate piece of work
  with its own hazards, and it is not part of this change. Dragging a file into the editor does
  nothing yet.

- **There is no »Entfernen« in the interface yet.** The API can take a file off a page; nothing
  here asks it to.

- **A deployment must raise the front end's request-body limit, or large uploads never reach
  the API.** `@sveltejs/adapter-node` refuses a body over `BODY_SIZE_LIMIT` — **512 kB by
  default** — before the upload is handled at all, while the wiki itself accepts 250 MB per
  file. The development server applies no such limit, so this appears only in a container, and
  it appears as the adapter's own error page rather than as any of the sentences above. Set
  `BODY_SIZE_LIMIT` on the web service to at least the size the API accepts.
