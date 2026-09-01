import { currentHref, parseTabHrefs } from '$lib/tabs';
import { SIDEBAR_PARAM, sidebarModeOf } from '$lib/topics';
import type { LayoutLoad } from './$types';

/**
 * Which tabs are open, which one the address is on, and which half of the sidebar is showing.
 *
 * **A universal load rather than a server one, and the difference is the whole reason
 * this file exists.** The tab set is a query parameter, so it changes on every navigation
 * within the workspace — and a *server* load that read `url` would be re-run on each of
 * those, which means another HTTP round trip to fetch `/api/me` and `/api/tree` again for
 * an answer that has not changed. This runs on the server for the first response (so the
 * strip is in the HTML, before any script) and in the browser for every navigation after
 * it, where it is two function calls and no network at all.
 *
 * The sidebar's choice rides here for exactly the same reason and at exactly the same price:
 * it is a query parameter, so it is rendered into the first response and read back with no
 * network at all, and the server load above — which fetches the topics themselves — stays
 * free of `url` and therefore runs once per page rather than once per navigation.
 *
 * Nothing is decided here. `$lib/tabs` says what may become a tab and what it is called, and
 * `$lib/topics` says what the sidebar's parameter may mean; this only hands the address to
 * them.
 */
export const load: LayoutLoad = async ({ url, data }) => ({
  ...data,
  tabHrefs: parseTabHrefs(url),
  /** The tab the address itself is — the one whose content the routed view is rendering. */
  hier: currentHref(url),
  /** Page tree or topics. Anything unrecognised is the page tree — see `sidebarModeOf`. */
  seitenleiste: sidebarModeOf(url.searchParams.get(SIDEBAR_PARAM))
});
