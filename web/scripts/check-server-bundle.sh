#!/bin/sh
#
# Refuse a server bundle that imports a package the runtime image will not contain.
#
# ONE definition, called by `docker/gw-web.Dockerfile` (inside `docker build`) and by
# `just verify-server-bundle` (which `just build` runs, so `just ci` and `just agent-ci`
# run it too). It used to live only in the Dockerfile, and that was the whole problem:
# `npm run build`, `just lint`, `just test` and `just ci` all passed green with a missing
# `ssr.noExternal` entry, and the failure appeared at image build, after review. The
# highlighter added in the rich-blocks work is the second package to need such an entry,
# and it would have been the first one to find that out the expensive way.
#
# WHY THE RUNTIME HAS NO node_modules
# -----------------------------------
# `docker/gw-web.Dockerfile`'s runtime stage carries `build/`, a `package.json` and the
# node binary — no vite, no typescript, no svelte compiler, and npm itself removed. That
# is only possible while the server bundle imports nothing but `node:` builtins and its
# own relative chunks. It is a property of the CURRENT vite config rather than a law, and
# if it stops being true the failure is `ERR_MODULE_NOT_FOUND` on the first request in
# production.
#
# `/bin/sh`, not bash: this runs inside `node:*-slim`, which has no bash.
#
# Usage: run it from `web/`, with `build/` already produced by `npm run build`.

set -eu

if [ ! -d build ]; then
  echo "::error::no build/ directory — run 'npm run build' first" >&2
  echo "A check that cannot run must not report a clean bundle." >&2
  exit 2
fi

# A check that looked at no files would report "clean" for the wrong reason — an adapter
# change, a moved output directory, the wrong working directory. This is the failure
# `scripts/scan-secrets.sh` documents having shipped once already.
scanned="$(find build -name '*.js' -not -path 'build/client/*' | wc -l)"
if [ "$scanned" -eq 0 ]; then
  echo "::error::the server bundle check found no server JavaScript at all" >&2
  echo "Expected files under build/ outside build/client/. Refusing to report clean." >&2
  exit 2
fi

# Anchored at column 0 on purpose: rollup emits real import statements there, while the
# bundle's JSDoc blocks are full of lines like `* import { mount } from 'svelte';` that are
# documentation, not code. `build/client/` is excluded — those files are served to
# browsers, not executed by node.
external="$(
  find build -name '*.js' -not -path 'build/client/*' \
    -exec grep -hE "^(import|export)[^']*from '[^']+'" {} + \
  | grep -oE "from '[^']+'" \
  | cut -d"'" -f2 \
  | grep -vE '^(node:|\.)' \
  | sort -u || true )"

if [ -n "$external" ]; then
  echo "REFUSING: the server bundle imports packages this image will not contain:" >&2
  echo "$external" >&2
  echo "Either add them to ssr.noExternal in web/vite.config.ts so they are" >&2
  echo "compiled in, or restore an 'npm ci --omit=dev' stage and copy" >&2
  echo "node_modules into the runtime — but check what that drags in first." >&2
  exit 1
fi

echo "server bundle is self-contained: node: builtins and relative chunks only ($scanned files)"
