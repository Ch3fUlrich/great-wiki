import { describe, expect, it } from 'vitest';
import { submitRow, type Send } from './rowedit';
import type { Field, Row } from './table';

const fields: Field[] = [
  { key: 'name', label: 'Name', kind: 'text', config: {}, position: 0 },
  { key: 'alter', label: 'Alter', kind: 'number', config: {}, position: 1 },
  { key: 'ok', label: 'Fertig', kind: 'bool', config: {}, position: 2 },
  { key: 'calc', label: 'Rechnung', kind: 'formula', config: {}, position: 3 }
];
const row: Row = {
  id: 'r1',
  values: { name: 'Ada', alter: 36, ok: true },
  version: 3,
  created_at: '',
  updated_at: ''
};

function spy(answer: { status: number; message?: string | null; body?: unknown }) {
  const calls: { method: string; path: string; body: unknown }[] = [];
  const send: Send = async (method, path, body) => {
    calls.push({ method, path, body });
    return { status: answer.status, message: answer.message ?? null, body: answer.body ?? null };
  };
  return { send, calls };
}

describe('submitRow', () => {
  it('PATCHes only changed cells, typed, with id and version', async () => {
    const { send, calls } = spy({ status: 200 });
    const r = await submitRow(send, '/d', fields, row, { name: 'Ada', alter: '37,5', ok: 'true' });
    expect(r.kind).toBe('saved');
    expect(calls).toEqual([
      { method: 'PATCH', path: '/api/datasets/rows/d', body: { id: 'r1', version: 3, values: { alter: 37.5 } } }
    ]);
  });

  it('clearing a cell sends null', async () => {
    const { send, calls } = spy({ status: 200 });
    await submitRow(send, '/d', fields, row, { name: '', alter: '36', ok: 'true' });
    expect((calls[0].body as { values: unknown }).values).toEqual({ name: null });
  });

  it('creates a row with only the filled cells', async () => {
    const { send, calls } = spy({ status: 201 });
    const r = await submitRow(send, '/d', fields, null, { name: 'Bo', alter: '', ok: '' });
    expect(r.kind).toBe('saved');
    expect(calls[0]).toEqual({ method: 'POST', path: '/api/datasets/rows/d', body: { values: { name: 'Bo' } } });
  });

  it('refuses invalid input without a request', async () => {
    const { send, calls } = spy({ status: 200 });
    const r = await submitRow(send, '/d', fields, row, { name: 'Ada', alter: 'x', ok: 'true' });
    expect(r).toEqual({ kind: 'invalid', errors: { alter: 'Bitte eine Zahl eingeben.' } });
    expect(calls).toEqual([]);
  });

  it('says nothing changed instead of sending', async () => {
    const { send, calls } = spy({ status: 200 });
    const r = await submitRow(send, '/d', fields, row, { name: 'Ada', alter: '36', ok: 'true' });
    expect(r.kind).toBe('unchanged');
    expect(calls).toEqual([]);
  });

  it('409 returns the current row so the draft can be kept', async () => {
    const current = { ...row, version: 4, values: { name: 'Eva' } };
    const { send } = spy({ status: 409, message: 'the row changed', body: { error: 'x', current } });
    const r = await submitRow(send, '/d', fields, row, { name: 'Neu', alter: '36', ok: 'true' });
    expect(r).toEqual({ kind: 'stale', current });
  });

  it('other failures carry a German message', async () => {
    const { send } = spy({ status: 403, message: 'no' });
    const r = await submitRow(send, '/d', fields, row, { name: 'Neu', alter: '36', ok: 'true' });
    expect(r.kind).toBe('failed');
    expect(JSON.stringify(r)).toContain('403');
  });
});
