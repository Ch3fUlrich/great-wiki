# 0007 — The Content-Security-Policy is issued by the application, not by either proxy

**Status:** Accepted (2026-08-20)

## Context

`wiki.ohje.ooguy.com` is on the public internet and renders content its users author.
`docs/operations/running-in-production.md` recorded the absence of a Content-Security-Policy
as a known gap and said, correctly, that it is the header that matters most here.

Nothing known is broken. `safeHref` (`web/src/lib/blocks/render.ts`) refuses a dangerous
scheme at the render sink, the renderer escapes attribute values, and
`auth::invite::escape` escapes the one piece of authored text the API itself interpolates
into HTML. A CSP is not a patch for any of those. It is the layer that catches the hole
nobody has thought of yet, which is the only kind of hole that matters after the known ones
are closed.

Three places could issue it, and they are genuinely different places rather than three
spellings of one choice.

**The edge `secure_headers` snippet** (`Server/server/network/opnsense/caddy.d/00-snippets.conf`)
is imported by more than forty site blocks — Jellyfin, Harbor, Authelia, Omnigraph, a
Cockpit, a CI. It already carries HSTS, `nosniff`, `SAMEORIGIN` and a referrer policy, all
of which are the same correct answer for every service behind it. A CSP is not: one policy
that suits a wiki suits none of the others, and the failure mode of getting it wrong there
is a different service going blank. A wiki-specific snippet beside it would avoid that, but
still leaves the other two problems below.

**The stack's internal Caddy** (`docker/Caddyfile`) is great-wiki only. It is also a
`header` line in a proxy: a reader of the application sees no policy anywhere in the code,
a developer running `just dev` gets no policy at all, and `gw-caddy` is a separately built
and separately deployed image, so the policy and the markup it has to match ship on
different schedules.

**SvelteKit** has `kit.csp`, which is the only one of the three that can mint a nonce.

## Decision

**The policy is configured in the application: `kit.csp` in `web/vite.config.ts`, in
`mode: 'nonce'`.** Neither proxy is changed.

A second, much stricter policy is attached by `gw-api` to the HTML it renders itself
(`crates/gw-api/src/csp.rs`), because `/auth/*` is routed to the API and never reaches
SvelteKit.

The deciding argument is the nonce. `script-src 'self'` with no `'unsafe-inline'` is the
directive the whole header exists for, and it is only reachable if the one inline script in
`web/src/app.html` — the pre-paint theme and typeface resolution — can be authorised
individually. A proxy cannot do that: it sees the response as bytes, has no idea what is in
it, and would have to be handed either `'unsafe-inline'`, which surrenders the point, or a
hash that has to be maintained by hand against a file in another repository.

Everything else follows from that one:

- It travels with the code. The nonce in `app.html` and the policy that permits it are in
  the same commit, and neither can be deployed without the other.
- It is in force in `just dev`, so a policy that breaks something breaks it locally.
- It is per response, and it is asserted by `web/scripts/behaviour.mjs` group G.

Note that there is no `svelte.config.js` in this project: SvelteKit's options are passed
inline to the `sveltekit()` plugin in `vite.config.ts` and split out by SvelteKit's own
`split_config`. That is where to look for `csp`.

## The policy

```
default-src 'self';
frame-src 'none';
connect-src 'self';
font-src 'self';
img-src 'self' data:;
object-src 'none';
script-src 'self' 'nonce-<per response>';
style-src 'self' 'nonce-<per response>';
style-src-attr 'unsafe-inline';
base-uri 'self';
form-action 'self';
frame-ancestors 'self'
```

and, for the API's own HTML only:

```
default-src 'none'; style-src 'unsafe-inline'; form-action 'self';
base-uri 'none'; frame-ancestors 'self'
```

`'self'` stays in `script-src` beside the nonce deliberately. A nonce does not propagate to
a dynamic `import()`, and TipTap and Yjs arrive as dynamically imported chunks; the URL
allow-list is what admits them.

