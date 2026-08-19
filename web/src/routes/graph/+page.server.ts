import { apiGet, type Graph } from '$lib/api';
import type { PageServerLoad } from './$types';

/**
 * The graph, fetched on the server so the picture arrives in the first response.
 *
 * Loaded here rather than from the browser on mount because the layout is computed during
 * rendering (see `$lib/graph/layout`): the whole diagram is in the HTML, and a reader with
 * JavaScript switched off gets exactly the same one.
 *
 * **Nothing here filters anything.** `/api/links/graph` answers what THIS caller may read —
 * an edge is on the wire only when both its ends are readable — and that property belongs to
 * `Store::graph_for`, where it is mutation-tested. A second filter in this file would be a
 * second place for it to be wrong.
 *
 * The empty graph and the failed request are kept apart. `data === null` means the request
 * did not answer, and rendering "there are no links yet" for it would be a lie about a
 * server that is down; the console's loader makes the same distinction for the same reason.
 */
const EMPTY: Graph = { nodes: [], edges: [] };

function describe(status: number): string {
  if (status === 0) return 'Der Graph konnte nicht geladen werden: die Anwendung antwortet nicht.';
  return `Der Graph konnte nicht geladen werden (Fehler ${status}).`;
}

export const load: PageServerLoad = async ({ fetch, request, url }) => {
  const cookie = request.headers.get('cookie');

  // In the URL, so a link to "the graph beneath /darm" is a link somebody can send and the
  // back button walks back through the subtrees they looked at — the same reason the admin
  // console keeps its selection in `?pfad=`. German in the address bar, `root` on the wire,
  // because that is what the API calls it.
  const root = url.searchParams.get('wurzel');
  const query = root ? `?root=${encodeURIComponent(root)}` : '';

  try {
    const { status, data } = await apiGet<Graph>(fetch, `/api/links/graph${query}`, cookie);
    if (!data) return { graph: EMPTY, root, error: describe(status) };
    return { graph: data, root, error: null };
  } catch {
    // `apiGet` throws when the request never got an answer at all.
    return { graph: EMPTY, root, error: describe(0) };
  }
};
