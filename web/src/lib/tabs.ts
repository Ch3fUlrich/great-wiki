/**
 * The workspace's tab set — what is open, which one you are looking at, and every link
 * the strip is made of.
 *
 * **THE URL IS THE TRUTH.** A workspace is `?reiter=…&reiter=…` on whatever address the
 * active tab is showing, which buys four things at once and is why it was chosen over a
 * client-side store: a workspace is a link somebody can send, it survives a reload and a
 * bookmark, the back button walks through it, and — because it is a query parameter — the
 * server can render the strip in the first response, before any script has arrived. The
 * same reasoning `/graph` records for `?wurzel=` and `/aufgaben` for `?projekt=`, one
 * level up.
 *
 * **A TAB IS AN ADDRESS.** There is no tab type, no per-kind payload and no registry of
 * openable things: a document, the global board, a filtered board, `/projekte`, the graph
 * and a page's history are all just addresses this wiki already serves, so a tab is one
 * of those and the active tab's content is rendered by the route that owns it. Nothing
 * here duplicates a loader, and a view added later is openable in a tab the day it has a
 * URL. {@link labelFor} is the only place that knows the difference, and only so a tab can
 * be *named*.
 *
 * The corollary is that tabs are identified by address, so opening a page that is already
 * open switches to it rather than opening a second copy. Two tabs on one address would be
 * two names for one thing, and closing either would look like closing both.
 *
 * **NOTHING HERE TOUCHES THE DOM.** Every function is pure, which is what lets the whole
 * mechanism be tested in a project that has no DOM environment at all — and, more
 * importantly, is what makes "the tab set is readable on the server" true rather than
 * intended. `readStored`/`writeStored` take a `Storage` for the same reason.
 */

import type { TreeNode } from '$lib/api';
import { SIDEBAR_PARAM, TOPICS_PATH, withSidebar, type SidebarMode } from '$lib/topics';

/**
 * The query parameter that carries the set. German, like `?wurzel=`, `?projekt=` and
 * `?loeschen=` — this is a German interface and the address bar is part of it.
 *
 * Repeated (`?reiter=%2Fa&reiter=%2Fb`) rather than one comma-joined value: order is
 * preserved by `URLSearchParams.getAll`, each entry is encoded once instead of twice, and
 * a stray comma in an address cannot split a tab in half.
 */
export const TAB_PARAM = 'reiter';

/**
 * How many tabs one workspace may hold.
 *
 * A cap rather than none, because the set arrives from the address bar and from storage:
 * both are things a person can hand-edit and one is a thing another page could have
 * written. Twelve is past what fits on a strip anyway, so the limit is never reached by
 * ordinary use and is never the reason something is missing.
 */
export const MAX_TABS = 12;

/** Where the last set is remembered when the address carries none. */
export const STORAGE_KEY = 'gw-reiter';

/** What a tab is, for naming purposes only. See the note above about there being no types. */
export type TabKind =
  | 'start'
  | 'dokument'
  | 'aufgaben'
  | 'projekte'
  | 'themen'
  | 'thema'
  | 'graph'
  | 'verlauf'
  | 'verwaltung';

/** One tab, ready to render: what it is called and every link it needs. */
export interface Tab {
  /** The address this tab shows. Its identity. */
  href: string;
  kind: TabKind;
  label: string;
  /** Where "switch to this tab" goes — the same set, on this tab's address. */
  switchHref: string;
  /** Where "close this tab" goes, or `null` when this is the only tab open. */
  closeHref: string | null;
}

/** A set and which of it is in front. The shape stored, and the shape merged. */
export interface TabSet {
  hrefs: string[];
  active: number;
}

/**
 * Query parameters that are not part of which page a tab is showing.
 *
 * Most of them describe what just happened rather than where you are: a tab that remembered
 * one would re-announce a finished move, or re-open a deletion prompt, every single time it
 * was switched back to. `edit` is in the list for a near-identical reason: it asks for the
 * editor, which is an act, not a place — and an editor that reopened itself on every switch
 * is a way to file a revision nobody typed.
 *
 * `seitenleiste` is the one entry that is neither an act nor an announcement, and it is here
 * for the rule underneath both: **a tab is a page, and this parameter is about the shell
 * around it.** Kept, one page would become two tabs that render the same content, cannot be
 * told apart on the strip, and would each look like the other when one was closed. The
 * sidebar's choice survives a navigation the other way instead — every link the shell renders
 * carries it (see `withSidebar`) — which is where it belongs.
 */
