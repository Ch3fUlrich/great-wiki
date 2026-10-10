import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import TableView from './TableView.svelte';
import type { Field, RowPage } from './table';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'alter', label: 'Alter', kind: 'number', config: {}, position: 1 },
  { key: 'url', label: 'Seite', kind: 'url', config: {}, position: 2 },
  { key: 'calc', label: 'Rechnung', kind: 'formula', config: {}, position: 3 }
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
    props: { path: '/d', fields, mayWrite: false, sort: null, filter: null, error: null, ...props }
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

  it('shows an error cell with its reason as text', () => {
    const out = html({ page: page([row('1', { calc: { error: 'Division durch Null' } })]) });
    expect(out).toContain('#ERR');
    expect(out).toContain('Division durch Null');
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
});
