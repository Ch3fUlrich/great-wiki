import { describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import EmbeddedView from './EmbeddedView.svelte';
import { resolveEmbed, type EmbedState } from './embed';
import type { DatasetLoad } from './load';
import type { Field } from './table';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  {
    key: 'status',
    label: 'Status',
    kind: 'select',
    config: { options: [{ id: 'offen', label: 'Offen' }] },
    position: 1
  }
];

function load(kind: string, config: unknown): DatasetLoad {
  return {
    fields,
    page: {
      rows: [{ id: '1', values: { name: 'Ada' }, version: 1, created_at: '', updated_at: '' }],
      total: 1
    },
    error: null,
    sort: null,
    filter: null,
    labels: {},
    views: [],
    view: { id: 'v1', name: 'Ansicht', kind, config, position: 0 },
    attachments: []
  };
}

/** A fetch that answers 200 with whatever JSON is given, recording the URL it was asked for. */
function fetchJson(body: unknown, status = 200) {
  const seen: string[] = [];
  const f = vi.fn(async (url: string | URL | Request) => {
    seen.push(String(url));
    return new Response(JSON.stringify(body), {
      status,
      headers: { 'content-type': 'application/json' }
    });
  }) as unknown as typeof fetch;
  return { f, seen };
}

const boardAnswer = { state: 'ok', path: '/raum/tabelle', title: 'Tabelle', view: { id: 'v1', name: 'Alle', kind: 'board', config: { group_by: 'status' }, position: 0 } };

describe('resolveEmbed', () => {
  it('maps the uniform refusal to unavailable, and does not load a row', async () => {
    const loader = vi.fn();
    const { f } = fetchJson({ state: 'unavailable' });
    expect(await resolveEmbed(f, 'doc', 'view', loader)).toEqual({ status: 'unavailable' });
    expect(loader).not.toHaveBeenCalled();
  });

  it('is the SAME state for every refusal, so absent and unreadable cannot be told', async () => {
    const loader = vi.fn();
    const a = await resolveEmbed(fetchJson({ state: 'unavailable' }).f, 'doc', 'view', loader);
    const b = await resolveEmbed(fetchJson({ nonsense: true }).f, 'other', 'other', loader);
    expect(a).toEqual({ status: 'unavailable' });
    expect(a).toEqual(b);
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });

  it('maps view_missing to its own line', async () => {
    const out = await resolveEmbed(fetchJson({ state: 'view_missing' }).f, 'doc', 'view', vi.fn());
    expect(out).toEqual({ status: 'view_missing' });
  });

  it('fetches the embed address before asking for anything else', async () => {
    const { f, seen } = fetchJson({ state: 'unavailable' });
    await resolveEmbed(f, 'doc-1', 'view-1', vi.fn());
    expect(seen[0]).toBe('/api/datasets/embed/doc-1/view-1');
  });

  it('loads the rows, under the caller, only for an embeddable ok answer', async () => {
    const calls: Array<{ path: string; params: URLSearchParams }> = [];
    const loader = vi.fn(async (_f: typeof fetch, path: string, params: URLSearchParams) => {
      calls.push({ path, params });
      return load('board', { group_by: 'status' });
    }) as unknown as (f: typeof fetch, p: string, q: URLSearchParams, c: string | null) => Promise<DatasetLoad>;
    const out = await resolveEmbed(fetchJson(boardAnswer).f, 'doc', 'view', loader);
    expect(out.status).toBe('ready');
    expect(calls[0]?.path).toBe('/raum/tabelle');
    expect(calls[0]?.params.get('view')).toBe('v1');
    if (out.status === 'ready') expect(out.path).toBe('/raum/tabelle');
  });

  it('refuses to embed a form and never loads its rows', async () => {
    const loader = vi.fn();
    const answer = { ...boardAnswer, view: { ...boardAnswer.view, kind: 'form' } };
    const out = await resolveEmbed(fetchJson(answer).f, 'doc', 'view', loader);
    expect(out).toEqual({ status: 'not_embeddable' });
    expect(loader).not.toHaveBeenCalled();
  });

  it('treats a malformed 200 body as unavailable', async () => {
    const out = await resolveEmbed(fetchJson({ state: 'ok', path: 5 }).f, 'doc', 'view', vi.fn());
    expect(out).toEqual({ status: 'unavailable' });
  });

  it('reports a network rejection as an error', async () => {
    const f = vi.fn(async () => {
      throw new TypeError('failed');
    }) as unknown as typeof fetch;
    expect(await resolveEmbed(f, 'doc', 'view', vi.fn())).toEqual({ status: 'error' });
  });

  it('reports a non-2xx as an error, not a refusal', async () => {
    const out = await resolveEmbed(fetchJson({ error: 'boom' }, 500).f, 'doc', 'view', vi.fn());
    expect(out).toEqual({ status: 'error' });
  });
});

function html(props: { datasetDoc: string; view: string; label?: string; seed?: EmbedState }): string {
  return render(EmbeddedView, { props }).body.replace(/<!--.*?-->/g, '');
}

describe('EmbeddedView markup', () => {
  it('renders the loading line first (that is all SSR ever emits)', () => {
    const out = html({ datasetDoc: 'doc', view: 'view' });
    expect(out).toContain('Datenansicht wird geladen …');
  });

  it('renders nothing for unavailable, identically for two of them, and never an id', () => {
    const a = html({ datasetDoc: 'SECRETDOC', view: 'SECRETVIEW', seed: { status: 'unavailable' } });
    const b = html({ datasetDoc: 'OTHER', view: 'OTHER', seed: { status: 'unavailable' } });
    expect(a).toBe(b);
    expect(a).not.toContain('SECRETDOC');
    expect(a).not.toContain('SECRETVIEW');
    expect(a).not.toContain('/raum');
    expect(a).not.toContain('Tabelle');
  });

  it('says the view is gone for view_missing', () => {
    expect(html({ datasetDoc: 'd', view: 'v', seed: { status: 'view_missing' } })).toContain(
      'Diese Ansicht gibt es nicht mehr.'
    );
  });

  it('shows the one-line note for a view that cannot be embedded', () => {
    expect(html({ datasetDoc: 'd', view: 'v', seed: { status: 'not_embeddable' } })).toContain(
      'Diese Ansicht kann nicht eingebettet werden.'
    );
  });

  it('announces a load failure in an alert', () => {
    const out = html({ datasetDoc: 'd', view: 'v', seed: { status: 'error' } });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Die Datenansicht konnte nicht geladen werden.');
  });

  it('renders the picked board for a ready embeddable view', () => {
    const out = html({
      datasetDoc: 'd',
      view: 'v',
      seed: { status: 'ready', path: '/raum/tabelle', load: load('board', { group_by: 'status' }) }
    });
    expect(out).toContain('data-board');
  });

  it('renders the ready table embedded, with no sort link and no add-row form', () => {
    const out = html({
      datasetDoc: 'd',
      view: 'v',
      seed: { status: 'ready', path: '/d', load: load('table', {}) }
    });
    expect(out).toContain('Ada');
    expect(out).not.toContain('href="?sort=');
  });
});
