import { describe, expect, it } from 'vitest';
import { embedApiPath, embeddable, parseEmbedAnswer } from './embed';

const VIEW = { id: 'v1', name: 'Alle', kind: 'table', config: { fields: ['name'] }, position: 0 };

describe('parseEmbedAnswer', () => {
  it('keeps a well-formed ok answer, view and all', () => {
    const out = parseEmbedAnswer({ state: 'ok', path: '/raum/tabelle', title: 'Tabelle', view: VIEW });
    expect(out).toEqual({
      state: 'ok',
      path: '/raum/tabelle',
      title: 'Tabelle',
      view: VIEW
    });
  });

  it('passes the view through so its id and kind survive to the loader', () => {
    const board = { state: 'ok', path: '/p', title: 'T', view: { ...VIEW, kind: 'board' } };
    const out = parseEmbedAnswer(board);
    expect(out.state === 'ok' && out.view.id).toBe('v1');
    expect(out.state === 'ok' && out.view.kind).toBe('board');
  });

  it('reads the view_missing answer as itself', () => {
    expect(parseEmbedAnswer({ state: 'view_missing' })).toEqual({ state: 'view_missing' });
  });

  it('reads the uniform refusal as itself', () => {
    expect(parseEmbedAnswer({ state: 'unavailable' })).toEqual({ state: 'unavailable' });
  });

  // Every malformed shape is the SAME answer, so a broken response cannot say anything about
  // what the wiki holds — ADR 0022's rule, applied by the reader.
  const malformed: [string, unknown][] = [
    ['null', null],
    ['undefined', undefined],
    ['a number', 7],
    ['a string', 'ok'],
    ['an array', [{ state: 'ok' }]],
    ['no state', { path: '/p', title: 'T' }],
    ['an unknown state', { state: 'perhaps' }],
    ['ok with no path', { state: 'ok', title: 'T', view: VIEW }],
    ['ok with an empty path', { state: 'ok', path: '', title: 'T', view: VIEW }],
    ['ok with a non-string path', { state: 'ok', path: 5, title: 'T', view: VIEW }],
    ['ok with no title', { state: 'ok', path: '/p', view: VIEW }],
    ['ok with a non-string title', { state: 'ok', path: '/p', title: 9, view: VIEW }],
    ['ok with no view', { state: 'ok', path: '/p', title: 'T' }],
    ['ok with a view that is not an object', { state: 'ok', path: '/p', title: 'T', view: 'v1' }],
    ['ok with a null view', { state: 'ok', path: '/p', title: 'T', view: null }],
    ['ok with a view missing its id', { state: 'ok', path: '/p', title: 'T', view: { kind: 'table' } }],
    ['ok with a view missing its kind', { state: 'ok', path: '/p', title: 'T', view: { id: 'v1' } }],
    ['ok with an empty view id', { state: 'ok', path: '/p', title: 'T', view: { id: '', kind: 'table' } }],
    [
      'ok with a non-string view kind',
      { state: 'ok', path: '/p', title: 'T', view: { id: 'v1', kind: 3 } }
    ]
  ];
  for (const [what, json] of malformed) {
    it(`collapses ${what} to unavailable`, () => {
      expect(parseEmbedAnswer(json)).toEqual({ state: 'unavailable' });
    });
  }

  it('never throws, whatever is handed it', () => {
    for (const json of [Symbol('x'), () => {}, new Map(), 0, '', [], {}]) {
      expect(() => parseEmbedAnswer(json)).not.toThrow();
    }
  });
});

describe('embedApiPath', () => {
  it('builds the embed address under the API', () => {
    expect(embedApiPath('doc-1', 'view-1')).toBe('/api/datasets/embed/doc-1/view-1');
  });

  it('encodes a hostile id so it cannot escape its one segment', () => {
    const path = embedApiPath('../../etc/passwd?a=1#frag', 'v/..?x#y');
    expect(path).toBe(
      `/api/datasets/embed/${encodeURIComponent('../../etc/passwd?a=1#frag')}/${encodeURIComponent('v/..?x#y')}`
    );
    // No raw slash beyond the fixed prefix, no query or fragment introduced by the id.
    expect(path.slice('/api/datasets/embed'.length).split('/')[1]).not.toContain('?');
    expect(path).not.toContain('#');
    expect(path).not.toContain('?');
  });
});

describe('embeddable', () => {
  it('admits the four read-only kinds', () => {
    for (const kind of ['table', 'board', 'calendar', 'gallery']) {
      expect(embeddable(kind)).toBe(true);
    }
  });

  it('refuses a form and anything it does not know', () => {
    for (const kind of ['form', 'timeline', '', 'Table', 'boardish']) {
      expect(embeddable(kind)).toBe(false);
    }
  });
});
