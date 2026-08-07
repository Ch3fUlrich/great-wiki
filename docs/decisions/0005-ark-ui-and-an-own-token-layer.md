# 0005 — Ark UI with our own token layer, not a styled component library

**Status:** Accepted (2026-08-07)

## Context

The platform must support **runtime-installable styling plugins**: a designer packages a
theme, someone installs it from a folder or a Git URL, and it takes effect without
rebuilding the application (ADR context in spec §12.1).

That single constraint decides the component library, and not in the direction reputation
suggests. A plugin arrives *after* the build. Anything compiled at build time — a Tailwind
config, Sass token overrides — has already been resolved by then, so classes a plugin
invents do not exist in the stylesheet at all. Only **CSS custom properties** and overrides
of stable class names can be swapped at runtime.

Six candidates were evaluated against current sources rather than from memory:

| Library | Runtime-themeable | Data grid | Disqualifier |
|---|---|---|---|
| Skeleton v3 | No — Tailwind-coupled | Basic | Theming resolves at build time |
| shadcn-svelte | Partly | Pair externally | Tailwind-coupled; `bits-ui` had 4 commits in 90 days |
| Carbon Svelte | Barely — ~18% variable coverage | **Best in field** | Built on Carbon **v10**; inescapable shape language |
| Web Awesome | **Best contract** — 419 tokens, `::part()` | Paywalled, 5 weeks old, experimental | Experimental SSR on a SvelteKit bug open since 2023; **icons fetched from a third-party CDN** |
| Material Web | n/a | Never will ship one | Maintenance mode |
| **Ark UI Svelte** | **Total — ships zero CSS** | Pair with TanStack Table | You build the design system |

Two findings deserve emphasis because they contradict common assumptions. **Carbon is
weaker than its reputation here** — the Svelte port tracks Carbon v10, and only about a
fifth of its styling is reachable through CSS variables, so a plugin system on top of it
would be largely decorative. And **Web Awesome's CDN icon dependency is a compliance
problem, not a performance one**: a self-hosted German platform silently issuing a
third-party request per icon is a data-protection issue.

Ark UI's Svelte target is not a second-class port — 62 Svelte component directories against
61 for React, identical release dates, and 13 open issues, the healthiest project examined.

## Decision

**Ark UI Svelte for behaviour, our own CSS-custom-property token layer for appearance, and
TanStack Table for data grids.**

The argument is one sentence: **Ark UI ships zero CSS.** Every rule in the application is
one we wrote, so all of it can reference our variables and be swapped at runtime. There is
no vendor token vocabulary to negotiate with, no shadow boundary to fight, and the plugin
API versions on our schedule.

The mechanism that makes this cheap: **application CSS goes in named cascade layers, plugin
CSS is loaded unlayered.** Unlayered normal declarations beat every layered one regardless
of specificity, so plugins win by construction — no `!important`, no specificity arms race,
and no need to pre-declare every possible override.

## Consequences

- We build the design system. For a product whose differentiator *is* runtime-installable
  themes, that system is the product surface, not overhead — but it is real work before the
  first screen looks finished.
- **Uploaded CSS is untrusted input.** One person's theme renders in another person's
  session, and CSS alone can exfiltrate through `url()` and `@import`. Plugins are
  restricted to custom-property declarations plus an allowlisted selector set, parsed and
  rejected rather than sanitised in place, with Content-Security-Policy as backstop.
- **Every chart needs a visually-hidden data-table twin.** ECharts' built-in accessibility
  support is a verified no-op under server rendering, so it cannot carry the accessibility
  requirement on its own.
- Two things to verify by building rather than reading, both cheap: Ark UI under SvelteKit
  server rendering (Zag.js is SSR-aware and Ark runs under Next and Nuxt, but there is no
  SvelteKit-specific documentation and no VPAT), and whether `@svar-ui/svelte-grid` is worth
  using instead of building `aria-sort` ourselves — it is the only Svelte-native grid that
  actually emits it.
- Token vocabulary starts from `@primer/primitives` or Open Props v1. Open Props v2 is dead;
  Radix Colors is finished rather than abandoned and remains usable.

## Switch-back criteria

Revisit if building the design system proves slower than the plugin system is worth — that
is, if after M4 there is still no theme anyone but us has written. The behaviour layer
(Ark UI) would stay either way; only the appearance layer would change.
