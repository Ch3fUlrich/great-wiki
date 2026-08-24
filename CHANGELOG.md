# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); newest first.
Entries describe the *effect* of a change, not the diff.

## [Unreleased]

### Added

- **Aufgaben**, at `/aufgaben` and in the header beside »Projekte« and »Graph«: every to-do
  you may see, in three columns — **Offen, Läuft, Fertig** — narrowable to one project. The
  columns are fixed and the same everywhere, so a card's column means the same thing on
  whichever board you meet it. The same board also sits on a project's own home page, below
  the page and above its subpages, because that is where you look when you are thinking about
  that project, and being sent elsewhere to see its tasks breaks exactly that. The two are
  **one board with a filter, not two boards**: one request, one rendering, one way to move a
  card. Two implementations would be two answers to "which tasks exist", and since a card
  says that a page exists and what somebody wrote on it, a second answer is a second chance
  to disclose one. The global board is the only place a to-do belonging to no project appears
  at all, and that is why both exist: a card used to be findable only on the board of the
  project it was filed under, which left one written on a page nobody had claimed with nowhere
  at all to appear — exactly how a to-do goes missing. Narrowing the
  board to a page that is nobody's home gives an **empty** board rather than the whole
  wiki's: that is what an ordinary page's board is, and it is the answer a page has to be
  able to give without knowing in advance whether it is a project's home.
- **A card can be moved without a pointer.** Every card carries a named button for each of
  the two columns it is not in — "»Kabel bestellen« nach Läuft verschieben", not
  "Verschieben" repeated down a column — and pressing one is an ordinary form submission to
  the server. It works with JavaScript switched off, and afterwards the page comes back to
  the board the card was moved on, filter and all, with the cursor placed on a sentence
  saying what happened, so the move is read out rather than merely drawn. Dragging a card
  presses the same button: an addition, never the only way in. The move changes the card and
  nothing else — no page is rewritten, no version is filed, and nobody needs permission to
  *write* a page in order to move a card that came from it. The page owns the words; the
  record owns the state.
- **A card says which page it was written on, and says so when no page holds it.** A card
  written as a line in a page names that page and links there; one created on a board says it
  was made there and belongs to no page. Both are facts about the card and neither is a
  blank — "where did I write this?" is the first question anybody asks of a board, and naming
  the project's home page for a card no page ever held would claim a line exists somewhere
  that never did. The name is looked up when the board is read rather than
  copied onto the card when it is made, so renaming or moving a page does not leave boards
  saying what it used to be called; that is the rule a link between pages already follows. A
  card whose line has been deleted stays, marked »Abgelöst«, and the marker says what is
  still true of it: the page no longer holds the words, and the due date and the person it
  rests on are still somebody's.
- **A card you may see but may not move is shown, and says so.** Hiding it would hide nothing
  — if it came from a page, its checkbox is on that page for anyone who may read it — and a
  to-do that quietly vanishes from a board is the failure this whole design exists to
  prevent.
- **Due dates are shown, and an overdue one says "Überfällig seit …" in words.** Colour is
  the second channel, never the only one, which is the line this project already holds in the
  diff views and the sortable tables. A date with no time is a whole day, so a task due today
  is due today until the day ends rather than overdue from one second past midnight.
- **Projekte**, at `/projekte` and beside »Aufgaben« in the header: every project you may
  see, and the form that starts a new one. A project is made by naming the page it belongs to
  — its Startseite — and that is the whole of it; there is no separate object to fill in,
  because a project *is* a page and the pages beneath it, and its board is that subtree's
  to-dos together with the loose cards filed on it directly. Whoever may write that page may
  make it a project's home. The path may be typed with or without its leading slash, or
  pasted whole out of the address bar, which is how people actually say "this page".
  **The list is the page, deliberately.** A »zu einem Projekt machen« button on each page
  would have been cheaper and would have buried the one place you go to ask *which projects
  exist*; putting it in the admin console would have made a project something you ask an
  administrator for, which is how a thing meant to be used every week ends up used twice a
  year.
- **Creating and deleting a project work with JavaScript switched off, and a refusal is a
  sentence rather than a number.** Both are ordinary form submissions; creating comes back as
  a redirect, so reloading the list does not offer to make the same project a second time.
  Naming a page that is already another project's home says exactly that and says which of
  the two ways out to take, with the project it collides with on the list right there; a page
  you may not edit says the write right is missing, not that "ein Fehler" occurred; a page
  that does not exist is named, so a typo is visible as a typo. Deleting asks first, names
  the project, and says what goes with it — the cards made on its board, and neither the
  pages nor the to-dos written as lines in them. Every control names what it acts on rather
  than repeating "Löschen" down a column, a failed field is announced in words as well as
  outlined in red, and a project with no tag says so instead of leaving a cell blank.
- **A checkbox written in a page is a to-do, all the way through.** `- [ ] Stuhlprobe
  einschicken` used to come back as an ordinary bullet whose *words* were "[ ] Stuhlprobe
  einschicken": the brackets sat on the page, went into the search index, and would have gone
  into the anchor of any heading written that way. A checkbox line now imports as a checklist,
  ticked or unticked exactly as written; the reader draws a real checkbox for each line, so
  its state is announced as "checked" or "not checked" rather than left to whoever can see the
  tick; and `great-wiki export` writes it back out as `- [ ] ` and `- [x] ` and it comes back
  the same document — mixed lists stay split, a numbered list's plain runs keep the numbers
  they had, nesting keeps its depth, and a bullet whose words merely *look* like `[ ] etwas`
  stays a bullet rather than turning into somebody's new to-do. Export had been naming and
  skipping every page holding a checkbox, and one refusal fails the whole run, so a single
  checkbox anywhere would have shut the backup path for the entire wiki.
  The boxes are **deliberately not clickable while reading**. A to-do's state belongs to its
  record and not to the words on the page: a checkbox wired up here would mean needing
  permission to *edit* the page in order to tick something off, and would file a revision
  nobody typed. Ticking is done on the board.
- **Publishing a page reconciles its checklist against the to-dos.** A line nobody has a
  record for gets one, a line whose words changed updates its record's title, and a line that
  is gone leaves its record behind, marked. It happens inside the same transaction that
  writes the revision, so there is no moment where the page says one thing and the board says
  another, and a publish that fails leaves neither. The words come from the page and nothing
  else: status, assignee, due date and the card's place in its column live on the record and
  are never written by a publish — which is what lets a card be dragged without filing a
  revision nobody typed, and stops the next save from quietly undoing the drag. A ticked box
  is read exactly once, when the record is created, so a checklist imported from markdown
  arrives with its finished lines finished; after that the record decides, and the page's own
  box is a stale copy that publishing ignores.
