# syntax=docker/dockerfile:1
#
# ===========================================================================
#  gw-caddy — the stack-internal reverse proxy, which is the upstream Caddy
#  image with this repository's Caddyfile baked in.
#
#      docker build -f docker/gw-caddy.Dockerfile -t gw-caddy:dev .
#
#  WHY AN IMAGE AND NOT A BIND MOUNT. `app-deploy.yml` ships exactly one file to
#  the target: `docker-compose.yml`. A `volumes:` entry pointing at a Caddyfile
#  would point at a path that does not exist on cloud.vm, and mounting a
#  directory over a missing file makes Docker create an empty directory there —
#  Caddy then starts on its default config and proxies nothing, which looks like
#  a routing bug rather than a missing file. Baking it in makes the routing
#  version with the code that depends on it.
#
#  ALPINE, DELIBERATELY. The rest of the stack is Debian because the Rust binary
#  links glibc. Nothing is compiled here: this is the upstream image plus one
#  COPY, and switching it to a Debian base would mean building Caddy from source
#  for no benefit.
# ===========================================================================

# Pinned to a minor. Caddy's 2.x line is compatible across patches and this is a
# routing config with no plugins.
FROM caddy:2.10-alpine

COPY docker/Caddyfile /etc/caddy/Caddyfile

# Parse and PROVISION the config at build time. `caddy validate` loads every
# module the file names, so a misspelled directive, an unknown matcher or a
# `trusted_proxies` range Caddy cannot parse fails the build rather than the
# deploy. It dials no upstreams — `gw-api` and `gw-web` do not have to resolve.
RUN caddy validate --adapter caddyfile --config /etc/caddy/Caddyfile

EXPOSE 8100
