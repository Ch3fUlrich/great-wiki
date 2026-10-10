import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import RowEditor from './RowEditor.svelte';
import type { Field, Row } from './table';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'alter', label: 'Alter', kind: 'number', config: {}, position: 1 },
  { key: 'ok', label: 'Fertig', kind: 'bool', config: {}, position: 2 },
  { key: 'tag', label: 'Datum', kind: 'date', config: {}, position: 3 },
  { key: 'url', label: 'Seite', kind: 'url', config: {}, position: 4 },
  { key: 'sel', label: 'Stufe', kind: 'select', config: { options: [{ id: 'hi', label: 'Hoch' }, { id: 'lo', label: 'Niedrig' }] }, position: 5 },
  { key: 'mul', label: 'Mehrfach', kind: 'multi_select', config: { options: [{ id: 'x', label: 'Ex' }, { id: 'y', label: 'Why' }] }, position: 6 },
  { key: 'tgs', label: 'Schlagworte', kind: 'tags', config: {}, position: 7 },
  { key: 'calc', label: 'Rechnung', kind: 'formula', config: {}, position: 5 }
];
const row: Row = {
  id: 'r1',
  values: { name: '<b>Ada</b>', alter: 36.5, ok: true, tag: '2026-10-10' },
  version: 2,
  created_at: '',
  updated_at: ''
};
const html = (props: Record<string, unknown>) =>
  render(RowEditor, { props: { path: '/d', fields, row: null, ...props } }).body.replace(/<!--.*?-->/g, '');

describe('RowEditor', () => {
  it('offers one typed control per editable field and none for computed ones', () => {
    const out = html({});
    expect(out).toContain('type="number"');
    expect(out).toContain('type="date"');
    expect(out).toContain('type="url"');
    expect(out).toContain('Name');
    expect(out).not.toContain('Rechnung');
  });

  it('prefills an existing row and escapes markup in it', () => {
    const out = html({ row });
    expect(out).toContain('value="&lt;b>Ada&lt;/b>"');
    expect(out).not.toContain('<b>Ada');
    expect(out).toContain('36.5');
    expect(out).toContain('2026-10-10');
  });

  it('renders options as labels and marks the stored choice', () => {
    const out = html({ row: { ...row, values: { sel: 'lo', mul: ['y'], tgs: ['a', 'b'] } } });
    expect(out).toContain('Hoch');
    expect(out).toMatch(/<option value="lo"[^>]*selected/);
    expect(out).toMatch(/value="y"[^>]*checked|checked[^>]*value="y"/);
    expect(out).toContain('value="a, b"');
  });

  it('names the action by mode', () => {
    expect(html({})).toContain('Zeile hinzufügen');
    expect(html({ row })).toContain('Zeile bearbeiten');
  });
});
