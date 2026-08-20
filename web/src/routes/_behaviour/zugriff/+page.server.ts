import { error } from '@sveltejs/kit';
import { dev } from '$app/environment';
import type { PageServerLoad } from './$types';

/**
 * Dev-only mount for AccessPanel, for the same reason `../+page.svelte` mounts Dialog:
 * the sentences this panel says about the reach of a grant live inside an Ark `Portal`,
 * which renders nothing on the server — so a `svelte/server` test cannot see them, and
 * only a browser can. The real console at `/admin` is not usable for that: it needs the
 * `admin` baseline, and `just behaviour` deliberately runs as `sergej:editors` (a group
 * with no `group_roles` row) because an `admin` baseline reads every `restricted`
 * document and would quietly break the D-group checks.
 *
 * `fall` picks which of the two states the panel has to describe:
 *   `geerbt` — the grants come from an ancestor, and the page is `restricted`
 *   `eigen`  — the grants are written on this very path
 *
 * Guarded exactly as `../+page.server.ts` is: a leading underscore hides nothing from
 * SvelteKit 2's router, so `dev` is the actual gate and this route does not exist in a
 * production build.
 */
export const load: PageServerLoad = ({ url }) => {
  if (!dev) error(404, 'Not found');
  return { fall: url.searchParams.get('fall') === 'eigen' ? 'eigen' : 'geerbt' } as const;
};
