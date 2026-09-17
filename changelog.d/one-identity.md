### Added

- **One person, one identity.** An invitation now names an **e-mail address**, and that
  address is what the same person is recognised by later. Somebody you invite sets their own
  password as before; when they afterwards sign in through Authelia and Authelia confirms
  that address, it is **the same account** — the same rights, the same history, the same name
  on the pages they have written. One password and one homelab login, not two accounts with
  one of each. Until now those were two separate people as far as the wiki was concerned: an
  invitation made a local account, an Authelia sign-in made a second one with no access at
  all, and the console said nothing about the difference anywhere.
  - The address is the owner's to set, never the invitee's: the page they accept the
    invitation on asks for a display name and a password and nothing else.
  - It is matched ignoring capitals and surrounding spaces, so `Oma@Example.de` and
    `oma@example.de ` are one person. An address with characters outside the plain Latin set
    is stored and shown but never matched automatically — two addresses that *look* the same
    can belong to two different people, and guessing there would hand one of them the other's
    pages.
  - The recognition only happens on an address **Authelia itself confirms**. One that it
    merely holds on file does not count and never will, because anybody who can edit their
    own profile there could otherwise type your address into it.
  - »Personen« now shows such an account as *Lokal + Authelia*, with the homelab name beside
    it — so that switching it off is visibly one act that closes both doors.
- **Accounts that share an address can be listed**, under
  `GET /api/admin/identity/candidates`, for whoever administers the whole wiki. It only
  looks: nothing that already exists is joined together automatically, because two accounts
  on one address might be one person with two logins or might be a shared family address, and
  the second joined up is one person quietly holding the other's pages.

### Changed

- **An invitation must carry an e-mail address**, and one that is not already in use here.
  An address with no account behind it is the only thing that lets the wiki recognise the
  same person later, and there is no fixing it after the link has been used. If the address
  already belongs to somebody, the invitation is refused and says whose it is — give that
  account access under »Zugriff« instead of inviting the same person a second time.

### Fixed

- **A homelab account could take over a wiki account that happened to share its name.**
  Signing in through Authelia as `oma` silently became the invited `oma` — their pages, their
  rights, their history — with no address checked, nothing confirmed and nothing written to
  the log. It is now refused outright unless a confirmed address says the two really are one
  person.
- **Switching off a merged account ends every way in to it.** Because it is one account
  rather than two, deactivating it takes the password and the homelab login together. Before,
  the other half kept signing in.
