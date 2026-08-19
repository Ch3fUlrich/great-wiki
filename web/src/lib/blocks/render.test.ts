import { describe, expect, it } from 'vitest';
import { outline, plainText, type Block } from './render';

const doc: Block = {
  kind: 'doc',
  content: [
    { kind: 'heading', attrs: { level: 2 }, content: [{ kind: 'text', text: 'Größe und Maß' }] },
    { kind: 'paragraph', content: [{ kind: 'text', text: 'Ein Satz.' }] }
  ]
};

// Duplicated verbatim from `PLAIN_TEXT_CASES` in crates/gw-core/src/block.rs — as JSON,
// exactly as that suite writes them, so the two cannot drift into "the same case, spelled
// differently". `plainText` and `Block::plain_text` are deliberate mirrors: this one feeds
// the outline, the heading anchor ids and the table column labels, that one feeds the
// search index and the seeder's exact title comparison. If they ever disagree, the reader
// gets a heading in its table of contents that is not the heading the anchor points at.
// (The `marks` in these fixtures are what SPLIT a sentence into several leaves in the
// first place; this renderer has no mark support yet and ignores them, which is precisely
// why the split must not be visible in the text.)
const PLAIN_TEXT_CASES: [string, string][] = [
  [
    `{"kind":"paragraph","content":[
       {"kind":"text","text":"Siehe "},
       {"kind":"text","text":"das Handbuch","marks":[{"kind":"link","attrs":{"href":"/h"}}]},
       {"kind":"text","text":"."}]}`,
    'Siehe das Handbuch.'
  ],
  [
    `{"kind":"paragraph","content":[
       {"kind":"text","text":"Der "},
       {"kind":"text","text":"Darm","marks":[{"kind":"strong"}]},
       {"kind":"text","text":"-Trakt"}]}`,
    'Der Darm-Trakt'
  ],
  [
    `{"kind":"doc","content":[
       {"kind":"heading","content":[{"kind":"text","text":"Maß"}]},
       {"kind":"paragraph","content":[{"kind":"text","text":"Einheit"}]}]}`,
    'Maß Einheit'
  ]
];

describe('block helpers', () => {
  it('extracts plain text in document order', () => {
    expect(plainText(doc)).toBe('Größe und Maß Ein Satz.');
  });

  it('joins adjacent inline leaves as one run of prose and still separates blocks', () => {
    for (const [json, expected] of PLAIN_TEXT_CASES) {
      expect(plainText(JSON.parse(json) as Block)).toBe(expected);
    }
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