- **Identity is minted by the store, once.** A checklist line acquires an invisible id the
  first time its page is published and that id is stored with the page, so publishing the
  same page again finds the same to-dos rather than a fresh set. A line arriving with no id —
  everything imported from markdown does, and `seed --update` re-converts the same file on
  every run — adopts the record for its words rather than minting a second one. Without that,
  every save would shed every card on the page, with its dates and its assignments, and
  nothing would have gone wrong loudly enough to notice. Two lines reading the same words
  keep two records, and a checklist copied and pasted in the editor becomes a to-do of its
  own. Retyping a deleted line makes a *new* to-do and leaves the old one visibly detached
  rather than one task quietly turning into another; putting the same line back — an undo, or
  restoring an older version of the page — re-attaches the card it had, with its state.
  One rough edge, and it needs its own fix rather than time: `seed --update` decides whether
  a file changed anything by comparing the stored block tree against the freshly converted
  one, and a markdown file cannot carry those ids. So a page holding a checkbox looks changed
  on every run and gets a revision that says nothing — the thing that comparison exists to
  prevent. The cards themselves are unharmed, being found again by their words, and pages
  without checkboxes are untouched. The fix is for the comparison to ignore a task item's id
  the way `export` already does.
- **To-dos and boards have somewhere to live, and can be reached over the API.** A task is
  one record with an optional anchor to the line in a page that authored it, so a to-do
  written while planning and a card created on a board are the same kind of thing rather than
  two that have to be kept in step. Over the API a card can be created on a board, moved,
  given a due date, handed to somebody, and thrown away. The three columns are built in and
  the database itself refuses a fourth, so a status this software does not understand cannot
  be written by anything, including a repair script. A board answers with all three even when
  two are empty, because "nichts läuft gerade" is something a board has to be able to say,
  and a status that is not one of the three is refused by name rather than quietly filed as
  *Offen*, which would silently reopen something somebody had finished.
- **Every page now has a history you can read, compare and restore from.** »Verlauf«, beside
  »Bearbeiten«, lists every published Fassung of the page — newest first, with who wrote it,
  how long ago, what they said they were doing, and how much the page grew or shrank. Until
  now the wiki had thirty-four versions of some pages and no way to look at a single one of
  them.
- **Two versions can be compared three ways, and three is the point.** A **Prosa** diff shows
  which words changed; a **Struktur** diff shows which blocks were added, removed, moved or
  rewritten in place; a **Design** diff shows what changed about how the page looks — a
  heading's level, a table column's alignment, a sentence somebody made bold. A word-level
  diff on its own answers "keine Änderungen" for a page that was plainly restyled or
  reordered, and a history that says nothing changed is worse than no history, because it is
  believed. A block that moved is reported as **one** change rather than as a deletion plus
  an addition, so tidying a page does not read like rewriting it. Additions and removals are
  marked with a word and a symbol as well as a colour, so the diff is legible without colour
  vision, in print, and in a black-and-white screenshot.
- **Any version can be read as a whole file**, in the same three files an export writes: the
  markdown, the metadata, and the block tree the database actually holds. When a version
  cannot be written as markdown faithfully — an image, a link the tree cannot express — it
  says so rather than showing a quietly lossy file.
- **Restoring publishes the old version as a new one and deletes nothing.** What you restored
  past is still in the history afterwards, so the restore is itself undoable — by restoring
  the other one. It asks first, and the question names the version and says what happens to
  the current one.

- **A page's visibility can now be changed, in the admin console, beside access.** Until now
  the value arrived from frontmatter at import and nothing in the running system could write
  it — the console showed it as a badge, which reads as settable state and was not. It is
  one deliberate act by a person: whoever administers the page's path may do it, an
  unrecognised value is refused rather than defaulted, and every change writes an audit row
  recording what the page **was** as well as what it became. Who may do it and why is
  [ADR 0008](docs/decisions/0008-who-may-change-a-page-s-visibility.md); the short answer is
  that being able to read or write a page never widens it, and anybody who *can* do this
  could already publish the page by writing an "Alle, auch nicht angemeldete" entry on it.
  `seed --update` still refuses metadata changes, unchanged: a bulk file drop is not a
  person, and a stray `visibility: public` in one of two hundred files must not publish a
  page with nobody watching.
- The access panel now answers its own question. Above the table it lists **every** way into
  the page, most open first: a public share link, the page's visibility, the »Verwaltung«
  reach that reads every page without any entry at all, and then the entries. The two that reach past everybody somebody deliberately named — an
  "Alle, auch nicht angemeldete" entry, and a public page — are marked as such, and such an
  entry now carries "Offenes Internet" in the table so it cannot be mistaken for a team.
- **A Content-Security-Policy**, issued by the application rather than by either proxy —
  see [ADR 0007](docs/decisions/0007-content-security-policy.md). Scripts are admitted by a
  per-response nonce with no `'unsafe-inline'`, which is what a proxy could not have done:
  neither Caddy can mint a nonce, and without one the only way to keep the page working is
  to allow every inline script on it. The API's own pages — the sign-in form and the
  invitation page — get a second, stricter `default-src 'none'` policy of their own, because
  `/auth/*` is routed to the API and never sees SvelteKit's headers at all.
  One directive is loosened and it is worth knowing which: `style-src-attr 'unsafe-inline'`,
  because Svelte renders its `style:` directive as a literal `style=` attribute and CSP has
  no nonce or hash mechanism for attributes. It is confined to attributes — an injected
  `<style>` element is still refused.
- **A graph of your pages and the links between them**, at `/graph`, optionally narrowed to a
  subtree. Nodes are pages and edges are links somebody deliberately wrote — topics are not
  drawn, by decision, so the graph shows connections a person made rather than similarity a
  machine inferred. An edge appears only when you may read **both** of its ends: one readable
  end would still reveal that the other page exists, and a node label would reveal its title.
  Drawn as plain SVG with a deterministic layout and no charting library, because a corpus of
  tens of pages does not need one.
