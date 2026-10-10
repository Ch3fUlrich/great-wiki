import { describe, expect, it } from 'vitest';
import { fromRaw, toRaw } from './edit';

const ok = (v: unknown) => ({ ok: true, value: v });
const bad = expect.objectContaining({ ok: false });

describe('fromRaw scalar kinds', () => {
  it('text keeps the string, empty clears with null', () => {
    expect(fromRaw('text', {}, 'Ada')).toEqual(ok('Ada'));
    expect(fromRaw('text', {}, '')).toEqual(ok(null));
  });
  it('number is a number, comma accepted, garbage refused', () => {
    expect(fromRaw('number', {}, '36,5')).toEqual(ok(36.5));
    expect(fromRaw('number', {}, '7')).toEqual(ok(7));
    expect(fromRaw('number', {}, 'abc')).toEqual(bad);
    expect(fromRaw('number', {}, 'Infinity')).toEqual(bad);
    expect(fromRaw('number', {}, '')).toEqual(ok(null));
  });
  it('bool is a boolean, empty clears', () => {
    expect(fromRaw('bool', {}, 'true')).toEqual(ok(true));
    expect(fromRaw('bool', {}, 'false')).toEqual(ok(false));
    expect(fromRaw('bool', {}, '')).toEqual(ok(null));
  });
  it('date is YYYY-MM-DD', () => {
    expect(fromRaw('date', {}, '2026-10-10')).toEqual(ok('2026-10-10'));
    expect(fromRaw('date', {}, '10.10.2026')).toEqual(bad);
  });
  it('url allows http, https, mailto only', () => {
    expect(fromRaw('url', {}, 'https://x.de')).toEqual(ok('https://x.de'));
    expect(fromRaw('url', {}, 'mailto:a@b.de')).toEqual(ok('mailto:a@b.de'));
    expect(fromRaw('url', {}, 'javascript:alert(1)')).toEqual(bad);
    expect(fromRaw('url', {}, 'https://a b')).toEqual(bad);
  });
  it('refuses a computed kind', () => {
    expect(fromRaw('formula', {}, '1')).toEqual(bad);
  });
});

describe('fromRaw choice kinds', () => {
  const cfg = { options: [{ id: 'a', label: 'A' }, { id: 'b', label: 'B' }] };
  it('select emits the option id, refuses an unknown one, empty clears', () => {
    expect(fromRaw('select', cfg, 'a')).toEqual(ok('a'));
    expect(fromRaw('select', cfg, 'zz')).toEqual(bad);
    expect(fromRaw('select', cfg, '')).toEqual(ok(null));
  });
  it('multi_select emits a deduped id array, refuses unknown, empty list clears', () => {
    expect(fromRaw('multi_select', cfg, ['a', 'b', 'a'])).toEqual(ok(['a', 'b']));
    expect(fromRaw('multi_select', cfg, ['a', 'zz'])).toEqual(bad);
    expect(fromRaw('multi_select', cfg, [])).toEqual(ok(null));
  });
  it('tags split on commas into a trimmed deduped string array', () => {
    expect(fromRaw('tags', {}, ' x, y ,x,, z')).toEqual(ok(['x', 'y', 'z']));
    expect(fromRaw('tags', {}, 'a'.repeat(129))).toEqual(bad);
    expect(fromRaw('tags', {}, ' , ')).toEqual(ok(null));
  });
  it('toRaw returns the id, the id array, or comma text', () => {
    expect(toRaw('select', 'a')).toBe('a');
    expect(toRaw('multi_select', ['a', 'b'])).toEqual(['a', 'b']);
    expect(toRaw('multi_select', null)).toEqual([]);
    expect(toRaw('tags', ['x', 'y'])).toBe('x, y');
  });
});

describe('toRaw', () => {
  it('shows stored values as editable strings', () => {
    expect(toRaw('number', 36.5)).toBe('36.5');
    expect(toRaw('bool', true)).toBe('true');
    expect(toRaw('text', null)).toBe('');
    expect(toRaw('date', '2026-10-10')).toBe('2026-10-10');
  });
});
