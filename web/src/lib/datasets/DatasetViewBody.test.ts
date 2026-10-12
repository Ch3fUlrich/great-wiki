import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import DatasetViewBody from './DatasetViewBody.svelte';
import type { DatasetLoad } from './load';
import type { Field } from './table';
import type { DatasetView } from './views';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  {
    key: 'status',
    label: 'Status',
    kind: 'select',
    config: { options: [{ id: 'offen', label: 'Offen' }] },
    position: 1
  },
  { key: 'wann', label: 'Wann', kind: 'date', config: {}, position: 2 }
];

const view = (kind: string, config: unknown): DatasetView => ({
  id: 'v1',
  name: 'Ansicht',
  kind,
  config,
  position: 0
});

function load(datasetView: DatasetView | null, over: Partial<DatasetLoad> = {}): DatasetLoad {
  return {
    fields,
    page: { rows: [{ id: '1', values: { name: 'Ada' }, version: 1, created_at: '', updated_at: '' }], total: 1 },
    error: null,
    sort: null,
    filter: null,
    labels: {},
    views: [],
    view: datasetView,
    attachments: [],
    ...over
  };
}

function html(props: { path: string; dataset: DatasetLoad; mayWrite?: boolean; month?: string | null; embedded?: boolean }): string {
  return render(DatasetViewBody, { props }).body.replace(/<!--.*?-->/g, '');
}

describe('DatasetViewBody', () => {
  it('draws the form for a form view', () => {
    expect(html({ path: '/d', dataset: load(view('form', { fields: ['name'] })), mayWrite: true })).toContain(
      'datensatz-formular'
    );
  });

  it('draws the board for a board view', () => {
    expect(html({ path: '/d', dataset: load(view('board', { group_by: 'status' })) })).toContain(
      'data-board'
    );
  });

  it('draws the calendar for a calendar view', () => {
    expect(html({ path: '/d', dataset: load(view('calendar', { date: 'wann' })), month: '2026-03' })).toContain(
      'data-calendar'
    );
  });

  it('draws the gallery for a gallery view', () => {
    expect(html({ path: '/d', dataset: load(view('gallery', { cover: 'name' })) })).toContain(
      'data-gallery'
    );
  });

  it('falls back to the table for no picked view', () => {
    const out = html({ path: '/d', dataset: load(null) });
    expect(out).toContain('aria-label="Tabelle"');
    expect(out).toContain('Ada');
  });

  it('hands the table the embedded flag', () => {
    const out = html({ path: '/d', dataset: load(null), mayWrite: true, embedded: true });
    expect(out).toContain('aria-label="Tabelle"');
    // Embedded: no sort links, no add-row form.
    expect(out).not.toContain('Zeile hinzufügen');
    expect(out).not.toContain('href="?sort=');
  });

  it('states a failed read as the table error, not an empty body', () => {
    const out = html({
      path: '/d',
      dataset: load(null, { page: null, error: 'Die Tabelle konnte nicht geladen werden (Fehler 500).' })
    });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Fehler 500');
  });
});
