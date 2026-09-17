# Walking the invitation flow, 17 September 2026

The roadmap's entry for 2026-09-02 flagged this: *"the invite flow has 42 tests and has never
been walked end to end by a real second human. A first invitation that fails is the worst
possible first impression of a wiki somebody was asked to trust with medical notes. Walk it
before they arrive, not after."*

This is that walk. It was done as a second person would do it — anonymously first, then as
the owner, then as the invitee in a browser that had never held the owner's cookies — through
a **production build** of the web application behind a reverse proxy that mirrors
`docker/Caddyfile`, because dev's policy is looser and a relative meets production.

It found one missing flow, four defects and two things worth the owner's decision. All the
defects are fixed, with tests. The headline is the first one, and it is not a bug:

> **There was no way to invite anybody through the interface.** The API could create, list
> and withdraw an invitation; the acceptance page was complete and had tests; the
> administration console had four tabs and none of them could make one. The only route the
> interface offered for bringing somebody in was »Person anlegen« — which means the owner
> chooses that person's password and sends it through a chat app, exactly what D-M2-3 exists
> to prevent. The decision was documented, implemented, tested and unreachable.

---

## How it was run

Nothing touched production or `data/great-wiki.db`.

| Piece | Where | Notes |
|---|---|---|
| Database | `data/invite-walk.db` | Fresh, seeded from `content-example`, 12 pages |
| API | `127.0.0.1:8093` | `GW_DEV_IDENTITY=sergej:admins` for the owner's legs; unset for the anonymous leg |
| Web | `127.0.0.1:4174` | `npm run build` + `node build/index.js` — adapter-node, the real thing |
| Proxy | `127.0.0.1:8700` | Mirrors `docker/Caddyfile`: `/api/*` and `/auth/*` → API, everything else → web |

The proxy is not decoration. `adapter-node` proxies nothing, so without it `/api` does not
exist from the browser on the production port and no client-side control in the console can
work at all. In the real deployment Caddy does this; a walk that skipped it would have been
walking something nobody deploys. It forwards `X-GW-Proxy` and never sets it, for the reason
the Caddyfile writes out at length.

**The scenario.** The owner turns `/verweisbeispiel` into a private page (`Eingeschränkt`)
and invites `oma` with `Schreiben` on it. `/rundgang/nur-intern` stays restricted and is
never granted — that is the page that must stay hidden. Both are restricted; one is granted
and one is not, which is what makes revocation mean something.

---

## The four proofs

| # | Proof | Verdict |
|---|---|---|
| 1 | A restricted page stays hidden, in every view | **Proven**, with one deliberate trade-off for the owner to re-confirm |
| 2 | They can leave their mark, and the owner can see who | **Proven** |
| 3 | Revocation works, with nothing left behind | **Proven** |
| 4 | The German reads right on every screen an invitee meets | **Failed as found; now proven.** Two screens were in English |

---

## Proof 1 — a restricted page stays hidden

`/rundgang/nur-intern` (visibility `restricted`, no grant to `oma`), checked as the signed-in
invitee against every surface the owner named.

| View | Result |
|---|---|
| The page itself (`/rundgang/nur-intern`) | 403, German refusal, none of its words |
| Page tree (sidebar and `/api/tree`) | absent |
| Start page, "Alle Seiten" | absent |
| Board (`/aufgaben`, `/api/board`) | absent |
| Topics (`/themen`, `/api/topics`) | absent |
| Trash (`/papierkorb`, `/api/trash`) | absent |
| Graph (`/graph`, `/api/links/graph`) | absent |
| Projects (`/projekte`) | absent |
| Link picker (editor, »Link«) | absent — eleven pages offered, not twelve |
| Embed picker (editor, »Seite einbetten«) | absent |
| Backlinks | 403 |
| Revision history | 403 |
| Attachments | 403 |