- **"Verweist hierher"** on every page: which pages link to this one, filtered per document,
  and rendered not at all when the list is empty rather than as an empty heading.
- An absolute address pointing at this wiki now counts as an internal link. Pasting a page's
  URL out of the address bar is the natural way to link it, and it previously produced a
  working link that never appeared in the graph or in any backlinks panel, with nothing to
  say why. The deployment's own origin arrives as configuration — the store cannot know it,
  and a request's `Host` header must never be used to guess, being attacker-controlled. With
  no origin configured, every absolute URL stays external exactly as before.

### Changed

- **A list that mixes checkbox lines with plain ones stays mixed.** It comes back as a
  checklist and an ordinary list side by side, in the order written, rather than as one list
  with every line turned into a to-do. In this wiki a checkbox line *is* a to-do, so an
  unticked box invented on a line nobody marked would put an item on a board that nobody
  wrote — the one cost this design was weighed against, and accepted on the grounds that it
  does not happen. A **numbered** list holding checkboxes keeps the checkboxes and loses the
  numbering on those lines, because a checklist has no numbers; that is reported with
  everything else an import could not carry rather than changing the page quietly, and the
  list's plain lines keep the number they had, so nothing renumbers behind your back.
- **The graph is readable at the size this wiki is actually used at.** Thirty-five pages with
  titles like »Table 4: Foods & Nutrients for Microbiota/SCFA Balance + Neurotransmitter
  Precursors« used to be drawn with every name centred under its node whatever was already
  there — forty-four pairs of names printed on top of one another, and seventeen of the
  thirty-five running off the side of the picture, where the frame cut them mid-word with
  nothing to say they had been cut. Now none of them overlap. Pages are spaced by how wide
  their names are rather than as bare points, each name goes in the first free place around
  its node — underneath it, as before, wherever there is still room — and a name too wide to
  draw is shortened with an ellipsis instead of running past the edge. **No page is ever left
  unnamed**: a name that cannot be placed cleanly is drawn anyway, because a nameless node
  hides that the page exists at all. The whole title is never lost either — it is still what
  a pointer shows and what the text list under the drawing says, which is what makes the
  graph usable without seeing it and is unchanged. The frame is wider now and grows downwards
  as pages are added, so a wiki twice this size still gets a diagram rather than a thicket.

- **The access table no longer claims to say who reaches a page; it says what is entered on
  it.** It never showed the two ways in that need no entry — somebody with »Verwaltung«
  reach reads every restricted page in the wiki, and an internal page is open to every group
  with internal reach — and its empty state said "kein Zugriff eingetragen … es gilt allein
  die Sichtbarkeit der Seite", which is true about entries and reads as "nobody else gets
  in". That sentence is gone.
- Revoking the **last** entry on a page now says what it does. Removing it does not close
  the page: the nearest entry further up applies again, here and on every page below that
  carries nothing of its own, and the dialog names that page and lists the rights that come
  back. The old wording said inherited rights "bleiben davon unberührt" — true, and read as
  "they stay where they are".

- Between two publishes, what an editor opens and what a reader sees are allowed to differ.
  A page says what its newest revision says; the editing session is somewhere else until
  somebody publishes it. That is what "publish" means, and it is a real change from a wiki
  that saved every thirty seconds whether you meant it or not.
- `X-Forwarded-For` is now read, but only on a request the proxy boundary attested, and
  only its rightmost entry — the one Caddy appended and no client can write. There was no
  trustworthy client address in the application before this; the TCP peer is Caddy and a
  raw header is whatever the caller typed.


- An authenticated account no longer reaches restricted content by virtue of being signed
  in. Reach follows the Authelia group, so an account by itself confers nothing beyond
  public.

- Identity storage: principals (from OpenID Connect or local accounts), teams and
  memberships, path-scoped access grants that inherit down the document tree, and an audit
  log.
- Default reach follows the verified Authelia group, held in a `group_roles` table rather
  than in code, so mapping a new group is a row and not a release. `admins` reach
  restricted content, `users` reach internal, anything unmapped reaches public only —
  expressed as the *absence* of a row, so a forgotten configuration can never widen access.
  The admin baseline confers reads only; writing still needs an explicit grant.
- Grants do not union up the tree: the nearest ancestor holding any grants wins outright,
  which is what makes it possible to narrow access on a subtree rather than only widen it.

- The proxy shared secret is now an enforced per-request check rather than a startup
  assertion. Requests without it, or with a wrong value, are refused before routing — so an
  unknown path returns 403 rather than 404 and cannot be used to probe what exists.
  Comparison is constant-time; an empty configured secret returns 503 rather than allowing
  through, because an unconfigured secret must never silently disable the boundary.
- Enforcement is derived from the bind address: loopback disables it, so local development
  needs no proxy in front, and anything public requires it.
- `gw-auth`: the permission engine as its own crate. One `can()` decides every
  authorisation in the system and checks authentication *before* group or team membership,
  so a forged group on an anonymous request cannot pass. Local account credentials use
  argon2id with Authelia's exact parameters, and a malformed stored hash denies access
  rather than panicking.

- `great-wiki seed --content <dir>` loads a folder of markdown files with YAML frontmatter
  into the database, so there is real content to develop against before the editor exists.
  The markdown-to-block conversion it needs is also half of the export round-trip, so this
  is foundation rather than scaffolding.
- Seeding refuses to guess. A missing title, absent frontmatter, a colliding path or a
  parent document that does not exist are each reported by name with the reason, and the
  command exits non-zero — deriving a title from a filename would silently publish a page
  at a path nobody chose.
- Markdown constructs the block model cannot yet represent are reported and their text
  kept, never dropped. Emphasis and link destinations survive; an image keeps only its alt
  text, and a horizontal rule is dropped.
- `content-example/` makes the repository runnable straight after cloning, and gives CI
  something real to validate.

- Reader interface: layout with a skip link, dark and light themes following system
  preference, recursive tree navigation, document pages with an on-this-page outline, and
  error pages. Blocks render through a component that emits only the kinds it knows, so no
  untrusted HTML is ever constructed and there is no sanitisation step to get wrong.

- Document content model: a ProseMirror-shaped `Block` tree with plain-text extraction and
  a heading outline whose anchor ids are transliterated to ASCII. `Visibility` defaults to
  `Restricted`, so a document arriving with no stated visibility is never world-readable.
