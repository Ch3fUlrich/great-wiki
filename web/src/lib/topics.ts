/**
 * Topics, on the wire and in words.
 *
 * Pure on purpose, exactly like `$lib/board` and `$lib/projects`: this module is imported
 * from `+page.server.ts` files **and** from the components that render the three placements,
 * so it may not touch `$env/dynamic/private` — importing a server-only module from a
 * component poisons the client bundle. The calls themselves live in `$lib/api`.
 *
 * **Why so much of this is a function in here rather than markup in a component.** The owner
 * put topic browsing in two places — a page at `/themen`, and a switcher in the shell's
 * sidebar — and named the cost in the same breath, which is the cost D-12 named for the
 * board: two things that must agree. They agree the same way the board's two placements do.
 * **One query**: `GET /api/topics` is asked once, in `+layout.server.ts`, and the index, the
 * sidebar and the suggestion list beside every page's chips are three renderings of that one
 * answer — not three requests, and not three ideas of what a topic is. **One rendering**:
 * `$lib/components/TopicTree.svelte`. Everything that could still drift between them — how a
 * flat list becomes a hierarchy, what a topic's address is, how a count reads — is a
 * function here with a test beside it.
 *
 * **Nothing here decides who may see anything.** `GET /api/topics` answers only the topics
 * the caller may see at all, and `Store::topics_for` is where that is decided and
 * mutation-tested. ADR 0011 states the rule this interface must not undo: a topic exists,
 * for a given caller, exactly when they may read at least one document under it or under a
 * topic inside it — so a topic they can see nothing of is not listed, not counted, and **not
 * offered as a suggestion**. That last one is the one that gets forgotten, because it feels
 * like a UI convenience rather than a disclosure surface. It cannot be forgotten here: the
 * suggestion list is not a separate request that somebody has to remember to filter, it is
 * the very array the index renders.
 *
 * The one thing this file could add that the filtering cannot take back is a NUMBER about
 * what was left out. There is none: {@link countText} takes a single count — the length of
 * the list the reader is being handed — and there is no second parameter for a total to
 * arrive in later.
 */

// --- What comes off the wire ---------------------------------------------------------------

/**
 * One topic, by the two names it has. Mirrors `gw_api::routes::topics::TopicView`.
 *
 * It carries no id, exactly as a backlink and a graph node do not: `path` identifies a topic
 * just as uniquely, is what a URL has to spell, and an internal uuid on the wire is a table
 * the client would have to keep.
 */
export interface Topic {
  /** The canonical key: `/rundgang/tabellen`. What `/api/topics/tagged/…` takes. */
  path: string;
  /** The leaf as somebody typed it: `Tabellen`. */
  name: string;
  /** The whole ancestry as somebody typed it: `Rundgang/Tabellen`. What a file states. */
  display_path: string;
}

/**
 * A topic in a listing. Mirrors `gw_api::routes::topics::TopicSummaryView`, which flattens
 * the topic into the row rather than nesting it.
 */
export interface TopicSummary extends Topic {
  /**
   * **Documents the caller may read**, here and in every topic inside this one.
   *
   * The length of the list `/api/topics/tagged/{path}` would hand *this* caller, taken from
   * the same filtered set rather than counted beside it. It says nothing about what the
   * filter removed — see ADR 0011's Disclosure section, which is what licenses rendering it
   * at all.
   */
  documents: number;
}

/** `GET /api/topics`. Already filtered — see the module comment. */
export interface TopicsResponse {
  topics: TopicSummary[];
}

/** One page under a topic: somewhere to go, and something to call it. */
export interface TopicDocument {
  path: string;
  title: string;
}

/** `GET /api/topics/tagged/{path}`. Mirrors `gw_api::routes::topics::TopicPageResponse`. */
export interface TopicPageResponse {
  topic: Topic;
  /** Every page the caller may read under this topic **or any topic inside it**. */
  documents: TopicDocument[];
  /** The topics directly inside this one that the caller may see. */
  children: TopicSummary[];
}

/** `GET`/`PUT /api/topics/document/{path}`. */
export interface DocumentTopicsResponse {
  topics: Topic[];
}

