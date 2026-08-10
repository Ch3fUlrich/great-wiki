import { error } from '@sveltejs/kit';
import { dev } from '$app/environment';
import type { PageServerLoad } from './$types';

/**
 * This route exists only so the Playwright behaviour check (web/scripts/behaviour.mjs)
 * has somewhere to mount Dialog.svelte in isolation. It is not a page a reader should
 * ever reach.
 *
 * A leading underscore does NOT hide a route from SvelteKit 2's router — verified by
 * reading `create_manifest_data` (it walks every subdirectory of `src/routes`
 * unconditionally, with no name-based exclusion) and confirmed empirically: a probe
 * route at `/_probe` served 200. So the underscore here is only a naming hint for
 * anyone reading the tree; the actual guard is this load function, which 404s unless
 * the app is running under `vite dev` — i.e. it never exists in a production build.
 */
export const load: PageServerLoad = () => {
  if (!dev) error(404, 'Not found');
};