- SQLite store with the initial schema: documents keyed by a materialised path, with
  sibling ordering, soft delete, and a UNIQUE path so a slug collision fails loudly instead
  of silently overwriting a page.
- `great-wiki` binary with `serve` and `check`, and fail-closed startup validation: a
  synthesised development identity cannot be combined with a non-loopback bind, a public
  bind without a proxy secret is refused, and port 8090 is rejected outright because
  `omnigraph-viewer` already owns it.
- Read API — `/api/health`, `/api/tree`, `/api/documents/{*path}` — with visibility enforced
  in the retriever. Restricted documents return 403 rather than a misleading 404, and
  restricted titles are filtered out of the navigation tree entirely.
- Integration tests that exercise the real router rather than calling handlers directly, so
  a route registered without its permission check cannot pass the suite.

- Rust workspace with `gw-core`, the pure-domain crate, and the `cargo test` /
  `clippy -D warnings` / `fmt --check` gate that every later task must pass.
- `slugify` with German transliteration (ä→ae, ö→oe, ü→ue, ß→ss), so German titles produce
  readable, collision-free slugs. `Präbiotika` becomes `praebiotika`, not `pr-biotika`.
- SvelteKit 2 / Svelte 5 application skeleton with the Node adapter, Vitest, and the
  `npm run check` type gate. Node 24.19.0 pinned in `.nvmrc`.
- `slugify` in TypeScript, mirroring the Rust implementation, with a test corpus shared
  verbatim between the two so they cannot drift apart.

- Repository foundations: MIT licence, public-repo-safe `.gitignore`, `.gitattributes`
  enforcing LF on scripts and configs, and `.graphifyignore`.
- Agent instruction files — `AGENTS.md` (hub: skills, memory, architecture rules,
  verification commands) and `CLAUDE.md` (Claude-specific delta only).
- MCP wiring: project-scoped `.mcp.json` pinning the Omnigraph graph to `great-wiki`, with
  the approval gate in untracked `.claude/settings.local.json`. Graphify is deliberately
  left to user scope; Serena is configured by `.serena/project.yml`.
- `.serena/project.yml` with a conservative language list. TypeScript is omitted on purpose
  until the running image is verified to carry its language server — an absent server takes
  the entire Serena instance down rather than degrading.

### Fixed

- **Opening a page that contains a checkbox no longer destroys it.** This was the serious
  one. The editor builds the document by looking each block up by name, and a name it did not
  know was not skipped — it was **deleted from the shared document**, sent to everyone else
  editing, and saved into the next revision, with nothing shown and nothing logged. So from
  the moment a checkbox could be written, the first person to open that page for editing
  would have silently removed the checklist from it. The editor now knows checklists, keeps
  each box as it was, and keeps the identity a to-do is tracked by, so nothing is lost by
  opening a page and nothing is lost by editing one. Nothing already written was affected:
  there is not one checkbox line in the content this wiki was seeded from, and the editor
  learned this before any content with a checkbox reached it.
- A failed request for the tasks is never reported as a board with nothing on it. On the
  global board it says so plainly; on an ordinary page it says only that *if* a board belongs
  here it could not be loaded — because a request that failed cannot tell a project's home
  page from any of the other pages in the wiki, and claiming one either way would be
  inventing the half it never learnt.

- The visibility and permission dropdowns can be operated with a mouse. Both are portalled
  out of their dialog, and `@zag-js/popper` writes `z-index: var(--z-index)` on them inline —
  a variable it fills by mirroring a child's computed `z-index`, which was never set, so it
  mirrored `auto` and the dialog covered the open list. Every check drove them with the
  keyboard, which worked throughout, so nothing noticed that the mouse route was dead.
- A visibility or subject picked on one page and then abandoned no longer follows you to the
  next one. The choice lived in component state that a client-side navigation does not reset,
  so opening another page's dialog offered the previous page's answer with the confirm button
  already live — on the one control that publishes to the open internet, and against that
  code's own stated reason for putting »Öffentlich« last in the list.
- The panel no longer claims "Kein Zugriffseintrag" for a page that is reached through an
  ancestor's entry. That sentence was the exact under-statement this screen was rewritten to
  remove, and it could be reintroduced with every one of the 298 tests still passing — the
  derivation behind it had no test at all.
- Two routes into a page were described by group where an individual promotion also carries
  them, so the copy credited Authelia with a decision it does not make.
- A protocol-relative link no longer becomes a page of this wiki. `//example.org/seite`
  failed to parse without a base, fell through to the relative branch, and was resolved
  against the page being edited — so a link meant for another site silently turned into a
  link to a different page here, and would have been published that way.
- A relative link is resolved against the page it was written on, not against the root.
  `nachbar` written on `/rundgang/tabellen` recorded an edge to `/nachbar` while clicking it
  navigated to `/rundgang/nachbar` — so the graph named one page and the link went to
  another.
- Two edges could collide into one key. `/x → /y/z` and `/x/y → /z` produced the identical
  key when the two paths were concatenated, which server-rendered fine and then failed
  hydration of the entire page in the browser. It would have arrived silently as the wiki
  grew.
- The graph's screen-reader label counted differently from its visible caption, and
  under-reported — the wrong one of the two to be wrong.
- A subtree whose pages all link outward said "no links here", which was untrue: there were
  links, they simply left the subtree. It now says which of the three things it means.
- A misspelled `GW_PUBLIC_URL` is refused at startup rather than accepted and silently
  ignored. A value like `mailto:…` parses as a valid URL but can never match a page origin,
  so the feature would have been permanently missing with nothing to explain why.
- Text can be **bold, italic, struck through, code, or a link**, and stays that way. Until
  now `Block` — what a published revision stores — had no field for inline formatting, so
  the CRDT carried a bold word faithfully while publishing threw it away, and the editor
  deliberately shipped with no formatting controls rather than offer something the system
  would discard. Marks now survive the whole chain: markdown import, the CRDT, a published
  revision, markdown export, and the rendered page.
- One canonical nesting order, defined once. `[**Text**](url)` and `**[Text](url)**` mean the
  same thing and are now stored the same way, which is what lets an export re-import to the
  identical document. Two definitions that agreed by coincidence is exactly how the two
  halves drifted apart the first time.
