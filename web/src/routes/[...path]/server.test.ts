import { describe, expect, it, vi } from 'vitest';
import { isActionFailure, isRedirect } from '@sveltejs/kit';
import { actions, load } from './+page.server';
import type { DocumentView } from '$lib/api';
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
const doc: DocumentView = {
  id: 'd1',
  path: '/rundgang/tabellen',
  parent_path: '/rundgang',
  slug: 'tabellen',
  doc_type: 'page',
  title: 'Tabellen',
  language: 'de',
  visibility: 'restricted',
  body: JSON.stringify({ kind: 'doc', content: [] }),
  sort_key: 1,
  // The bit `/api/documents` has answered since 073281b and this interface used to ignore:
  // the same verdict a write would get, from the authorisation that produced this response.
  may_write: true
};

/** What `/api/topics/document/{path}` says this page is about. */
const seitenThemen = [
  { path: '/format', name: 'Format', display_path: 'Format' },
  { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen' }
];

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
function spyFetch(overrides: Record<string, Answer> = {}, schreiben?: Answer) {
  const urls: string[] = [];
  const table: Record<string, Answer> = {
    '/api/documents': { status: 200, body: doc },
    '/api/tree': { status: 200, body: [] },
    '/api/links/backlinks': { status: 200, body: { backlinks: [] } },
    '/api/board': { status: 200, body: board },
    '/api/topics/document': { status: 200, body: { topics: seitenThemen } },
    ...overrides
  };
  const sent: { url: string; method: string; body: string | undefined }[] = [];
  const fetchFn = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    const asked = String(url);
    urls.push(asked);
    sent.push({ url: asked, method: init?.method ?? 'GET', body: init?.body as string | undefined });
    // A write gets its own answer when the test named one. Without that split, a test that
    // wanted a refused PUT would also refuse the GET that reads the current set — and would
    // then be testing a page that could not be read, which is not the case in question.
    const key = Object.keys(table).find((prefix) => asked.includes(prefix));
    const answer =
      schreiben && (init?.method ?? 'GET') !== 'GET' ? schreiben : key ? table[key] : { status: 404 };
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  return { urls, sent, fetchFn: fetchFn as unknown as typeof fetch };
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
  doc: DocumentView;
  board: BoardResponse | null;
  boardFehler: string | null;
  hinweis: BoardNotice | null;
  zurueck: string;
  now: number;
  seitenThemen: { path: string; name: string; display_path: string }[];
  seitenThemenFehler: string | null;
}

/** The parts of an action event the two topic actions read. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function actionEvent(fetchFn: typeof fetch, fields: Record<string, string>): any {
  const form = new FormData();
  for (const [name, value] of Object.entries(fields)) form.append(name, value);
  return {
    params: { path: 'rundgang/tabellen' },
    fetch: fetchFn,
    request: new Request('http://wiki.test/rundgang/tabellen', {
      method: 'POST',
      headers: { cookie: 'gw_session=abc' },
      body: form
    })
  };
}

/** Run an action and give back whatever it threw or returned, whichever it was. */
async function runAction(
  which: 'themaHinzufuegen' | 'themaEntfernen' | 'loeschen',
  fetchFn: typeof fetch,
  fields: Record<string, string>
) {
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return { returned: await (actions as any)[which](actionEvent(fetchFn, fields)), thrown: null };
  } catch (thrown) {
    return { returned: null, thrown };
  }
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

/**
 * The owner's second decision: **a page's topics are shown and edited on the page itself.**
 *
 * So the loader reads them here, and the two form actions below are what change them —
 * ordinary form actions, called by these tests exactly the way a browser calls them, with a
 * `FormData` body and nothing else. A click handler could not pass any of this, which is the
 * point: tagging has to work while reading, before any bundle arrives.
 *
 * `PUT /api/topics/document/{path}` takes the WHOLE set, because that is what a frontmatter
 * line says and what a file drop has to be able to mean. So both actions are read–modify–write
 * and both read fresh: a hidden field carrying the set the reader was shown would resurrect a
 * topic somebody else had just removed.
 */
describe('what this page is about', () => {
  it('asks the one endpoint for it, once', async () => {
    const { urls, fetchFn } = spyFetch();
    await runLoad(fetchFn);
    const asked = urls.filter((url) => url.includes('/api/topics'));
    expect(asked).toHaveLength(1);
    expect(asked[0]).toContain('/api/topics/document/rundgang/tabellen');
  });

  it('renders the topics it was answered with, and none it assembled itself', async () => {
    const { fetchFn } = spyFetch();
    const data = await runLoad(fetchFn);
    expect(data.seitenThemen).toEqual(seitenThemen);
    expect(data.seitenThemenFehler).toBeNull();
  });

  it('never takes the page down when that request fails, and says what happened', async () => {
    // The chips are an addition to a page, never a precondition for one — the same rule the
    // embedded board follows. But an empty chip row and a failed request are different
    // things, and rendering the first for the second would say this page is about nothing.
    const { fetchFn } = spyFetch({ '/api/topics/document': { status: 500, body: {} } });
    const data = await runLoad(fetchFn);
    expect(data.doc.path).toBe('/rundgang/tabellen');
    expect(data.seitenThemen).toEqual([]);
    expect(data.seitenThemenFehler).toContain('500');
  });
});

describe('adding a topic to the page you are reading', () => {
  it('puts back the whole set — what was there, plus the new one', async () => {
    const { sent, fetchFn } = spyFetch();
    const { thrown } = await runAction('themaHinzufuegen', fetchFn, { thema: 'Medizin/Darm' });

    const put = sent.find((call) => call.method === 'PUT');
    expect(put?.url).toContain('/api/topics/document/rundgang/tabellen');
    expect(JSON.parse(put?.body ?? '{}')).toEqual({
      topics: ['Format', 'Rundgang/Tabellen', 'Medizin/Darm']
    });
    // Post, redirect, get: a reload must not offer to file the topic a second time.
    expect(isRedirect(thrown)).toBe(true);
  });

  it('sends the spelling somebody typed, not a canonical path', async () => {
    // `set_document_topics` parses what it is given, and a leading separator makes an empty
    // first segment — so the stored `path` would be REFUSED. `display_path` is the string a
    // file states and the string the API takes.
    const { sent, fetchFn } = spyFetch();
    await runAction('themaHinzufuegen', fetchFn, { thema: 'Neu' });
    const put = sent.find((call) => call.method === 'PUT');
    expect(put?.body).not.toContain('"/rundgang/tabellen"');
  });

  it('comes back to the topics of the page it was added on', async () => {
    const { fetchFn } = spyFetch();
    const { thrown } = await runAction('themaHinzufuegen', fetchFn, { thema: 'Neu' });
    // The fragment is what makes the change announced rather than merely drawn: the browser
    // puts focus on the region, and the region is read out. No script involved.
    expect(isRedirect(thrown) && (thrown as { location: string }).location).toBe(
      '/rundgang/tabellen#gw-themen'
    );
  });

  it('refuses an empty field itself rather than asking the API about it', async () => {
    const { sent, fetchFn } = spyFetch();
    const { returned } = await runAction('themaHinzufuegen', fetchFn, { thema: '   ' });
    expect(isActionFailure(returned)).toBe(true);
    expect(sent.some((call) => call.method === 'PUT')).toBe(false);
  });

  it('passes on what the API said about a topic it would not take', async () => {
    const { fetchFn } = spyFetch({}, { status: 400, body: { error: '`a//b` ist kein Thema' } });
    const { returned } = await runAction('themaHinzufuegen', fetchFn, { thema: 'a//b' });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (returned as any).data;
    expect(data.fehler).toContain('`a//b` ist kein Thema');
    // The promise every refusal here makes, and one the API lets it make: the whole list is
    // parsed before anything is written, so a page with one bad topic keeps the ones it had.
    expect(data.fehler).toContain('nicht geändert');
    expect(data.getippt).toBe('a//b');
  });

  it('reports a refused write as the status it was, not as a cheerful 200', async () => {
    const { fetchFn } = spyFetch({}, { status: 403, body: { error: 'forbidden' } });
    const { returned } = await runAction('themaHinzufuegen', fetchFn, { thema: 'Neu' });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((returned as any).status).toBe(403);
  });
});

describe('taking a topic off the page you are reading', () => {
  it('puts back everything except the one named', async () => {
    const { sent, fetchFn } = spyFetch();
    await runAction('themaEntfernen', fetchFn, { pfad: '/format' });
    const put = sent.find((call) => call.method === 'PUT');
    expect(JSON.parse(put?.body ?? '{}')).toEqual({ topics: ['Rundgang/Tabellen'] });
  });

  it('reads the set fresh rather than trusting what the page was rendered with', async () => {
    // A hidden field carrying the whole set would put back a topic somebody else removed
    // between the render and the press, silently.
    const { urls, fetchFn } = spyFetch();
    await runAction('themaEntfernen', fetchFn, { pfad: '/format' });
    expect(urls.filter((url) => url.includes('/api/topics/document'))).toHaveLength(2);
  });

  it('empties the set when the last topic is taken off', async () => {
    const { sent, fetchFn } = spyFetch({
      '/api/topics/document': { status: 200, body: { topics: [seitenThemen[0]] } }
    });
    await runAction('themaEntfernen', fetchFn, { pfad: '/format' });
    const put = sent.find((call) => call.method === 'PUT');
    expect(JSON.parse(put?.body ?? '{}')).toEqual({ topics: [] });
  });

  it('names nothing and changes nothing when the form carried no topic', async () => {
    const { sent, fetchFn } = spyFetch();
    const { returned } = await runAction('themaEntfernen', fetchFn, {});
    expect(isActionFailure(returned)).toBe(true);
    expect(sent.some((call) => call.method === 'PUT')).toBe(false);
  });
});

describe('putting the page in the Papierkorb', () => {
  it('asks the question in the address, so the page renders it in the first response', async () => {
    const { fetchFn } = spyFetch();
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (await runLoad(fetchFn, '?loeschen=1')) as any;
    expect(data.loeschen).toBe(true);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect(((await runLoad(fetchFn)) as any).loeschen).toBe(false);
  });

  it('deletes through the document itself, with the verb that says which operation it is', async () => {
    const { sent, fetchFn } = spyFetch({}, { status: 200, body: { path: '/rundgang/tabellen', title: 'Tabellen', pages: 3 } });
    const { thrown } = await runAction('loeschen', fetchFn, {});
    const call = sent.find((one) => one.method === 'DELETE');
    expect(call?.url).toContain('/api/documents/rundgang/tabellen');
    // Back to the Papierkorb, which is where the page now is and where it can be brought
    // back from — never to the address it used to have, which now answers 404.
    expect(isRedirect(thrown) && (thrown as { location: string }).location).toBe(
      '/papierkorb?geloescht=%2Frundgang%2Ftabellen#gw-papierkorb'
    );
  });

  it('says what stood in the way, on the page, rather than sending anybody to the Papierkorb', async () => {
    const { fetchFn } = spyFetch({}, { status: 409, body: { error: 'has a subpage you may not write' } });
    const { returned } = await runAction('loeschen', fetchFn, {});
    expect(isActionFailure(returned)).toBe(true);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (returned as any).data;
    expect(data.wo).toBe('loeschen');
    expect(data.fehler).toContain('Es wurde nichts gelöscht.');
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((returned as any).status).toBe(409);
  });

  it('keeps a refused delete out of the topic field, which is a different control', async () => {
    const { fetchFn } = spyFetch({}, { status: 403, body: { error: 'forbidden' } });
    const { returned } = await runAction('loeschen', fetchFn, {});
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((returned as any).data.wo).toBe('loeschen');
    const { fetchFn: zweit } = spyFetch({}, { status: 403, body: { error: 'forbidden' } });
    const { returned: thema } = await runAction('themaHinzufuegen', zweit, { thema: 'Neu' });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((thema as any).data.wo).toBe('thema');
  });
});
