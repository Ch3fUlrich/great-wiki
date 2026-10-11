import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import BoardView from './BoardView.svelte';
import type { DatasetView } from './views';
import type { Field, RowPage } from './table';

const FIELDS: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  {
    key: 'status',
    label: 'Status',
    kind: 'select',
    config: { options: [{ id: 'offen', label: 'Offen' }, { id: 'fest', label: 'Erledigt' }] },
    position: 1
  }
];
const BOARD: DatasetView = { id: 'b1', name: 'Status', kind: 'board', config: { group_by: 'status' }, position: 0 };
const row = (id: string, values: Record<string, unknown>) => ({ id, values, version: 1, created_at: '', updated_at: '' });
const PAGE: RowPage = { rows: [row('r1', { name: 'Erstes', status: 'offen' }), row('r2', { name: 'Zweites' })], total: 2 };

const html = (props: Record<string, unknown> = {}): string =>
  render(BoardView, {
    props: { path: '/d', fields: FIELDS, page: PAGE, view: BOARD, mayWrite: true, error: null, ...props }
  })
    .body.replace(/<!--.*?-->/g, '')
    .trim();

describe('BoardView', () => {
  it('renders the generic board with one column per select option and "Ohne Wert" last', () => {
    const out = html();
    expect(out).toContain('data-board');
    const offen = out.indexOf('data-column="offen"');
    const fest = out.indexOf('data-column="fest"');
    const none = out.indexOf('data-column="none"');
    expect(offen).toBeGreaterThanOrEqual(0);
    expect(fest).toBeGreaterThan(offen);
    expect(none).toBeGreaterThan(fest);
    expect(out).toContain('Ohne Wert');
  });

  it('puts each row in its column and rows without a value in the last one', () => {
    const out = html();
    const offen = out.indexOf('data-column="offen"');
    const none = out.indexOf('data-column="none"');
    expect(out.indexOf('data-card="r1"')).toBeGreaterThan(offen);
    expect(out.indexOf('data-card="r1"')).toBeLessThan(none);
    expect(out.indexOf('data-card="r2"')).toBeGreaterThan(none);
  });

  it('shows the card titles as text', () => {
    const out = html();
    expect(out).toContain('Erstes');
    expect(out).toContain('Zweites');
  });

  it('a writable board gives draggable cards and the move-to select', () => {
    const out = html();
    expect(out).toContain('draggable="true"');
    expect(out).toContain('<select');
  });

  it('a read-only board shows cards that cannot be dragged and offers no select', () => {
    const out = html({ mayWrite: false });
    expect(out).toContain('data-board');
    expect(out).toContain('data-card="r1"');
    expect(out).toContain('data-readonly');
    expect(out).not.toContain('draggable');
    expect(out).not.toContain('<select');
  });

  it('a group_by that is not a select field says so instead of a board', () => {
    const out = html({ view: { ...BOARD, config: { group_by: 'name' } } });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-board');
  });

  it('a failed read states its error instead of an empty board', () => {
    const out = html({ error: 'Die Tabelle konnte nicht geladen werden (Fehler 500).' });
    expect(out).toContain('role="alert"');
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('data-board');
  });

  it('no rows answered and no error stated is still not an empty board', () => {
    const out = html({ page: null });
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('data-board');
  });
});
