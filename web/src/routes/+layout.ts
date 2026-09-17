import { DIAGRAM_FRAME_PATH } from '$lib/blocks/diagram';
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
  seitenleiste: sidebarModeOf(url.searchParams.get(SIDEBAR_PARAM)),
  /**
   * Is this the diagram frame rather than a view of the wiki? (D-26)
   *
   * `+layout.svelte` renders nothing but the routed view when it is. The question is asked
   * HERE because this load already has the address — and only because of that: a root layout
   * cannot be opted out of in SvelteKit, so the frame's document would otherwise carry a
   * second copy of the whole workspace, hydrated, invisible, and writing the reader's tab
   * set to the `localStorage` it shares with them. Adding it to the SERVER load instead
   * would make that load read `url` and re-run on every navigation, which is exactly what
   * the paragraph above says it must never do.
   *
   * **Present only when true**, which is not a stylistic choice. Every `+page.svelte` in
   * this application infers its `data` type from this load, and a required boolean here
   * makes the `data` literal in eight page tests — none of them about diagrams, several of
   * them somebody else's — fail to type-check until each one is given a `rahmen: false` it
   * has no opinion about. An optional marker for one route costs `=== true` at the one place
   * that reads it.
   */
  ...(url.pathname === DIAGRAM_FRAME_PATH ? { rahmen: true as const } : {})
});
