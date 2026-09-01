import { describe, expect, it, vi } from 'vitest';
import { isHttpError } from '@sveltejs/kit';
import { load } from './+page.server';
import type { TopicPageResponse } from '$lib/topics';

/**
 * The loader behind one topic's page.
 *
 * **This is the endpoint that could leak a name**, so what is pinned here is narrow and
 * deliberate. ADR 0011: a topic exists, for a given caller, exactly when they may read at
 * least one document under it — and a topic they may see nothing of must answer **the same
 * way as a topic nobody ever typed**, because a refusal that differed from an absence would
 * confirm the name. `gw_api::routes::topics::topic_page` answers 404 to both; the one thing
 * this loader must not do is tell them apart, or wrap one of them in a sentence that hints.
 *
 * The listing itself is filtered in `Store::topic_for`, where it is mutation-tested. There is
 * exactly one request here for the reason `/projekte` gives about its own: a second source of
 * documents would be a second answer, and every row of this page says a page exists and what
 * it is called.
 */
const listing: TopicPageResponse = {
  topic: { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang' },
  documents: [
    { path: '/rundgang', title: 'Rundgang' },
    { path: '/rundgang/tabellen', title: 'Tabellen' }
  ],
  children: [
    {
      path: '/rundgang/tabellen',
      name: 'Tabellen',
      display_path: 'Rundgang/Tabellen',
      documents: 1
    }
  ]
};

interface Call {
  url: string;
  headers: Record<string, string>;
}

function spyFetch(answer: { status: number; body?: unknown } | 'wirft') {
  const calls: Call[] = [];
  const fetchFn = vi.fn(async (url: string | URL, init?: RequestInit) => {
    calls.push({ url: String(url), headers: (init?.headers ?? {}) as Record<string, string> });
    if (answer === 'wirft') throw new TypeError('fetch failed');
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function event(fetchFn: typeof fetch, pfad = 'rundgang'): any {
  return {
    params: { pfad },
    fetch: fetchFn,
    request: new Request(`http://wiki.test/themen/${pfad}`, {
      headers: { cookie: 'gw_session=abc' }
    })
  };
}

/** `load`'s own return type is widened by SvelteKit's generics; this is what it answers. */
async function run(fetchFn: typeof fetch, pfad = 'rundgang'): Promise<{ thema: TopicPageResponse }> {
  return (await load(event(fetchFn, pfad))) as unknown as { thema: TopicPageResponse };
}

/** What `load` threw, as a status and a message. */
async function refusal(fetchFn: typeof fetch, pfad = 'rundgang') {
  try {
    await load(event(fetchFn, pfad));
  } catch (thrown) {
    if (isHttpError(thrown)) return { status: thrown.status, message: thrown.body.message };
    throw thrown;
  }
  throw new Error('expected the loader to refuse');
}

describe('reading one topic', () => {
  it('asks the topic endpoint once, for the topic the address named', async () => {
    const { calls, fetchFn } = spyFetch({ status: 200, body: listing });
    await load(event(fetchFn, 'rundgang/tabellen'));
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toContain('/api/topics/tagged/rundgang/tabellen');
  });

  it('forwards the caller‘s cookie, so the listing is filtered for THEM', async () => {
    const { calls, fetchFn } = spyFetch({ status: 200, body: listing });
    await load(event(fetchFn));
    expect(calls[0].headers).toMatchObject({ cookie: 'gw_session=abc' });
  });

  it('hands on what it was answered, adding nothing and dropping nothing', async () => {
    const { fetchFn } = spyFetch({ status: 200, body: listing });
    const data = await run(fetchFn);
    expect(data.thema).toEqual(listing);
  });
});

describe('a topic that is not there for this reader', () => {
  it('is a 404 — the same answer as a topic nobody ever typed', async () => {
    const { status } = await refusal(spyFetch({ status: 404 }).fetchFn);
    expect(status).toBe(404);
  });

  it('says nothing that would tell the two apart', async () => {
    // The oracle this prevents: ask for `/kündigung-mietvertrag`, get "you may not see this"
    // rather than "no such topic", and the name is confirmed. The sentence must be true of
    // both cases and say nothing about permission.
    const { message } = await refusal(spyFetch({ status: 404 }).fetchFn);
    expect(message).not.toMatch(/dürfen|Recht|Berechtigung|gesperrt|verboten/i);
  });

  it('is refused before anything about it is rendered', async () => {
    // `error()` rather than a page that says "nothing here": a page rendered for a topic the
    // API refused would have had to be handed something to render.
    const { fetchFn } = spyFetch({ status: 404 });
    await expect(load(event(fetchFn))).rejects.toBeTruthy();
  });
});

describe('a request that failed for some other reason', () => {
  it('says so, rather than reporting the topic as missing', async () => {
    const { status, message } = await refusal(spyFetch({ status: 500 }).fetchFn);
    expect(status).toBe(500);
    expect(message).toContain('500');
  });

  it('tells »no answer at all« apart from »answered with an error«', async () => {
    const { status, message } = await refusal(spyFetch('wirft').fetchFn);
    expect(status).toBe(503);
    expect(message).toContain('antwortet nicht');
  });
});
