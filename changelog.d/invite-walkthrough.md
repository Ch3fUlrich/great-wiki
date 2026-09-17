### Added

- **Einladungen**, a fifth tab in the Verwaltung console: invite somebody into this wiki
  without ever choosing their password for them. Name the account and the page they should
  reach, and the console hands back a link to pass on; the person who receives it picks their
  own display name and password and is signed in at the end of it. The tab also lists every
  outstanding invitation — who it is for, what it carries, who made it, when it runs out — and
  withdraws one that should not be used. Until now this was the one thing the wiki could do
  and could not be asked to do: the invitation itself, the acceptance page and the whole
  permission model behind them had been built and tested, but there was no control anywhere
  that made one, so the only way to bring somebody in was to create an account *for* them and
  send the password through a chat app — which is the one thing an invitation exists to avoid.
  The link is shown **exactly once**, because nothing stores it and nothing can show it again,
  and the screen says so plainly along with the part that is easy to miss: until it is used,
  whoever holds that link can create the account, so it wants sending by a route you trust.
  What the form offers follows what you are actually allowed to hand out — somebody who
  administers one area can invite into that area and cannot attach a team, because a team
  reaches wherever it has been granted and that is reach they do not have and cannot see.

### Fixed

- **Being refused a page is now said in German.** Following a link to a page you have not been
  given access to answered "You do not have access to this page." — in a wiki whose every
  other screen is German, on what is very often the first boundary a newly invited person ever
  meets. It, the "page not found" beside it and the error page they both land on are now
  German and address the reader as »Sie«, like the sign-in and invitation pages. The page's
  version history was already German but spoke »du«; it now matches the rest.
- **An invitation says when it expires in a date a person reads.** It was showing
  `2026-10-17` — the way the database stores it — in the middle of a German sentence, on the
  one page in this wiki written for somebody who has never seen it before. Now `17.10.2026`,
  like every other date here.
- **Choosing a password that is too short no longer costs you your name as well.** The
  invitation form came back empty apart from its complaint, so a relative who picked something
  under twelve characters had to type their display name again and could reasonably conclude
  the page had thrown their input away. The name comes back; the password, deliberately, does
  not.
- **The Protokoll is in German throughout.** Half of what it recorded was shown as the raw
  internal verb — `invite.create`, `attachment.attach`, `document.trash` — sitting beside
  entries like »Zugriff gewährt«, so the log of what happened to your own wiki read as machine
  output for anything outside the handful of actions that had been translated. All twenty now
  have words.
