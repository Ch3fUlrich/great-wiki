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
			adapter: adapter()
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
