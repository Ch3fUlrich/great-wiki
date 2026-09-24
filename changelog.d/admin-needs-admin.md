<!-- Fold into CHANGELOG.md under [Unreleased], in the sections named below. -->

### Fixed

- **The Verwaltung is only shown to people who administer something.** Opening `/admin`
  without being signed in used to show the whole console — the heading, the introduction and
  every panel — and each panel then said in German that it could not be loaded. No data came
  out, because every request behind the panels was refused, but the page never decided
  whether the visitor belonged there. Now it decides first. Anyone who administers nothing —
  not signed in, or signed in without admin rights — is sent to the sign-in page before any
  panel is loaded. Whoever administers the whole wiki, or holds admin on any page at all
  (directly or through a team), still gets the console as before.

  "Administers something" is worked out by the same check the admin endpoints already use,
  so the console cannot turn away somebody a panel would have served, or open for somebody
  every panel would refuse. `/api/me` now reports that answer as `administers`. It only
  describes the caller: every admin endpoint still makes its own check. If `/api/me` cannot
  be read, the page sends the visitor to the sign-in page rather than showing the console.

- **The sign-in page says so when you are already signed in.** A reader sent there from
  `/admin` is told who they are signed in as, that the Verwaltung needs an account with
  Verwaltungsrechte, and gets an »Abmelden« button so they can sign in as someone else.
  Without this they would sign in again as the same person and be sent straight back. The
  redirect goes to this page, never straight to Authelia. For a signed-in reader, Authelia
  would sign them in again as themselves and send them back to `/admin`, round and round for
  ever. The redirect adds no return-to address either, because that would create an
  open-redirect hole for a small convenience.

  When an administrator is **viewing the wiki as somebody else**, `administers` describes
  that person, like everything else `/api/me` reports, because the admin endpoints also act
  as that person while the mode is on. So the console is closed for exactly as long as all
  of its panels would refuse. The sign-in page then names both people and offers »Ansicht
  beenden« instead of a sign-out, because a sign-out would be refused while the mode is on.

### Known limitations

- **For a signed-in user who has no admin rights, `/api/me` now checks every path that
  holds a grant.** `/api/me` is asked on every page, so that is one small query per such
  path per page. Anonymous visitors and instance admins stop after the first step. On a wiki
  with a few dozen spaces this makes no difference; if it ever grows to thousands, this is
  the first thing to cache.
- **The header has no link to the Verwaltung.** Nothing links to `/admin` except the
  address itself, so there was no link to hide.
