import { describe, expect, it } from 'vitest';
import {
  MAX_QUERY_LENGTH,
  NO_RESULTS,
  describeSearch,
  groupCountText,
  isEmpty,
  isTypingTarget,
  mergeSegments,
  normaliseQuery,
  pageHitHref,
  searchApiPath,
  searchHref,
  segmentsText,
  taskHitHref,
  topicHitHref
} from '$lib/search';

describe('query normalisation', () => {
  it('trims, collapses whitespace and treats blank as empty', () => {
    expect(normaliseQuery('  Darm \t  Flora\n')).toBe('Darm Flora');
    expect(normaliseQuery('   ')).toBe('');
    expect(normaliseQuery(null)).toBe('');
    expect(normaliseQuery(undefined)).toBe('');
  });
  it('composes umlauts so a decomposed spelling searches like a composed one', () => {
    expect(normaliseQuery('Ärzte'.normalize('NFD'))).toBe('Ärzte');
  });
  it('caps the length by characters, not bytes', () => {
    expect(Array.from(normaliseQuery('ä'.repeat(500)))).toHaveLength(MAX_QUERY_LENGTH);
  });
  it('encodes the query into the API path and the page address', () => {
    expect(searchApiPath('a&b c')).toBe('/api/search?q=a%26b+c');
    expect(searchHref('  Gliederung ')).toBe('/suche?q=Gliederung');
    expect(searchHref('  ')).toBe('/suche');
  });
});

describe('segments', () => {
  it('drops empty runs and joins neighbours that agree', () => {
    const merged = mergeSegments([
      { text: 'ab', hit: false },
      { text: '', hit: true },
      { text: 'c', hit: false },
      { text: 'D', hit: true },
      { text: 'e', hit: true }
    ]);
    expect(merged).toEqual([
      { text: 'abc', hit: false },
      { text: 'De', hit: true }
    ]);
  });
  it('does not mutate its input', () => {
    const input = [
      { text: 'a', hit: false },
      { text: 'b', hit: false }
    ];
    mergeSegments(input);
    expect(input[0].text).toBe('a');
  });
  it('keeps markup as text and joins to the plain snippet', () => {
    const s = [{ text: '<b>x</b>', hit: true }];
    expect(mergeSegments(s)[0].text).toBe('<b>x</b>');
    expect(segmentsText(s)).toBe('<b>x</b>');
  });
});

describe('where a hit goes', () => {
  it('sends a page to its path, a topic to its route, a task to its page or the board', () => {
    expect(pageHitHref({ path: '/rundgang/tabellen' })).toBe('/rundgang/tabellen');
    expect(topicHitHref({ path: '/rundgang/tabellen' })).toBe('/themen/rundgang/tabellen');
    expect(taskHitHref({ page_path: '/projekt/a' })).toBe('/projekt/a');
    expect(taskHitHref({ page_path: null })).toBe('/aufgaben');
  });
});

describe('results', () => {
  it('knows empty from non-empty', () => {
    expect(isEmpty(NO_RESULTS)).toBe(true);
    const topic = { name: 'a', display_path: 'a', path: '/a', documents: 1 };
    expect(isEmpty({ ...NO_RESULTS, topics: [topic] })).toBe(false);
  });
  it('says only the length of the list it was handed', () => {
    expect(groupCountText(1)).toBe('1 Treffer');
    expect(groupCountText(3)).toBe('3 Treffer');
    expect(describeSearch(0)).toContain('nicht erreichbar');
    expect(describeSearch(500)).toContain('500');
  });
});

describe('the / shortcut', () => {
  it('is not taken while typing', () => {
    expect(isTypingTarget({ tagName: 'INPUT' })).toBe(true);
    expect(isTypingTarget({ tagName: 'TEXTAREA' })).toBe(true);
    expect(isTypingTarget({ tagName: 'DIV', isContentEditable: true })).toBe(true);
    expect(isTypingTarget({ tagName: 'BODY' })).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
  });
});
