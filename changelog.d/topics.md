<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Added

- **Topics: what a page is about, and a way to browse by it.** A page states its topics in
  its frontmatter — `tags: [Darm, Ernährung]` — so they travel with the file through import
  and export exactly as its title and visibility do, and they can be changed through the API
  without rewriting the file. Anyone can type a new topic; there is no list to curate first
  and no step to forget. What already exists is offered while you type, so people reuse a
  topic rather than inventing a second spelling of it.

  Topics nest: `Medizin/Darm` is the topic `Darm` inside the topic `Medizin`, and that tree
  is the topics' own — independent of where the pages themselves live. **Opening a topic
  shows everything inside it, not only what carries that exact name.** A `Medizin` that
  listed two documents while forty sat under `Medizin/Darm` would be a dead end, and there is
  no other route to those forty: the graph deliberately has no topic in it, so a topic page
  is the only way in. Somebody who wrote `Medizin/Darm` did say the page is about Medizin;
  reading it otherwise contradicts the syntax they used.

  A topic's parent is its own path with the last segment removed, enforced by the database
  rather than checked in code — which is also what makes a cycle unrepresentable rather than
  merely rejected.

### Security

- **A topic exists, for you, exactly when you can read something filed under it.** Every
  aggregate view here filters per document, but a topic adds a leak the others do not have:
  its **name**. A board card's words belong to a page already checked; a backlinks panel has
  no strings of its own. A topic called `Kündigung Mietvertrag`, carried only by pages you
  may not read, would tell you such a page exists and roughly what it says — with its
  document list correctly empty.

  So a topic you can see no page of is not listed, not counted, not suggested, and answers
  the same way as a topic nobody ever typed. That last part is the point: a refusal that
  differed from an absence would confirm the name. Recorded as
  [ADR 0011](docs/decisions/0011-what-a-topic-discloses.md), including the residual channel
  and what would make it worth revisiting.

- A topic that no page carries is **deleted**, so a name typed once on a page later retagged
  cannot sit in the table forever, unreachable and uncounted, still saying what somebody
  once called something.
