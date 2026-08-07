# 0002 — Authenticate with OpenID Connect, not proxy identity headers

**Status:** Accepted (2026-08-07)

## Context

The homelab's established pattern is Authelia forward-auth: the reverse proxy calls
Authelia and copies `Remote-User`, `Remote-Groups`, `Remote-Name` and `Remote-Email` into
the upstream request. The application trusts those headers.

Investigation of the live deployment found this pattern is load-bearing but fragile here:

- The shared Caddy `(authelia)` snippet **does not strip client-supplied `Remote-*`
  headers** — it only copies. Nothing stops a client sending its own.
- The only application currently consuming those headers defends itself with a *second*
  mechanism: a shared secret header checked in constant time before any identity is read.
  Without that, the headers alone would be forgeable.
- The predecessor plan promoted `Remote-Groups` from informational to *the* authorisation
  decision, and none of its acceptance checks tested group forgery.
- Its proposed `authelia_optional` snippet was built on the premise that Authelia returns
  401 for anonymous requests. Measured against the live deployment, it returns **302** when
  the domain is in `access_control` and **403** when it is not — never 401. The snippet
  could not have worked as written.

Authelia already runs OIDC with twelve registered clients, and `groups` is a first-class
claim in its discovery document.

## Decision

Authenticate with the OpenID Connect authorization-code flow with PKCE. Identity and group
membership come from verified token claims, not from headers a proxy was trusted to have
sanitised.

The proxy shared-secret header is retained as defence in depth — the application binds
`0.0.0.0` out of necessity (the proxy runs on a different host, so a loopback bind is
unreachable from it), which means the port is LAN-reachable and the bind address cannot be
the boundary.

## Consequences

- The header-forgery attack surface is removed entirely rather than mitigated.
- A `great-wiki` OIDC client must be registered in Authelia, which requires a container
  restart, and a real hostname with TLS must exist before authentication can be developed
  at all. This is why exposure is milestone M1 rather than a late deployment step.
- Sessions, token refresh and logout become the application's responsibility.
- Local accounts for external collaborators are independent of this and use argon2id with
  Authelia's parameters, so the two paths produce the same kind of principal.
