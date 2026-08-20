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
 * `fall` picks which state the panel has to describe:
 *   `geerbt`   — the entries come from an ancestor, and the page is `restricted`
 *   `eigen`    — the entries are written on this very path, with nothing above
 *   `letzter`  — the LAST entry on this path, with an ancestor that carries some, so
 *                revoking it hands the page and its subtree back to that ancestor
 *   `freigabe` — an `anyone` entry: a share link into the open internet
 *
 * Guarded exactly as `../+page.server.ts` is: a leading underscore hides nothing from
 * SvelteKit 2's router, so `dev` is the actual gate and this route does not exist in a
 * production build.
 */
const FAELLE = ['geerbt', 'eigen', 'letzter', 'freigabe'] as const;

export const load: PageServerLoad = ({ url }) => {
  if (!dev) error(404, 'Not found');
  const asked = url.searchParams.get('fall');
  const fall = FAELLE.find((known) => known === asked) ?? 'geerbt';
  return { fall };
};