**The tab strip needs a note, because it looks like a leak and is not.** The refusal page's
tab is labelled »Nur intern«, which is the page's real title. It is not the page's title: it
is the address the reader typed, title-cased by `fromSlug` in `web/src/lib/tabs.ts`, and the
slug happens to match. This is provable rather than a matter of reading the code — `labelFor`
has exactly two sources, the tree and the slug, and `/api/tree` as this invitee demonstrably
does not contain the page. The comment on `labelFor` already says this is the one place in
the strip where a title could have leaked, and it does not.

### One thing for the owner to decide, not a defect

Four derived endpoints answer **403 for a restricted page and 404 for a page that does not
exist**:

```
/api/links/backlinks/rundgang/nur-intern     403      .../rundgang/gibt-es-nicht   404
/api/revisions/document/rundgang/nur-intern  403      .../rundgang/gibt-es-nicht   404
/api/attachments/rundgang/nur-intern         403      .../rundgang/gibt-es-nicht   404
/api/tasks/document/rundgang/nur-intern      403      .../rundgang/gibt-es-nicht   404
```

That difference lets a signed-in invitee enumerate, by path, which pages exist but are
withheld from them. It is **deliberate and documented** — `crates/gw-api/src/routes/docs.rs`
argues it out: *"Collapsing both to 404 would hide configuration mistakes behind a status
code that says 'you spelled it wrong'; collapsing both to 403 would confirm the existence of
every path somebody guesses."* Nothing was changed here, because reversing a written decision
is the owner's call and not a walkthrough's.

But it is worth re-confirming now, and the roadmap says why in its own words: ADR 0009, ADR
0011 and the per-document filtering *"were all written against a threat model with exactly one
person in it. They are about to have a second."* This is one of those. The disclosure is
"a page exists at this path and is not yours", which for a family wiki holding medical notes
may be acceptable and may not. `/api/topics/tagged/` already answers 404 for both, so the
convention is not uniform today either.

---

## Proof 2 — they can leave their mark

As `oma`, with `Schreiben` on `/verweisbeispiel`:

- **Edited the page** and published with a summary. The history shows
  `Eine Aufgabe für den Salat ergänzt · Oma Erika · 17.09.2026, 19:54 · +59 B`, with a working
  prose diff against the imported first revision.
- **Ticked a task on the board.** `/aufgaben` said »Einbettungen mit Ankern« steht jetzt in
  Fertig.
- **Tagged a topic.** Added »Familie« to the page; it appeared in the page's topic list and
  in `/api/topics`.
- **Uploaded a file.** The attachment panel said: *»rezept.txt« ist jetzt angehängt — Datei,
  50 B.* and, beside it, **»Hochgeladen von Oma Erika am 17.09.2026, 19:51«**.

The owner sees all of it. The Protokoll:

```
17.09.2026, 19:51   Oma Erika (oma)   Datei angehängt rezept.txt        /verweisbeispiel
17.09.2026, 19:41   Oma Erika (oma)   Einladung angenommen …            /verweisbeispiel
17.09.2026, 19:41   sergej (sergej)   Konto angelegt …                  instanzweit
17.09.2026, 19:41   sergej (sergej)   Zugriff gewährt /verweisbeispiel  /verweisbeispiel
17.09.2026, 19:40   sergej (sergej)   Einladung erstellt …              /verweisbeispiel
17.09.2026, 19:39   sergej (sergej)   Sichtbarkeit geändert …           /verweisbeispiel
```

The whole invitation is traceable: created, accepted, and the account and grant it produced.
(Three of those six lines read `invite.create`, `invite.accept` and `attachment.attach` when
first walked — see defect 5.)

---

## Proof 3 — revocation works

The owner removed the grant through the Zugriff panel. The confirmation said, before doing
anything:

> **Zugriff entziehen?** Oma Erika (oma) verliert »Schreiben« auf /verweisbeispiel und allen
> darunter liegenden Seiten, die nichts Eigenes eingetragen haben. Der Eintrag wird auf
> /verweisbeispiel gelöscht. Danach trägt weder /verweisbeispiel noch eine übergeordnete
> Seite einen Eintrag.

