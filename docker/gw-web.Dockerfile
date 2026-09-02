# syntax=docker/dockerfile:1
#
# ===========================================================================
#  gw-web — the SvelteKit front end (`web/`, `@sveltejs/adapter-node`). Built
#  from the repository root so that one `.dockerignore` covers every image:
#
#      docker build -f docker/gw-web.Dockerfile -t gw-web:dev .
#
#  Two stages. The runtime carries `build/`, a `package.json` and the node
#  binary. No vite, no typescript, no svelte compiler, no node_modules at all,
#  and npm itself is removed — what is left cannot build anything.
# ===========================================================================

# Pinned to the patch, because `.nvmrc` is and this repository is strict about
# it. The builder asserts the two agree rather than trusting this line.
ARG NODE_VERSION=24.19.0
# Same suite as gw-api, for one less moving part across the stack.
ARG DEBIAN_SUITE=trixie

# ---------------------------------------------------------------------------
#  Builder
# ---------------------------------------------------------------------------
FROM node:${NODE_VERSION}-${DEBIAN_SUITE}-slim AS builder

WORKDIR /src/web

COPY .nvmrc /tmp/.nvmrc
RUN set -eu; \
    want="$(tr -d ' \t\r\n' < /tmp/.nvmrc)"; \
    have="$(node --version | tr -d 'v')"; \
    [ "$want" = "$have" ] || { echo ".nvmrc pins $want, base image has $have — bump ARG NODE_VERSION" >&2; exit 1; }; \
    echo "node $have matches .nvmrc"; \
    rm /tmp/.nvmrc

# --- Dependency layer ------------------------------------------------------
# Lockfile only, so a change under `web/src/` reuses this layer.
#
# `npm ci` runs the `prepare` script, which is `svelte-kit sync || echo ''`. The
# sources are not here yet, so sync fails — and the `|| echo ''` in package.json
# is what makes that harmless. Do not "fix" that script: without the fallback,
# this layer cannot exist and every source edit reinstalls the tree.
COPY web/package.json web/package-lock.json ./
RUN npm ci

# --- The real build --------------------------------------------------------
# `vite build` runs `svelte-kit sync` itself, so there is no separate step. The
# adapter is `adapter-node` (set in `vite.config.ts`) and it writes
# `build/index.js`.
COPY web/ ./
RUN npm run build

# --- The assertion that lets the runtime stage carry no node_modules --------
# As configured today the server bundle imports nothing but `node:` builtins and
# its own relative chunks: `@ark-ui/svelte` and `shiki` are listed in
# `ssr.noExternal` and are compiled in. So `node_modules` is dead weight in the
# runtime image — and not harmless dead weight, because `npm ci --omit=dev` pulls
# `svelte` in as a transitive dependency and that ships the SVELTE COMPILER in a
# production image.
#
# But "nothing is external" is a property of the CURRENT vite config, not a law.
# If it ever stops being true the failure is `ERR_MODULE_NOT_FOUND` on the first
# request in production.
#
# The check itself moved into `web/scripts/check-server-bundle.sh` so that
# `just build` can run the SAME one — it used to live only here, and that meant a
# missing `ssr.noExternal` entry passed every gate command green and failed at
# image build, after review. Two copies of a gate are two gates with two opinions
# (`scripts/scan-secrets.sh` says the same thing about itself), so there is one.
RUN sh scripts/check-server-bundle.sh

# ---------------------------------------------------------------------------
#  Runtime
# ---------------------------------------------------------------------------
FROM node:${NODE_VERSION}-${DEBIAN_SUITE}-slim AS runtime

# npm and corepack are build tooling, and this image does not build. Removing
# them takes ~60 MB off and leaves nothing in the container that installs
# packages.
RUN set -eux; \
    rm -rf /usr/local/lib/node_modules/npm \
           /usr/local/lib/node_modules/corepack \
           /usr/local/bin/npm /usr/local/bin/npx /usr/local/bin/corepack; \
    mkdir -p /app; \
    chown node:node /app

WORKDIR /app

# `package.json` is not decoration: it carries `"type": "module"`, and without it
# node reads `build/index.js` as CommonJS and dies on the first `import`.
COPY --from=builder --chown=node:node /src/web/package.json ./package.json
COPY --from=builder --chown=node:node /src/web/build ./build

# `node` is UID 1000 in the official image — the same UID gw-api runs as and the
# owner of $APPS_ROOT on cloud.vm. This service writes nothing, but the
# consistency is worth having.
USER node

ENV NODE_ENV=production

# Documentation only; only the internal Caddy publishes a port.
EXPOSE 3000

# `HOST`, `PORT`, `GW_API`, `GW_PROXY_SECRET` and `ORIGIN` all come from the
# compose file — see the note there about `GW_PROXY_SECRET`, without which every
# server-side render is refused by the API and the site renders as signed out.
CMD ["node", "build/index.js"]
