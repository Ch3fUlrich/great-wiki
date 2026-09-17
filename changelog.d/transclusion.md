### Added

- **A page can show another page, or one section of it, live.** Press »Seite einbetten« while
  writing and pick a page from the list of the ones you may read — the same picker the link
  dialog uses — then either take the whole page or choose one of its headings. What lands on
  your page is a **reference**, not a copy: nothing of the other page is stored here, and what
  a reader sees is fetched when they open your page. Correct the source and every page quoting
  it is correct too, at once, with nothing to re-copy and nothing left stale.

  It is **visibly a quotation**: a framed block headed »Eingebettet aus« with the source page's
  **current** title as a link to it. Never seamless, deliberately — you have to be able to tell
  which words on a page are that page's own, and an edit made "here" would land silently on a
  different page that different people may read.

  **A section is named by an anchor that survives being re-worded.** Every heading quietly
  acquires a permanent identity the first time its page is published, and an embed holds *that*
  rather than the heading's words or its position. Rewrite »Dosierung« as »Dosierung bei
  Erwachsenen« and every page quoting that section still quotes that section.

- **An embed never shows a reader a page they may not read.** If the quoted page is one they
  have no access to — or it has been thrown away, or purged, or was never there — the frame is
  replaced by the words *you* wrote as its label, with no title, no address, no content and no
  count. Those four cases are deliberately told apart by nobody: "you may not see this" and
  "there is nothing here" differ only in confirming that something is there. The verdict is
  asked afresh on every read, so a withdrawn permission empties the frame at the next request
  rather than whenever somebody remembers to.

- **An embed whose section has been deleted says so, and stays.** The frame remains where you
  put it, reading that the section no longer exists, with a link to the page it came from. It
  does not quietly fall back to the whole page — that would swap a dosage table for somebody's
  entire page without saying a word — and it does not disappear, because nothing here vanishes
  silently.

- **A quoted checklist keeps its live state and is ticked where it was written.** The boxes
  inside a frame show exactly what the source page shows, right now, and cannot be pressed
  here; the frame says where they can be. One line, one record, one board — a box that could be
  ticked from two places would be two different answers about the same task.

- **A ring of embeds is caught and named.** If a page would end up quoting itself, directly or
  through others, the frame says so and names the pages that lead back — rather than drawing
  something misleading or nothing at all. An embed inside an embed is shown as its label rather
  than expanded, so one page can never pull in a chain of others.

- **Export and re-import keep every frame, not merely every word.** An embedded page is written
  into the backup as `![Beschriftung](einbettung:<seite>#<abschnitt> "/pfad")`, and a heading's
  permanent identity is written after its words as `{#…}`. Loading the files into a **fresh,
  empty** wiki therefore restores the frames as well as the prose: the addresses in the files
  find the pages again, and the anchors in the files still name the same sections. Without the
  anchors in the files, a restored copy would have kept every page and left every section embed
  pointing at nothing — and unlike a link, there would have been nothing to fall back to.
  `EXPORT-README.txt` states both, and states plainly that **none of an embedded page's words
  are in the embedding page's file**: the other page's own file is where they are.
