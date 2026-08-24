import { afterEach, describe, expect, it, vi } from 'vitest';
import { isActionFailure, isRedirect } from '@sveltejs/kit';
import { env } from '$env/dynamic/private';
import { actions, load } from './+page.server';
import type { Project } from '$lib/projects';

/**
 * The loader and the two form actions behind `/projekte`.
 *
 * These are the tests that matter for the property this page could quietly lose. `/projekte`
 * is an aggregate view: a project's row says that a page exists and what it is called, so
 * every row on it has to have come through `/api/projects`, which filters per document
 * through the same permission-checked accessor a page read uses. That is asserted here the
 * only way an interface test honestly can — by pinning that the loader asks that endpoint
 * and that what it returns is exactly what it was answered, with nothing added, nothing
 * dropped and no second request to fill in a gap. Whether the endpoint's own answer is right
 * belongs to `gw-store`, where it is mutation-tested.
 *
 * The other half is the form. It is a real form action, so it works with JavaScript switched
 * off — and these tests call it the way the browser would, with a `FormData` body and
 * nothing else, which is why a click handler could never pass them.
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
    tag_id: 't-umbau',
    created_at: '2026-08-21 11:30:00'
  }
];

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

/** The parts of a load event this loader reads, and no more. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function loadEvent(fetchFn: typeof fetch, query = ''): any {
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/projekte', { headers: { cookie: 'gw_session=abc' } }),
    url: new URL(`http://wiki.test/projekte${query}`)
  };
}

/** The parts of an action event the actions read. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function actionEvent(fetchFn: typeof fetch, fields: Record<string, string>): any {
  const form = new FormData();
  for (const [key, value] of Object.entries(fields)) form.append(key, value);
  return {
    fetch: fetchFn,
    request: new Request('http://wiki.test/projekte', {
      method: 'POST',
      headers: { cookie: 'gw_session=abc' },
      body: form
    })
  };
}

/** What the loader returns, named. `PageServerLoad` widens it to `void | Record<…>`. */
interface Loaded {
  projects: Project[];
  error: string | null;
  confirming: Project | null;
  created: Project | null;
}

async function runLoad(fetchFn: typeof fetch, query = ''): Promise<Loaded> {
  return (await load(loadEvent(fetchFn, query))) as unknown as Loaded;
}

/** A refused action returns; a successful one throws a redirect. Both are outcomes. */
interface Refusal {
  status: number;
  data: { wo: string; fehler: string; startseite: string };
}

async function runAction(
  name: 'anlegen' | 'loeschen',
  fetchFn: typeof fetch,
  fields: Record<string, string>
): Promise<{ returned: unknown; thrown: unknown }> {
  try {
    return { returned: await actions[name](actionEvent(fetchFn, fields)), thrown: null };
  } catch (thrown) {
    return { returned: null, thrown };
  }
}

const ORIGINAL = { ...env };

afterEach(() => {
  for (const key of Object.keys(env)) delete env[key];
  Object.assign(env, ORIGINAL);
});

describe('the loader', () => {
  it('asks the filtered endpoint and renders exactly what it answered', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { projects } }]);

    const data = await runLoad(fetchFn);

    expect(calls).toHaveLength(1);
    expect(calls[0].url).toMatch(/\/api\/projects$/);
    // One source, and it is the one that filters. Anything this loader added — a second
    // request, a list from the tree, a locally cached row — would be a second answer to
    // "which projects exist", and a second answer is a second chance to disclose one.
    expect(data.projects).toEqual(projects);
    expect(data.error).toBeNull();
  });

  it('forwards the caller cookie, so the API sees the session the browser has', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { projects } }]);
    await runLoad(fetchFn);
    // Without this the API would answer as nobody and the page would show the public view
    // to somebody signed in — which is the failure `$lib/api`'s own test records.
    expect(calls[0].headers.cookie).toBe('gw_session=abc');
  });

  it('reports a failed request instead of an empty list', async () => {
    const { fetchFn } = spyFetch([{ status: 500, body: { error: 'internal error' } }]);

    const data = await runLoad(fetchFn);

    expect(data.projects).toEqual([]);
    expect(data.error).toContain('500');
  });

  it('takes the deletion being confirmed from the list, not from the query', async () => {
    const { fetchFn } = spyFetch([{ status: 200, body: { projects } }]);

    const data = await runLoad(fetchFn, '?loeschen=p2');

    expect(data.confirming?.id).toBe('p2');
  });

  it('confirms nothing for an id that is not in the list', async () => {
    // A hand-typed id must not produce a question naming a project this reader was not
    // shown. The list is already filtered; matching against it is what keeps the URL from
    // becoming a second way to ask about a project.
    const { fetchFn } = spyFetch([{ status: 200, body: { projects } }]);

    const data = await runLoad(fetchFn, '?loeschen=p-fremd');

    expect(data.confirming).toBeNull();
  });

  it('confirms a creation only against a project that is actually in the list', async () => {
    const { fetchFn } = spyFetch([{ status: 200, body: { projects } }]);

    const shown = await runLoad(fetchFn, '?angelegt=%2Frundgang%2Ftabellen');
    expect(shown.created?.id).toBe('p1');

    const { fetchFn: second } = spyFetch([{ status: 200, body: { projects } }]);
    const invented = await runLoad(second, '?angelegt=%2Fetwas%2Fanderes');
    expect(invented.created).toBeNull();
  });
});

