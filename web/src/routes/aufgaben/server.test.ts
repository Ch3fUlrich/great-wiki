import { afterEach, describe, expect, it, vi } from 'vitest';
import { isRedirect } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { actions, load } from './+page.server';
import { BOARD_NOTICE_ID, type BoardNotice, type BoardResponse, type BoardTask } from '$lib/board';
import type { Project } from '$lib/projects';

/**
 * The loader and the one form action behind `/aufgaben`.
 *
 * **The property this page could quietly lose is D-12's.** The board exists in two places —
 * here, and embedded in a project's home page — and the decision that permitted that said
 * plainly what it costs: two places that must agree. They agree by being one query with a
 * filter and one component, so what is pinned here is exactly that: one request to
 * `/api/board`, the filter bound into it, and the answer rendered as it arrived. A second
 * source of cards would be a second answer to "which tasks exist", and because every card
 * is a disclosure surface, a second answer is also a second chance to leak one. Whether the
 * endpoint's own answer is right belongs to `gw-store`, where it is mutation-tested.
 *
 * The other half is the move. It is a real form action, so it works with JavaScript switched
 * off — and these tests call it the way the browser would, with a `FormData` body and nothing
 * else, which is why a click handler could never pass them.
 */
const projects: Project[] = [
  {
    id: 'p1',
    home_path: '/rundgang/tabellen',
    home_title: 'Tabellen',
    tag_id: null,
    created_at: '2026-08-20 09:00:00'
  },
  {
    id: 'p2',
    home_path: '/verweisbeispiel',
    home_title: 'Verweisbeispiel',
    tag_id: null,
    created_at: '2026-08-21 11:30:00'
  }
];

function task(over: Partial<BoardTask> = {}): BoardTask {
  return {
    id: 't1',
    title: 'Kabel bestellen',
    status: 'Offen',
    assignee: null,
    assignee_name: null,
    due_at: null,
    position: 0,
    anchored: true,
    page: { path: '/rundgang/tabellen', title: 'Tabellen' },
    detached: false,
    created_at: '2026-08-20 09:00:00',
    updated_at: '2026-08-20 09:00:00',
    ...over
  };
}

const board: BoardResponse = {
  project: null,
  columns: [
    { status: 'Offen', tasks: [task()] },
    { status: 'Läuft', tasks: [task({ id: 't2', title: 'Regal aufbauen', status: 'Läuft' })] },
    { status: 'Fertig', tasks: [] }
  ]
};

interface Call {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: string | undefined;
}

/** A `fetch` that records every call and answers whatever the test told it to. */
function spyFetch(answers: { status: number; body?: unknown }[]) {
  const calls: Call[] = [];
  let next = 0;
  const fetchFn = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    const answer = answers[Math.min(next++, answers.length - 1)];
    calls.push({
      url: String(url),
      method: init?.method ?? 'GET',
      headers: (init?.headers ?? {}) as Record<string, string>,
      body: init?.body as string | undefined
    });
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

/** The projects answer, then the board answer — the order the loader asks in. */
function answers(boardStatus = 200, boardBody: unknown = board) {
  return [
    { status: 200, body: { projects } },
    { status: boardStatus, body: boardBody }
  ];
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function loadEvent(fetchFn: typeof fetch, query = ''): any {
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/aufgaben', { headers: { cookie: 'gw_session=abc' } }),
    url: new URL(`http://wiki.test/aufgaben${query}`)
  };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function actionEvent(fetchFn: typeof fetch, fields: Record<string, string>): any {
  const form = new FormData();
  for (const [key, value] of Object.entries(fields)) form.append(key, value);
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/aufgaben', {
      method: 'POST',
      headers: { cookie: 'gw_session=abc' },
      body: form
    })
  };
}

interface Loaded {
  board: BoardResponse;
  projects: Project[];
  projekt: Project | null;
  filterUnbekannt: boolean;
  fehler: string | null;
  hinweis: BoardNotice | null;
  zurueck: string;
  now: number;
}

async function runLoad(fetchFn: typeof fetch, query = ''): Promise<Loaded> {
  return (await load(loadEvent(fetchFn, query))) as unknown as Loaded;
}

