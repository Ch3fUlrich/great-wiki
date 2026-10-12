import { describe, expect, it, vi } from 'vitest';
import { embedApiPath, embeddable, parseEmbedAnswer, resolveEmbedOnServer } from './embed';

const VIEW = { id: 'v1', name: 'Alle', kind: 'table', config: { fields: ['name'] }, position: 0 };

describe('parseEmbedAnswer', () => {
  it('keeps a well-formed ok answer, view and all', () => {
    const out = parseEmbedAnswer({ state: 'ok', path: '/raum/tabelle', title: 'Tabelle', view: VIEW });
    expect(out).toEqual({
      state: 'ok',
      path: '/raum/tabelle',
      title: 'Tabelle',
      view: VIEW
    });
  });

  it('passes the view through so its id and kind survive to the loader', () => {
    const board = { state: 'ok', path: '/p', title: 'T', view: { ...VIEW, kind: 'board' } };
    const out = parseEmbedAnswer(board);
    expect(out.state === 'ok' && out.view.id).toBe('v1');
    expect(out.state === 'ok' && out.view.kind).toBe('board');
  });

  it('reads the view_missing answer as itself', () => {
    expect(parseEmbedAnswer({ state: 'view_missing' })).toEqual({ state: 'view_missing' });
  });

  it('reads the uniform refusal as itself', () => {
    expect(parseEmbedAnswer({ state: 'unavailable' })).toEqual({ state: 'unavailable' });
  });

  // Every malformed shape is the SAME answer, so a broken response cannot say anything about
  // what the wiki holds — ADR 0022's rule, applied by the reader.
  const malformed: [string, unknown][] = [
    ['null', null],
    ['undefined', undefined],
    ['a number', 7],
    ['a string', 'ok'],
    ['an array', [{ state: 'ok' }]],
    ['no state', { path: '/p', title: 'T' }],
    ['an unknown state', { state: 'perhaps' }],
    ['ok with no path', { state: 'ok', title: 'T', view: VIEW }],
    ['ok with an empty path', { state: 'ok', path: '', title: 'T', view: VIEW }],
    ['ok with a non-string path', { state: 'ok', path: 5, title: 'T', view: VIEW }],
    ['ok with no title', { state: 'ok', path: '/p', view: VIEW }],
    ['ok with a non-string title', { state: 'ok', path: '/p', title: 9, view: VIEW }],
    ['ok with no view', { state: 'ok', path: '/p', title: 'T' }],
    ['ok with a view that is not an object', { state: 'ok', path: '/p', title: 'T', view: 'v1' }],
    ['ok with a null view', { state: 'ok', path: '/p', title: 'T', view: null }],
    ['ok with a view missing its id', { state: 'ok', path: '/p', title: 'T', view: { kind: 'table' } }],
    ['ok with a view missing its kind', { state: 'ok', path: '/p', title: 'T', view: { id: 'v1' } }],
    ['ok with an empty view id', { state: 'ok', path: '/p', title: 'T', view: { id: '', kind: 'table' } }],
    [
      'ok with a non-string view kind',
      { state: 'ok', path: '/p', title: 'T', view: { id: 'v1', kind: 3 } }
    ]
  ];
  for (const [what, json] of malformed) {
    it(`collapses ${what} to unavailable`, () => {
      expect(parseEmbedAnswer(json)).toEqual({ state: 'unavailable' });
    });
  }

  it('never throws, whatever is handed it', () => {
    for (const json of [Symbol('x'), () => {}, new Map(), 0, '', [], {}]) {
      expect(() => parseEmbedAnswer(json)).not.toThrow();
    }
  });
});

describe('embedApiPath', () => {
  it('builds the embed address under the API', () => {
    expect(embedApiPath('doc-1', 'view-1')).toBe('/api/datasets/embed/doc-1/view-1');
  });

  it('encodes a hostile id so it cannot escape its one segment', () => {
    const path = embedApiPath('../../etc/passwd?a=1#frag', 'v/..?x#y');
    expect(path).toBe(
      `/api/datasets/embed/${encodeURIComponent('../../etc/passwd?a=1#frag')}/${encodeURIComponent('v/..?x#y')}`
    );
    // No raw slash beyond the fixed prefix, no query or fragment introduced by the id.
    expect(path.slice('/api/datasets/embed'.length).split('/')[1]).not.toContain('?');
    expect(path).not.toContain('#');
    expect(path).not.toContain('?');
  });
});

describe('embeddable', () => {
  it('admits the four read-only kinds', () => {
    for (const kind of ['table', 'board', 'calendar', 'gallery']) {
      expect(embeddable(kind)).toBe(true);
    }
  });

  it('refuses a form and anything it does not know', () => {
    for (const kind of ['form', 'timeline', '', 'Table', 'boardish']) {
      expect(embeddable(kind)).toBe(false);
    }
  });
});

interface Answer {
  status: number;
  body?: unknown;
}

/**
 * A server-side `fetch` that answers by endpoint prefix and records the headers of every
 * call — because the two rules this module exists to keep are (1) nothing is asked until
 * the embed answer allows it, and (2) every question goes out under exactly the reader's
 * cookie.
 */
