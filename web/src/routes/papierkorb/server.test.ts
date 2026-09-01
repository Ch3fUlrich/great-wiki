import { describe, expect, it, vi } from 'vitest';
import { isActionFailure, isRedirect } from '@sveltejs/kit';
import { actions, load } from './+page.server';
import type { TrashEntry } from '$lib/trash';

/**
 * The loader and the two form actions behind `/papierkorb`.
 *
 * Two properties are worth pinning here and neither is visible by looking at the page.
 *
 * **The listing is an aggregate view.** Every row says that a page exists, what it is called
 * and who deleted it, so every row has to have come through `GET /api/trash` — which
 * authorises each entry, and each page inside it, through the same body a page read ends in.
 * That is asserted the only way an interface test honestly can: by pinning that the loader
 * asks that endpoint, and that what it renders is exactly what it was answered, with nothing
 * added, nothing dropped and no second request filling a gap. Whether the endpoint's own
 * answer is right belongs to `gw-store`, where it is mutation-tested.
 *
 * **The preview is not free and is not a query.** `GET /api/trash/purge/{path}` *runs the
 * purge and rolls it back* (ADR 0012), taking SQLite's write lock while it does. So it is
 * asked about one entry, only when somebody has asked to destroy that entry, and only about
 * an entry this caller was actually shown — never once per row, and never for a path typed
 * into the address bar.
 *
 * The forms are real form actions, so they work with JavaScript switched off — and these
 * tests call them the way a browser would, with a `FormData` body and nothing else, which is
 * why a click handler could never pass them.
 */

const entries: TrashEntry[] = [
  {
    path: '/handbuch',
    title: 'Handbuch',
    deleted_at: '2026-08-30 09:00:00',
    deleted_by_name: 'Sergej',
    pages: 3,
    may_restore: true
  },
  {
    path: '/rundgang/tabellen',
    title: 'Tabellen',
    deleted_at: '2026-08-29 17:45:00',
    deleted_by_name: 'Andere',
    pages: 1,
    may_restore: false
  }
];

const bericht = {
  committed: false,
  pages: [
    { path: '/handbuch', title: 'Handbuch' },
    { path: '/handbuch/onboarding', title: 'Onboarding' }
  ],
  revisions: 12,
  tasks: 3,
  projects: 1,
  links: 7,
  topic_filings: 4,
  topics: 2
};

interface Call {
  url: string;
  method: string;
}

/** A `fetch` that records every call and answers whatever the test told it to, in order. */
function spyFetch(answers: { status: number; body?: unknown }[]) {
  const calls: Call[] = [];
  let next = 0;
  const fetchFn = vi.fn(async (url: string | URL | Request, init?: RequestInit) => {
    const answer = answers[Math.min(next++, answers.length - 1)];
    calls.push({ url: String(url), method: init?.method ?? 'GET' });
    return new Response(answer.body === undefined ? '' : JSON.stringify(answer.body), {
      status: answer.status,
      headers: { 'content-type': 'application/json' }
    });
  });
  return { calls, fetchFn: fetchFn as unknown as typeof fetch };
}