/** Every action here answers with a redirect, refusals included — see `+page.server.ts`. */
async function runAction(
  fetchFn: typeof fetch,
  fields: Record<string, string>
): Promise<{ returned: unknown; thrown: unknown }> {
  try {
    return { returned: await actions.verschieben(actionEvent(fetchFn, fields)), thrown: null };
  } catch (thrown) {
    return { returned: null, thrown };
  }
}

function location(thrown: unknown): string {
  expect(isRedirect(thrown)).toBe(true);
  return (thrown as { location: string }).location;
}

const ORIGINAL = { ...env };

afterEach(() => {
  for (const key of Object.keys(env)) delete env[key];
  Object.assign(env, ORIGINAL);
});

describe('the loader', () => {
  it('asks the one board endpoint, once, and renders exactly what it answered', async () => {
    const { calls, fetchFn } = spyFetch(answers());

    const data = await runLoad(fetchFn);

    const boardCalls = calls.filter((call) => call.url.includes('/api/board'));
    expect(boardCalls).toHaveLength(1);
    expect(boardCalls[0].url).toMatch(/\/api\/board$/);
    expect(data.board.columns).toEqual(board.columns);
    expect(data.fehler).toBeNull();
  });

  it('forwards the caller cookie, so the API sees the session the browser has', async () => {
    const { calls, fetchFn } = spyFetch(answers());
    await runLoad(fetchFn);
    for (const call of calls) expect(call.headers.cookie).toBe('gw_session=abc');
  });

  it('binds the filter to a project taken from the list, not from the query', async () => {
    // The list is already filtered per document, so matching against it is what stops a
    // hand-typed id turning the address bar into a second way to ask "is there a project
    // here" — exactly the guard `/projekte` puts on its own `?loeschen=`.
    const { calls, fetchFn } = spyFetch(answers());

    const data = await runLoad(fetchFn, '?projekt=p2');

    const boardCall = calls.find((call) => call.url.includes('/api/board'));
    expect(boardCall?.url).toContain('projekt=p2');
    expect(data.projekt?.id).toBe('p2');
    // And the way back keeps the filter, so moving a card does not silently widen the board.
    expect(data.zurueck).toBe('/aufgaben?projekt=p2');
  });

  it('ignores a project id that is not in the list, and says the filter did not apply', async () => {
    const { calls, fetchFn } = spyFetch(answers());

    const data = await runLoad(fetchFn, '?projekt=p-fremd');

    const boardCall = calls.find((call) => call.url.includes('/api/board'));
    expect(boardCall?.url).toMatch(/\/api\/board$/);
    expect(data.projekt).toBeNull();
    expect(data.filterUnbekannt).toBe(true);
  });

  it('reports a failed board instead of an empty one', async () => {
    const { fetchFn } = spyFetch(answers(500, { error: 'internal error' }));

    const data = await runLoad(fetchFn);

    expect(data.fehler).toContain('500');
    expect(data.board.columns).toEqual([]);
  });

  it('reads a board whose answer names no project, which is the global case', async () => {
    // `GET /api/board` with no filter spans every project and the cards that belong to
    // none, so there is no single project for it to name. An absent key is `null` here.
    const { fetchFn } = spyFetch(answers(200, { columns: board.columns }));

    const data = await runLoad(fetchFn);

    expect(data.board.project).toBeNull();
    expect(data.board.columns).toEqual(board.columns);
  });

  it('confirms a move against the board it just read, not against the address bar', async () => {
    const { fetchFn } = spyFetch(answers());

    const shown = await runLoad(fetchFn, '?verschoben=t2');
    expect(shown.hinweis?.art).toBe('ok');
    expect(shown.hinweis?.text).toContain('Regal aufbauen');
    expect(shown.hinweis?.text).toContain('Läuft');

    const { fetchFn: second } = spyFetch(answers());
    const invented = await runLoad(second, '?verschoben=t-fremd');
    expect(invented.hinweis).toBeNull();
  });

  it('turns a refused move into the sentence that promises nothing moved', async () => {
    const { fetchFn } = spyFetch(answers());

    const data = await runLoad(fetchFn, '?fehler=403');

    expect(data.hinweis?.art).toBe('fehler');
    expect(data.hinweis?.text).toContain('Schreibrecht');
    expect(data.hinweis?.text).toContain('nicht verschoben');
  });

  it('says nothing at all about a status code somebody invented', async () => {
    const { fetchFn } = spyFetch(answers());
    expect((await runLoad(fetchFn, '?fehler=keine-zahl')).hinweis).toBeNull();
  });
});