- The editor's formatting controls write the same mark names the server reads. TipTap's
  stock names are `bold` and `italic`; this system's are `strong` and `em`. Left alone, a
  word bolded in the browser would have been written into the shared document under a name
  the server does not recognise and dropped on publish — with every test still green,
  because no test crossed that boundary. The wire format is now pinned by a test that reads
  the actual bytes.
- A link's address is checked before it is rendered. `javascript:` and every other
  executable scheme fall through to plain text instead of becoming a clickable anchor —
  judged with the browser's own URL parser rather than a pattern, so the case-folded and
  embedded-whitespace spellings resolve the same way the browser would resolve them.
  Relative links keep working, which matters: a scheme-only rule would have silently
  de-linked 23 working links in the existing corpus.
- Pages record which pages they link to. Publishing a revision extracts the links from the
  body in the same transaction that writes the revision, so a publish that fails cannot
  leave edges behind for a version that does not exist. That table is the graph.
- **Backlinks are filtered per document.** A page that links here but which you may not read
  is omitted entirely — not shown as "a page you cannot see", because the fact that it
  exists, and how many there are, is itself the disclosure.
- In-place editing on the rendered page: TipTap over the shared CRDT, opened only when
  somebody asks to edit and only after the server has agreed to the session — so a person
  who may not write sees an honest German refusal, never an editor that discards what they
  type. A refused upgrade reaches the browser as close code 1006 with no status, which is
  indistinguishable from an outage, so the two are told apart by asking whether the page is
  still readable. The editor's node schema is asserted equal to the server's block kinds by
  a test, and so is every attribute `gw-core` writes: TipTap deletes an element it cannot
  name from the CRDT and drops an attribute it does not declare, which for the tables in
  this corpus would have silently destroyed column alignment one edited cell at a time.
  Inline formatting was deliberately absent when this shipped, because a revision had
  nowhere to store it — see the marks entry above, which closed that and gave the editor its
  controls. The page content is still server-rendered in full, with the editor loaded
  afterwards as a separate chunk.
- Collaborative editing over a WebSocket at `/api/collab/{path}`. Authorisation happens
  before the upgrade and asks for `Action::Write` through the store's one permission-checked
  accessor: reading a page is not permission to join its editing session, and an
  administrator viewing as somebody else cannot open one at all — an upgrade is a GET, so
  the layer that refuses every mutating request while that mode is active did not cover it.
  That hole was open, and a snapshot of such a session would have been filed in the
  append-only history under the impersonating administrator's name.
- The session is re-authorised every ten seconds and closed with a policy code and a reason
  when it should no longer exist — a revoked grant, a deactivated account, an ended session
  or a deleted page. A malformed or forged update closes the connection rather than being
  discarded, because a socket that swallows updates looks to the person typing like an
  editor that is saving their work.
- `POST /api/collab/{path}` publishes what is in the live session as a revision, and a
  background sweep writes out every room that has changed every thirty seconds and drops the
  ones nobody is in — so an unpublished editing session survives a restart, and a crash can
  lose at most that interval.
- Limits, because the endpoint faces the internet: one mebibyte a message, 240 messages and
  one mebibyte a second per connection, thirty-two connections per page.
- A reader can now see where a page sits, what hangs below it, and who is allowed to read
  it. Every page carries a breadcrumb from the root — titles taken from the tree, never
  assembled from path segments, so a page whose ancestors the caller may not see gets no
  invented parents — a list of its children as links, and a panel stating visibility in a
  full German sentence. "Öffentlich im Internet … ohne Anmeldung" rather than the bare
  "Öffentlich", because in an intranet that word routinely means "everyone in the
  organisation" and this wiki answers on the public internet. A visibility value the server
  cannot parse is reported as "Eingeschränkt", which is what the permission engine actually
  does with it, rather than as "unbekannt". Language is named only when it differs from the
  German interface, and document type only when it is not the default; a row of facts that
  says nothing teaches people to stop reading the panel.
- Subpages are listed because a container page rendered a back-link and nothing else, which
  reads as a page somebody forgot to write. Its children *are* its content, and they were
  previously visible only in the sidebar tree — which on a phone sits at the very bottom of
  the document.
- **Not** shown, and deliberately not faked: when a page was last edited and by whom.
  `/api/documents/{path}` still returns no revision fields, so the row is written, styled and
  tested and renders nothing at all until real data is passed — a dash or an "unbekannt"
  beside three genuine facts would be read as a fourth. »Verlauf«, above, is where a page's
  history lives in the meantime.
- An import creates the page **and** publishes its first revision, in one transaction.
  Creating a document used to write the body straight into `documents` and record no
  revision at all, so every seeded page began with an empty history: the first edit anybody
  made became revision 1 with no parent, and the first diff had nothing to compare against.
  A page can no longer exist with a body and no revision, nor a revision and no page — a
  failure anywhere in between takes both back, which is forced in a test rather than argued
  in a comment.
- Revision 1 says who wrote it, including when the honest answer is "no account did".
  `seed --as <account>` files it under that account. `seed` with no account — the operator
  bootstrap path — files it under an author that is not a person and can never become one:
  the id `system:import`, which is outside the space account ids are minted in, shown as
  "Import (kein Konto)". Attributing a bootstrap corpus to whoever happened to run the
  command is a lie a history then keeps for ever, because the byline is a snapshot that is
  deliberately never corrected. Anything rendering a byline asks
  `Revision::author_is_an_account()` rather than reading the name.
- `Store::insert_document` is now `Store::create_document(author, doc, summary)`. The rename
  is the point: it was one INSERT and is now a page together with its history, and the old
  name is how a second write path sat unnoticed beside the revision system (AGENTS.md rule
  1). Creation and editing now go through one function that writes a revision, so "a body
  changes only by publishing a revision" is a property of the code rather than a rule
  everyone has to remember.
- Pages imported before this change keep their empty histories until they are imported
  again; nothing was backfilled, because a revision nobody published is not history.
- `great-wiki export --content <dir> --as <account>`: the page tree written back out as
  markdown with YAML frontmatter, folders mirroring the tree, so `export` then `seed` into
  an empty wiki reproduces the database exactly — proven over the shipped corpus, compared
  document by document including the full block tree, not by eye. Reading is
  permission-filtered like every other retrieval path: pages the account cannot read are
  not in the export, and the report says so rather than pretending to be complete.
