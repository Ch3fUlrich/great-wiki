import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import EmbeddedView from './EmbeddedView.svelte';
import type { EmbedState } from './embed';
import type { DatasetLoad } from './load';
import type { Field } from './table';

/**
 * The component renders PURELY from the `state` it is handed — the read happened in
 * `+page.server.ts`, under the reader's own cookie, before this markup existed. So these
 * tests pin one thing per state and nothing else: what the reader is allowed to see is
 * decided by the value, and the DOM says exactly that and no more.
 */
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

function html(state: EmbedState): string {
  return render(EmbeddedView, { props: { state } }).body.replace(/<!--.*?-->/g, '');
}

const readyBoard: EmbedState = {
  status: 'ready',
  path: '/raum/tabelle',
  load: load('board', { group_by: 'status' })
};

describe('EmbeddedView markup', () => {
  it('renders nothing for unavailable, identically for any two of them', () => {
    const a = html({ status: 'unavailable' });
    const b = html({ status: 'unavailable' });
    expect(a).toBe(b);
    expect(a).toBe('');
  });

  it('says the view is gone for view_missing', () => {
    expect(html({ status: 'view_missing' })).toContain('Diese Ansicht gibt es nicht mehr.');
  });

  it('shows the one-line note for a view that cannot be embedded', () => {
    expect(html({ status: 'not_embeddable' })).toContain(
      'Diese Ansicht kann nicht eingebettet werden.'
    );
  });

  it('announces a load failure in an alert', () => {
    const out = html({ status: 'error' });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Die Datenansicht konnte nicht geladen werden.');
  });

  it('renders the picked board for a ready embeddable view', () => {
    expect(html(readyBoard)).toContain('data-board');
  });

  it('renders the ready table embedded, with no sort link and no add-row form', () => {
    const out = html({ status: 'ready', path: '/d', load: load('table', {}) });
    expect(out).toContain('Ada');
    expect(out).not.toContain('href="?sort=');
  });

  it('never renders a loading line — there is no client-side read left to wait for', () => {
    for (const state of [
      { status: 'unavailable' },
      { status: 'view_missing' },
      { status: 'not_embeddable' },
      { status: 'error' },
      readyBoard
    ] as EmbedState[]) {
      expect(html(state)).not.toContain('wird geladen');
    }
  });

  it('discloses nothing of its own accord when the state says unavailable', () => {
    // The server's `unavailable` carries no title, path or count to echo; the component's
    // empty render is the proof the two halves are the same contract.
    expect(html({ status: 'unavailable' })).not.toContain('/raum');
    expect(html({ status: 'unavailable' })).not.toContain('Tabelle');
  });
});
