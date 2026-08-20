# great-wiki in production

Written 2026-08-12, the day the first deployment went out. Everything here was checked
against the running system rather than against the compose file, because the two disagreeing
is exactly the thing this document exists to catch.

## Which name is production

| Name | Points at | What it is |
|---|---|---|
| `wiki.ohje.ooguy.com` | `cloud.vm:8100` | **Production.** The deployed containers. |
| `wiki-dev.ohje.ooguy.com` | `coding.vm:5173` | The Vite dev server. **502 unless somebody is running `just dev`.** |

Both are registered in Authelia as redirect URIs for the one OIDC client `great-wiki`, so a
sign-in works against either — but only one of them is a service.

## Who can reach it

**The whole internet.** `wiki.ohje.ooguy.com` resolves publicly to `109.250.70.12`.

This document first said the opposite, and the way that happened is worth keeping: `dig`
against 1.1.1.1, 8.8.8.8 and 9.9.9.9 all returned empty, so the name looked unresolvable —
but **outbound DNS to public resolvers is blocked from coding.vm**, and a control query for
`example.com` came back just as empty. Three resolvers agreeing meant nothing; they were
all failing the same way. Anything measured from inside this network about what the outside
can see must be checked from outside it — resolving through a DoH endpoint, or fetching the
site from somewhere else entirely.

What stands between the internet and the content:

- Every imported page is `restricted`, so an anonymous caller gets `[]` from `/api/tree`
  and 403 from every document. **This is the only thing making the corpus private**, and it
  is a per-page property: publishing one page makes it world-readable. There is no second
  gate behind it.
- `/api/me` answers anonymously but reports nothing — no username, no groups, no hint that
  any particular account exists.
- Admin sign-in is Authelia OIDC with `authorization_policy: two_factor`.
- Local accounts (from invites) are a password login exposed to the internet. It is rate
  limited at ten failures per five minutes, counted **twice** — once per submitted username
  and once per client address — so neither a single account nor a single source can be
  ground down, and a wrong username costs the attacker the same as a right one.
- HSTS (two years, `includeSubDomains`, `preload`), `nosniff`, `SAMEORIGIN` and
  `strict-origin-when-cross-origin` come from the edge's `secure_headers` snippet.
- A Content-Security-Policy, issued by the **application** rather than by either proxy —
  see [ADR 0007](../decisions/0007-content-security-policy.md) for why, and for the two
  things about it that only a browser could have told us. `script-src` is nonce-based with
  no `'unsafe-inline'`; the API's own HTML (`/auth/login`, the invitation page) gets a
  separate, stricter `default-src 'none'` policy from `crates/gw-api/src/csp.rs`, because
  those pages are routed to the API and SvelteKit's policy never reaches them.

**What the policy does NOT cover**, so that nobody reads the line above as "done":

- **Static files.** `/fonts/*`, `/_app/*` and `robots.txt` are served by adapter-node with
  no policy. None of them is HTML, so nothing executes from them under a policy or without
  one — but the day something serves an uploaded file, that is the gap to close first.
- **Violation reporting.** There is no `report-to` and no `report-uri`. A policy that
  breaks a page in production is silent unless somebody opens a browser console.
- **`wiki-dev.ohje.ooguy.com`.** It is the Vite dev server, and SvelteKit deliberately
  relaxes `style-src` to `'unsafe-inline'` in development so it can inject component
  styles. The dev name is public; the policy it serves is the weaker one.

## Where the content actually is

```
/home/s/apps/great-wiki/data/great-wiki.db     the wiki. NVMe bind mount on cloud.vm.
/mnt/cloud/great-wiki/media                    blobs. NFS. Empty so far.
```

**The database must never move to the NFS share.** SQLite depends on POSIX advisory
locking, which is fragile over NFS; the Server repo records karakeep and homebox being
moved off the share for exactly this reason, and a Postgres data directory on the same
share was one of the amplifiers behind the 2026-08-11 NFS livelock. Blobs on NFS are fine —
they are written once and read whole.

There is no second copy. The `content-darm/` directory in the working tree is the *import
source*, not a backup, and it is gitignored because it is personal medical information
about a child in a repository that is public on GitHub.

## The shape of the stack

```
edge Caddy (OPNsense, a DIFFERENT host) --injects X-GW-Proxy--> cloud.vm:8100
cloud.vm:8100 = gw-caddy
    /healthz -> answered by Caddy itself, 200. The deploy's health gate.
    /api/*   -> gw-api:8092
    /auth/*  -> gw-api:8092
    /*       -> gw-web:3000  (SvelteKit SSR)
```

Three things about this that are load-bearing:

- **The internal Caddy forwards `X-GW-Proxy` and must never set it.** gw-api binds 0.0.0.0
  and its port is reachable from the LAN, so that header is the only thing separating a
  request that came through the edge from one that did not. A proxy that injected it would
  attest everything that reached it. The caddy container is given no access to the value at
  all, which is what makes the rule hold by construction. Verified: `caddy adapt` on the
  built image contains no occurrence of `gw-proxy`.
- **`gw-web` holds the secret too**, because server-side rendering calls the API over HTTP
  like anything else and is refused without it. It was missing until the day of the first
  deploy, and the failure mode is worth remembering: the layout turns a failed `/api/me`
  into "nobody signed in", so the site would have shown the public view to signed-in people
  with nothing in any log a reader would look at.
- **`/api/health` is not reachable unauthenticated.** The proxy guard is the outermost
  layer *after* every route, deliberately, so it wraps the 404 fallback — and therefore the
  health route too. That is why the deploy gates on `/healthz`, which Caddy answers itself.

## Deploying a new version

1. Build and push **all three** images at the same short SHA:
   `harbor.ohje.ooguy.com/great-wiki/{gw-api,gw-web,gw-caddy}:<sha>`
2. Bump all three `image:` lines in `Server/server/cloud/great-wiki/docker-compose.yml`
   and push that repo — Semaphore pulls **origin/improve** from GitHub.
3. Launch Semaphore template 36 with `image_repos` listing all three.

`app-deploy.yml` re-pins every repository named in `image_repos` and checks the
substitution **per repository**, not in total: with three images and two named, a
total-based check sees two matches, clears its threshold and deploys the skew. The
committed placeholder tag is `0000000`, so an image that is never re-pinned fails to pull
rather than quietly running an old build.

## Seeding content

The container's root filesystem is read-only, so `docker cp` into it fails. Run a one-off
container from the same image with the data volume mounted:

```sh
docker run --rm --user 1000:1000 \
  -v /home/s/apps/great-wiki/data:/data \
  -v /path/to/content:/content:ro \
  -e GW_DATABASE_URL=sqlite:///data/great-wiki.db \
  harbor.ohje.ooguy.com/great-wiki/gw-api:<sha> seed --content /content
```

Delete the content from the host afterwards if it is personal.

## Things that have already gone wrong here

- **Caddy would not start under the stack's hardening.** `exec /usr/bin/caddy: operation
  not permitted`, with gw-api and gw-web healthy beside it. The upstream image ships the
  binary with `cap_net_bind_service`, and the kernel refuses `execve` of a file carrying
  file capabilities for a non-root user under `no-new-privileges`. The image now strips the
  capability — the proxy listens on 8100 and never needed it.
- **Harbor's robot could not push to a new project.** `robot$seeder` is system-scoped with
  per-namespace grants, and a new Harbor project is not in them. Both the push *and the
  deploy's pull* fail until it is granted.