// --- Where things are ------------------------------------------------------------------------

/** The one index endpoint. Named once so the three placements cannot drift onto two. */
export const TOPICS_ENDPOINT = '/api/topics';

/** Where the index lives in this interface, and what a topic's address is built on. */
export const TOPICS_PATH = '/themen';

/**
 * The separator, in the interface as well as in the key.
 *
 * `gw-store/src/topics.rs` leaves the rendering to us and notes that the design's prose
 * writes `›`. It is deliberately **not** used. The string beside a chip is the string a
 * person retypes into the field next to it and states in a file, and `Rundgang › Tabellen`
 * retyped would not be refused — it would slugify to the single topic `rundgang-tabellen`,
 * quietly filing the page under a topic nobody meant. A prettier glyph is not worth a
 * silently wrong write.
 */
export const TOPIC_SEPARATOR = '/';

/**
 * The element a page's topics live in — the anchor a change comes back to.
 *
 * A `role="status"` region that is already in the document when it loads announces nothing;
 * a live region announces what CHANGES. So adding or removing a topic redirects to this id,
 * the region carries `tabindex="-1"`, and the browser moves focus there: the reader lands on
 * the list that just changed and it is read out. That works with no JavaScript at all, which
 * is the requirement. The same mechanism, and the same reason, as `BOARD_NOTICE_ID`.
 */
export const TOPICS_REGION_ID = 'gw-themen';

/**
 * Percent-encode each segment of a topic path, and nothing else.
 *
 * `gw_core::slugify` emits ASCII alphanumerics and dashes, so this is a no-op on every topic
 * that exists today. It is here for the day it is not: an unencoded segment carrying a `/`
 * would be a *different* topic path, silently, in an address that looked right.
 */
function encodePath(path: string): string {
  return path
    .split(TOPIC_SEPARATOR)
    .map((segment) => encodeURIComponent(segment))
    .join(TOPIC_SEPARATOR);
}

/** `/rundgang/tabellen` → `/api/topics/tagged/rundgang/tabellen`. */
export function topicApiPath(path: string): string {
  return `/api/topics/tagged${encodePath(withLeadingSlash(path))}`;
}

/** `/rundgang/tabellen` → `/api/topics/document/rundgang/tabellen`. */
export function documentTopicsApiPath(documentPath: string): string {
  return `/api/topics/document${encodePath(withLeadingSlash(documentPath))}`;
}

/** Where a topic is browsed in this interface: `/themen/rundgang/tabellen`. */
export function topicHref(topic: Pick<Topic, 'path'>): string {
  return `${TOPICS_PATH}${encodePath(withLeadingSlash(topic.path))}`;
}

/** What `/themen/[...pfad]` captured, back as the canonical path the API takes. */
export function topicPathFromRoute(captured: string): string {
  return withLeadingSlash(captured);
}

/**
 * The topic an address is showing, or `null` when it is showing something else.
 *
 * The shell needs this to mark where you are in the sidebar's topic tree, and it reads it off
 * the address rather than being told: the sidebar is drawn for every view, and a view that
 * had to announce "by the way, I am a topic page" would be one more thing each route has to
 * remember. `/themen` itself is not a topic — it is the index — so it answers `null`.
 */
export function activeTopicPath(pathname: string): string | null {
  if (!pathname.startsWith(`${TOPICS_PATH}${TOPIC_SEPARATOR}`)) return null;
  const rest = pathname.slice(TOPICS_PATH.length);
  return rest === TOPIC_SEPARATOR ? null : decodeURI(rest);
}

function withLeadingSlash(path: string): string {
  return path.startsWith(TOPIC_SEPARATOR) ? path : `${TOPIC_SEPARATOR}${path}`;
}

// --- The hierarchy ---------------------------------------------------------------------------

/** One topic and the topics inside it. */
export interface TopicNode {
  topic: TopicSummary;
  children: TopicNode[];
}

