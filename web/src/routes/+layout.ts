import { currentHref, parseTabHrefs } from '$lib/tabs';
import type { LayoutLoad } from './$types';

/**
 * Which tabs are open, and which one the address is on.
 *
 * **A universal load rather than a server one, and the difference is the whole reason
 * this file exists.** The tab set is a query parameter, so it changes on every navigation
 * within the workspace — and a *server* load that read `url` would be re-run on each of
 * those, which means another HTTP round trip to fetch `/api/me` and `/api/tree` again for
 * an answer that has not changed. This runs on the server for the first response (so the
 * strip is in the HTML, before any script) and in the browser for every navigation after
 * it, where it is two function calls and no network at all.
 *
 * Nothing is decided here. `$lib/tabs` says what may become a tab and what it is called;
 * this only hands the address to it.
 */
export const load: LayoutLoad = async ({ url, data }) => ({
  ...data,
  tabHrefs: parseTabHrefs(url),
  /** The tab the address itself is — the one whose content the routed view is rendering. */
  hier: currentHref(url)
});
