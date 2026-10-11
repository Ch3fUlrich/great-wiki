import { describe, expect, it } from 'vitest';
import { applyMove, type BoardView } from './move';
import type { BoardCard, BoardColumn, MoveResult, RowSource } from './RowSource';

const card = (id: string, column: string | null): BoardCard => ({ id, title: id, column, version: 1 });
const viewOf = (cards: BoardCard[]): BoardView => ({
  columnOf: new Map(cards.map((c) => [c.id, c.column])),
  message: null
});
function fake(result: MoveResult, writable = true): { source: RowSource; calls: [string, string | null][] } {
  const calls: [string, string | null][] = [];
  const columns: BoardColumn[] = [{ id: 'b', label: 'B' }, { id: null, label: 'Ohne Wert' }];
  return {
    calls,
    source: {
      columns: () => columns,
      cards: () => [card('1', 'a')],
      canMove: () => writable,
      move: async (c, col) => {
        calls.push([c.id, col]);
        return result;
      }
    }
  };
}

describe('applyMove', () => {
  it('a moved result updates the column, clears the message and calls onMoved', async () => {
    const f = fake({ kind: 'moved', card: card('1', 'b') });
    const hooks: { moved: BoardCard[] } = { moved: [] };
    const view = await applyMove(f.source, viewOf([card('1', 'a')]), card('1', 'a'), 'b', {
      onMoved: (c) => hooks.moved.push(c)
    });
    expect(view.columnOf.get('1')).toBe('b');
    expect(view.message).toBeNull();
    expect(hooks.moved).toEqual([card('1', 'b')]);
    expect(f.calls).toEqual([['1', 'b']]);
  });

  it('moving to the null column passes null to source.move', async () => {
    const f = fake({ kind: 'moved', card: card('1', null) });
    const view = await applyMove(f.source, viewOf([card('1', 'a')]), card('1', 'a'), null);
    expect(f.calls).toEqual([['1', null]]);
    expect(view.columnOf.get('1')).toBeNull();
  });

  it('the same column as now does not call source.move', async () => {
    const f = fake({ kind: 'moved', card: card('1', 'a') });
    const view = await applyMove(f.source, viewOf([card('1', 'a')]), card('1', 'a'), 'a');
    expect(f.calls).toEqual([]);
    expect(view.columnOf.get('1')).toBe('a');
  });

  it('a card the source will not move does not call source.move', async () => {
    const f = fake({ kind: 'moved', card: card('1', 'b') }, false);
    const view = await applyMove(f.source, viewOf([card('1', 'a')]), card('1', 'a'), 'b');
    expect(f.calls).toEqual([]);
    expect(view.columnOf.get('1')).toBe('a');
    expect(view.message).toBeNull();
  });

  it('stale keeps the column, says so in words and hands onStale the current card', async () => {
    const cur = card('1', 'b');
    const f = fake({ kind: 'stale', current: cur });
    let seen: BoardCard | null | undefined;
    const view = await applyMove(f.source, viewOf([card('1', 'a')]), card('1', 'a'), 'b', {
      onStale: (c) => (seen = c)
    });
    expect(view.columnOf.get('1')).toBe('a');
    expect(view.message).toBe('Die Zeile wurde inzwischen geändert; bitte neu laden.');
    expect(seen).toEqual(cur);
  });

  it('stale without a current card still announces and calls onStale with null', async () => {
    let seen: BoardCard | null | undefined;
    const view = await applyMove(fake({ kind: 'stale', current: null }).source, viewOf([card('1', 'a')]), card('1', 'a'), 'b', {
      onStale: (c) => (seen = c)
    });
    expect(seen).toBeNull();
    expect(view.message).toBe('Die Zeile wurde inzwischen geändert; bitte neu laden.');
  });

  it('refused and failed keep the column and show the result message', async () => {
    for (const kind of ['refused', 'failed'] as const) {
      const view = await applyMove(fake({ kind, message: `${kind}-msg` }).source, viewOf([card('1', 'a')]), card('1', 'a'), 'b');
      expect(view.columnOf.get('1')).toBe('a');
      expect(view.message).toBe(`${kind}-msg`);
    }
  });

  it('returns a new view; the argument is not mutated', async () => {
    const view = viewOf([card('1', 'a')]);
    const out = await applyMove(fake({ kind: 'moved', card: card('1', 'b') }).source, view, card('1', 'a'), 'b');
    expect(out).not.toBe(view);
    expect(view.columnOf.get('1')).toBe('a');
    expect(view.message).toBeNull();
  });
});