## What had to be loosened, and what forced it

**`style-src-attr 'unsafe-inline'`.** Svelte's `style:` directive server-renders as a
literal `style="…"` attribute — `BlockView.svelte`, `TableView.svelte`, the outline in
`[...path]/+page.svelte` — and TipTap's table extension and collaboration caret write
widths and colours the same way. One editor page was measured carrying 37 of them. CSP has
no nonce or hash mechanism for attributes at all, so the choice was `'unsafe-inline'` or
deleting a rendering feature. It is confined to `style-src-attr` rather than added to
`style-src`, so `<style>` ELEMENTS are still refused; the residual risk is CSS-only, and
nothing renders authored content into a style attribute.

**`style-src 'unsafe-inline'` on the API's two HTML pages.** Both carry their stylesheet in
a `<style>` block on purpose — a sign-in page has to render when the rest of the stack is
broken, which is when people most need to look at it. It is paid for by `default-src 'none'`
on those pages, which denies script outright rather than merely restricting where it may
come from.

Nothing else. `script-src` has no `'unsafe-inline'`, no `'unsafe-eval'` and no
`'wasm-unsafe-eval'` anywhere.

## Two things that were measured rather than reasoned about

**SvelteKit puts its nonce in `script-src` and, in a production build, nowhere else.**
`add_style` — the function that would add it to `style-src` — is only called for inline
styles SvelteKit itself emits, and with the default `inlineStyleThreshold` of 0 it never is.
TipTap appends the ProseMirror base stylesheet as a `<style>` element and accepts a nonce
for it (`injectNonce`), but the nonce meant nothing while the policy did not name it there.
`web/src/hooks.server.ts` copies the response's own nonce into `style-src`, and
`web/src/lib/csp.ts` holds that as a pure, tested function.

Because SvelteKit adds `'unsafe-inline'` to `style-src` in DEVELOPMENT, this failed *only in
production* — an unstyled editing surface with the console as the only evidence.

**A nonce beside `'unsafe-inline'` removes permission rather than adding it.** Browsers
ignore `'unsafe-inline'` in any source list that also contains a nonce or a hash. The first
version of the hook widened `style-src` unconditionally and turned `just dev` into fourteen
`Applying inline style violates …` errors and an unstyled page while production stayed
clean — the same production/development split as above, in the other direction. The hook now
leaves any directive that already permits inline content exactly as it is, and
`csp.test.ts` pins that with the real development header.

Both of these were found by loading the site in a browser. Neither was visible from reading
the configuration, and the second was introduced by a change that had already been verified
in production mode.

## Consequences

- **`mode: 'nonce'` and prerendering are mutually exclusive.** SvelteKit throws rather than
  shipping an unenforceable policy. Adding `export const prerender = true` to any route
  means revisiting this decision, not working around the error.
- **`app.html`'s inline script needs `nonce="%sveltekit.nonce%"`.** Removing that attribute
  reintroduces the flash of the wrong theme and the wrong typeface, and nothing but the
  console says why. Group G4 of `behaviour.mjs` is what notices.
- **A library that injects a `<style>` element must be handed the nonce.** TipTap is, in
  `Editor.svelte`; the next one will have to be. Reading it off the DOM
  (`document.querySelector('script[nonce]')?.nonce`) is the documented way — the HTML parser
  blanks the content attribute and keeps the value on the IDL property.
- **There is no violation reporting.** No `report-to` and no `report-uri`, so a policy that
  breaks something in production is silent unless somebody opens a console. That is a
  deliberate deferral: a report endpoint is a public, unauthenticated write path into this
  deployment, and it is a decision of its own.
- **The edge keeps its `X-Frame-Options: SAMEORIGIN`.** `frame-ancestors 'self'` says the
  same thing to a modern browser, but the edge's copy also covers every response that never
  reaches either policy.
