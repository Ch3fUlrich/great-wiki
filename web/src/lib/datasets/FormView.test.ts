import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import FormView from './FormView.svelte';
import type { Field } from './table';
import type { DatasetView } from './views';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'alter', label: 'Alter', kind: 'number', config: {}, position: 1 },
  { key: 'wann', label: 'Wann', kind: 'date', config: {}, position: 2 },
  { key: 'ok', label: 'Fertig', kind: 'bool', config: {}, position: 3 },
  { key: 'sel', label: 'Stufe', kind: 'select', config: { options: [{ id: 'hi', label: 'Hoch' }] }, position: 4 },
  { key: 'calc', label: 'Rechnung', kind: 'formula', config: {}, position: 5 }
];

const view: DatasetView = {
  id: 'v1',
  name: 'Meldebogen',
  kind: 'form',
  config: {
    title: 'Erfassen',
    fields: [{ key: 'name', required: true }, { key: 'alter' }, { key: 'wann' }, { key: 'ok' }, { key: 'sel' }]
  },
  position: 0
};

const html = (props: Record<string, unknown> = {}) =>
  render(FormView, { props: { path: '/d', fields, view, mayWrite: true, ...props } }).body.replace(/<!--.*?-->/g, '');

describe('FormView', () => {
  it('renders one labelled editor per form field, and none for computed ones', () => {
    const out = html();
    expect(out).toContain('type="text"');
    expect(out).toContain('type="number"');
    expect(out).toContain('type="date"');
    expect(out).toContain('<select');
    expect(out).toContain('name="ok"');
    expect(out).toContain('Hoch');
    expect(out).toContain('Stufe');
    expect(out).not.toContain('Rechnung');
  });

  it('marks a required field visibly and in ARIA', () => {
    const out = html();
    expect(out.match(/aria-required="true"/g)?.length).toBe(1);
    expect(out).toContain('<span aria-hidden="true">*</span>');
  });

  it('says the title as text, and falls back to the view name', () => {
    expect(html()).toContain('<h2>Erfassen</h2>');
    const noTitle = { ...view, config: { fields: [{ key: 'name' }] } };
    expect(html({ view: noTitle })).toContain('<h2>Meldebogen</h2>');
  });

  it('escapes a hostile label', () => {
    const hostile = { ...view, config: { fields: [{ key: 'name', label: '<img src=x onerror=1>' }] } };
    const out = html({ view: hostile });
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img src=x onerror=1>');
  });

  it('without write says so and offers no form at all', () => {
    const out = html({ mayWrite: false });
    expect(out).toContain('role="status"');
    expect(out).toContain('Zum Ausfüllen ist Schreibrecht nötig.');
    expect(out).not.toContain('<form');
    expect(out).not.toContain('<input');
  });

  it('offers submit named Absenden', () => {
    expect(html()).toContain('Absenden');
  });
});