describe('creating a project', () => {
  it('posts the path to /api/projects and comes back to the list', async () => {
    env.GW_PROXY_SECRET = 'geheim';
    const { calls, fetchFn } = spyFetch([{ status: 201, body: { id: 'p9' } }]);

    const { thrown } = await runAction('anlegen', fetchFn, { startseite: 'rundgang/tabellen' });

    expect(calls[0].method).toBe('POST');
    expect(calls[0].url).toMatch(/\/api\/projects$/);
    expect(JSON.parse(calls[0].body ?? '{}')).toEqual({ home_path: '/rundgang/tabellen' });
    expect(calls[0].headers.cookie).toBe('gw_session=abc');
    // The same attestation `apiGet` sends. A write refused by `proxy_guard` would look
    // exactly like a permission refusal from here, which is the worst kind of confusion.
    expect(calls[0].headers['X-GW-Proxy']).toBe('geheim');

    // Post, redirect, get: reloading the list must not offer to create the project again.
    expect(isRedirect(thrown)).toBe(true);
    expect((thrown as { status: number; location: string }).status).toBe(303);
    expect((thrown as { location: string }).location).toContain('angelegt=');
  });

  it('turns a 409 into the sentence that names the way out, keeping what was typed', async () => {
    const { fetchFn } = spyFetch([
      {
        status: 409,
        body: { error: 'that page is already the home of a project; open that board, or pick another page' }
      }
    ]);

    const { returned } = await runAction('anlegen', fetchFn, { startseite: '/rundgang/tabellen' });

    expect(isActionFailure(returned)).toBe(true);
    const data = (returned as Refusal).data;
    expect(data.fehler).toContain('/rundgang/tabellen');
    expect(data.fehler).toContain('Startseite eines Projekts');
    expect(data.startseite).toBe('/rundgang/tabellen');
  });

  it('says which page is missing when the API answers 404', async () => {
    const { fetchFn } = spyFetch([{ status: 404, body: { error: 'not found' } }]);

    const { returned } = await runAction('anlegen', fetchFn, { startseite: '/gibtsnicht' });

    expect((returned as Refusal).data.fehler).toContain('/gibtsnicht');
  });

  it('asks for a path rather than sending an empty one', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 201, body: {} }]);

    const { returned } = await runAction('anlegen', fetchFn, { startseite: '   ' });

    expect(calls).toHaveLength(0);
    expect(isActionFailure(returned)).toBe(true);
    expect((returned as Refusal).status).toBe(400);
  });
});

describe('deleting a project', () => {
  it('deletes by id and comes back to the list', async () => {
    const { calls, fetchFn } = spyFetch([{ status: 200, body: { changed: true } }]);

    const { thrown } = await runAction('loeschen', fetchFn, { id: 'p2' });

    expect(calls[0].method).toBe('DELETE');
    expect(calls[0].url).toMatch(/\/api\/projects\/p2$/);
    expect(isRedirect(thrown)).toBe(true);
  });

  it('promises nothing was deleted when the API refuses', async () => {
    const { fetchFn } = spyFetch([{ status: 403, body: { error: 'forbidden' } }]);

    const { returned } = await runAction('loeschen', fetchFn, { id: 'p2' });

    expect(isActionFailure(returned)).toBe(true);
    expect((returned as Refusal).data.fehler).toContain('nichts gelöscht');
  });
});