The invitee then signed in again with her own password. On that next request:

| Surface | Before | After |
|---|---|---|
| `/verweisbeispiel` | 200, editable | 403, German refusal, no title, no words |
| Page tree | present | absent |
| Board | her two tasks | `Offen []`, `Läuft []`, `Fertig []` |
| Graph | nodes and edges | `{"nodes":[],"edges":[]}` |
| Topics | »Familie« | gone — no readable document carries it |
| Her own attachment | downloadable | 403 |
| Revision history | 200 | 403 |

**Nothing to clean up, and nothing destroyed.** Her revision, her attachment and her topic
all still exist and the owner still sees them, with her name on them — they belong to the
page now, not to her access. The database afterwards: one `acl` row (the fixture's unrelated
`/rundgang` grant), the `oma` principal still active, the invitation still recorded as
accepted. Revocation removed exactly one row and everything derived followed from it on the
next request, which is D-M2-7 working: the principal is re-read per request rather than
captured at sign-in.

A note on what revocation does **not** do: it does not deactivate the account. `oma` can
still sign in and see the public pages, which is correct and is what the console says. If the
intent is "this person is gone", that is »Personen« → deactivate, and the Einladungen panel
now says so on any invitation that has already been accepted.

---

## Proof 4 — the German

**Failed as found.** Two screens an invitee meets were in English, and one German screen was
half machine output. All fixed.

What was already right, quoted as met:

> **Einladung zu great-wiki** — sergej hat Sie zu great-wiki eingeladen. Damit erhalten Sie:
> Schreibzugriff auf /verweisbeispiel. Benutzername: oma. Diese Einladung gilt einmalig und
> läuft am 17.10.2026 ab.

> **Einladung ungültig** — Diese Einladung ist nicht (mehr) gültig. Sie wurde bereits benutzt,
> zurückgezogen oder ist abgelaufen — oder der Link ist unvollständig. Bitte fragen Sie nach
> einer neuen.

> **Bei great-wiki anmelden** — Öffentliche Seiten sind ohne Anmeldung lesbar. Gastzugang.

> *Das Passwort muss mindestens 12 Zeichen lang sein.*

All plain, all »Sie«, all comprehensible to somebody who has never used the wiki. The
invitation page also loads nothing from anywhere — no script, no font, no stylesheet — which
for a page somebody types a new password into is the difference between failing visibly and
failing silently.

---

## Every defect, and its fix

### 1. There was no way to invite anybody — a design gap, now closed

**Found:** the console has tabs Zugriff, Personen, Teams, Protokoll. No invitation control
anywhere. `POST/GET/DELETE /api/admin/invites` had no caller in the entire front end;
`grep -ri invite web/src` returned only unrelated prose.

**Fixed:** a fifth tab, **Einladungen**
(`web/src/lib/components/admin/InvitesPanel.svelte`), with the client half in
`web/src/lib/adminApi.ts` and the listing loaded server-side in `web/src/routes/admin/+page.server.ts`.

It creates an invitation, lists the outstanding ones, and withdraws one. Three things it
takes care over:

- **The link exists exactly once.** The plaintext token is never stored — the table holds
  only its SHA-256 — so the answer to the POST is the only copy there will ever be. It is
  held in *page* state, not panel state, because creating one calls `invalidateAll()` and
  state that died with a re-rendered child would take the invitation with it. The block says
  so in words: *Er wird **nur dieses eine Mal** angezeigt.*
- **The link is itself a credential.** *Wer den Link hat, kann das Konto anlegen. Er ist
  selbst das Passwort, bis er benutzt wurde.*
