import { describe, expect, it, vi } from 'vitest';
import type { Row } from '../table';
import {
  MISSING_LABEL,
  RELATION_FETCH_FAILURE,
  fetchTargetRows,
  labelMap,
  pickerOptions,
  relationConfig,
  rowLabel,
  toggleId
} from './relation';

const row = (id: string, values: Record<string, unknown>): Row => ({
  id,
  values,
  version: 1,
  created_at: '',
  updated_at: ''
});

describe('relationConfig', () => {
  it('reads multiple and the target path from the schema config', () => {
    expect(relationConfig({ multiple: true, target_path: '/a/b' })).toEqual({
      multiple: true,
      targetPath: '/a/b'
    });
    expect(relationConfig({ target_path: '/a' })).toEqual({ multiple: false, targetPath: '/a' });
  });
  it('is nothing without a readable target path', () => {
    expect(relationConfig({ multiple: false })).toBeNull();
    expect(relationConfig(null)).toBeNull();
    expect(relationConfig('garbage')).toBeNull();
  });
});

describe('rowLabel', () => {
  it('is the first non-empty string value, else the first number, else the id', () => {
    expect(rowLabel(row('1', { name: 'Ada', alter: 36 }))).toBe('Ada');
    expect(rowLabel(row('1', { name: '  ', note: 'Hi' }))).toBe('Hi');
    expect(rowLabel(row('1', { alter: 0 }))).toBe('0');
    // key-sorted maps put the number first; the text still names the row
    expect(rowLabel(row('1', { alter: 7, name: 'Zenta' }))).toBe('Zenta');
    expect(rowLabel(row('1', { ok: true, tags: ['x'] }))).toBe('1');
    expect(rowLabel(row('1', {}))).toBe('1');
  });
  it('maps rows to labels by id', () => {
    expect(labelMap([row('a', { name: 'Ada' }), row('b', {})])).toEqual({ a: 'Ada', b: 'b' });
  });
});

describe('pickerOptions', () => {
  const rows = [row('a', { name: 'Ada' }), row('b', { name: 'Bo' })];
  it('lists exactly the rows it was given', () => {
    expect(pickerOptions(rows, [])).toEqual([
      { id: 'a', label: 'Ada' },
      { id: 'b', label: 'Bo' }
    ]);
  });
  it('adds a selected id that is no longer among them, once, as a placeholder', () => {
    expect(pickerOptions(rows, ['gone', 'a', 'gone'])).toEqual([
      { id: 'a', label: 'Ada' },
      { id: 'b', label: 'Bo' },
      { id: 'gone', label: MISSING_LABEL }
    ]);
  });
});

describe('toggleId', () => {
  it('adds without duplicating and removes', () => {
    expect(toggleId(['a'], 'b', true)).toEqual(['a', 'b']);
    expect(toggleId(['a'], 'a', true)).toEqual(['a']);
    expect(toggleId(['a', 'b'], 'a', false)).toEqual(['b']);
  });
});

describe('fetchTargetRows', () => {
  it('asks the rows endpoint for the target once and returns its page', async () => {
    const seen: string[] = [];
    const f = vi.fn(async (url: string | URL | Request) => {
      seen.push(String(url));
      return new Response(JSON.stringify({ rows: [{ id: 'a', values: {}, version: 1, created_at: '', updated_at: '' }], total: 1 }), {
        status: 200,
        headers: { 'content-type': 'application/json' }
      });
    }) as unknown as typeof fetch;
    const rows = await fetchTargetRows(f, '/d2');
    expect(rows.map((r) => r.id)).toEqual(['a']);
    expect(seen).toEqual(['/api/datasets/rows/d2']);
  });
  it('throws on a failing target so the component can state it', async () => {
    const f = vi.fn(async () => new Response('no', { status: 500 })) as unknown as typeof fetch;
    await expect(fetchTargetRows(f, '/d2')).rejects.toThrow();
  });
});

describe('messages', () => {
  it('are German', () => {
    expect(RELATION_FETCH_FAILURE).toBe('Die Zielzeilen konnten nicht geladen werden.');
    expect(MISSING_LABEL).toBe('(nicht mehr vorhanden)');
  });
});