const TRANSIENT = new Set([
  TAB_PARAM,
  'verschoben',
  'fehler',
  'angelegt',
  'loeschen',
  'edit',
  SIDEBAR_PARAM
]);

/** Longer than any address this wiki can serve. Past it, the value is not an address. */
const MAX_HREF = 512;

/**
 * Turn something that claims to be an address into one, or refuse it.
 *
 * **This is the security boundary of the whole feature**, and it is here rather than at
 * the point of rendering because there is exactly one way into the set and several ways
 * out of it. Every tab becomes a link wearing this application's chrome; a set arrives
 * from the address bar, where anybody can put anything, and from `localStorage`, which
 * any script on this origin could have written. So an entry is admitted only if it is a
 * path inside this wiki — never a scheme, never `//host`, never `/\host` (which several
 * browsers still normalise to a protocol-relative URL), never a control character.
 */
export function normaliseHref(raw: unknown): string | null {
  if (typeof raw !== 'string') return null;
  const trimmed = raw.trim();
  if (trimmed.length === 0 || trimmed.length > MAX_HREF) return null;
  if (!trimmed.startsWith('/')) return null;
  if (trimmed.startsWith('//') || trimmed.startsWith('/\\')) return null;
  // No whitespace and no C0/C1 controls. Nothing this wiki serves contains either, and a
  // newline in an address is only ever an attempt at something.
  if (/[\s\u0000-\u001f\u007f-\u009f]/.test(trimmed)) return null;

  const withoutHash = trimmed.split('#')[0];
  const cut = withoutHash.indexOf('?');
  const path = cut === -1 ? withoutHash : withoutHash.slice(0, cut);
  if (path.length === 0) return null;

  const params = new URLSearchParams(cut === -1 ? '' : withoutHash.slice(cut + 1));
  for (const name of [...params.keys()]) {
    if (TRANSIENT.has(name)) params.delete(name);
  }
  const query = params.toString();
  return query ? `${path}?${query}` : path;
}

/** The addresses named by the URL, filtered, de-duplicated and capped. */
export function parseTabHrefs(url: URL): string[] {
  return capped(dedupe(url.searchParams.getAll(TAB_PARAM)));
}

/**
 * Which tab the address itself is: this path, with the state this view carries.
 *
 * `?projekt=` and `?wurzel=` are kept because a filtered board and an open one are
 * genuinely different things to have open; `?verschoben=` and friends are dropped by
 * {@link normaliseHref} because they are not.
 */
export function currentHref(url: URL): string {
  return normaliseHref(`${url.pathname}${url.search}`) ?? '/';
}

/** Drop entries that are not addresses, and entries that are already in the list. */
function dedupe(raw: readonly unknown[]): string[] {
  const out: string[] = [];
  for (const candidate of raw) {
    const href = normaliseHref(candidate);
    if (href !== null && !out.includes(href)) out.push(href);
  }
  return out;
}

function capped(hrefs: string[]): string[] {
  return hrefs.length > MAX_TABS ? hrefs.slice(0, MAX_TABS) : hrefs;
}

/**
 * The set that is actually open, given what the address named and which page is being read.
 *
 * **The page in front of the reader is always a tab.** An address can name a set that has
 * forgotten to include itself — hand-typed, or outlived the page it named — and a strip
 * that does not contain the thing it is describing is worse than no strip. When the set is
 * already full, the last entry gives way rather than the page being read.
 */
export function resolveTabs(raw: readonly unknown[], hier: string): TabSet {
  const hrefs = capped(dedupe(raw));
  const found = hrefs.indexOf(hier);
  if (found !== -1) return { hrefs, active: found };
  if (hrefs.length >= MAX_TABS) hrefs[hrefs.length - 1] = hier;
  else hrefs.push(hier);
  return { hrefs, active: hrefs.indexOf(hier) };
}

