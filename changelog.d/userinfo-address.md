### Fixed

- **Signing in through Authelia still finds your invited account if Authelia is ever set to
  put your name in the sign-in token but not your address.** The wiki used to ask Authelia's
  profile endpoint only when the name or the groups were missing, so in that configuration it
  would never have learned the address, and nobody would have been matched to their
  invitation again. Nothing would have said so beyond one log line per sign-in. It now asks
  whenever the address is missing too. The way Authelia is set up today already worked, and a
  test now proves that exact shape, which no test did before.