function serverFetch(table: Record<string, Answer>) {
  const calls: { url: string; headers: Record<string, string> }[] = [];
  const fetchFn = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    const asked = String(url);
    calls.push({ url: asked, headers: (init?.headers ?? {}) as Record<string, string> });
    const key = Object.keys(table).find((prefix) => asked.includes(prefix));
    const answer = key ? table[key] : { status: 404 };
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  }) as unknown as typeof fetch;
  return { calls, fetchFn };
}

const OK_BOARD = {
  state: 'ok',
  path: '/raum/tabelle',
  title: 'Tabelle',
  view: { id: 'v1', name: 'Alle', kind: 'board', config: { group_by: 'status' }, position: 0 }
};

/** The row-read endpoints, answered well enough for `loadDataset` to complete. */
const ROWS_READY: Record<string, Answer> = {
  '/api/datasets/schema': { status: 200, body: { fields: [] } },
  '/api/datasets/rows': { status: 200, body: { rows: [], total: 0 } },
  '/api/datasets/views': { status: 200, body: { views: [] } }
};

describe('resolveEmbedOnServer', () => {
  it('maps the uniform refusal to unavailable and asks for no rows', async () => {
    const { calls, fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: { state: 'unavailable' } },
      ...ROWS_READY
    });
    expect(await resolveEmbedOnServer(fetchFn, 'gw_session=abc', 'doc', 'view')).toEqual({
      status: 'unavailable'
    });
    expect(calls.filter((c) => c.url.includes('/api/datasets/schema'))).toHaveLength(0);
  });

  it('is byte-identical for an absent and an unreadable dataset', async () => {
    const { fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: { state: 'unavailable' } }
    });
    const a = await resolveEmbedOnServer(fetchFn, 'gw_session=abc', 'd-fehlt', 'v1');
    const b = await resolveEmbedOnServer(fetchFn, 'gw_session=abc', 'd-geheim', 'v2');
    expect(JSON.stringify(a)).toBe('{"status":"unavailable"}');
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });

  it('maps view_missing to its own state, with no rows asked', async () => {
    const { calls, fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: { state: 'view_missing' } },
      ...ROWS_READY
    });
    expect(await resolveEmbedOnServer(fetchFn, 'c', 'doc', 'view')).toEqual({
      status: 'view_missing'
    });
    expect(calls.filter((c) => c.url.includes('/api/datasets/rows'))).toHaveLength(0);
  });

  it('reports a non-OK embed answer as an error, not a refusal', async () => {
    const { fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 500, body: { error: 'boom' } }
    });
    expect(await resolveEmbedOnServer(fetchFn, 'c', 'doc', 'view')).toEqual({ status: 'error' });
  });

  it('reports a fetch that throws as an error', async () => {
    const fetchFn = vi.fn(async () => {
      throw new TypeError('fetch failed');
    }) as unknown as typeof fetch;
    expect(await resolveEmbedOnServer(fetchFn, 'c', 'doc', 'view')).toEqual({ status: 'error' });
  });

  it('collapses a malformed ok answer to unavailable', async () => {
    const { fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: { state: 'ok', path: 5 } }
    });
    expect(await resolveEmbedOnServer(fetchFn, 'c', 'doc', 'view')).toEqual({
      status: 'unavailable'
    });
  });

  it('refuses to load rows for a kind that may not be embedded', async () => {
    const { calls, fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: { ...OK_BOARD, view: { ...OK_BOARD.view, kind: 'form' } } },
      ...ROWS_READY
    });
    expect(await resolveEmbedOnServer(fetchFn, 'c', 'doc', 'view')).toEqual({
      status: 'not_embeddable'
    });
    expect(calls.filter((c) => c.url.includes('/api/datasets/schema'))).toHaveLength(0);
  });

  it('loads the rows of the resolved path for an embeddable ok answer', async () => {
    const { calls, fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: OK_BOARD },
      ...ROWS_READY
    });
    const out = await resolveEmbedOnServer(fetchFn, 'gw_session=abc', 'doc', 'view');
    expect(out.status).toBe('ready');
    if (out.status === 'ready') expect(out.path).toBe('/raum/tabelle');
    expect(calls.some((c) => c.url.includes('/api/datasets/schema/raum/tabelle'))).toBe(true);
  });

  it("carries exactly the reader's cookie on every call, and nothing else", async () => {
    const { calls, fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: OK_BOARD },
      ...ROWS_READY
    });
    await resolveEmbedOnServer(fetchFn, 'gw_session=abc', 'doc', 'view');
    expect(calls.length).toBeGreaterThan(1);
    for (const call of calls) {
      expect(call.headers).toEqual({ cookie: 'gw_session=abc' });
      expect(Object.keys(call.headers)).not.toContain('x-gw-proxy');
      expect(Object.keys(call.headers)).not.toContain('authorization');
    }
  });

  it('sends no cookie at all when the reader came without one', async () => {
    const { calls, fetchFn } = serverFetch({
      '/api/datasets/embed': { status: 200, body: OK_BOARD },
      ...ROWS_READY
    });
    await resolveEmbedOnServer(fetchFn, null, 'doc', 'view');
    expect(calls.length).toBeGreaterThan(1);
    for (const call of calls) {
      expect(Object.keys(call.headers)).not.toContain('cookie');
    }
  });
});
