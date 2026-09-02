import { describe, expect, it } from 'vitest';
import { formulaFor, isMathFence, type Formulas } from './maths';

describe('which fence is a formula', () => {
  it('is ```math and nothing that merely looks like it', () => {
    // One spelling, deliberately. `isMathFence` is asked by the SERVER walker (which
    // decides what to typeset) and by the READER (which decides what to draw), and the two
    // agreeing is the whole reason it is one function: a fence the walker typeset and the
    // reader drew as code would be silently unrendered maths, and the reverse would be a
    // formula reported as being over a limit it never reached.
    for (const yes of ['math', 'MATH', ' math ', 'Math']) expect(isMathFence(yes), yes).toBe(true);
    for (const no of ['maths', 'latex', 'tex', 'katex', 'mathematica', '', 'rust']) {
      expect(isMathFence(no), no).toBe(false);
    }
  });

  it('says no to anything that is not a string, because a block attribute is arbitrary', () => {
    // `attrs.language` is the info string's first token on the way in
    // (`crates/gw-core/src/markdown.rs`) and an arbitrary JSON value over the collab
    // socket; nothing between the editor and `documents.body` validates it.
    for (const nothing of [undefined, null, 42, {}, ['math'], true]) {
      expect(isMathFence(nothing), String(nothing)).toBe(false);
    }
  });
});

describe('looking a formula up', () => {
  const html = '<span class="katex">…</span>';
  const typeset: Formulas = new Map([['E = mc^2', { kind: 'typeset', html } as const]]);

  it('finds what the server typeset, by the fence text itself', () => {
    expect(formulaFor(typeset, 'E = mc^2')).toEqual({ kind: 'typeset', html });
  });

  it('answers null where nobody typeset this page at all', () => {
    // The editor renders the same `BlockView` while TipTap mounts, and it has no page load
    // behind it. `null` is "nothing was typeset here", which draws the source and says
    // nothing — not "this formula was refused", which would be a false statement about a
    // limit that was never reached.
    expect(formulaFor(null, 'E = mc^2')).toBeNull();
    expect(formulaFor(undefined, 'E = mc^2')).toBeNull();
    expect(formulaFor(typeset, 'x^2')).toBeNull();
  });

  it('is a Map, so a formula may be called anything at all', () => {
    // Not a plain object, and this is the reason rather than a preference. The key is text
    // somebody with write access typed, and it crosses the wire as page data: SvelteKit
    // serialises that with `devalue`, whose parser THROWS on an object carrying a
    // `__proto__` property (`node_modules/devalue/src/parse.js`). A ```math fence
    // containing exactly `__proto__` would therefore have taken the whole page down —
    // a 500 on a route that is also the only way to edit the page that caused it.
    const odd: Formulas = new Map([
      ['__proto__', { kind: 'source', note: 'x' } as const],
      ['constructor', { kind: 'source', note: 'y' } as const]
    ]);
    expect(formulaFor(odd, '__proto__')).toEqual({ kind: 'source', note: 'x' });
    expect(formulaFor(odd, 'constructor')).toEqual({ kind: 'source', note: 'y' });
    expect(formulaFor(odd, 'toString')).toBeNull();
    expect(formulaFor(typeset, '__proto__')).toBeNull();
  });
});