- Export refuses rather than degrades. Every document is re-imported and compared *before*
  its file is written; one that would come back different is not written at all, is named,
  and fails the run — the mirror of the seeder skipping a file it cannot place. It also
  refuses to write into a directory holding markdown it did not put there, because
  replacing hand-written source with an export destroys the bold and the links the database
  never kept.
- The fidelity warning outlives the terminal: `EXPORT-README.txt` sits beside the files
  saying that the database stores no inline formatting, that this is a faithful copy of the
  DATABASE rather than of the markdown imported into it, and that these files must not be
  written over source. Not a `.md` file, so re-importing never turns the warning into a page.
- `seed --update --as <account>` can now change a page that already exists. Off by default:
  a slug collision is still an error, never an overwrite. An update appends a revision
  through the same permission-checked call the editor makes, attributed to the named account
  with the file it came from as the summary; a page whose file says exactly what it already
  holds is left untouched, because a no-op revision per file per run buries the real edits.
  Title, type, visibility, language and ordering are reported and refused — a bulk file drop
  does not get to move a page or publish one. Nothing is ever deleted: a page in the wiki
  that no file claims is listed and left where it is.
- Tables of six rows or more can be sorted and filtered. Every column header is a real
  button carrying `aria-sort`, every column has its own filter box named after it, there is
  one search box over the whole table, and the row count is always stated as displayed out
  of total — a filtered table that looks complete is the defect this project keeps finding,
  and the denominator is the only thing that tells a reader the difference. A filter
  matching nothing says so instead of showing an empty table.
- All of that is a progressive enhancement: the server sends the complete table in the
  author's order with no control of any kind, and the controls appear only once the page
  has mounted in a browser, where they can actually do something. Without JavaScript the
  document is intact — never a shell, never a spinner, never a filter box that silently
  does nothing.
- The comparison rules are written down and tested rather than left to `Array#sort`, which
  is codepoint order: quantities are read through their units, comparator prefixes and the
  German decimal comma (`>1200 ppm`, `<0,5 %`, `3-5 %`, `1.200 g`), ✅ sorts after ❌,
  umlauts sort where a German reader looks for them, and empty cells stay LAST in both
  directions — reversing them to the top would push the rows being hunted for off the
  bottom. Whether a column is numeric is decided per column, not per cell, so a strain name
  like `5-HTP` does not become a five. Sorting is stable and applies to the order on screen,
  so sorting by a second column keeps the first one inside its ties. Below six rows a table
  is left exactly as it was.
- Append-only revisions. Publishing writes the revision and advances the document in one
  transaction; restoring appends a new revision rather than rewinding, so a restore can
  never destroy the history it restored from — and the schema refuses an `UPDATE` to a
  revision outright, so that holds for code nobody has written yet. Every accessor takes a
  `Principal` and goes through the one permission-checked document accessor: seeing history
  follows read, restoring follows write (D-M3-5). The author recorded is the authenticated
  principal, never a name a caller supplied, and the display name is snapshotted beside the
  id so attribution survives the account being deleted (D-M3-4).
- `gw-collab`: the collaborative editing core. A Yjs-compatible CRDT (`yrs` 0.27) holds live
  document state, so concurrent edits from people and agents merge rather than clobber.
  Block trees round-trip through it losslessly — every block kind, per-column table
  alignment, ragged and empty cells, deeply nested lists — proven as a property over
  hundreds of generated trees and through the encoded bytes, not by example. Malformed
  updates and malformed state vectors from a client are errors rather than panics, and a
  rejected update is never relayed to the other editors. `Rooms::join` returns one room per
  document under a single lock, so two connections arriving together cannot end up editing
  two copies of one page.
  This was written when inline marks and link destinations lived in the CRDT but had no
  field in `Block`, so publishing kept the text and dropped the emphasis. `Block` carries
  marks now, and a published snapshot keeps them.


- Invitations: a single-use link that creates an account and gives it access in the same
  act. There is no moment where the account exists with nothing granted, which is the gap
  the decision exists to close (D-M2-20) — an invite that would grant nothing is refused
  outright, in the API, in the store's own outcome type, and as a database constraint.
- An invite carries a grant on a path, membership of a team, or both, and who may issue
  which differs on purpose: a path grant needs the same authority as granting it directly,
  but attaching a **team** needs an instance admin, because a team's reach is bounded by no
  path and may be widened tomorrow by somebody else. A space admin could otherwise hand out
  reach they neither hold nor can see.
- Only the link's SHA-256 is stored, exactly as for a session, so the database never holds
  anything that could be presented. Unknown, expired, revoked and already-used links are
  answered identically — the page cannot be used to discover which invitations exist. They
  expire after 30 days (D-M2-21) and can be revoked before that.
- Redeeming one is a single transaction that consumes the link first and refuses to consume
  it twice, so two people opening the same link at the same moment produce one account, not
  two. It writes the account, the grant, the team membership, the session and four audit
  rows together, or none of them.
- The password chosen on the invitation page goes through the same policy as every other:
  a length floor **and** the breach corpus. That second half had never actually been proven
  for accounts an administrator creates — the test meant to prove it submitted an
  eight-character password, so the length floor refused it and the corpus was never
  consulted. Both the test and its stub were wrong; both are fixed, and two tests now fail
  if the corpus stops being asked.
- "Was sieht diese Person?" — an instance admin can look at great-wiki exactly as somebody
  else sees it, from a button beside that person in the console. The permission engine is
  not simulated: the request runs as the substituted principal, read from the store the way
  a real one is, so `can()` and the baseline decide unchanged and the answer is that
  person's view rather than a filtered copy of the administrator's.
- While that mode is active every non-GET request is refused **before routing**, so a path
  that does not exist yet is already covered and an endpoint written next year cannot
  forget the check (D-M2-17). A per-handler check would have failed open for code nobody
  has written. The one exemption is the request that ends the mode, matched on exactly one
  method and one path.
- The mode cannot be entered by anything a caller can write down. The cookie carries a
  256-bit token and nothing else; who is viewing, as whom and until when lives server-side,
  is bound to the administrator who started it, and is re-checked against the caller on
  every request — so a copied or invented cookie confers nothing, and a demoted or
  deactivated viewer stops substituting on their next click rather than at their next
  sign-in. Where the substitution cannot be completed the request continues as *nobody*,
  never as the administrator.
- A persistent banner above every page names both identities — whose view is shown and who
  is really signed in — says that writing is refused, and offers the way out as a plain
  form POST that needs no JavaScript. Being unable to leave is the one failure this mode
  must not have.
