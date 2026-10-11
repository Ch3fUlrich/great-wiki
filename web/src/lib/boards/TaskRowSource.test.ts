import { describe, expect, it } from 'vitest';
import { ANONYMOUS, type Me } from '$lib/api';
import type { BoardResponse, BoardTask, TaskStatus } from '$lib/board';
import type { Project } from '$lib/projects';
import { TaskRowSource } from './TaskRowSource';

// Copied from web/src/lib/boards/TaskBoard.test.ts so this source is fed the same wire shape.
function task(over: Partial<BoardTask> = {}): BoardTask {
  return {
    id: 't1',
    title: 'Kabel bestellen',
    status: 'Offen',
    assignee: null,
    assignee_name: null,
    due_at: null,
    position: 0,
    anchored: true,
    page: { path: '/rundgang/tabellen', title: 'Tabellen' },
    detached: false,
    created_at: '2026-08-20 09:00:00',
    updated_at: '2026-08-20 09:00:00',
    ...over
  };
}

function board(tasks: BoardTask[], project: Project | null = null): BoardResponse {
  return {
    project,
    columns: [
      { status: 'Offen', tasks: tasks.filter((entry) => entry.status === 'Offen') },
      { status: 'Läuft', tasks: tasks.filter((entry) => entry.status === 'Läuft') },
      { status: 'Fertig', tasks: tasks.filter((entry) => entry.status === 'Fertig') }
    ]
  };
}

const signedIn: Me = {
  ...ANONYMOUS,
  authenticated: true,
  username: 'sergej',
  display_name: 'Sergej',
  source: 'session'
};

const noop = async (_id: string, _status: TaskStatus): Promise<void> => {};

describe('TaskRowSource columns', () => {
  it('are the three statuses in board order, labelled, and never the none column', () => {
    const cols = TaskRowSource(board([]), signedIn, noop).columns();
    expect(cols.map((c) => c.id)).toEqual(['Offen', 'Läuft', 'Fertig']);
    expect(cols.map((c) => c.label)).toEqual(['Offen', 'Läuft', 'Fertig']);
    expect(cols.every((c) => c.id !== null)).toBe(true);
  });
});

describe('TaskRowSource cards', () => {
  const src = TaskRowSource(
    board([
      task({ id: 'a', title: 'Erstens', status: 'Offen' }),
      task({ id: 'b', title: 'Zweitens', status: 'Offen' }),
      task({ id: 'd', title: 'Drittens', status: 'Läuft', may_write: false }),
      task({ id: 'c', title: 'Viertens', status: 'Fertig', detached: true })
    ]),
    signedIn,
    noop
  );

  it('keeps the API order within a column and the column order across columns', () => {
    expect(src.cards().map((c) => c.id)).toEqual(['a', 'b', 'd', 'c']);
  });

  it('keeps a detached and a read-only card rather than dropping it', () => {
    const ids = src.cards().map((c) => c.id);
    expect(ids).toContain('c');
    expect(ids).toContain('d');
  });

  it('maps a task to a card carrying its status, a null version and the task itself', () => {
    const a = src.cards()[0];
    expect(a.column).toBe('Offen');
    expect(a.version).toBeNull();
    expect((a.meta as { task: BoardTask }).task.title).toBe('Erstens');
  });
});

describe('TaskRowSource canMove', () => {
  it('is false for an anonymous caller', () => {
    const src = TaskRowSource(board([task({ id: 'a', may_write: true })]), ANONYMOUS, noop);
    expect(src.canMove(src.cards()[0])).toBe(false);
  });

  it('is false for a signed-in caller without write', () => {
    const src = TaskRowSource(board([task({ id: 'a', may_write: false })]), signedIn, noop);
    expect(src.canMove(src.cards()[0])).toBe(false);
  });

  it('is true for a signed-in caller who may write', () => {
    const src = TaskRowSource(board([task({ id: 'a', may_write: true })]), signedIn, noop);
    expect(src.canMove(src.cards()[0])).toBe(true);
  });
});

describe('TaskRowSource move', () => {
  it('calls submit exactly once with the id and status, and returns the moved card', async () => {
    const calls: Array<[string, TaskStatus]> = [];
    const src = TaskRowSource(board([task({ id: 'a', status: 'Offen' })]), signedIn, async (id, status) => {
      calls.push([id, status]);
    });
    const card = src.cards()[0];
    const result = await src.move(card, 'Läuft');
    expect(calls).toEqual([['a', 'Läuft']]);
    expect(result.kind).toBe('moved');
    if (result.kind === 'moved') expect(result.card.column).toBe('Läuft');
  });

  it('never calls submit for a read-only card, and refuses', async () => {
    let called = 0;
    const src = TaskRowSource(board([task({ id: 'a', may_write: false })]), signedIn, async () => {
      called += 1;
    });
    const result = await src.move(src.cards()[0], 'Läuft');
    expect(called).toBe(0);
    expect(result.kind).toBe('refused');
  });

  it('refuses a move to no column, without calling submit', async () => {
    let called = 0;
    const src = TaskRowSource(board([task({ id: 'a' })]), signedIn, async () => {
      called += 1;
    });
    const result = await src.move(src.cards()[0], null);
    expect(result.kind).toBe('refused');
    expect(called).toBe(0);
  });

  it('refuses a move to an unknown status, without calling submit', async () => {
    let called = 0;
    const src = TaskRowSource(board([task({ id: 'a' })]), signedIn, async () => {
      called += 1;
    });
    const result = await src.move(src.cards()[0], 'Bla');
    expect(result.kind).toBe('refused');
    expect(called).toBe(0);
  });

  it('returns failed when submit rejects', async () => {
    const src = TaskRowSource(board([task({ id: 'a' })]), signedIn, async () => {
      throw new Error('boom');
    });
    const result = await src.move(src.cards()[0], 'Fertig');
    expect(result).toEqual({ kind: 'failed', message: 'Die Aufgabe konnte nicht verschoben werden.' });
  });
});
