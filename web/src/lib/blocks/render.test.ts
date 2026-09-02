import { describe, expect, it } from 'vitest';
import { outline, placedFile, plainText, safeHref, type Block } from './render';

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

describe('safeHref', () => {
  // The renderer and the editor's Link control both ask this one function, so the rule is
  // stated once and both sides of the wiki agree on it by construction rather than by two
  // people remembering the same list.

  it('refuses every scheme a browser would run code for', () => {
    // A stored `href` is attacker-controlled the moment one person with write access to one
    // page is not trusted, and this wiki is on the public internet with no CSP. The variants
    // are not padding: the WHATWG URL parser lower-cases the scheme and removes tabs and
    // newlines from anywhere in the string, so all four of these ARE `javascript:` to a
    // browser, and a regex on the raw string is how that gets missed.
    expect(safeHref('javascript:alert(1)')).toBeNull();
    expect(safeHref('JaVaScRiPt:alert(1)')).toBeNull();
    expect(safeHref('  javascript:alert(1)')).toBeNull();
    expect(safeHref('java\nscript:alert(1)')).toBeNull();
    expect(safeHref('java\tscript:alert(1)')).toBeNull();
    expect(safeHref('data:text/html,<script>alert(1)</script>')).toBeNull();
    expect(safeHref('vbscript:msgbox(1)')).toBeNull();
    expect(safeHref('file:///etc/passwd')).toBeNull();
  });

  it('refuses anything that is not a usable string at all', () => {
    expect(safeHref(undefined)).toBeNull();
    expect(safeHref(null)).toBeNull();
    expect(safeHref(42)).toBeNull();
    expect(safeHref('')).toBeNull();
    expect(safeHref('   ')).toBeNull();
  });

  it('passes the schemes a wiki links with, and gives back the address unchanged', () => {
    // Unchanged matters: a relative link must stay relative, or every internal link in the
    // corpus would silently start pointing at the placeholder base this function parses with.
    for (const href of [
      'https://example.org/seite?a=1#b',
      'http://192.168.178.159:4000/v1',
      'mailto:jemand@example.org',
      '/rundgang/tabellen',
      '../nachbar',
      '#abschnitt',
      // Protocol-relative, and deliberately allowed: resolved against the placeholder base it
      // takes that base's https scheme, and an outright `https://evil.example/phish` is
      // allowed anyway — the shorter spelling grants nothing new. `normalizeLinkAddress`
      // agrees: since it started comparing origins, it leaves a foreign `//host/…` alone
      // rather than collapsing it to a path of THIS wiki.
      '//evil.example/phish'
    ]) {
      expect(safeHref(href), href).toBe(href);
    }
  });
});

describe('placedFile', () => {
  // A placement is a REFERENCE to a row in the page's `Anhänge` list, so everything this
  // reads is a name to look up rather than an address to fetch. There is no `href` in a
  // block and there must not be one: a download is authorised against the page it was
  // reached through (D-16), and an address stored in a document is an address that outlives
  // the permission that produced it.

  it('reads the file and the description a placement carries', () => {
    expect(
      placedFile({ kind: 'attachment', attrs: { filename: 'befund.png', alt: 'Röntgenbild' } })
    ).toEqual({ filename: 'befund.png', alt: 'Röntgenbild' });
  });

  it('reads an empty description as empty rather than as missing', () => {
    // `alt: ""` is what `![](anhang:befund.png)` imports as and what the editor's schema
    // fills a missing one in with, so the two agree — see `gw_core::BlockKind::Attachment`.
    expect(placedFile({ kind: 'attachment', attrs: { filename: 'a.png', alt: '' } })).toEqual({
      filename: 'a.png',
      alt: ''
    });
    expect(placedFile({ kind: 'attachment', attrs: { filename: 'a.png' } })).toEqual({
      filename: 'a.png',
      alt: ''
    });
  });

  it('names no file rather than an empty one when the block does not say', () => {
    // An empty name would be looked up in the list and would match nothing, which draws the
    // "not attached" sentence about a file nobody ever named. A malformed block draws
    // nothing at all instead.
    for (const attrs of [undefined, {}, { filename: '' }, { filename: '   ' }, { filename: 7 }]) {
      expect(placedFile({ kind: 'attachment', attrs }), JSON.stringify(attrs)).toBeNull();
    }
  });

  it('carries no address of any kind', () => {
    // Pins the SHAPE, not just today's fields: a placement that grew an `href` or a digest
    // would be an address a page's body could hand out, which is the one thing D-16 exists
    // to make impossible.
    const placed = placedFile({
      kind: 'attachment',
      attrs: { filename: 'a.png', alt: 'x', href: '/api/attachment/a.png/seite', sha256: 'ab'.repeat(32) }
    });
    expect(Object.keys(placed ?? {}).sort()).toEqual(['alt', 'filename']);
  });
});