- **What is offered mirrors what the API will accept.** The team field appears only for an
  instance administrator (D-M2-2: a team reaches instance-wide, so handing one out is not a
  space admin's to do), the permission field appears only once a page is chosen, and the
  submit button is disabled — with the reason on screen — while the invitation would carry
  neither a page nor a team (D-M2-20).

The URL is made absolute in the browser, from the origin it is already on. The API
deliberately returns an origin-relative path, because the only origin it could build one from
is the `Host` header and any client can set that.

**Tests:** `web/src/lib/components/admin/InvitesPanel.test.ts` (12), invite client tests in
`adminApi.test.ts` (5), and Group P in the behaviour harness (10).

### 2. The refusal an invitee is most likely to meet was in English

**Found:** following a link to a page you were not granted gave:

> **403** — You do not have access to this page. — Back to the start page

**Fixed:** `web/src/lib/refusals.ts` holds both sentences, `web/src/routes/[...path]/+page.server.ts`
throws them, `web/src/routes/+error.svelte` is German throughout with a per-status fallback:

> **403** — Diese Seite ist nicht für Sie freigegeben. Bitten Sie um Zugriff, wenn Sie ihn
> brauchen. — Zurück zur Startseite

The constants live in `$lib` and not beside the loader because SvelteKit refuses any export
from a `+page.server.ts` that is not one of its own — a **build** error, not a type error,
which is why `just build` and not `npm run check` is what catches it. That happened during
this work and is worth knowing.

The sibling loader one directory down (`history/+page.server.ts`) had been German all along,
which is what makes this an oversight rather than a convention. Its register was »du«; it and
the restore dialog beside it are now »Sie«, matching the invitation page, the sign-in page and
the console.

**Tests:** four in `web/src/routes/[...path]/server.test.ts`, plus P9.

### 3. The invitation's expiry date was an ISO date in a German sentence

**Found:** *"Diese Einladung gilt einmalig und läuft am **2026-10-17** ab."* — on the one page
in this application written for somebody who has never used it. The rest of the interface
formats dates `TT.MM.JJJJ`.

**Fixed:** `german_date` in `crates/gw-api/src/auth/invite.rs`. Anything that does not parse
comes back unchanged rather than blank — an invitation must not fail to render over its own
expiry label.

**Tests:** `a_stored_timestamp_becomes_a_date_a_relative_would_recognise`, the updated
`the_page_says_who_invited_them_and_what_they_will_get`, and P5 (which also asserts no ISO
date survives anywhere on the page).

### 4. A refused password also threw away the name

**Found:** typing a name and a too-short password returned the correct German message and an
**empty form**. A relative has to type their name again, which reads as the page having
discarded their input.

**Fixed:** `render` takes the typed display name and echoes it, escaped, on every refusal
path. The password is never echoed — that would put it in the page, in any cache that took
it, and in the browser's history for a response that was an error.

**Tests:** `a_refused_attempt_keeps_the_name_but_never_the_password`,
`a_name_with_markup_in_it_cannot_escape_the_attribute_it_is_echoed_into`,
`the_page_a_fresh_visitor_gets_has_an_empty_name_box`, and P6.

### 5. Half the audit log was untranslated

**Found:** the Protokoll showed `invite.create`, `invite.accept` and `attachment.attach`
verbatim, beside »Zugriff gewährt« and »Konto angelegt«. Eleven of the twenty action names the
backend records had no German word; the panel's deliberate fall-back-to-raw had quietly become
the normal case for half the log.

**Fixed:** `AUDIT_ACTION_LABEL` moved to `web/src/lib/adminApi.ts` — where the rest of the
"German for everything the API says in English" vocabulary already lives — and completed.
The raw fallback stays, for a verb added tomorrow.

**Tests:** three in `adminApi.test.ts`, including one that pins the full set the backend emits
with the `grep` that regenerates it, so adding an audited action in Rust and forgetting the
German fails a test instead of shipping. Plus P10.

---

## Not fixed, and why

**The editor speaks »du«.** `web/src/lib/editor/session.ts` says *»Du darfst diese Seite nicht
bearbeiten«* and *»Abgelehnt: du darfst diese Seite nicht schreiben«*, and the surrounding
editor text is consistently informal (*Schließe*, *Lade*, *Kopiere*). Everything else an
invitee meets is »Sie«. This is plain German either way — it is a register inconsistency, not
a comprehension failure, and the English was the comprehension failure. Changing only the
refusals would leave the editor talking to itself in two voices; changing the editor's whole
voice is the owner's call about the product's tone, not a walkthrough's. **Recommended:** move
the editor to »Sie«. It is about fifteen strings.

**An anonymous visitor is offered the administration console.** `/admin` renders its shell and
its dialogs to anybody; the panels then say *»Die Personenliste konnte nicht geladen werden:
Dafür fehlen die Rechte (403).«* Submitting »Person anlegen« is correctly refused, in German.
This is a control offered that then fails, but it follows from a deliberate design — each
endpoint is loaded independently so one dead endpoint does not take the other three down —
and it discloses nothing. Left alone; worth a decision about whether `/admin` should redirect
an unauthorised visitor instead.

---

## The Authelia leg — what it still needs from the owner

Not walked. It could not be, and the blockers are the owner's to clear, not a defect:

1. **OIDC is not configured in this checkout.** `/auth/login` offers only the Gastzugang form;
   there is no Authelia button, because `read_oidc()` found nothing. A walk needs
   `GW_OIDC_*` pointing at the real Authelia, which means it happens on `cloud.vm`, not here.
2. **A throwaway user only the owner can create.** great-wiki never writes Authelia's user
   database (ADR 0002), so the test account has to be made in the Konten-App first. Nothing in
   this repository can do it or should be able to.
3. **The invitation flow does not apply to it.** An invitation creates a **local** account
   with a password (`kind: 'local'`). An Authelia person arrives by signing in, and great-wiki
   mirrors them into `principals` on first sign-in (`kind: 'oidc'`) with **no access at all**.
   So the Authelia leg is a different flow: sign in first, then the owner grants access under
   »Zugriff«. Worth knowing before walking it, because "send them an invitation link" is not
   how a homelab account gets in, and the console does not currently say so anywhere.
4. **What to check when it is walked:** that the mirrored account arrives with the right
   `groups` from the verified claim and *no* baseline beyond `users`→`internal`; that the
   group→reach mapping in `group_roles` does what the Zugriff panel claims; that signing out
   ends the great-wiki session without touching the Authelia one; and Proof 1 again, because
   an Authelia account has a group and therefore a different baseline than `oma` had.

One thing the owner may want to decide first: **whether a homelab person should be invitable
at all.** Today an invitation always creates a local account with its own password. A person
who already has an Authelia login getting a second credential for the same wiki is a second
thing to lose. The alternative — an invitation that pre-grants access to a username that will
arrive via OIDC — does not exist and would need designing.

---

## What is now guarded

`web/scripts/behaviour.mjs` gained **Group P**, ten checks, end to end and in order: create an
invitation through the console, read the link it shows once, open it as the recipient, watch a
bad password be refused in German, withdraw it, confirm the dead link is byte-identical to a
link that never existed, and read the Protokoll. A unit test can hold any one of those. Only
the harness holds the seam between them — and the seam is exactly where the whole flow was
missing.

Group P runs as `sergej:editors`, which `just behaviour` provides and which is **not** an
instance administrator. That is the more interesting case, and it is what makes P3 possible:
this identity administers one page and may not attach a team, so the panel must withhold the
field rather than offer a form that will be refused.

The harness is **102/102** (was 92). Writing Group P turned up two things worth recording,
because both are the kind of failure that reports "ok":

- **A group inserted before the reachability gate's `browser.close()` never runs, and the
  count is the only thing that says so.** Group P's first run reported 92/92 — green, and
  with ten checks silently skipped. The total is not decoration.
- **Group P needed a page of its own to invite into, because Group J purges the only page
  this identity administered.** `behaviour-fixture` now grants `admin` on a second, surviving
  leaf, with the reasoning beside the grant. Without it Group P could only ever have watched
  the gate refuse — and would have reported "ok" for correctly detecting a refusal, which is
  the exact failure the justfile's own comments keep being written about.