/**
 * Path, query and fragment, split apart so a tab set can be appended to whatever is already
 * there — and so the fragment survives it.
 *
 * **The fragment is not decoration in this interface.** It is how a change announces itself
 * with no script at all: a redirect or a link that carries `#gw-…` moves focus to a region
 * with `tabindex="-1"`, and a region that has just received focus is read out, where a live
 * region already in the document announces nothing. So a chrome link that loses its fragment
 * loses the announcement, silently — the link still navigates, focus simply stays at the top
 * of the page, and only somebody listening would notice.
 *
 * Splitting on `?` alone read `#gw-loeschen` as part of the last parameter's value and
 * percent-escaped it into the query. `$lib/topics`'s `withSidebar` has always split the
 * fragment off; this is the other half of the same chrome link agreeing with it.
 */
function split(href: string): [string, URLSearchParams, string] {
  const [before, ...rest] = href.split('#');
  const fragment = rest.length > 0 ? `#${rest.join('#')}` : '';
  const cut = before.indexOf('?');
  if (cut === -1) return [before, new URLSearchParams(), fragment];
  return [before.slice(0, cut), new URLSearchParams(before.slice(cut + 1)), fragment];
}

/**
 * `target`, carrying `hrefs` as the workspace.
 *
 * **Nothing is written while a single tab is open**, and that is the rule that keeps this
 * feature out of the way of a wiki nobody has opened a second tab in: one tab and no tabs
 * are the same thing, so every link in the interface stays exactly the address it was.
 * The parameters appear the moment a workspace genuinely exists and disappear again when
 * it stops.
 */
export function withTabs(target: string, hrefs: readonly string[]): string {
  if (hrefs.length <= 1) return target;
  return withTabsAlways(target, hrefs);
}

/**
 * `target`, carrying `hrefs` — **even when that is a single tab**.
 *
 * Used by the close control and nothing else, for a bug that only a browser could find.
 * Closing down to one tab produced a bare address, on the rule above that one tab and no
 * tabs are the same thing. They are not, in one place: the shell's storage fallback reads
 * "this address names no workspace" as "restore the last one" — so closing the second tab
 * put the reader on an address with no set, which the fallback dutifully re-populated with
 * the set it had remembered, re-opening the tab that had just been closed.
 *
 * A close therefore has to STATE what is now open, so that the fallback learns it. The
 * single parameter costs one ugly address for exactly one navigation: every link rendered
 * on the page it lands on is bare again, because by then the set really is one tab.
 */
function withTabsAlways(target: string, hrefs: readonly string[]): string {
  const [path, params, fragment] = split(target);
  params.delete(TAB_PARAM);
  for (const href of hrefs) params.append(TAB_PARAM, href);
  return `${path}?${params.toString()}${fragment}`;
}

/** Where "switch to tab `index`" goes. */
export function switchHref(hrefs: readonly string[], index: number): string {
  return withTabs(hrefs[index] ?? '/', hrefs);
}

/**
 * Where "close tab `index`" goes, given which tab is active.
 *
 * Closing the tab you are on lands on its neighbour — the one that takes its place, or the
 * new last tab if it was the last. Closing any other tab leaves you where you were.
 * Closing the only tab there is leaves the start page, which lists every page in the wiki:
 * a workspace with nothing open is a blank screen, and that is not an answer.
 */
export function closeHref(hrefs: readonly string[], index: number, active: number): string {
  const rest = hrefs.filter((_, i) => i !== index);
  if (rest.length === 0) return withTabsAlways('/', ['/']);
  const landing =
    index === active ? Math.min(index, rest.length - 1) : active < index ? active : active - 1;
  // `withTabsAlways`, never `withTabs` — see the note on it. A close that did not say what
  // is left would be undone by the shell's own storage fallback.
  return withTabsAlways(rest[landing], rest);
}

/**
 * Where "move this tab one place" goes.
 *
 * The moved tab is the active one — the strip only offers this for the tab you are on —
 * so the destination is its own address with the reordered set. A move past either end is
 * a no-op rather than a wrap-around: a tab that jumped from one end of the strip to the
 * other because somebody pressed the control once too often is a control nobody trusts.
 */
export function moveHref(hrefs: readonly string[], from: number, to: number): string {
  if (from < 0 || from >= hrefs.length || to < 0 || to >= hrefs.length) {
    return withTabs(hrefs[Math.min(Math.max(from, 0), hrefs.length - 1)] ?? '/', hrefs);
  }
  const next = [...hrefs];
  const [moved] = next.splice(from, 1);
  next.splice(to, 0, moved);
  return withTabs(moved, next);
}

/**
 * Where "open a new tab" goes: the start page, which is the one page that lists every
 * other one — the same thing a browser's own new tab does, and the reason no picker is
 * needed. You navigate from there and the tab keeps its place in the strip.
 */
