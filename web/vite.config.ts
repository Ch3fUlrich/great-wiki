import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

/**
 * Where the dev proxy sends `/api` and `/auth`.
 *
 * `GW_BEHAVIOUR_PORT` is the same variable `just behaviour` takes, so the harness and the
 * proxy it drives cannot end up pointing at different ports — which is exactly what
 * happened when only one of them was made overridable.
 */
const apiTarget = `http://127.0.0.1:${process.env.GW_BEHAVIOUR_PORT ?? '8092'}`;

export default defineConfig({
	plugins: [
		sveltekit({
			compilerOptions: {
				// Force runes mode for the project, except for libraries. Can be removed in svelte 6.
				runes: ({ filename }) =>
					filename.split(/[/\\]/).includes('node_modules') ? undefined : true
			},

			// adapter-auto only supports some environments, see https://svelte.dev/docs/kit/adapter-auto for a list.
			// If your environment is not supported, or you settled on a specific environment, switch out the adapter.
			// See https://svelte.dev/docs/kit/adapters for more information about adapters.
			adapter: adapter(),

			// ---------------------------------------------------------------------------
			//  Content-Security-Policy. See docs/decisions/0007-content-security-policy.md
			//  for why it is HERE and not at either proxy; the short version is that this is
			//  the only one of the three places that can mint a per-response nonce, and a
			//  nonce is what buys `script-src 'self'` with no `unsafe-inline`.
			//
			//  There is no svelte.config.js in this project — SvelteKit's own options are
			//  passed inline to the `sveltekit()` plugin and split out of this object by
			//  `split_config` — so this IS the kit config, and grep for "csp" finds it here.
			// ---------------------------------------------------------------------------
			csp: {
				// `nonce`, not `auto`. `auto` means "hash when prerendering, nonce otherwise",
				// and nothing here prerenders — but `auto` would silently switch modes the day
				// something does, and a hash cannot cover the one inline script in app.html
				// unless that hash is maintained by hand. A nonce is regenerated per response
				// and app.html asks for it by name (`%sveltekit.nonce%`).
				//
				// The cost is recorded so nobody rediscovers it: `mode: 'nonce'` and
				// prerendering are mutually exclusive, and SvelteKit throws rather than
				// shipping an unenforceable policy. Adding `export const prerender = true` to
				// any route means revisiting this line.
				mode: 'nonce',
				directives: {
					// Everything falls back to here: media, workers, manifests, frames. Named
					// directives below only exist where the fallback is either wrong or worth
					// stating out loud.
					'default-src': ['self'],

					// No 'unsafe-inline' and no 'unsafe-eval'. SvelteKit's bootstrap script and
					// app.html's pre-paint theme script both carry the nonce; the module chunks
					// TipTap and Yjs arrive in are `import()`ed by URL. A dynamic `import()` is
					// checked as an ordinary resource fetch against script-src's host/scheme
					// sources rather than against the nonce — a nonce only ever authorises an
					// element carrying the `nonce` attribute, and an `import()` is not one — so
					// 'self' is what admits those chunks, which is why it stays alongside the
					// nonce. (`'strict-dynamic'` would let the nonce'd bootstrap propagate trust
					// to them instead and drop the need for 'self' — measured working, not
					// adopted; see docs/decisions/0007-content-security-policy.md.)
					'script-src': ['self'],

					// Stylesheets are real files with real URLs, so they need nothing looser.
					'style-src': ['self'],

					// LOOSENED, and this is the one loosening in the policy. Svelte's `style:`
					// directive server-renders as a literal `style="…"` ATTRIBUTE — see
					// BlockView.svelte, TableView.svelte and the outline in [...path]/+page.svelte
					// — and TipTap's table extension writes column widths the same way. A
					// `style` attribute is indistinguishable to CSP from an injected one, and
					// there is no nonce or hash mechanism for attributes at all, so the choice
					// is 'unsafe-inline' here or deleting a rendering feature.
					//
					// It is confined to `style-src-attr`, which is why it is written as its own
					// directive rather than added to `style-src`: CSP3 splits attribute styles
					// from `<style>` ELEMENTS, so `style-src-elem` still inherits the strict
					// 'self' above and an injected `<style>` block is still refused — which is
					// most of why the residual risk is small: the attribute-selector techniques
					// used to exfiltrate page content need a selector to attach to, and a
					// `style="…"` attribute holds only declarations for the one element that
					// carries it, no selector, so that needs a `<style>` element regardless. A
					// `url()` inside a declaration is still bound by img-src/font-src, which admit
					// no remote host. No script executes from a style attribute, and the renderer
					// does not emit authored CSS into one either way.
					'style-src-attr': ['unsafe-inline'],

					// Local font files only (static/fonts). Nothing is fetched from a CDN and
					// nothing may start being.
					'font-src': ['self'],

					// `data:` because an inline SVG data URI is the cheap way to ship an icon and
					// one may appear; it cannot execute script in an `<img>`. No remote hosts:
					// a wiki that fetched images from anywhere would leak every reader's address
					// to whoever authored the page.
					'img-src': ['self', 'data:'],

					// Covers the editor's WebSocket to /api/collab/*. CSP3 matches 'self' against
					// ws:/wss: at the same host and port, which is the whole reason no scheme is
					// listed here — verify this one in a browser after any change, because it is
					// the directive whose failure looks like "the editor just never connects".
					'connect-src': ['self'],

					// Nothing is embedded and nothing embeds this. `frame-ancestors 'self'` is
					// the modern spelling of the X-Frame-Options: SAMEORIGIN the edge already
					// sends; both are kept, because the edge's copy also covers the responses
					// SvelteKit never renders.
					'frame-ancestors': ['self'],
					'frame-src': ['none'],

					// Plugins. Nothing uses them; `<object>` and `<embed>` are script-execution
					// sinks that survive most other hardening.
					'object-src': ['none'],

					// 'none', not 'self'. There is no `<base>` element anywhere in this app —
					// app.html has none, and nothing renders one — and SvelteKit's own bootstrap
					// references its chunks with base-relative specifiers, so an injected
					// `<base href>` under 'self' could still re-point them within the origin.
					// 'none' closes that outright, and matches crates/gw-api/src/csp.rs, which
					// had no reason to disagree with this one.
					'base-uri': ['none'],

					// Sign-out and view-as-exit both post same-origin. The OIDC hand-off to
					// Authelia is a redirect, not a form submission, so it is unaffected.
					'form-action': ['self']
				}
			}
		})
	],
	ssr: {
		// Ark UI ships uncompiled `.svelte` sources. Left externalised, Node tries to
		// `require` them during server rendering and dies with
		// `ERR_UNKNOWN_FILE_EXTENSION: Unknown file extension ".svelte"` — a 500 on any
		// page using an Ark component, with nothing in the browser console to say why.
		// `noExternal` puts the package through Vite's Svelte pipeline instead.
		noExternal: ['@ark-ui/svelte']
	},
	server: {
		host: true, // bind 0.0.0.0 — Caddy is on another host
		allowedHosts: ['wiki-dev.ohje.ooguy.com'], // Vite rejects unknown Host headers by default
		// Without this, /api/* 404s in `npm run dev` — in production Caddy routes it.
		// `/auth/*` is the OIDC login flow and lives on the same API: the browser is
		// redirected out to Authelia and back to /auth/callback, so the path has to reach
		// the application rather than SvelteKit's router.
		proxy: {
			// `ws: true` is what makes `/api/collab/*` work at all in development, and it was
			// measured rather than assumed: Vite registers an `upgrade` listener only for a
			// proxy entry that asks for one, so without it the handshake is never forwarded
			// and the client simply HANGS — no open, no error, no close, until whatever
			// timeout the caller brought. The same socket opened straight at 127.0.0.1:8092
			// answers immediately. Caddy proxies WebSockets natively, so production would
			// never have shown this and the editor would have looked broken only in
			// development, which is the worst place for a difference to live.
			// The port is overridable for the same reason `just behaviour` makes it
			// overridable: 8092 is not reliably free on this machine — another project on
			// coding.vm binds it — and a proxy still pointing at 8092 while the API listens
			// elsewhere fails in the least helpful way available. The app loads, the page
			// renders, and every call answers "Der Server antwortet nicht": six behaviour
			// checks went red that way, none of them naming a port.
			'/api': { target: apiTarget, changeOrigin: true, ws: true },
			'/auth': { target: apiTarget, changeOrigin: true }
		}
	}
});
