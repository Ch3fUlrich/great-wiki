import { error } from '@sveltejs/kit';
import { apiGet } from '$lib/api';
import {
  NO_RESULTS,
  SEARCH_PARAM,
  describeSearch,
  normaliseQuery,
  searchApiPath,
  type SearchResults
} from '$lib/search';
import type { PageServerLoad } from './$types';

/**
 * Search results for `?q=`.
 *
 * **This loader filters nothing.** `GET /api/search` answers only what the caller may read,
 * and answers a query that matches only withheld pages exactly as one that matches nothing
 * (ADR 0024). The session cookie is forwarded so the API sees the caller's own principal;
 * everything about who may see what happens there. A blank query is not asked at all.
 */
export const load: PageServerLoad = async ({ url, fetch, request }) => {
  const query = normaliseQuery(url.searchParams.get(SEARCH_PARAM));
  if (!query) return { query, results: NO_RESULTS };

  const cookie = request.headers.get('cookie');
  let status: number;
  let data: SearchResults | null;
  try {
    ({ status, data } = await apiGet<SearchResults>(fetch, searchApiPath(query), cookie));
  } catch {
    error(503, describeSearch(0));
  }
  if (!data) error(status >= 400 && status <= 599 ? status : 502, describeSearch(status));

  return { query, results: data };
};