export function newTabHref(hrefs: readonly string[]): string {
  if (hrefs.includes('/')) return withTabs('/', hrefs);
  const next = capped([...hrefs, '/']);
  return withTabs('/', next);
}

/**
 * Where a link followed *inside* the active tab goes: the same set, with the active entry
 * replaced. Following a link is navigation, not opening — the tab you are in shows
 * something else, and the workspace is unchanged around it.
 */
export function navigateHref(target: string, hrefs: readonly string[], active: number): string {
  const merged = mergeStored({ hrefs: [...hrefs], active }, target);
  return withTabs(target, merged.hrefs);
}

/**
 * Where a link in a VIEW'S OWN CHROME goes: the same workspace, and the same sidebar.
 *
 * Chrome is the breadcrumb, a subpage list, a topic's trail — every link a route puts around
 * its content, as against the links inside a document, which are addresses somebody wrote and
 * are deliberately left exactly as written (see the effect in `+layout.svelte`).
 *
 * It is one function rather than the same three lines in each of the routes that need it,
 * because the three lines are not obvious: a chrome link has to preserve BOTH the tab set and
 * the sidebar's choice, and a route that remembered only the first would quietly snap the
 * sidebar back to the page tree every time somebody followed a topic. The shell keeps its own
 * spelling of this, one function up, because it also has to serve the switcher — which needs
 * to name a mode other than the current one.
 */
export function chromeHref(
  target: string,
  raw: readonly unknown[],
  hier: string,
  mode: SidebarMode = 'seiten'
): string {
  const { hrefs, active } = resolveTabs(raw, hier);
  return navigateHref(withSidebar(target, mode), hrefs, active);
}

/** The tree entry for a path, or `null`. Already filtered to what this reader may see. */
function titleIn(nodes: readonly TreeNode[], path: string): string | null {
  for (const node of nodes) {
    if (node.path === path) return node.title;
    const deeper = titleIn(node.children, path);
    if (deeper !== null) return deeper;
  }
  return null;
}

/** `heikler-text` → `Heikler text`. Only ever reached for a page the tree does not name. */
function fromSlug(path: string): string {
  const slug = path.split('/').filter(Boolean).pop() ?? '';
  const words = decodeURIComponent(slug).replace(/[-_]+/g, ' ').trim();
  if (words.length === 0) return 'Seite';
  return words[0].toUpperCase() + words.slice(1);
}

/**
 * What a tab is called, and what kind of thing it is.
 *
 * **A page's title comes from the tree and from nowhere else.** `/api/tree` is filtered in
 * the retriever to what this caller may read (AGENTS.md rule 2), so a page the reader may
 * not open simply is not in it — and the fallback is the address they typed themselves,
 * never a title fetched from somewhere less careful. A title is a disclosure on its own;
 * this is the one place in the strip where one could have leaked.
 */
export function labelFor(
  href: string,
  tree: readonly TreeNode[]
): { kind: TabKind; label: string } {
  const [path, params] = split(href);

  if (path === '/') return { kind: 'start', label: 'Start' };
  if (path === '/aufgaben') {
    return {
      kind: 'aufgaben',
      // Which project is not named: the id in the address is not a title, and the title is
      // on a page this strip has no business asking about.
      label: params.get('projekt') ? 'Aufgaben (gefiltert)' : 'Aufgaben'
    };
  }
  if (path === '/projekte') return { kind: 'projekte', label: 'Projekte' };
  if (path === TOPICS_PATH) return { kind: 'themen', label: 'Themen' };
  if (path.startsWith(`${TOPICS_PATH}/`)) {
    // Named from the ADDRESS, never from a lookup — the same rule this function already
    // follows for a page the tree does not name, and it is sharper here: ADR 0011 makes a
    // topic's own name the disclosure, so a strip that fetched a prettier spelling would be
    // a second, unfiltered answer to "which topics exist". The slug in the address is one
    // the reader is already looking at.
    return { kind: 'thema', label: `Thema: ${fromSlug(path)}` };
  }
  if (path === '/graph') return { kind: 'graph', label: 'Graph' };
  if (path === '/admin') return { kind: 'verwaltung', label: 'Verwaltung' };

  if (path.endsWith('/history')) {
    const of = path.slice(0, -'/history'.length) || '/';
    return { kind: 'verlauf', label: `Verlauf: ${titleIn(tree, of) ?? fromSlug(of)}` };
  }

  return { kind: 'dokument', label: titleIn(tree, path) ?? fromSlug(path) };
}

