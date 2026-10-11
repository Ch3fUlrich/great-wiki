import { describe, expect, it } from 'vitest';
import { boardGroupKey, pickView, viewHref, viewsApiPath, type DatasetView } from './views';

const view = (over: Partial<DatasetView> = {}): DatasetView => ({
  id: 'v1',
  name: 'Status',
  kind: 'board',
  config: { group_by: 'status' },
  position: 0,
  ...over
});

describe('viewsApiPath', () => {
  it('builds the views endpoint without the leading slash', () => {
    expect(viewsApiPath('/rundgang/t')).toBe('/api/datasets/views/rundgang/t');
    expect(viewsApiPath('t')).toBe('/api/datasets/views/t');
    expect(viewsApiPath('//t')).toBe('/api/datasets/views/t');
  });
});

describe('viewHref', () => {
  it('points at a view by encoded id', () => {
    expect(viewHref('v1')).toBe('?view=v1');
    expect(viewHref('zwei eins')).toBe('?view=zwei%20eins');
  });

  it('points at the bare path for the table', () => {
    // '?' and not '': an empty relative reference KEEPS the current query (RFC 3986 §5.4),
    // so '' could never leave an active view. Same convention as `withoutFilterHref`.
    expect(viewHref(null)).toBe('?');
  });
});

describe('pickView', () => {
  const views = [view(), view({ id: 'v2', kind: 'table' })];

  it('returns the view the id names', () => {
    expect(pickView(views, 'v2')).toBe(views[1]);
  });

  it('an unknown id falls back to the table', () => {
    expect(pickView(views, 'nope')).toBeNull();
  });

  it('no id is the table', () => {
    expect(pickView(views, null)).toBeNull();
    expect(pickView([], 'v1')).toBeNull();
  });
});

describe('boardGroupKey', () => {
  it('is the group_by of a board view', () => {
    expect(boardGroupKey(view())).toBe('status');
  });

  it('is null for every other kind, even one that carries a group_by', () => {
    expect(boardGroupKey(view({ kind: 'table' }))).toBeNull();
    expect(boardGroupKey(view({ kind: 'form' }))).toBeNull();
  });

  it('is null when there is no usable group_by', () => {
    expect(boardGroupKey(null)).toBeNull();
    expect(boardGroupKey(view({ config: {} }))).toBeNull();
    expect(boardGroupKey(view({ config: { group_by: '' } }))).toBeNull();
    expect(boardGroupKey(view({ config: { group_by: 7 } }))).toBeNull();
    expect(boardGroupKey(view({ config: null }))).toBeNull();
    expect(boardGroupKey(view({ config: 'x' }))).toBeNull();
  });
});