/** The parts of a load event this loader reads, and no more. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function loadEvent(fetchFn: typeof fetch, query = ''): any {
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/papierkorb', {
      headers: { cookie: 'gw_session=abc' }
    }),
    url: new URL(`http://wiki.test/papierkorb${query}`)
  };
}

/** The parts of an action event the actions read. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function actionEvent(fetchFn: typeof fetch, fields: Record<string, string>): any {
  const form = new FormData();
  for (const [key, value] of Object.entries(fields)) form.append(key, value);
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/papierkorb', {
      method: 'POST',
      headers: { cookie: 'gw_session=abc' },
      body: form
    })
  };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
async function runLoad(fetchFn: typeof fetch, query = ''): Promise<any> {
  return load(loadEvent(fetchFn, query));
}

async function runAction(name: keyof typeof actions, fetchFn: typeof fetch, fields: Record<string, string>) {
  let thrown: unknown = null;
  let returned: unknown = null;
  try {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    returned = await (actions[name] as any)(actionEvent(fetchFn, fields));
  } catch (err) {
    thrown = err;
  }
  return { thrown, returned };
}

describe('the listing', () => {
  it('asks the one filtered endpoint, once, and renders what it answered', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { entries } }]);
    const data = await runLoad(fetchFn);

    expect(calls).toHaveLength(1);
    expect(calls[0].url).toContain('/api/trash');
    expect(calls[0].method).toBe('GET');
    expect(data.entries).toEqual(entries);
    expect(data.fehler).toBeNull();
  });

  it('forwards the caller‘s cookie, because the answer is different for every caller', async () => {
    const seen: (string | undefined)[] = [];
    const fetchFn = vi.fn(async (_url: string | URL | Request, init?: RequestInit) => {
      seen.push((init?.headers as Record<string, string> | undefined)?.cookie);
      return new Response(JSON.stringify({ entries }), { status: 200 });
    }) as unknown as typeof fetch;
    await runLoad(fetchFn);
    expect(seen[0]).toBe('gw_session=abc');
  });

  it('never reports a failed request as an empty Papierkorb', async () => {
    const { fetchFn } = spyFetch([{ status: 500 }]);
    const data = await runLoad(fetchFn);
    expect(data.entries).toEqual([]);
    expect(data.fehler).toContain('Fehler 500');
  });

  it('says the application did not answer when it did not answer at all', async () => {
    const fetchFn = vi.fn(async () => {
      throw new Error('offline');
    }) as unknown as typeof fetch;
    const data = await runLoad(fetchFn);
    expect(data.fehler).toContain('antwortet nicht');
  });
});

describe('the question before a purge', () => {
  it('asks nothing extra while nobody has asked to destroy anything', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { entries } }]);
    const data = await runLoad(fetchFn);
    expect(calls).toHaveLength(1);
    expect(data.entfernen).toBeNull();
    expect(data.bericht).toBeNull();
  });

  it('runs the preview once, for the one entry that was asked about', async () => {
    const { calls, fetchFn } = spyFetch([
      { status: 200, body: { entries } },
      { status: 200, body: bericht }
    ]);
    const data = await runLoad(fetchFn, '?entfernen=%2Fhandbuch');

    expect(calls).toHaveLength(2);
    expect(calls[1].url).toContain('/api/trash/purge/handbuch');
    expect(calls[1].method).toBe('GET');
    expect(data.entfernen?.path).toBe('/handbuch');
    // The names come from the API, in full. Summarising them into a count is the one thing
    // this confirmation may not do.
    expect(data.bericht.pages).toEqual(bericht.pages);
    expect(data.bericht.revisions).toBe(12);
  });

  it('does not ask about a path it never listed, however it reached the address bar', async () => {
    // The preview takes the write lock and names every page in a subtree. A hand-typed path
    // must not be able to start one — the API would refuse it, but an interface that asks the
    // question has already decided to ask it.
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { entries } }]);
    const data = await runLoad(fetchFn, '?entfernen=%2Fgeheim');
    expect(calls).toHaveLength(1);
    expect(data.entfernen).toBeNull();
  });

  it('states a refused preview instead of an empty confirmation', async () => {
    const { fetchFn } = spyFetch([
      { status: 200, body: { entries } },
      { status: 403 }
    ]);
    const data = await runLoad(fetchFn, '?entfernen=%2Fhandbuch');
    expect(data.bericht).toBeNull();
    expect(data.berichtFehler).toMatch(/verwalt/i);
    // The entry is still known, so the page can say WHICH page it is refusing to describe.
    expect(data.entfernen?.title).toBe('Handbuch');
  });

  it('keeps the Papierkorb readable when only the preview failed', async () => {
    const { fetchFn } = spyFetch([
      { status: 200, body: { entries } },
      { status: 500 }
    ]);
    const data = await runLoad(fetchFn, '?entfernen=%2Fhandbuch');
    expect(data.entries).toEqual(entries);
    expect(data.fehler).toBeNull();
    expect(data.berichtFehler).toContain('Fehler 500');
  });
});

describe('what just happened', () => {
  it('confirms a restore only once the entry is genuinely out of the Papierkorb', async () => {
    const { fetchFn } = spyFetch([{ status: 200, body: { entries } }]);
    const gone = await runLoad(fetchFn, '?wiederhergestellt=%2Fandere');
    expect(gone.wiederhergestellt).toBe('/andere');

    const { fetchFn: second } = spyFetch([{ status: 200, body: { entries } }]);
    const still = await runLoad(second, '?wiederhergestellt=%2Fhandbuch');
    // Still listed, so it did not come back — and saying it did would be the interface
    // telling the reader something the data in front of it contradicts.
    expect(still.wiederhergestellt).toBeNull();
  });

  it('confirms a purge the same way', async () => {
    const { fetchFn } = spyFetch([{ status: 200, body: { entries } }]);
    const data = await runLoad(fetchFn, '?geleert=%2Fandere');
    expect(data.geleert).toBe('/andere');
  });
});

describe('putting one back', () => {
  it('posts to the restore endpoint for the page it was given', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { path: '/handbuch', title: 'Handbuch', pages: 3 } }]);
    const { thrown } = await runAction('wiederherstellen', fetchFn, { pfad: '/handbuch' });

    expect(calls[0].method).toBe('POST');
    expect(calls[0].url).toContain('/api/trash/restore/handbuch');
    expect(isRedirect(thrown)).toBe(true);
    expect(isRedirect(thrown) && (thrown as { location: string }).location).toBe(
      '/papierkorb?wiederhergestellt=%2Fhandbuch#gw-papierkorb'
    );
  });

  it('refuses an empty field itself rather than asking the API about it', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200 }]);
    const { returned } = await runAction('wiederherstellen', fetchFn, { pfad: '  ' });
    expect(isActionFailure(returned)).toBe(true);
    expect(calls).toHaveLength(0);
  });

  it('carries the refusal that names the parent, rather than reducing it to a status', async () => {
    const said = '/handbuch is still in the trash: restore it first';
    const { fetchFn } = spyFetch([{ status: 409, body: { error: said } }]);
    const { returned } = await runAction('wiederherstellen', fetchFn, {
      pfad: '/handbuch/onboarding'
    });
    expect(isActionFailure(returned)).toBe(true);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (returned as any).data;
    expect(data.wo).toBe('wiederherstellen');
    expect(data.fehler).toContain('/handbuch');
    expect(data.fehler).toContain('nichts wiederhergestellt');
  });

  it('reports a refusal as the status it was, not as a cheerful 200', async () => {
    const { fetchFn } = spyFetch([{ status: 403, body: { error: 'forbidden' } }]);
    const { returned } = await runAction('wiederherstellen', fetchFn, { pfad: '/handbuch' });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    expect((returned as any).status).toBe(403);
  });
});

describe('destroying one', () => {
  it('posts to the same address the preview described, so the two cannot disagree', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { ...bericht, committed: true } }]);
    const { thrown } = await runAction('endgueltigLoeschen', fetchFn, { pfad: '/handbuch' });

    expect(calls[0].method).toBe('POST');
    expect(calls[0].url).toContain('/api/trash/purge/handbuch');
    expect(isRedirect(thrown) && (thrown as { location: string }).location).toBe(
      '/papierkorb?geleert=%2Fhandbuch#gw-papierkorb'
    );
  });

  it('says who may ask, and that nothing was destroyed, when the gate refused', async () => {
    const { fetchFn } = spyFetch([{ status: 403, body: { error: 'forbidden' } }]);
    const { returned } = await runAction('endgueltigLoeschen', fetchFn, { pfad: '/handbuch' });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const data = (returned as any).data;
    expect(data.wo).toBe('endgueltig');
    expect(data.fehler).toMatch(/verwalt/i);
    expect(data.fehler).toContain('Es wurde nichts endgültig gelöscht.');
  });

  it('refuses an empty field itself', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200 }]);
    const { returned } = await runAction('endgueltigLoeschen', fetchFn, { pfad: '' });
    expect(isActionFailure(returned)).toBe(true);
    expect(calls).toHaveLength(0);
  });
});
