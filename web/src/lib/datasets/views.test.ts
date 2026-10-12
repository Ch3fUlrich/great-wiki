import { describe, expect, it } from 'vitest';
import { boardGroupKey, calendarConfig, galleryConfig, pickView, viewHref, viewsApiPath, type DatasetView } from './views';

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

describe('calendarConfig', () => {
  const cal = (config: unknown): DatasetView => view({ id: 'c1', kind: 'calendar', config });

  it('is the date, end and title of a calendar view', () => {
    expect(calendarConfig(cal({ date: 'start', end: 'ende', title: 'name' }))).toEqual({
      date: 'start',
      end: 'ende',
      title: 'name'
    });
  });

  it('leaves the optional members null when the view does not name them', () => {
    expect(calendarConfig(cal({ date: 'start' }))).toEqual({ date: 'start', end: null, title: null });
    expect(calendarConfig(cal({ date: 'start', end: '', title: 7 }))).toEqual({
      date: 'start',
      end: null,
      title: null
    });
  });

  it('is null for every other kind, even one that carries a date', () => {
    expect(calendarConfig(view({ kind: 'table', config: { date: 'start' } }))).toBeNull();
    expect(calendarConfig(view({ kind: 'form', config: { date: 'start', title: 'Anmelden' } }))).toBeNull();
    expect(calendarConfig(view({ kind: 'board', config: { date: 'start' } }))).toBeNull();
  });

  it('is null without a usable date to place rows by', () => {
    expect(calendarConfig(null)).toBeNull();
    expect(calendarConfig(cal({}))).toBeNull();
    expect(calendarConfig(cal({ date: '' }))).toBeNull();
    expect(calendarConfig(cal({ date: 7 }))).toBeNull();
    expect(calendarConfig(cal({ date: null }))).toBeNull();
    expect(calendarConfig(cal(null))).toBeNull();
    expect(calendarConfig(cal('start'))).toBeNull();
  });
});

describe('galleryConfig', () => {
  const gal = (config: unknown): DatasetView => view({ id: 'g1', kind: 'gallery', config });

  it('is the cover, title and card fields of a gallery view', () => {
    expect(galleryConfig(gal({ cover: 'bild', title: 'name', card_fields: ['ort'] }))).toEqual({
      cover: 'bild',
      title: 'name',
      card_fields: ['ort']
    });
  });

  it('leaves the optional members empty when the view does not name them', () => {
    expect(galleryConfig(gal({ cover: 'bild' }))).toEqual({ cover: 'bild', title: null, card_fields: [] });
    expect(galleryConfig(gal({ cover: 'bild', title: '', card_fields: [7, '', null] }))).toEqual({
      cover: 'bild',
      title: null,
      card_fields: []
    });
    expect(galleryConfig(gal({ cover: 'bild', card_fields: 'ort' }))).toEqual({
      cover: 'bild',
      title: null,
      card_fields: []
    });
  });

  it('is null for every other kind, even one that carries a cover', () => {
    expect(galleryConfig(view({ kind: 'table', config: { cover: 'bild' } }))).toBeNull();
    expect(galleryConfig(view({ kind: 'calendar', config: { cover: 'bild' } }))).toBeNull();
    expect(galleryConfig(view({ kind: 'board', config: { cover: 'bild' } }))).toBeNull();
  });

  it('is null without a cover to draw by', () => {
    expect(galleryConfig(null)).toBeNull();
    expect(galleryConfig(gal({}))).toBeNull();
    expect(galleryConfig(gal({ cover: '' }))).toBeNull();
    expect(galleryConfig(gal({ cover: 7 }))).toBeNull();
    expect(galleryConfig(gal({ cover: null }))).toBeNull();
    expect(galleryConfig(gal(null))).toBeNull();
    expect(galleryConfig(gal('bild'))).toBeNull();
  });
});
