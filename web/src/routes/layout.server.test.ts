import { describe, expect, it, vi } from 'vitest';
import { load } from './+layout.server';
import type { TopicSummary } from '$lib/topics';

/**
 * What the shell fetches, and — the point of this file — **how many times it fetches it.**
 *
 * Browsing by topic lives in two places at the owner's decision: the page at `/themen` and
 * the switcher in this shell's sidebar. The cost that decision names is two things that must
 * agree, and the answer is the one D-12 gave for the board: **one query, rendered twice.**
 * `GET /api/topics` is asked here, once per page render, and the index, the sidebar and the
 * suggestion list beside a page's chips are three renderings of that one array.
 *
 * That is not merely tidy. ADR 0011 makes a topic's NAME the disclosure, so every list of
 * topics is a permission-shaped answer; a second request for "just the topics, for the
 * autocomplete" is exactly the shape that gets written unfiltered, and the tests below are
 * what stop one appearing. If a future change needs topics somewhere new, it reads them from
 * here.
 */
interface Call {
  url: string;
}

function spyFetch(byPath: Record<string, { status: number; body?: unknown }>) {
  const calls: Call[] = [];
  const fetchFn = vi.fn(async (url: string | URL) => {
    const text = String(url);
    calls.push({ url: text });
    const key = Object.keys(byPath).find((path) => text.endsWith(path));
    const answer = key ? byPath[key] : { status: 404 };
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

const topics: TopicSummary[] = [
  { path: '/format', name: 'Format', display_path: 'Format', documents: 1 },
  { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang', documents: 3 }
];

const ok = {
  '/api/me': { status: 200, body: { authenticated: false } },
  '/api/tree': { status: 200, body: [] },
  '/api/topics': { status: 200, body: { topics } }
};

interface Loaded {
  me: unknown;
  tree: unknown[];
  themen: TopicSummary[];
  themenFehler: string | null;
}

/** `load`'s own return type is widened by SvelteKit's generics; this is what it answers. */
async function run(fetchFn: typeof fetch): Promise<Loaded> {
  return (await load(event(fetchFn))) as unknown as Loaded;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function event(fetchFn: typeof fetch): any {
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/rundgang', { headers: { cookie: 'gw_session=abc' } })
  };
}

describe('the one topic query', () => {
  it('asks /api/topics exactly once per render, and asks nothing else about topics', async () => {
    const { calls, fetchFn } = spyFetch(ok);
    await load(event(fetchFn));

    const asked = calls.filter((call) => call.url.includes('/api/topics'));
    expect(asked).toHaveLength(1);
    expect(asked[0].url).toMatch(/\/api\/topics$/);
  });

  it('hands on exactly what it was answered, adding nothing and dropping nothing', async () => {
    const { fetchFn } = spyFetch(ok);
    const data = await run(fetchFn);
    expect(data.themen).toEqual(topics);
    expect(data.themenFehler).toBeNull();
  });

  it('forwards the caller‘s cookie, so the answer is filtered for THEM', async () => {
    const { fetchFn } = spyFetch(ok);
    await load(event(fetchFn));
    const call = (fetchFn as unknown as ReturnType<typeof vi.fn>).mock.calls.find((args) =>
      String(args[0]).includes('/api/topics')
    );
    expect((call?.[1] as RequestInit).headers).toMatchObject({ cookie: 'gw_session=abc' });
  });
});

describe('when the topics cannot be fetched', () => {
  it('never takes the page down with them', async () => {
    const { fetchFn } = spyFetch({ ...ok, '/api/topics': { status: 500 } });
    const data = await run(fetchFn);
    // The tree and the identity are untouched: the sidebar's second half is an addition to
    // the shell, never a precondition for the page inside it.
    expect(data.tree).toEqual([]);
    expect(data.me).toBeTruthy();
  });

  it('says so, rather than rendering a wiki with no topics in it', async () => {
    const { fetchFn } = spyFetch({ ...ok, '/api/topics': { status: 500 } });
    const data = await run(fetchFn);
    expect(data.themen).toEqual([]);
    expect(data.themenFehler).toContain('500');
  });

  it('says so when the request got no answer at all', async () => {
    const fetchFn = vi.fn(async (url: string | URL) => {
      if (String(url).includes('/api/topics')) throw new TypeError('fetch failed');
      return new Response('null', { status: 200, headers: { 'content-type': 'application/json' } });
    }) as unknown as typeof fetch;
    const data = await run(fetchFn);
    expect(data.themenFehler).toContain('antwortet nicht');
  });
});
