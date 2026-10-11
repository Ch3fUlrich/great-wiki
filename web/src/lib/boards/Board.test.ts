import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { createRawSnippet } from 'svelte';
import Board from './Board.svelte';
import type { BoardCard, BoardColumn, RowSource } from './RowSource';

const columns: BoardColumn[] = [
  { id: 'a', label: 'Spalte A' },
  { id: 'b', label: 'Spalte B' },
  { id: null, label: 'Ohne Wert' }
];
const card = (id: string, column: string | null, title = id): BoardCard => ({ id, title, column, version: 1 });

function src(cards: BoardCard[], writable = true): RowSource {
  return {
    columns: () => columns,
    cards: () => cards,
    canMove: () => writable,
    move: async (c, col) => ({ kind: 'moved', card: { ...c, column: col } })
  };
}
const snippet = (inhalt: string) => createRawSnippet(() => ({ render: () => inhalt }));

function html(props: Record<string, unknown> = {}, cards = [card('k1', 'a')], writable = true): string {
  return render(Board, { props: { source: src(cards, writable), ...props } }).body.replace(/<!--.*?-->/g, '');
}

describe('Board columns', () => {
  it('renders every column in source order with the empty none column last', () => {
    const out = html({}, [card('k1', 'a')]);
    expect(out).toContain('data-column="a"');
    expect(out).toContain('data-column="b"');
    expect(out).toContain('data-column="none"');
    expect(out.indexOf('data-column="a"')).toBeLessThan(out.indexOf('data-column="none"'));
    expect(out.indexOf('data-column="a"')).toBeLessThan(out.indexOf('data-column="none"'));
    expect(out).toContain('Ohne Wert');
    const b = out.indexOf('data-column="b"');
    expect(out.slice(b, out.indexOf('data-column="none"'))).not.toContain('data-card');
  });

  it('puts each card in the column div its current column names', () => {
    const out = html({}, [card('k1', 'a'), card('k2', 'b')]);
    const a = out.indexOf('data-column="a"');
    const b = out.indexOf('data-column="b"');
    const n = out.indexOf('data-column="none"');
    expect(out.indexOf('data-card="k1"')).toBeGreaterThan(a);
    expect(out.indexOf('data-card="k1"')).toBeLessThan(b);
    expect(out.indexOf('data-card="k2"')).toBeGreaterThan(b);
    expect(out.indexOf('data-card="k2"')).toBeLessThan(n);
  });

  it('routes a card whose column the source does not list into the none column', () => {
    const out = html({}, [card('k1', 'gesperrt')]);
    const n = out.indexOf('data-column="none"');
    expect(out.indexOf('data-card="k1"')).toBeGreaterThan(n);
  });
});

describe('a card', () => {
  it('movable cards are draggable and get the move-to select', () => {
    const out = html();
    expect(out).toContain('draggable="true"');
    expect(out).toContain('<select aria-label="Verschieben nach"');
    expect(out).toMatch(/<option value="a" selected(="")?>/);
    expect(out.match(/<option/g)).toHaveLength(3);
    expect(out).toContain('data-board-message');
  });

  it('a read-only card is shown, marked data-readonly, undraggable and without a select', () => {
    const out = html({}, [card('k1', 'a')], false);
    expect(out).toContain('data-card="k1"');
    expect(out).toContain('data-readonly');
    expect(out).not.toContain('draggable');
    expect(out).not.toContain('<select');
  });

  it('renders a title as escaped text, never as markup', () => {
    const out = html({}, [card('k1', 'a', '<img src=x onerror=1>')]);
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img');
  });

  it('the actions snippet replaces the built-in select', () => {
    const out = html({ actions: snippet('<em data-custom-actions>custom</em>') });
    expect(out).toContain('data-custom-actions');
    expect(out).not.toContain('<select');
  });

  it('the card snippet replaces the default card body', () => {
    const out = html({ card: snippet('<em data-custom-card>jk</em>') }, [card('k1', 'a', 'Jemand')]);
    expect(out).toContain('data-custom-card');
    expect(out).not.toContain('Jemand');
  });

  it('the columnHead snippet replaces the default heading', () => {
    const out = html({ columnHead: snippet('<h4 data-custom-head>Kopf</h4>') });
    expect(out).toContain('data-custom-head');
    expect(out).not.toContain('<h3');
  });
});

describe('the empty snippet', () => {
  it('shows only when every column is empty', () => {
    const leere = { empty: snippet('<p data-leer>Alles leer</p>') };
    expect(html(leere, [])).toContain('data-leer');
    expect(html(leere, [card('k1', 'a')])).not.toContain('data-leer');
  });
});
