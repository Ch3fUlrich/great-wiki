import { describe, expect, it } from 'vitest';
import {
  cellOf,
  describeRowsFailure,
  filterFrom,
  nextSort,
  rowsApiPath,
  sortHref,
  type Field
} from './table';

const f = (key: string, kind: string): Field => ({ key, label: key, kind, config: {}, position: 0 });

describe('rowsApiPath', () => {
  it('encodes sort, direction, filter and cursor', () => {
    const p = rowsApiPath('/a/b', { sort: 'n', desc: true, filter: { key: 'k', value: 'x y' }, after: 'c=' });
    expect(p.startsWith('/api/datasets/rows/a/b?')).toBe(true);
    const q = new URLSearchParams(p.split('?')[1]);
    expect(q.get('sort')).toBe('n');
    expect(q.get('desc')).toBe('true');
    expect(JSON.parse(q.get('filter')!)).toEqual([{ op: 'contains', key: 'k', value: 'x y' }]);
    expect(q.get('after')).toBe('c=');
  });
  it('asks for nothing extra by default', () => {
    expect(rowsApiPath('/a', {})).toBe('/api/datasets/rows/a');
  });
});

describe('nextSort', () => {
  it('cycles asc, desc for the same key and restarts for another', () => {
    expect(nextSort(null, 'a')).toEqual({ sort: 'a', desc: false });
    expect(nextSort({ sort: 'a', desc: false }, 'a')).toEqual({ sort: 'a', desc: true });
    expect(nextSort({ sort: 'a', desc: true }, 'b')).toEqual({ sort: 'b', desc: false });
  });
  it('builds a link that keeps the filter', () => {
    expect(sortHref({ sort: 'a', desc: false }, 'a', { key: 'k', value: 'v' })).toBe(
      '?sort=a&desc=1&f=k%3Av'
    );
  });
});

describe('filterFrom', () => {
  it('reads key:value and refuses anything else', () => {
    expect(filterFrom('k:v:w', [f('k', 'text')])).toEqual({ key: 'k', value: 'v:w' });
    expect(filterFrom('zz:v', [f('k', 'text')])).toBeNull();
    expect(filterFrom(null, [])).toBeNull();
  });
});

describe('cellOf', () => {
  it('types each kind', () => {
    expect(cellOf(f('a', 'number'), 1234.5)).toEqual({ kind: 'text', text: '1.234,5', align: 'end' });
    expect(cellOf(f('a', 'bool'), true)).toEqual({ kind: 'text', text: 'Ja' });
    expect(cellOf(f('a', 'bool'), false)).toEqual({ kind: 'text', text: 'Nein' });
    expect(cellOf(f('a', 'date'), '2026-10-09')).toEqual({ kind: 'text', text: '09.10.2026' });
    expect(cellOf(f('a', 'tags'), ['x', 'y'])).toEqual({ kind: 'chips', items: ['x', 'y'] });
    expect(cellOf(f('a', 'select'), 'hoch')).toEqual({ kind: 'chips', items: ['hoch'] });
    expect(cellOf(f('a', 'text'), null)).toEqual({ kind: 'text', text: '' });
  });
  it('links only http(s) urls', () => {
    expect(cellOf(f('a', 'url'), 'https://x.de')).toEqual({ kind: 'link', href: 'https://x.de', text: 'https://x.de' });
    expect(cellOf(f('a', 'url'), 'javascript:alert(1)')).toEqual({ kind: 'text', text: 'javascript:alert(1)' });
  });
  it('turns an error value into an error cell with its reason', () => {
    expect(cellOf(f('a', 'formula'), { error: 'Division durch Null' })).toEqual({
      kind: 'error',
      reason: 'Division durch Null'
    });
  });
});

describe('describeRowsFailure', () => {
  it('is German and names the status', () => {
    expect(describeRowsFailure(0)).toContain('keine Antwort');
    expect(describeRowsFailure(500)).toContain('500');
  });
});