/**
 * The flat index, as the tree it already describes.
 *
 * Nesting is the third of the decisions `gw-store/src/topics.rs` records, and an interface
 * that rendered `Rundgang` and `Rundgang/Tabellen` as two unrelated rows would have thrown
 * it away — the API answers a flat list precisely because the tree is derivable from the
 * paths, and this is the one place that derives it.
 *
 * **A topic whose parent is not in the list is kept, at the top.** It cannot happen through
 * the API — a topic is visible only when a document under it is, and that document is under
 * its ancestors too, so an ancestor is always at least as visible as its child. But the
 * failure mode of assuming it is a topic the reader may see and cannot reach, and a topic
 * page is the ONLY way topics are reachable (D-4). Surfacing it is the harmless mistake.
 *
 * The order the API answered in is kept at every level. `Store::topics_for` walks a BTreeMap
 * keyed by canonical path, so parents already precede their children and siblings are
 * already sorted; re-sorting here would be a second opinion about an order that has one.
 */
export function treeOf(topics: readonly TopicSummary[]): TopicNode[] {
  const nodes = new Map<string, TopicNode>();
  for (const topic of topics) nodes.set(topic.path, { topic, children: [] });

  const roots: TopicNode[] = [];
  for (const topic of topics) {
    const node = nodes.get(topic.path);
    if (!node) continue;
    const parent = parentPath(topic.path);
    const above = parent === null ? undefined : nodes.get(parent);
    if (above) above.children.push(node);
    else roots.push(node);
  }
  return roots;
}

/** `/rundgang/tabellen` → `/rundgang`; a top-level topic has no parent. */
function parentPath(path: string): string | null {
  const cut = path.lastIndexOf(TOPIC_SEPARATOR);
  return cut <= 0 ? null : path.slice(0, cut);
}

/** One step of a topic's ancestry: enough to link to it and to name it. */
export interface TopicStep {
  path: string;
  name: string;
}

/**
 * The topics above this one, outermost first — the trail on a topic's own page.
 *
 * Assembled from `path` and `display_path` in step, because those are the only two strings
 * that exist: the API answers one topic, not its ancestry, and a trail is what turns
 * `Medizin/Darm` from a label into a way back up to `Medizin`.
 *
 * **It gives up rather than guessing.** The store assembles `display_path` from the very
 * ancestry `path` walks, so the two always have the same number of segments; if they ever
 * did not, a trail built anyway would show a slug to somebody as a name they had typed.
 */
export function ancestryOf(topic: Topic): TopicStep[] {
  const keys = topic.path.split(TOPIC_SEPARATOR).slice(1);
  const names = topic.display_path.split(TOPIC_SEPARATOR);
  if (keys.length !== names.length) return [];

  const steps: TopicStep[] = [];
  let path = '';
  for (let i = 0; i < keys.length - 1; i += 1) {
    path += `${TOPIC_SEPARATOR}${keys[i]}`;
    steps.push({ path, name: names[i] });
  }
  return steps;
}

// --- Words ------------------------------------------------------------------------------------

/**
 * How many pages are under a topic, in words.
 *
 * **One parameter, and that is the point.** ADR 0011 licenses exactly one number here — the
 * length of the list this caller would be handed — and forbids any number about what the
 * filter removed. A signature that also took a total is where a "von 12" would one day be
 * written; there is nowhere for it to go.
 */
export function countText(documents: number): string {
  return documents === 1 ? '1 Seite' : `${documents} Seiten`;
}

/**
 * Why the topics are not there.
 *
 * Never conflated with "there are no topics". A list that failed to load and a wiki nobody
 * has filed anything in are different things, and "keine Themen" about a server that is down
 * is the lie every other view here refuses to tell.
 */
export function describeTopics(status: number): string {
  if (status === 0) return 'Die Themen konnten nicht geladen werden: Die Anwendung antwortet nicht.';
  return `Die Themen konnten nicht geladen werden (Fehler ${status}).`;
}

/**
 * Why one topic's page is not there.
 *
 * **404 says the same thing about a topic nobody ever typed and a topic you may see no page
 * of**, because the API deliberately answers the same status to both: a refusal that differed
 * from an absence would confirm the name, and a topic's name is exactly what ADR 0011 keeps
 * back. So this sentence must not hint at a permission either — it says the topic is not
 * here, which is true in both cases and complete in neither.
 */