/**
 * The DOM id of tab `index`.
 *
 * Exported rather than spelled in the two components that need it, because they need it
 * for OPPOSITE ends of one relationship — the strip puts the id on the tab, the shell
 * points the panel's `aria-labelledby` at it — and two spellings that drift apart produce
 * a panel with no accessible name and nothing that looks wrong on screen.
 */
export function tabDomId(index: number): string {
  return `gw-reiter-${index}`;
}

/** Everything the strip needs, from the set the URL named and the page being read. */
export function buildTabs(
  raw: readonly unknown[],
  hier: string,
  tree: readonly TreeNode[]
): { tabs: Tab[]; active: number } {
  const { hrefs, active } = resolveTabs(raw, hier);
  const tabs = hrefs.map((href, index) => ({
    href,
    ...labelFor(href, tree),
    switchHref: switchHref(hrefs, index),
    // No close control on the only tab there is. Closing it would land on the start page,
    // which is a navigation dressed up as a close — and an interface that offers to close
    // the last thing you have open invites exactly one confused click.
    closeHref: hrefs.length > 1 ? closeHref(hrefs, index, active) : null
  }));
  return { tabs, active };
}

/**
 * The set remembered for when the address carries none.
 *
 * Every read and every write is wrapped, and both answer "nothing" rather than throwing:
 * private browsing, a browser configured to block site data, and a full quota all raise
 * from `localStorage` itself, and a workspace is not worth a blank page. `null` for the
 * store is a real case too — this runs on the server, where there is none.
 */
export function readStored(store: Storage | null | undefined): TabSet | null {
  if (!store) return null;
  try {
    const raw = store.getItem(STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== 'object' || parsed === null) return null;
    const { hrefs, active } = parsed as { hrefs?: unknown; active?: unknown };
    if (!Array.isArray(hrefs)) return null;
    // Filtered exactly as a set from the address bar is: anything on this origin could
    // have written this value, so it is not trusted for being ours.
    const clean = capped(dedupe(hrefs));
    if (clean.length === 0) return null;
    const at = typeof active === 'number' && Number.isInteger(active) ? active : 0;
    return { hrefs: clean, active: Math.min(Math.max(at, 0), clean.length - 1) };
  } catch {
    return null;
  }
}

/** Remember a set. Silent on failure, for the reasons {@link readStored} gives. */
export function writeStored(store: Storage | null | undefined, value: TabSet): void {
  if (!store) return;
  try {
    store.setItem(STORAGE_KEY, JSON.stringify(value));
  } catch {
    // A workspace that could not be remembered is a workspace that has to be reopened.
    // Nothing else about this page depends on it.
  }
}

/**
 * Put the page being read into the tab it was reached from.
 *
 * The case this exists for: a workspace is open, the reader follows an ordinary link
 * inside the active tab, and the address that comes back carries no set at all — because
 * the links inside a document belong to the document, not to the shell. The set is
 * restored around the page rather than lost, and the followed page becomes what that tab
 * now shows rather than a new tab, which is what stops browsing from growing a strip.
 */
export function mergeStored(stored: TabSet, hier: string): TabSet {
  const hrefs = capped(dedupe(stored.hrefs));
  // Normalised, because the target is not always already one: a link may ask for a page in
  // a STATE — `?edit=1` is the one that exists today — and the tab is the page, not the
  // state. Without this, "edit this page" would replace the tab you are on with a second,
  // near-identical entry that reopens the editor every time it is switched back to.
  const ziel = normaliseHref(hier) ?? '/';
  if (hrefs.length === 0) return { hrefs: [ziel], active: 0 };

  const found = hrefs.indexOf(ziel);
  if (found !== -1) return { hrefs, active: found };

  const at = Math.min(Math.max(stored.active, 0), hrefs.length - 1);
  const next = [...hrefs];
  next[at] = ziel;
  // The replacement can produce the same tab twice — following a link back to a page that
  // is already open in another tab. Collapse it and land on the one that is left.
  const collapsed = dedupe(next);
  return { hrefs: collapsed, active: Math.max(collapsed.indexOf(ziel), 0) };
}