- Both ends are audited with both identities. The start row carries the mode's lifetime, so
  a session with no matching stop row is bounded by the record itself rather than reading
  as open-ended.
- An administration API: list and create principals, activate and deactivate accounts,
  manage teams and memberships, read and change the grants on a path, and read the audit
  log. Two gates, and the difference is the point — instance-wide operations need the
  `admin` baseline that comes from the verified Authelia group, while anything scoped to a
  path asks the permission engine for `admin` on *that* path. Somebody who administers one
  space administers that space and its descendants, and is refused everything else,
  including the list of who else has an account.
- Every administrative change is written in the same transaction as its audit entry, so
  an action cannot succeed while the record of it is rolled back. A change that turns out
  to be a no-op is recorded as nothing at all rather than as an action that happened.
- Both gates check authentication before consulting any grant. `can()` answers an `anyone`
  grant before it looks at who is asking — that is how a public share link works — so on a
  path carrying `anyone: admin` the engine on its own would have handed the access editor
  to a request that had not said who it was.

- The audit log is now scoped and readable. Instance admins see everything; anyone
  holding `admin` on a subtree sees the entries concerning that subtree and nothing else.
  Scope is an explicit column rather than a guess at what `target` means, and its absence
  means instance-wide — so an action that forgets to state a scope becomes invisible to
  space admins rather than visible to the wrong ones. Permission is evaluated per path,
  not by prefix, because grants do not union up the tree: a prefix query would hand over
  a subtree that had been deliberately narrowed.
- Adding to a team, removing from a team and revoking a grant now report whether they
  did anything. Each silent version was a lie an administrator would have believed — a
  mistyped team name withheld access while reporting success, and revoking an inherited
  grant from a child page matched nothing at all.

- CI on Forgejo, which is now the primary forge. Every job runs in the Node image and
  installs Rust through rustup, so `rust-toolchain.toml` stays the only place the
  compiler version is written down, and the Node version comes from the image that
  matches `.nvmrc` exactly. Dependency and build caching goes through the runner's own
  cache service, and cargo is capped to two parallel jobs because the runner has 1.7 GiB
  of memory and an out-of-memory kill there is indistinguishable from a compiler crash.
- The GitHub workflow is now guarded on the forge it is running against. Forgejo executes
  `.github/workflows` as well as its own, and those jobs target a runner label that no
  longer exists — which does not fail, it queues forever.

- One *Anmelden* control, with both ways in behind it (D-M2-11). `/auth/login` was a 302
  straight to Authelia; it is now great-wiki's own page offering a homelab button and a
  guest username and password. The redirect moved to `/auth/oidc`. Where no identity
  provider is configured the homelab button is absent rather than present and answering
  503, and the guest form still works — a deployment with only local accounts no longer
  has no way in.
- `POST /auth/local`: signing in with a great-wiki account. It issues the same session the
  provider path issues — one token generator, one hash, one `__Host-` cookie, one table —
  because a second kind of session is a second place to get expiry and revocation wrong.
- Throttling for that form, which is the price of putting a password field on a public
  hostname. Ten failures then a five-minute lockout, counted per account **and** per source
  address independently, either one enough to refuse. Counting only per account would miss
  a spray across many accounts from one place; counting only per address would miss a
  distributed guess at one account. The counters are in SQLite, so a restart is not a way
  out of a lockout, and a success clears that account's counter but deliberately not the
  address's — otherwise one valid credential would buy an unlimited spray budget.
  **The Authelia path never consults them**: somebody guessing at guest passwords cannot
  stop a homelab sign-in.
- A wrong password, an unknown username and a deactivated account produce the same status
  and the same bytes, and take the same kind of time — an unknown username is verified
  against a stand-in hash so it costs argon2's tens of milliseconds like everyone else.
  Without that, the form is a list of who has an account here, readable off a stopwatch.
- A password policy for accounts created by invite or by an administrator: twelve
  characters, no composition rules, and a breach check against Have I Been Pwned by
  k-anonymity — only the first five characters of the SHA-1 ever leave the machine. An
  unreachable corpus **allows** the password and writes the gap to the audit log, because
  failing closed would mean an outage stops everybody, including whoever is trying to fix
  it, from setting a password at all.


- The temporary edge gate is gone. Both wiki hosts now rely on the application's own
  sign-in, so published pages are readable without a homelab account and guest accounts
  will be able to sign in without Authelia at all.

- OpenID Connect sign-in against Authelia: authorization-code flow with PKCE, verified
  `state` and `nonce`, and id-token signature, issuer and audience all checked. Groups are
  read from the id token, falling back to userinfo with a mandatory subject match.
- Sessions are stored in SQLite with only a hash of the token, so a database disclosure
  does not hand over live sessions. Tokens carry at least 256 bits; the cookie is
  `__Host-` prefixed, HttpOnly, Secure and SameSite=Lax.
- Signing out deletes the server-side session rather than merely clearing the cookie, and
  deactivating an account ends its sessions everywhere in the same transaction.
- Three typefaces the reader chooses between — IBM Plex, Literata with JetBrains Mono, or
  the platform's own — applied before first paint like the theme, with each family's
  licence shipped beside its files.

- Real tables. Markdown tables become `table`, `row`, `header` and `cell` blocks and render
  as an actual table with `scope="col"` on headers and per-column alignment carried through,
  inside a focusable horizontally scrolling container — so a wide table scrolls in its own
  box and the page never scrolls sideways.
- Screenshot tooling: the reader is captured at desktop and phone widths in both themes, so
  a layout can be looked at rather than inferred from a status code.


- `/api/collab/*` now works in development. Vite proxies WebSocket upgrades only for a proxy
  entry that asks for one; without `ws: true` the handshake was never forwarded and the
  client simply hung. Caddy proxies WebSockets natively, so this was a development-only
  difference — the worst place for one to live.
- Autosave no longer files a revision. An actively edited page collected one every thirty
  seconds — a history full of versions nobody published, which is the opposite of what an
  append-only history is for. The background sweep now writes the live CRDT state to the
  `crdt_state` table migration 0008 created for it, and `POST /api/collab/{path}` is the
  only thing that writes a revision: publishing is a person saying "this is the version I
  mean", and a timer cannot say that.