export function describeTopic(status: number): string {
  if (status === 404) {
    return 'Dieses Thema gibt es hier nicht. Vielleicht ist es anders geschrieben — ' +
      'unter »Themen« steht, was es gibt.';
  }
  if (status === 0) return 'Das Thema konnte nicht geladen werden: Die Anwendung antwortet nicht.';
  return `Das Thema konnte nicht geladen werden (Fehler ${status}).`;
}

/**
 * Why a page's topics did not change — and, in every branch, that they did not.
 *
 * The promise is the point, exactly as it is for a refused move on a board: a change that
 * half happened is the thing somebody would go and check for. `set_document_topics` parses
 * the whole list before it writes anything, so a page with one unusable topic in it keeps the
 * topics it had — which makes this a promise the interface may actually make.
 *
 * `said` is what the API's own refusal named. A 400 from this endpoint names the string it
 * would not take and why, and dropping that turns a typo into "Fehler 400" — a refusal
 * nobody can act on.
 */
export function describeSetTopics(status: number, said: string | null): string {
  const unchanged = 'Die Themen dieser Seite wurden nicht geändert.';
  if (status === 0) return `Die Anwendung antwortet nicht. ${unchanged}`;
  if (status === 400) {
    return said ? `${said} ${unchanged}` : `Das ist kein Thema. ${unchanged}`;
  }
  if (status === 401) return `Nicht angemeldet — bitte erneut anmelden. ${unchanged}`;
  if (status === 403) return `Dafür fehlt das Schreibrecht auf dieser Seite. ${unchanged}`;
  if (status === 404) return `Diese Seite gibt es nicht (mehr). ${unchanged}`;
  return `Fehler ${status}. ${unchanged}`;
}

// --- Which half of the sidebar is showing -------------------------------------------------

/**
 * The parameter the sidebar's choice rides in. German, like `?wurzel=`, `?projekt=` and
 * `?reiter=` — this is a German interface and the address bar is part of it.
 *
 * In the URL rather than in a store, for the four things `$lib/tabs` lists about the tab set
 * and which apply unchanged here: the choice is server-rendered in the first response, it
 * survives a reload, the back button walks through it, and — because there is no DOM
 * environment in this project — both halves can be rendered and asserted by a test.
 */
export const SIDEBAR_PARAM = 'seitenleiste';

/** The two things the sidebar can be showing. There is no third. */
export type SidebarMode = 'seiten' | 'themen';

/**
 * Which half was asked for. **Anything unrecognised is the page tree**, not nothing: the
 * value arrives from the address bar, and a sidebar that rendered neither tree because
 * somebody typed `?seitenleiste=x` would be a blank column with no way back out of it.
 */
export function sidebarModeOf(value: string | null | undefined): SidebarMode {
  return value === 'themen' ? 'themen' : 'seiten';
}

/**
 * `target`, carrying the sidebar's choice — and dropping it again when it is the default.
 *
 * **Nothing is written for the page tree**, so a wiki where nobody has touched the switcher
 * keeps exactly the addresses it had. The parameter appears the moment the choice is a real
 * one and disappears again the moment it stops being.
 *
 * It is applied to every link the SHELL and the page's own chrome render, which is what makes
 * the choice survive a navigation. A link inside a document is deliberately untouched, for
 * the reason `+layout.svelte` gives about the tab set: those are addresses somebody wrote,
 * and rewriting them would be a lie about the text.
 */
export function withSidebar(target: string, mode: SidebarMode): string {
  const [before, ...rest] = target.split('#');
  const fragment = rest.length > 0 ? `#${rest.join('#')}` : '';
  const cut = before.indexOf('?');
  const path = cut === -1 ? before : before.slice(0, cut);
  const params = new URLSearchParams(cut === -1 ? '' : before.slice(cut + 1));

  params.delete(SIDEBAR_PARAM);
  if (mode === 'themen') params.set(SIDEBAR_PARAM, mode);

  const query = params.toString();
  return `${path}${query ? `?${query}` : ''}${fragment}`;
}
