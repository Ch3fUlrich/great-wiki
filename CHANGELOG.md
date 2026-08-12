# Changelog

All notable changes to this project are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); newest first.
Entries describe the *effect* of a change, not the diff.

## [Unreleased]

### Added

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
- **Not** shown, and deliberately not faked: when a page was last edited and by whom. There
  is no revisions endpoint yet. The row is written, styled and tested, and renders nothing
  at all until real data is passed — a dash or an "unbekannt" beside three genuine facts
  would be read as a fourth.
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
  Known and tested limit: inline marks and link destinations live in the CRDT but have no
  field in `Block`, so a published snapshot keeps the text and drops the emphasis until M4
  adds marks.


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

### Changed

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
  kept, never dropped. Emphasis is currently flattened.
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