describe('moving a card', () => {
  it('patches the one task endpoint and comes back to the board', async () => {
    env.GW_PROXY_SECRET = 'geheim';
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { id: 't1' } }]);

    const { thrown } = await runAction(fetchFn, {
      karte: 't1',
      status: 'Läuft',
      zurueck: '/aufgaben?projekt=p1'
    });

    expect(calls[0].method).toBe('PATCH');
    expect(calls[0].url).toMatch(/\/api\/tasks\/t1$/);
    expect(JSON.parse(calls[0].body ?? '{}')).toEqual({ status: 'Läuft' });
    expect(calls[0].headers.cookie).toBe('gw_session=abc');
    // The same attestation `apiGet` sends. A write refused by `proxy_guard` would look
    // exactly like a permission refusal from here, which is the worst kind of confusion.
    expect(calls[0].headers['X-GW-Proxy']).toBe('geheim');

    // Post, redirect, get — and back to the board the card was moved on, filter and all,
    // with the fragment that puts the reader inside the announcement.
    const to = location(thrown);
    expect(to).toContain('/aufgaben?projekt=p1');
    expect(to).toContain('verschoben=t1');
    expect(to).toContain(`#${BOARD_NOTICE_ID}`);
  });

  it('comes back to the PAGE when the move was made on an embedded board', async () => {
    // D-12's second placement posts to this very action, which is what keeps a move one
    // implementation rather than one per page. The only thing that differs is where it
    // returns to, and the form carries that.
    const { fetchFn } = spyFetch([{ status: 200, body: {} }]);

    const { thrown } = await runAction(fetchFn, {
      karte: 't1',
      status: 'Fertig',
      zurueck: '/rundgang/tabellen'
    });

    expect(location(thrown)).toBe(
      `/rundgang/tabellen?verschoben=t1#${BOARD_NOTICE_ID}`
    );
  });

  it('refuses to send anybody off this site, however the field was filled in', async () => {
    const { fetchFn } = spyFetch([{ status: 200, body: {} }]);

    const { thrown } = await runAction(fetchFn, {
      karte: 't1',
      status: 'Fertig',
      zurueck: 'https://example.invalid/'
    });

    expect(location(thrown)).toBe(`/aufgaben?verschoben=t1#${BOARD_NOTICE_ID}`);
  });

  it('never asks the API for a status that is not one of the three', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: {} }]);

    const { thrown } = await runAction(fetchFn, {
      karte: 't1',
      status: 'erledigt',
      zurueck: '/aufgaben'
    });

    expect(calls).toHaveLength(0);
    expect(location(thrown)).toContain('fehler=400');
  });

  it('never asks the API without a card', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: {} }]);

    const { thrown } = await runAction(fetchFn, { karte: '  ', status: 'Fertig' });

    expect(calls).toHaveLength(0);
    expect(location(thrown)).toContain('fehler=400');
  });

  it('carries a refusal back to the board it came from, not to a page nobody asked for', async () => {
    const { fetchFn } = spyFetch([{ status: 403, body: { error: 'forbidden' } }]);

    const { thrown } = await runAction(fetchFn, {
      karte: 't1',
      status: 'Fertig',
      zurueck: '/rundgang/tabellen'
    });

    const to = location(thrown);
    expect(to).toContain('/rundgang/tabellen');
    expect(to).toContain('fehler=403');
    expect(to).not.toContain('verschoben=');
  });

  it('tells "no answer at all" apart from "answered with an error"', async () => {
    const fetchFn = vi.fn(async () => {
      throw new TypeError('fetch failed');
    }) as unknown as typeof fetch;

    const { thrown } = await runAction(fetchFn, {
      karte: 't1',
      status: 'Fertig',
      zurueck: '/aufgaben'
    });

    expect(location(thrown)).toContain('fehler=0');
  });
});
