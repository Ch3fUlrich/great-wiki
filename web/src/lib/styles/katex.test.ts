import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  CSS_FILE,
  FONT_DIR,
  UPSTREAM,
  expectedCss,
  installedVersion,
  upstreamFonts,
  vendorCss
} from '../../../scripts/vendor-katex.mjs';

/*
 * `src/lib/styles/katex.css` and `static/fonts/katex/` are COPIES of what `npm install`
 * put in `node_modules/katex`. A copy is a thing that goes stale, and this one goes stale
 * silently: a version bump that moves a glyph's metrics leaves a stylesheet computed
 * against the old ones, and the symptom is maths that is subtly mis-spaced on a page
 * nobody is looking at. So the copy is checked against the original on every test run,
 * and `node scripts/vendor-katex.mjs` is what makes it right again.
 *
 * The transformation itself lives in that script rather than here, so that the thing which
 * writes the file and the thing which checks it cannot disagree about what it should say.
 */

const vendored = readFileSync(CSS_FILE, 'utf8');

describe('the vendored KaTeX stylesheet', () => {
  it('is exactly what the vendoring script would write today', () => {
    // The whole check in one line. If this fails, run `node scripts/vendor-katex.mjs` from
    // `web/` and read the diff before committing it: a KaTeX release can change layout
    // rules, class names and font metrics together.
    expect(vendored).toBe(expectedCss());
  });

  it('names the version it was taken from, so the copy can be dated', () => {
    expect(vendored).toContain(`katex@${installedVersion()}`);
  });

  it('changes nothing but the font addresses', () => {
    // The claim the header makes, checked rather than asserted in prose: every line that is
    // not a `src:` is upstream's, byte for byte.
    const upstream = readFileSync(join(UPSTREAM, 'dist/katex.css'), 'utf8');
    const withoutSrc = (css: string) => css.replace(/^\s*src:.*$/gm, '');
    expect(withoutSrc(vendorCss(upstream))).toBe(withoutSrc(upstream));
  });

  it('addresses every face from this origin and none from node_modules', () => {
    // `font-src 'self'` (ADR 0007). A `url()` that still pointed into the package would not
    // fail loudly — the face would simply never load, and the formula would be laid out in
    // whatever serif the machine has at metrics KaTeX did not compute.
    expect(vendored).not.toContain('url(fonts/');
    const addresses = [...vendored.matchAll(/url\(([^)]*)\)/g)].map((match) => match[1]);
    expect(addresses.length).toBe(upstreamFonts().length);
    for (const address of addresses) {
      expect(address).toMatch(/^'\/fonts\/katex\/KaTeX_[A-Za-z0-9_-]+\.woff2'$/);
    }
  });

  it('is not layered, which is deliberate and is written down', () => {
    // Every other stylesheet here is inside a cascade layer (ADR 0005). This one is not,
    // and the header says why — a `@font-face` takes no part in the cascade, and wrapping
    // the rest would be an edit to vendored bytes that the sync test above forbids.
    expect(vendored).not.toContain('@layer');
    expect(vendored).toContain('UNLAYERED');
  });
});

describe('the vendored KaTeX faces', () => {
  it('are the package’s own files, byte for byte', () => {
    const shipped = readdirSync(FONT_DIR)
      .filter((name) => name.endsWith('.woff2'))
      .sort();
    expect(shipped).toEqual(upstreamFonts());
    for (const name of shipped) {
      expect(
        readFileSync(join(FONT_DIR, name)).equals(
          readFileSync(join(UPSTREAM, 'dist/fonts', name))
        ),
        `${name} differs from the one in node_modules`
      ).toBe(true);
    }
  });

  it('carry only the woff2 cut', () => {
    // KaTeX also publishes `.woff` and `.ttf` for browsers that predate woff2. Carrying all
    // three would be about 1.2 MB in the image to serve none of it — this application's own
    // faces are woff2-only already.
    for (const name of readdirSync(FONT_DIR)) {
      expect(name === 'LICENSE' || name.endsWith('.woff2'), name).toBe(true);
    }
  });

  it('sit beside KaTeX’s own licence, which is MIT and not the OFL', () => {
    const licence = readFileSync(join(FONT_DIR, 'LICENSE'), 'utf8');
    expect(licence).toBe(readFileSync(join(UPSTREAM, 'LICENSE'), 'utf8'));
    expect(licence).toContain('MIT License');
  });
});
