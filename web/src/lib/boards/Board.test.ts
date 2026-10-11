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

describe('cards as a list', () => {
  it('renders each card as an <li> inside a per-column <ul class="karten">', () => {
    const out = html({}, [card('k1', 'a'), card('k2', 'a')]);
    expect(out).toMatch(/<ul class="karten[^"]*"/);
    expect(out).toMatch(/<li[^>]*data-card="k1"/);
    expect(out).toMatch(/<li[^>]*data-card="k2"/);
    expect(out).not.toContain('<article');
  });

  it('renders no <ul> in a column that holds no card', () => {
    const out = html({}, [card('k1', 'a')]);
    const a = out.indexOf('data-column="a"');
    const b = out.indexOf('data-column="b"');
    const n = out.indexOf('data-column="none"');
    expect(out.slice(a, b)).toMatch(/<ul class="karten/);
    expect(out.slice(b, n)).not.toMatch(/<ul class="karten/);
  });
});

describe('cardAttrs and columnAttrs', () => {
  it('spreads cardAttrs onto the <li>, escaping the value', () => {
    const out = html(
      { cardAttrs: (c: BoardCard) => ({ 'data-status': String(c.column), 'data-esc': 'a"b' }) },
      [card('k1', 'a')]
    );
    expect(out).toContain('data-status="a"');
    expect(out).toContain('data-esc="a&quot;b"');
    expect(out).not.toContain('data-esc="a"b"');
  });

  it('cardAttrs cannot make a read-only card draggable or hide its read-only mark', () => {
    const out = html(
      { cardAttrs: () => ({ draggable: 'true', 'data-readonly': 'false', 'data-card': 'other' }) },
      [card('k1', 'a')],
      false
    );
    expect(out).not.toContain('draggable');
    expect(out).toContain('data-card="k1"');
    expect(out).not.toContain('data-card="other"');
    expect(out).not.toContain('data-readonly="false"');
    expect(out).toContain('data-readonly');
  });

  it('spreads columnAttrs onto the column div, escaping the value', () => {
    const out = html({ columnAttrs: (col: BoardColumn) => ({ 'data-spalte': String(col.id), 'data-esc': 'x"y' }) });
    expect(out).toContain('data-spalte="a"');
    expect(out).toContain('data-esc="x&quot;y"');
    expect(out).not.toContain('data-esc="x"y"');
  });
});

describe('the emptyColumn snippet', () => {
  it('renders only in a column with zero cards', () => {
    const out = html({ emptyColumn: snippet('<p data-leer-spalte>Nichts hier.</p>') }, [card('k1', 'a')]);
    const a = out.indexOf('data-column="a"');
    const b = out.indexOf('data-column="b"');
    const n = out.indexOf('data-column="none"');
    expect(out.slice(a, b)).not.toContain('data-leer-spalte');
    expect(out.slice(b, n)).toContain('data-leer-spalte');
    expect(out.match(/data-leer-spalte/g)).toHaveLength(2);
  });

  it('renders nothing in an empty column by default', () => {
    expect(html({}, [card('k1', 'a')])).not.toContain('Nichts hier');
  });
});
