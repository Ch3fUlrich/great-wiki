<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Themen**, at `/themen` and in the header beside »Projekte«: every topic you may see, and a
  page per topic listing what is filed under it. Topics were already in the file format and in
  the API; nothing could reach them. They can now be reached, and the way in matters more here
  than for the board or the projects list: the knowledge graph deliberately has no topic in it,
  so a topic page is the *only* route to a topic at all — an index nothing linked to would have
  left the whole feature unreachable rather than merely inconvenient.

  **Nesting is real, and it is real in the markup.** `Medizin/Darm` appears as »Darm« inside
  »Medizin«, as a list inside a list — not as a row indented a few pixels further, which says
  nothing to anybody who is not looking at the pixels. Opening »Medizin« shows everything
  underneath it, and the page says so, because a reader who did not know that would read a
  list of forty as a list of two and a mistake. A nested topic's page offers the way back up.

- **The same topics in the sidebar, as a second way through the same corpus.** The page tree
  and the topics are two answers to "what is in here", and the sidebar now switches between
  them — with two links, so the choice works before any script arrives, survives a reload, and
  is part of the address you would send somebody. **They are one query rendered twice, not two
  implementations**: the wiki asks for your topics once per page, and the index, the sidebar
  and the suggestions below every page's title are three renderings of that one answer. Two
  would be two answers to "which topics exist", and since a topic's own name says something
  about the pages carrying it, a second answer is a second chance to disclose one.

- **A page's topics are shown and changed on the page itself**, as chips beneath the title.
  Clicking one browses that topic; a field beside them files the page under another, and each
  chip has a named control that takes it off again. Tagging is something you do while
  reading — it does not mean opening the editor, and it files no new version of the page,
  because the page's words have not changed. Everything there is a link or an ordinary form
  submission, so it all works with JavaScript switched off, and afterwards the page comes back
  with the cursor on the topics themselves, so the change is read out rather than merely drawn.

- **Typing a topic offers the ones that already exist**, so people reuse a topic rather than
  inventing a second spelling of it — and the list offered is the same list the index shows,
  filtered the same way, because it *is* that list rather than a second request for one. A
  suggestion box is the surface where this is easiest to forget, precisely because it feels
  like a convenience rather than a disclosure.

  What is offered is the spelling a file states — `Medizin/Darm` — and the interface spells a
  topic that way everywhere for a plain reason: the string beside a chip is the string somebody
  retypes into the field next to it. A prettier separator would be retyped and quietly file the
  page under a topic nobody meant.

- **No count of what you were not shown.** A topic says how many pages *you* can see under it
  and nothing else — no total, no "und 3 weitere". A number about the rest is a fact about
  pages you may not read, and it is the one thing the filtering cannot take back afterwards. A
  topic nobody can show you a page of does not appear, is not counted, and answers exactly as a
  topic nobody ever typed — including when you type its address yourself, because a refusal
  that differed from an absence would confirm the name.

### Fixed

- **»Bearbeiten« no longer offers itself to people the page will refuse.** A page has said
  whether you may write it since topics landed, and the reader view was still deciding from
  "are you signed in" — so the control was offered to everybody with an account and the real
  answer arrived only after they had opened an editor and typed into it. It now reads the bit
  the page actually sends, which is the same verdict the refusal would come from. Filing a
  version still additionally needs an account, because a version records who wrote it; putting
  a page under a topic does not, because no version is filed.
