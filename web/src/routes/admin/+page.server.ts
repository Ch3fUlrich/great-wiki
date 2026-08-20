import { apiGet, type TreeNode } from '$lib/api';
import {
  describeStatus,
  type AclView,
  type AdminPrincipal,
  type AuditPage,
  type GroupRole,
  type Loaded,
  type Team
} from '$lib/adminApi';
import type { PageServerLoad } from './$types';

/**
 * Everything the console shows, fetched on the server so the first paint is real.
 *
 * Loaded here rather than from the browser on mount for two reasons. Without it the page
 * would render four empty panels and then fill them in, which is exactly the "spinner
 * forever" this screen must not have if a script fails. And the administration API is
 * built separately from this interface: when it is not there yet, this load turns four
 * 404s into four German sentences instead of four blank rectangles.
 *
 * Each endpoint is loaded independently and its failure kept beside its data. One dead
 * endpoint must not take the other three down — "Teams could not be loaded" is a useful
 * screen; "the admin console is broken" is not.
 */

const DEFAULT_LIMIT = 50;
/** Mirrors the choices the Protokoll panel offers. Anything else falls back. */
const ALLOWED_LIMITS = [50, 200, 1000];

async function loadOne<T>(
  fetchFn: typeof fetch,
  cookie: string | null,
  path: string,
  clause: string
): Promise<Loaded<T>> {
  try {
    const { status, data } = await apiGet<T>(fetchFn, path, cookie);
    if (data === null) return { data: null, error: describeStatus(status, clause) };
    return { data, error: null };
  } catch {
    // `apiGet` throws when the request never got an answer at all.
    return { data: null, error: describeStatus(0, clause) };
  }
}

export const load: PageServerLoad = async ({ fetch, request, url }) => {
  const cookie = request.headers.get('cookie');

  // Both selections live in the URL, so a link to "who can reach /handbuch" is a link
  // somebody can send, and the back button walks back through the paths they looked at.
  const selectedPath = url.searchParams.get('pfad');
  const requested = Number(url.searchParams.get('anzahl') ?? DEFAULT_LIMIT);
  const limit = ALLOWED_LIMITS.includes(requested) ? requested : DEFAULT_LIMIT;

  const [tree, people, teams, roles, audit, acl] = await Promise.all([
    loadOne<TreeNode[]>(fetch, cookie, '/api/tree', 'Der Seitenbaum konnte nicht geladen werden'),
    loadOne<AdminPrincipal[]>(
      fetch,
      cookie,
      '/api/admin/principals',
      'Die Personenliste konnte nicht geladen werden'
    ),
    loadOne<Team[]>(fetch, cookie, '/api/admin/teams', 'Die Teamliste konnte nicht geladen werden'),
    // Default reach, before any entry: the access panel cannot answer "who reaches this
    // page" without it, because a group mapped to `admin` reads every restricted page in
    // the corpus and no entry anywhere shows that. Instance-wide, so a space admin is
    // refused it — which the panel says in words rather than reading as "no group has it".
    loadOne<GroupRole[]>(
      fetch,
      cookie,
      '/api/admin/roles',
      'Die Zuordnung von Gruppen zu Reichweiten konnte nicht geladen werden'
    ),
    loadOne<AuditPage>(
      fetch,
      cookie,
      `/api/admin/audit?limit=${limit}`,
      'Das Protokoll konnte nicht geladen werden'
    ),
    selectedPath
      ? loadOne<AclView>(
          fetch,
          cookie,
          `/api/admin/acl?path=${encodeURIComponent(selectedPath)}`,
          `Die Zugriffsrechte für ${selectedPath} konnten nicht geladen werden`
        )
      : Promise.resolve<Loaded<AclView>>({ data: null, error: null })
  ]);

  return {
    selectedPath,
    limit,
    tree,
    people,
    // A team with no membership list and a team with an empty one look the same to the
    // interface, and `undefined.length` does not. Normalised once, here.
    teams: {
      data: teams.data ? teams.data.map((team) => ({ ...team, members: team.members ?? [] })) : null,
      error: teams.error
    } satisfies Loaded<Team[]>,
    roles,
    audit,
    acl
  };
};
