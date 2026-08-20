import adapter from '@sveltejs/adapter-node';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

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
					// TipTap and Yjs arrive in are `import()`ed by URL and are covered by 'self'
					// (a nonce does not propagate to a dynamic import — the URL allow-list is
					// what admits them, which is exactly why 'self' stays alongside the nonce).
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
					// 'self' above and an injected `<style>` block is still refused. The residual
					// risk is CSS-only — no script executes from a style attribute — and the
					// renderer does not emit authored CSS into one.
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

					// Without this, one injected `<base href>` re-points every relative URL on
					// the page — including the ones 'self' was supposed to have pinned down.
					'base-uri': ['self'],

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
			'/api': { target: 'http://127.0.0.1:8092', changeOrigin: true, ws: true },
			'/auth': { target: 'http://127.0.0.1:8092', changeOrigin: true }
		}
	}
});
