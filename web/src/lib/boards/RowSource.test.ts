import { describe, expect, it } from 'vitest';
import { groupCards, type BoardCard, type BoardColumn, type MoveResult, type RowSource } from './RowSource';
import { DatasetRowSource } from './DatasetRowSource';
import type { Send } from '../datasets/rowedit';
import type { Field, Row } from '../datasets/table';

const card = (id: string, column: string | null): BoardCard => ({ id, title: id, column, version: 1 });

function spy(answer: { status: number; message?: string | null; body?: unknown }) {
  const calls: { method: string; path: string; body: unknown }[] = [];
  const send: Send = async (method, path, body) => {
    calls.push({ method, path, body });
    return { status: answer.status, message: answer.message ?? null, body: answer.body ?? null };
  };
  return { send, calls };
}

describe('RowSource contract (fake source)', () => {
  function fake(writable: boolean, moves: MoveResult[] = []): RowSource {
    const columns: BoardColumn[] = [
      { id: 'a', label: 'A' },
      { id: 'b', label: 'B' },
      { id: null, label: 'Ohne Wert' }
    ];
    const cards = [card('1', 'a'), card('2', 'b'), card('3', null)];
    return {
      columns: () => columns,
      cards: () => cards,
      canMove: () => writable,
      move: async (c, col) =>
        !writable ? { kind: 'refused', message: 'not allowed' } : (moves.shift() ?? { kind: 'moved', card: { ...c, column: col } })
    };
  }

  it('groups by its own columns, source order, none dropped', () => {
    const groups = groupCards(fake(true));
    expect([...groups.keys()]).toEqual(['a', 'b', null]);
    expect(groups.get('a')!.map((c) => c.id)).toEqual(['1']);
    expect(groups.get('b')!.map((c) => c.id)).toEqual(['2']);
    expect(groups.get(null)!.map((c) => c.id)).toEqual(['3']);
  });

  it('resolves a move through the contract', async () => {
    expect(await fake(true).move(card('1', 'a'), 'b')).toEqual({ kind: 'moved', card: card('1', 'b') });
  });

  it('a non-movable source refuses', async () => {
    expect((await fake(false).move(card('1', 'a'), 'b')).kind).toBe('refused');
  });
});

describe('groupCards', () => {
  it('keeps empty columns, routes unknown cards to the null column, never drops or duplicates', () => {
    const source: RowSource = {
      columns: () => [
        { id: 'todo', label: 'Todo' },
        { id: 'doing', label: 'Doing' },
        { id: 'done', label: 'Done' },
        { id: null, label: 'Ohne Wert' }
      ],
      cards: () => [card('1', 'todo'), card('2', 'zzz'), card('3', 'todo')],
      canMove: () => true,
      move: async (c, col) => ({ kind: 'moved', card: { ...c, column: col } })
    };
    const groups = groupCards(source);
    expect([...groups.keys()]).toEqual(['todo', 'doing', 'done', null]);
    expect(groups.get('todo')!.map((c) => c.id)).toEqual(['1', '3']);
    expect(groups.get('doing')).toEqual([]);
    expect(groups.get(null)!.map((c) => c.id)).toEqual(['2']);
    expect([...groups.values()].flat().map((c) => c.id)).toEqual(['1', '3', '2']);
  });
});

describe('DatasetRowSource', () => {
  const fields: Field[] = [
    { key: 'status', label: 'Status', kind: 'select', config: { options: [{ id: 'todo', label: 'Todo' }, { id: 'done', label: 'Done' }] }, position: 1 },
    { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 }
  ];
  const row = (id: string, values: Record<string, unknown>, version = 3): Row => ({
    id,
    values,
    version,
    created_at: '',
    updated_at: ''
  });
  const mk = (send: Send, writable = true, rows: Row[] = [row('r1', { name: 'Ada', status: 'todo' })]) =>
    DatasetRowSource(fields, rows, 'status', send, '/d1', writable);

  it('rejects a groupKey that is not a select field of the dataset', () => {
    const { send } = spy({ status: 200 });
    expect(() => DatasetRowSource(fields, [], 'name', send, '/d1', true)).toThrow(
      'group_by must be a select field of the dataset'
    );
    expect(() => DatasetRowSource(fields, [], 'nope', send, '/d1', true)).toThrow(
      'group_by must be a select field of the dataset'
    );
  });

  it('columns follow config order with the null column last', () => {
    expect(mk(spy({ status: 200 }).send).columns()).toEqual([
      { id: 'todo', label: 'Todo' },
      { id: 'done', label: 'Done' },
      { id: null, label: 'Ohne Wert' }
    ]);
  });

  it('cards take the first text field by position, else the row id', () => {
    const cards = mk(spy({ status: 200 }).send, true, [
      row('r1', { name: 'Ada', status: 'todo' }),
      row('r2', { status: 'done' })
    ]).cards();
    expect(cards.map((c) => c.title)).toEqual(['Ada', 'r2']);
    expect(cards[1].version).toBe(3);
  });

  it('a read-only source refuses without sending', async () => {
    const { send, calls } = spy({ status: 200 });
    expect((await mk(send, false).move(card('r1', 'todo'), 'done')).kind).toBe('refused');
    expect(calls).toEqual([]);
  });

  it('a move sends exactly one PATCH with id, version and the one value', async () => {
    const { send, calls } = spy({ status: 200 });
    const result = await mk(send).move(card('r1', 'todo'), 'done');
    expect(calls).toEqual([
      { method: 'PATCH', path: '/api/datasets/rows/d1', body: { id: 'r1', version: 1, values: { status: 'done' } } }
    ]);
    expect(result).toEqual({ kind: 'moved', card: { ...card('r1', 'todo'), column: 'done' } });
  });

  it('moving to the null column sends null', async () => {
    const { send, calls } = spy({ status: 200 });
    await mk(send).move(card('r1', 'todo'), null);
    expect(calls[0].body).toEqual({ id: 'r1', version: 1, values: { status: null } });
  });

  it('409 answers stale with the current row as a card', async () => {
    const { send } = spy({ status: 409, body: { current: row('r1', { name: 'Ada', status: 'done' }, 9) } });
    const result = await mk(send).move(card('r1', 'todo'), 'done');
    expect(result.kind).toBe('stale');
    if (result.kind === 'stale') expect(result.current).toEqual({ id: 'r1', title: 'Ada', column: 'done', version: 9 });
  });

  it('403 answers failed with a message', async () => {
    const { send } = spy({ status: 403 });
    const result = await mk(send).move(card('r1', 'todo'), 'done');
    expect(result.kind).toBe('failed');
    if (result.kind === 'failed') expect(result.message).toContain('403');
  });
});
