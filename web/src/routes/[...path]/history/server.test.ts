import { describe, expect, it, vi } from 'vitest';
import { isRedirect } from '@sveltejs/kit';
import { load } from './+page.server';
import { GERMAN_REFUSALS } from '$lib/refusals';

/**
 * The history of an address a page has moved away from (ADR 0022).
 *
 * The forward is asked about the PAGE (`/alt`), not about `/alt/history`: the API holds
 * forwards for pages, and the sub-route is appended to wherever the page went. The 307 is
 * only issued when the API names a target, and the API names one only to a caller who may
 * read the page there — so a caller who may not gets the ordinary missing page.
 */
type Answer = { status: number; body?: unknown };

function run(table: Record<string, Answer>, query = '') {
  const urls: string[] = [];
  const fetchFn = vi.fn(async (url: string | URL | Request) => {
    const asked = String(url);
    urls.push(asked);
    const key = Object.keys(table).find((prefix) => asked.includes(prefix));
    const answer = key ? table[key] : { status: 404 };
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  const event = {
    params: { path: 'alt' },
    fetch: fetchFn,
    request: new Request('http://wiki.test/alt/history'),
    url: new URL(`http://wiki.test/alt/history${query}`)
  };
  return { urls, result: load(event as unknown as Parameters<typeof load>[0]) };
}

describe('the history of an address a page has moved away from', () => {
  it('sends the reader to the history at the new address, keeping the query', async () => {
    const { urls, result } = run(
      {
        '/api/documents': { status: 404 },
        '/api/forwards': { status: 200, body: { path: '/b/neu' } }
      },
      '?von=r1&bis=r2'
    );
    try {
      await result;
      expect.unreachable('the loader did not redirect');
    } catch (thrown) {
      expect(isRedirect(thrown)).toBe(true);
      expect(thrown).toMatchObject({ status: 307, location: '/b/neu/history?von=r1&bis=r2' });
    }
    expect(urls.some((u) => u.includes('/api/forwards/alt') && !u.includes('history'))).toBe(true);
  });

  it('is the ordinary missing page when nothing forwards, or the caller may not read the target', async () => {
    // The API answers 404 for both, byte for byte: that is the whole of the authorisation.
    const { result } = run({ '/api/documents': { status: 404 } });
    await expect(result).rejects.toMatchObject({
      status: 404,
      body: { message: GERMAN_REFUSALS.missing }
    });
  });
});
