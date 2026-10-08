import { BOARD_PATH, type TaskStatus } from '$lib/board';
import { topicHref } from '$lib/topics';

/**
 * Search, as the interface needs it: the shapes `GET /api/search` answers with, and the few
 * rules that turn them into something to render.
 *
 * **Nothing here decides who may see what, and nothing here counts.** The API answers only
 * the hits this caller may read (ADR 0024), and its response deliberately has no total: a
 * number about the hits left out is a number about pages the reader may not read. The only
 * count this module offers is the length of a list that was handed over.
 */

/** A run of snippet text. `hit` marks the words that matched; the text is never HTML. */
export interface Segment {
  text: string;
  hit: boolean;
}

/** Mirrors `gw_store::PageHit`. */
export interface PageHit {
  title: string;
  path: string;
  snippet: Segment[];
}

/** Mirrors `gw_store::TopicHit`. */
export interface TopicHit {
  name: string;
  display_path: string;
  path: string;
  documents: number;
}

/** Mirrors `gw_store::TaskHit`. */
export interface TaskHit {
  id: string;
  title: string;
  status: TaskStatus;
  page_path: string | null;
}

/** Mirrors `gw_store::SearchResults`. */
export interface SearchResults {
  pages: PageHit[];
  topics: TopicHit[];
  tasks: TaskHit[];
}

export const SEARCH_ENDPOINT = '/api/search';
export const SEARCH_PATH = '/suche';
export const SEARCH_PARAM = 'q';

/** The longest query forwarded. The API answers a longer one as it answers no match. */
export const MAX_QUERY_LENGTH = 200;

/**
 * A query as typed, made ready to send: trimmed, inner whitespace collapsed, and capped.
 * Blank in, empty out — and empty means "do not ask".
 */
export function normaliseQuery(raw: string | null | undefined): string {
  const collapsed = (raw ?? '').normalize('NFC').replace(/\s+/g, ' ').trim();
  return Array.from(collapsed).slice(0, MAX_QUERY_LENGTH).join('').trim();
}

/** `Gliederung` → `/api/search?q=Gliederung`. */
export function searchApiPath(query: string): string {
  return `${SEARCH_ENDPOINT}?${new URLSearchParams({ [SEARCH_PARAM]: query })}`;
}

/** Where a search is shown in this interface. */
export function searchHref(query: string): string {
  const q = normaliseQuery(query);
  return q ? `${SEARCH_PATH}?${new URLSearchParams({ [SEARCH_PARAM]: q })}` : SEARCH_PATH;
}

/** A page hit goes to the page, where the index says it is now. */
export function pageHitHref(hit: Pick<PageHit, 'path'>): string {
  return hit.path;
}

/** A topic hit goes to its topic route. */
export function topicHitHref(hit: Pick<TopicHit, 'path'>): string {
  return topicHref(hit);
}

/** A task goes to the page its card hangs off, or to the board if it hangs off none. */
export function taskHitHref(hit: Pick<TaskHit, 'page_path'>): string {
  return hit.page_path ? hit.page_path : BOARD_PATH;
}

/**
 * Drop empty runs and join neighbours that agree about `hit`, so the markup is one `<mark>`
 * per matched stretch rather than one per fragment the API happened to cut.
 */
export function mergeSegments(segments: readonly Segment[]): Segment[] {
  const out: Segment[] = [];
  for (const segment of segments) {
    if (segment.text === '') continue;
    const last = out[out.length - 1];
    if (last && last.hit === segment.hit) last.text += segment.text;
    else out.push({ text: segment.text, hit: segment.hit });
  }
  return out;
}

/** The snippet without its marking. */
export function segmentsText(segments: readonly Segment[]): string {
  return segments.map((segment) => segment.text).join('');
}

/** Nothing found in any group. */
export function isEmpty(results: SearchResults): boolean {
  return !results.pages.length && !results.topics.length && !results.tasks.length;
}

/** The answer for a query that is not asked. */
export const NO_RESULTS: SearchResults = { pages: [], topics: [], tasks: [] };

/** One sentence for a search that could not be answered. */
export function describeSearch(status: number): string {
  if (status === 0) return 'Die Suche ist gerade nicht erreichbar.';
  return `Die Suche konnte nicht ausgeführt werden (Status ${status}).`;
}

/** The text announced for a group's list: the length of what was handed over, nothing more. */
export function groupCountText(count: number): string {
  return count === 1 ? '1 Treffer' : `${count} Treffer`;
}

/** `/` focuses the search box, unless the key is going into something being typed in. */
export function isTypingTarget(target: unknown): boolean {
  const el = target as { tagName?: string; isContentEditable?: boolean } | null;
  if (!el || typeof el.tagName !== 'string') return false;
  const tag = el.tagName.toLowerCase();
  return tag === 'input' || tag === 'textarea' || tag === 'select' || el.isContentEditable === true;
}
