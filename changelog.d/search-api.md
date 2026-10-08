<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **`GET /api/search?q=` finds pages, topics and tasks by their words.** Pages are found by
  title and text, a title counting for more; topics by their name (`kundigung` finds
  `Kündigung Mietvertrag`); tasks by their title, each with the page it hangs off. A page hit
  carries its title, where the page is now, and an excerpt around the first match, given as
  pieces of text with the matching ones marked rather than as markup. Signed-out visitors may
  search; they find what they could read anyway.

### Security

- **Search shows a caller only what they may read, and says nothing about the rest.** Every
  page the index offers is put through the same permission check as opening it, one by one,
  before it can be a result; a topic or task is offered only if the topic index or the board
  would show it to that caller. The excerpt is cut from the page as that caller is allowed to
  read it, never from the index's own copy. There is no total, no "n more" and no count of
  anything left out, and a search for words that occur only on a page the caller may not read
  is answered byte for byte like a search for nothing — as are a blank query, an over-long
  one, and one made of punctuation. A trashed page is never a result.