- An editing session that was closed and reopened silently lost its formatting. A revision
  stores a `Block` tree, which has no field for an inline mark, so bold, italic and link
  destinations were dropped every time a room was rebuilt from one. The stored CRDT state
  keeps them. Nothing renders marks yet, which is exactly why this had to be fixed before
  M4 adds them rather than after — everything written in between would already have been
  flattened.
- Restoring a revision is now visible to editors as well as readers. The live CRDT state is
  discarded with the restore; without that, the next editing session would have opened on
  the very content the restore was undoing.
- The server-rendered pages now attest themselves to the API. They did not, and the way
  that would have failed is worth recording: the API refuses any request without the
  proxy secret whenever it is bound to anything but loopback — which is every deployment —
  so every server-side call would have been refused. The layout deliberately turns a
  failed identity lookup into "nobody signed in" rather than an error page, so the result
  would have been a wiki that quietly showed the public view to people who were signed in,
  with nothing in any log a reader would see. It never reached anybody only because
  nothing had been deployed yet: every test ran against a loopback API that demands no
  attestation, which is exactly the configuration that cannot notice.


- Every page rendered its title twice — once from frontmatter and once from the body's own
  leading heading. The seeder now drops a leading level-1 heading when it exactly matches
  the title, and keeps it when it does not.
- The prose column stretched to the viewport while the text inside stayed capped at the
  measure, so on a wide screen the text hugged the left edge with dead space beside it.
- On a phone you scrolled past two navigation blocks before reaching the article. The short
  outline now comes first, then the article, then the site tree.

- A real design system: tokens for type, space and colour as CSS custom properties, and
  content typography for headings, lists, quotes, code, tables and figures. The reader
  previously styled only the page chrome, so documents rendered as unstyled browser
  defaults on a dark background.
- Application styles live in named cascade layers and plugin CSS will load unlayered, so a
  theme overrides anything by construction — no `!important`, no specificity contest.
- A three-way theme control in the header: light, dark, or follow the system. The choice
  is applied before first paint by a blocking inline script, so there is no flash of the
  wrong theme, and it is a radio group rather than a toggle because "follow the system" is
  a genuinely different choice from picking one.
- Print styles: the document without the application around it, link targets spelled out,
  and no page breaks inside code blocks or immediately after a heading.

- API authorisation now runs through the permission engine. `may_read` is deleted rather
  than deprecated, and `Store::tree`, `Store::document_by_path` and `Store::pool` are all
  crate-private — so no code outside the storage layer can obtain an unfiltered document,
  an unfiltered tree, or raw database access to go around either.
- Principals are re-read from the store on every request, so revoking a grant or
  deactivating an account takes effect on the next click rather than at the next sign-in.
- The development identity now drives the real engine rather than bypassing it: its groups
  determine its reach, so local work exercises the same rules production does.

### Security

- **A board card is a disclosure, and every view of one is filtered like a page read.** A
  card says that a page exists, what it is called, and — because a card's words are the
  page's own — what somebody wrote on it. So every card, every project in a listing and every
  task read back on its own goes through the same permission-checked accessor a page read
  goes through, **per document, never once for the whole subtree**: a project deliberately
  spans pages with different access, and a board that trusted the subtree would hand over the
  very words a restricted page was keeping. A card on a page you may not read is not shown,
  not greyed out and not counted. The page a card *names* is filtered by the same answer
  rather than by a second lookup made after the card survived filtering — the page a card
  names is the page the permission check handed back, so there is no version of this code
  that shows a card without having asked whether you may read what it is called. Asking twice
  is how two answers start to disagree, and the second one is always the one that gets it
  wrong.
- **The widest view in the wiki is the narrowest one's own query with the project left
  unnamed.** A view over every task there is would be the easiest place in the system to lose
  that filtering, so it is not a second query that could lose it. One consequence is worth
  stating, because it is the thing that would have been got wrong: a card created **on a
  board** belongs to no page, and a board bound to a single project already knew the answer
  for all of them, having been let in at that home page a moment earlier. A board bound to
  nothing spans every project and can assume nothing, so it asks about each. Keeping the
  shortcut would have handed over the loose cards of every project whose home page you may
  not open, and it would have looked correct, because for one project it is.
- **Nothing counts what was left out.** No total, no "und 3 weitere", no identifier for a card
  or a project that was filtered away; a board or a project you may not see answers exactly
  what one that does not exist answers. A wiki with no tasks and a wiki whose every task is
  somebody else's read the same, and the conflation is the point, because a count is a fact
  about pages you are not allowed to read.
- **Who may assign whom is answered rather than left open.** You may create or change a task,
  including putting somebody's name on it, if you may **write** the page that governs it —
  its own page, or its project's home page. You may only assign it to somebody who may
  **read** that page: assigning a colleague to a task on a page they cannot open would create
  an obligation they can never see, and would tell them what a page they may not read is
  called. The refusal says so, says what to do about it, and names nobody. Clearing a name
  needs only the write, so somebody who has since lost their access can be taken off a card
  rather than pinned to it for ever. Moving a card to another board is governed by both
  boards and refuses to carry an assignee onto a page they may not read. Creating a to-do by
  publishing a page needs exactly what publishing needs and nothing weaker: reconciliation
  runs behind the write check publishing has always made and asks no second question of its
  own, so a reader whose publish is refused creates no records at all — and it keeps its
  hands off every record it did not write itself, since only a to-do that came from a line is
  one a line can disappear from under.
- **Purging a page destroys its cards with it**, the same way it already destroys its history
  and its editing state. A card holds a copy of the page's words; leaving it behind would
  keep restricted text on a board after the page and the access rules protecting it were
  gone. Deleting the *line* is a different matter and keeps the record — that is what
  »Abgelöst« is for.
- **Reading a page's history needs exactly the right that reading the page needs, and nothing
  more; restoring needs write, which is never implied by being able to read.** A history is
  not metadata about a page — it says the page exists, who works on it and what every earlier
  draft said — so every one of those answers goes through the same permission-checked
  accessor as well.
- A project id typed into the address bar is matched against the projects you were actually
  shown before it is used as a filter, so the address bar cannot become a second way to ask
  whether a project exists; an id matching nothing shows the whole board and says the filter
  was not applied, without confirming or denying anything about it. After a card is moved the
  browser is sent back to where the move was made and only ever to a page of this wiki: the
  address is carried in the form, so it is whatever anybody put there, and anything that is
  not a path here is refused rather than repaired.

