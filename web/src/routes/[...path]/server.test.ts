import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';
import type { StoredDocument } from '$lib/api';
import type { BoardNotice, BoardResponse, BoardTask } from '$lib/board';
import type { Project } from '$lib/projects';

/**
 * The reader page's loader, and specifically D-12's **second placement**: the board embedded
 * in a project's home page.
 *
 * The decision put a board here as well as at `/aufgaben` because a project's own page is
 * where you look when you are thinking about that project, and sending you elsewhere breaks
 * that — and it named the cost in the same breath: two places that must agree. They agree by
 * being one endpoint and one component, so what is pinned here is that this loader asks
 * `GET /api/board` with the filter bound to this page and does not assemble a board from
 * anywhere else. A second retrieval path would be a second answer, and every card on it is a
 * disclosure surface.
 *
 * The other half is the one thing this placement has to decide that `/aufgaben` does not:
 * **most pages are nobody's home.** The endpoint says which by naming a project or not, and
 * a page that is not one gets no board and no error either — furniture on every page in the
 * wiki is not a cost D-12 asked anybody to pay.
 */
const doc: StoredDocument = {
  id: 'd1',
  path: '/rundgang/tabellen',
  parent_path: '/rundgang',
  slug: 'tabellen',
  doc_type: 'page',
  title: 'Tabellen',
  language: 'de',
  visibility: 'restricted',
  body: JSON.stringify({ kind: 'doc', content: [] }),
  sort_key: 1
};

const projekt: Project = {
  id: 'p1',
  home_path: '/rundgang/tabellen',
  home_title: 'Tabellen',
  tag_id: null,
  created_at: '2026-08-20 09:00:00'
};

function task(over: Partial<BoardTask> = {}): BoardTask {
  return {
    id: 't1',
    title: 'Kabel bestellen',
    status: 'Offen',
    assignee: null,
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
  project: projekt,
  columns: [
    { status: 'Offen', tasks: [task()] },
    { status: 'Läuft', tasks: [] },
    { status: 'Fertig', tasks: [] }
  ]
};

interface Answer {
  status: number;
  body?: unknown;
}

/**
 * A `fetch` that answers by endpoint rather than by call order, and records every URL it was
 * asked for. Order is what the loader is free to change; which endpoints it asks is not.
 */
function spyFetch(overrides: Record<string, Answer> = {}) {
  const urls: string[] = [];
  const table: Record<string, Answer> = {
    '/api/documents': { status: 200, body: doc },
    '/api/tree': { status: 200, body: [] },
    '/api/links/backlinks': { status: 200, body: { backlinks: [] } },
    '/api/board': { status: 200, body: board },
    ...overrides
  };
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
  return { urls, fetchFn: fetchFn as unknown as typeof fetch };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function loadEvent(fetchFn: typeof fetch, query = ''): any {
  return {
    params: { path: 'rundgang/tabellen' },
    fetch: fetchFn,
    request: new Request('http://wiki.test/rundgang/tabellen', {
      headers: { cookie: 'gw_session=abc' }
    }),
    url: new URL(`http://wiki.test/rundgang/tabellen${query}`)
  };
}

interface Loaded {
  doc: StoredDocument;
  board: BoardResponse | null;
  boardFehler: string | null;
  hinweis: BoardNotice | null;
  zurueck: string;
  now: number;
}

async function runLoad(fetchFn: typeof fetch, query = ''): Promise<Loaded> {
  return (await load(loadEvent(fetchFn, query))) as unknown as Loaded;
}

describe('the board embedded in a page', () => {
  it('asks the one board endpoint, once, bound to this page', async () => {
    const { urls, fetchFn } = spyFetch();

    await runLoad(fetchFn);

    const boardCalls = urls.filter((url) => url.includes('/api/board'));
    expect(boardCalls).toHaveLength(1);
    expect(boardCalls[0]).toContain('seite=%2Frundgang%2Ftabellen');
  });

  it('renders the board it was answered with, and nothing it assembled itself', async () => {
    const { fetchFn } = spyFetch();
    const data = await runLoad(fetchFn);
    expect(data.board?.columns).toEqual(board.columns);
    expect(data.board?.project?.id).toBe('p1');
    expect(data.boardFehler).toBeNull();
  });

  it('shows no board on a page that is nobody s home, and calls that no error', async () => {
    // The overwhelmingly common case: nearly every page in the wiki. A notice here would be
    // furniture on all of them, paid for by every reader who never asked about a project.
    const { fetchFn } = spyFetch({ '/api/board': { status: 200, body: { columns: [] } } });
    const data = await runLoad(fetchFn);
    expect(data.board).toBeNull();
    expect(data.boardFehler).toBeNull();
  });

  it('treats a 404 from the board the same way, rather than as a failure', async () => {
    const { fetchFn } = spyFetch({ '/api/board': { status: 404, body: { error: 'not found' } } });
    const data = await runLoad(fetchFn);
    expect(data.board).toBeNull();
    expect(data.boardFehler).toBeNull();
  });

  it('says a real failure happened without claiming this page is a project home', async () => {
    const { fetchFn } = spyFetch({ '/api/board': { status: 500, body: { error: 'boom' } } });
    const data = await runLoad(fetchFn);
    expect(data.board).toBeNull();
    expect(data.boardFehler).toContain('500');
  });

  it('comes back to this page after a move, not to the global board', async () => {
    const { fetchFn } = spyFetch();
    expect((await runLoad(fetchFn)).zurueck).toBe('/rundgang/tabellen');
  });

  it('confirms a move against the board it just read', async () => {
    const { fetchFn } = spyFetch();
    const shown = await runLoad(fetchFn, '?verschoben=t1');
    expect(shown.hinweis?.art).toBe('ok');
    expect(shown.hinweis?.text).toContain('Kabel bestellen');

    const { fetchFn: second } = spyFetch();
    expect((await runLoad(second, '?verschoben=t-fremd')).hinweis).toBeNull();
  });

  it('says a refused move refused, on the page it was refused on', async () => {
    const { fetchFn } = spyFetch();
    const data = await runLoad(fetchFn, '?fehler=403');
    expect(data.hinweis?.art).toBe('fehler');
    expect(data.hinweis?.text).toContain('nicht verschoben');
  });

  it('still loads the page when the board endpoint is not there at all', async () => {
    // The board is an addition to this page, never a precondition for it. An API that
    // cannot answer about tasks must not take the wiki's reading surface down with it.
    const { fetchFn } = spyFetch({ '/api/board': { status: 503, body: { error: 'down' } } });
    const data = await runLoad(fetchFn);
    expect(data.doc.path).toBe('/rundgang/tabellen');
    expect(data.board).toBeNull();
  });
});
