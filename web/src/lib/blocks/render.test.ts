import { describe, expect, it } from 'vitest';
import { outline, plainText, type Block } from './render';

const doc: Block = {
  kind: 'doc',
  content: [
    { kind: 'heading', attrs: { level: 2 }, content: [{ kind: 'text', text: 'Größe und Maß' }] },
    { kind: 'paragraph', content: [{ kind: 'text', text: 'Ein Satz.' }] }
  ]
};

describe('block helpers', () => {
  it('extracts plain text in document order', () => {
    expect(plainText(doc)).toBe('Größe und Maß Ein Satz.');
  });

  it('builds an outline with ASCII anchor ids', () => {
    const headings = outline(doc);
    expect(headings).toHaveLength(1);
    expect(headings[0]).toEqual({ level: 2, text: 'Größe und Maß', id: 'groesse-und-mass' });
  });

  it('defaults a heading without a level to 1', () => {
    const h: Block = { kind: 'doc', content: [{ kind: 'heading', content: [{ kind: 'text', text: 'T' }] }] };
    expect(outline(h)[0].level).toBe(1);
  });
});
