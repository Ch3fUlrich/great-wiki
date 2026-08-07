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
	server: {
		host: true, // bind 0.0.0.0 — Caddy is on another host
		allowedHosts: ['wiki-dev.ohje.ooguy.com'], // Vite rejects unknown Host headers by default
		// Without this, /api/* 404s in `npm run dev` — in production Caddy routes it.
		proxy: {
			'/api': { target: 'http://127.0.0.1:8092', changeOrigin: true }
		}
	}
});
