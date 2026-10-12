import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import TableView from './TableView.svelte';
import type { Field, RowPage } from './table';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'alter', label: 'Alter', kind: 'number', config: {}, position: 1 },
  { key: 'url', label: 'Seite', kind: 'url', config: {}, position: 2 },
  { key: 'calc', label: 'Rechnung', kind: 'formula', config: {}, position: 3 },
  {
    key: 'wer',
    label: 'Wer',
    kind: 'relation',
    config: { multiple: false, target_path: '/d2' },
    position: 4
  }
];
const row = (id: string, values: Record<string, unknown>) => ({
  id,
  values,
  version: 1,
  created_at: '',
  updated_at: ''
});
const page = (rows: ReturnType<typeof row>[], next?: string): RowPage => ({ rows, total: rows.length, next });

function html(props: { page: RowPage | null } & Record<string, unknown>): string {
  return render(TableView, {
    props: { path: '/d', fields, mayWrite: false, sort: null, filter: null, error: null, labels: {}, ...props }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('TableView', () => {
  it('renders typed cells under the field labels', () => {
    const out = html({ page: page([row('1', { name: 'Ada', alter: 36.5, url: 'https://x.de' })]) });
    expect(out).toContain('<th');
    expect(out).toContain('Ada');
    expect(out).toContain('36,5');
    expect(out).toContain('href="https://x.de"');
  });

  it('renders markup in a cell as text', () => {
    const out = html({ page: page([row('1', { name: '<img src=x onerror=alert(1)>' })]) });
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img');
  });

  it('puts a sort link in each header and marks the active one', () => {
    const out = html({ page: page([]), sort: { sort: 'alter', desc: false } });
    expect(out).toContain('href="?sort=name&amp;desc=0"');
    expect(out).toContain('aria-sort="ascending"');
    expect(out).toContain('href="?sort=alter&amp;desc=1"');
  });

  it('shows the active filter as a chip with a way to remove it', () => {
    const out = html({ page: page([]), filter: { key: 'name', value: 'Ad' } });
    expect(out).toContain('Name enthält');
    expect(out).toContain('Ad');
    expect(out).toContain('Filter entfernen');
  });

  it('offers load more only while there is a cursor', () => {
    expect(html({ page: page([row('1', {})], 'cur') })).toContain('Mehr laden');
    expect(html({ page: page([row('1', {})]) })).not.toContain('Mehr laden');
  });

  it('says so when there are no rows', () => {
    expect(html({ page: page([]) })).toContain('Noch keine Zeilen');
    expect(html({ page: page([]), filter: { key: 'name', value: 'q' } })).toContain('Keine Zeile passt');
  });

  it('shows an error cell with its reason as visible text', () => {
    const out = html({ page: page([row('1', { calc: { error: '#ERR(divide by zero)' } })]) });
    expect(out).toContain('#ERR(divide by zero)');
    expect(out).toMatch(/<span class="err[^"]*">#ERR\(divide by zero\)<\/span>/);
  });

  it('shows linked rows as label chips when it has their labels', () => {
    const out = html({ page: page([row('1', { wer: 'r1' })]), labels: { wer: { r1: 'Ada' } } });
    expect(out).toContain('Ada');
    expect(out).not.toContain('verknüpft');
  });

  it('counts linked ids when no label map arrived', () => {
    expect(html({ page: page([row('1', { wer: ['r1', 'r2'] })]) })).toContain('2 verknüpft');
  });

  it('states a failed load instead of an empty table', () => {
    const out = html({ page: null, error: 'Die Tabelle konnte nicht geladen werden (Fehler 500).' });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Noch keine Zeilen');
  });

  it('offers the add-row form only to a writer', () => {
    expect(html({ page: page([]), mayWrite: true })).toContain('Zeile hinzufügen');
    expect(html({ page: page([]), mayWrite: false })).not.toContain('Zeile hinzufügen');
  });

  describe('embedded in somebody else\'s page', () => {
    it('draws the headers as plain text with no sort links', () => {
      const out = html({ page: page([]), sort: { sort: 'name', desc: false }, embedded: true });
      expect(out).toContain('Name');
      expect(out).not.toContain('<th scope="col"><a');
      expect(out).not.toContain('aria-sort');
      expect(out).not.toContain('href="?sort=');
    });

    it('renders no filter form and no chip', () => {
      const out = html({ page: page([]), filter: { key: 'name', value: 'Ad' }, embedded: true });
      expect(out).not.toContain('<form class="filter"');
      expect(out).not.toContain('Filtern');
      expect(out).not.toContain('Filter entfernen');
      expect(out).not.toContain('Name enthält');
    });

    it('offers no way to load more or add a row, even with a cursor and a writer', () => {
      const out = html({ page: page([row('1', {})], 'cur'), mayWrite: true, embedded: true });
      expect(out).not.toContain('Mehr laden');
      expect(out).not.toContain('Zeile hinzufügen');
    });

    it('still shows the rows themselves', () => {
      const out = html({ page: page([row('1', { name: 'Ada' })]), embedded: true });
      expect(out).toContain('Ada');
      expect(out).toContain('<td');
    });
  });
});
