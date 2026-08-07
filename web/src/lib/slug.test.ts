import { describe, expect, it } from 'vitest';
import { slugify } from './slug';

// These cases are duplicated verbatim from crates/gw-core/src/slug.rs. If the two
// implementations ever disagree, one of these suites goes red — which is the whole
// point of duplicating them.
describe('slugify', () => {
  it('lowercases and collapses separators', () => {
    expect(slugify('  Table 0:  Dysbiotic   Shifts!! ')).toBe('table-0-dysbiotic-shifts');
  });

  it('strips em dashes without leaving double separators', () => {
    expect(slugify('Darm — ADHD Microbiota Reference')).toBe('darm-adhd-microbiota-reference');
  });

  it('transliterates German umlauts', () => {
    expect(slugify('Präbiotika Guide')).toBe('praebiotika-guide');
    expect(slugify('Größe und Maß')).toBe('groesse-und-mass');
    expect(slugify('Öl Überblick')).toBe('oel-ueberblick');
  });

  it('drops characters with no ASCII equivalent', () => {
    expect(slugify('Präbiotika 🧬 Guide')).toBe('praebiotika-guide');
  });

  it('returns empty for empty and separator-only input', () => {
    expect(slugify('')).toBe('');
    expect(slugify('---   ---')).toBe('');
  });
});
