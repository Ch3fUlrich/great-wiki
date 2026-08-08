import { readFileSync, readdirSync, statSync } from 'node:fs';
import { brotliDecompressSync } from 'node:zlib';
import { describe, expect, it } from 'vitest';

// The typeface system is CSS, so this suite reads the CSS. Asserting against a
// TypeScript copy of the mapping would only prove the copy agrees with itself; the
// thing that can actually break is a family renamed in tokens.css and not in
// fonts.css, or a file that stops being shipped, and neither shows up at runtime as
// anything louder than the wrong font quietly appearing.
const STYLES = new URL('.', import.meta.url);
const FONTS_DIR = new URL('../../../static/fonts/', import.meta.url);

const tokens = readFileSync(new URL('./tokens.css', STYLES), 'utf8');
const fonts = readFileSync(new URL('./fonts.css', STYLES), 'utf8');

/// The declarations of one top-level rule, comments stripped.
function rule(selector: string): string {
  const start = tokens.indexOf(`${selector} {`);
  expect(start, `tokens.css has no \`${selector}\` rule`).toBeGreaterThanOrEqual(0);
  const end = tokens.indexOf('\n}', start);
  return tokens.slice(start, end).replace(/\/\*[\s\S]*?\*\//g, '');
}

/// One custom property from a rule, with `var(--x)` followed one level — which is
/// exactly what --font-body does, and the only indirection the tokens use.
function token(selector: string, name: string): string {
  const block = rule(selector);
  const found = new RegExp(`${name}:\\s*([^;]+);`).exec(block);
  expect(found, `\`${selector}\` does not set \`${name}\``).not.toBeNull();
  const value = found![1].replace(/\s+/g, ' ').trim();
  const indirect = /^var\((--[\w-]+)\)$/.exec(value);
  return indirect ? token(selector, indirect[1]) : value;
}

/// The first family in a stack, unquoted.
function first(stack: string): string {
  return stack.split(',')[0].trim().replace(/^['"]|['"]$/g, '');
}

describe('the three typeface choices', () => {
  // The promise the control makes to the reader, per the header radio group.
  const EXPECTED = [
    { choice: ':root', reading: 'IBM Plex Sans Var', mono: 'IBM Plex Mono' },
    { choice: ":root[data-font='literata']", reading: 'Literata', mono: 'JetBrains Mono' },
    { choice: ":root[data-font='system']", reading: 'ui-sans-serif', mono: 'ui-monospace' }
  ];

  for (const { choice, reading, mono } of EXPECTED) {
    it(`${choice} reads in ${reading} and sets code in ${mono}`, () => {
      expect(first(token(choice, '--font-body'))).toBe(reading);
      expect(first(token(choice, '--font-mono'))).toBe(mono);
    });
  }

  it('makes Plex the default, so no attribute is the Plex attribute', () => {
    // The blocking script in app.html writes `data-font` only for the other two, and
    // FontToggle clears it for Plex. If :root ever stopped being Plex, both would be
    // wrong at once and nothing would say so.
    expect(first(token(':root', '--font-sans'))).toBe('IBM Plex Sans Var');
  });

  it('leaves the reading face on --font-sans for the two sans choices', () => {
    for (const choice of [':root', ":root[data-font='system']"]) {
      expect(token(choice, '--font-body')).toBe(token(choice, '--font-sans'));
    }
    expect(token(":root[data-font='literata']", '--font-body')).toBe(
      token(":root[data-font='literata']", '--font-serif')
    );
  });

  it('downloads nothing for the System choice', () => {
    const declared = [...fonts.matchAll(/font-family:\s*'([^']+)'/g)].map((m) => m[1]);
    const webfonts = declared.filter((f) => !f.endsWith('Fallback'));
    const system = rule(":root[data-font='system']");
    for (const family of webfonts) {
      expect(system, `System still names the webfont ${family}`).not.toContain(family);
    }
  });

  it('names only families that fonts.css actually declares', () => {
    // A misspelt family name in a font stack is silent: the browser skips it and uses
    // the next one, and the page looks almost right.
    const declared = new Set([...fonts.matchAll(/font-family:\s*'([^']+)'/g)].map((m) => m[1]));
    const stacks = EXPECTED.flatMap(({ choice }) =>
      ['--font-sans', '--font-serif', '--font-mono'].map((name) => token(choice, name))
    );
    const used = stacks
      .flatMap((stack) => [...stack.matchAll(/'([^']+)'/g)].map((m) => m[1]))
      // Generic platform names, not webfaces we ship.
      .filter((f) => !['Segoe UI', 'Times New Roman', 'SF Mono'].includes(f));
    expect(used.length).toBeGreaterThan(0);
    for (const family of used) {
      expect(declared, `tokens.css uses '${family}', which no @font-face declares`).toContain(
        family
      );
    }
  });
});

describe('every font value goes through a custom property', () => {
  // ADR 0005: a hard-coded family is one a theme author can never override. fonts.css
  // is the single exception — naming families is what an @font-face is for.
  function sources(dir: URL): URL[] {
    return readdirSync(dir).flatMap((entry) => {
      const child = new URL(entry, dir);
      if (statSync(child).isDirectory()) return sources(new URL(`${entry}/`, dir));
      return /\.(css|svelte)$/.test(entry) && entry !== 'fonts.css' ? [child] : [];
    });
  }

  it('never hard-codes a family outside fonts.css', () => {
    const offenders: string[] = [];
    for (const file of sources(new URL('../../', STYLES))) {
      const css = readFileSync(file, 'utf8');
      for (const [, value] of css.matchAll(/font-family:\s*([^;}]+)/g)) {
        const trimmed = value.trim();
        if (!trimmed.startsWith('var(--') && trimmed !== 'inherit') {
          offenders.push(`${file.pathname}: font-family: ${trimmed}`);
        }
      }
    }
    expect(offenders).toEqual([]);
  });
});

describe('the shipped font files', () => {
  const referenced = [...fonts.matchAll(/url\('(\/fonts\/[^']+)'\)/g)].map((m) => m[1]);
  const unique = [...new Set(referenced)];

  it('exist, and every one of them is referenced', () => {
    expect(unique.length).toBeGreaterThan(0);
    for (const href of unique) {
      const file = new URL(href.replace('/fonts/', ''), FONTS_DIR);
      expect(statSync(file).size, `${href} is empty`).toBeGreaterThan(0);
    }

    const shipped = readdirSync(FONTS_DIR, { recursive: true, encoding: 'utf8' })
      .filter((p) => p.endsWith('.woff2'))
      .map((p) => `/fonts/${p.split(/[\\/]/).join('/')}`)
      .sort();
    // Dead weight in static/ is bytes in the image that nothing ever serves.
    expect(shipped).toEqual([...unique].sort());
  });

  it('each sit beside their licence, because the fonts are OFL and this repo is MIT', () => {
    const families = readdirSync(FONTS_DIR).filter((entry) =>
      statSync(new URL(entry, FONTS_DIR)).isDirectory()
    );
    expect(families.length).toBeGreaterThan(0);
    for (const family of families) {
      const licence = readFileSync(new URL(`${family}/OFL.txt`, FONTS_DIR), 'utf8');
      expect(licence, `${family}/OFL.txt is not the OFL`).toContain('SIL OPEN FONT LICENSE');
    }
  });

  // The one that matters on a German site. `web/scripts/check-fonts.py` does this more
  // thoroughly with fontTools; this runs in `npm test`, so it holds without anyone
  // having to remember to install a Python package.
  it('all contain a real ẞ (U+1E9E), not two S in a trench coat', () => {
    for (const href of unique) {
      const file = new URL(href.replace('/fonts/', ''), FONTS_DIR);
      const font = readWoff2(readFileSync(file));

      const eszett = font.glyphFor(0x1e9e);
      expect(eszett, `${href} has no U+1E9E — STRAẞE would render as a box`).not.toBe(0);

      // A faked ẞ is a composite of S + S, which gives it about twice the advance.
      const s = font.glyphFor(0x53);
      const ratio = font.advance(eszett) / font.advance(s);
      expect(ratio, `${href} draws U+1E9E at ${ratio.toFixed(2)}x S — that is an SS`).toBeLessThan(
        1.7
      );
    }
  });

  it('really is reading the fonts, and not saying yes to everything', () => {
    // U+0141 (Ł) is outside the Latin+German subset, so the subset files must NOT have
    // it while the unsubset IBM Plex files must. If the reader below ever degrades into
    // "true", this is the assertion that notices.
    const subset = new URL('literata/Literata-Roman-latin.woff2', FONTS_DIR);
    const full = new URL('ibm-plex-sans/IBMPlexSansVar-Roman.woff2', FONTS_DIR);
    expect(readWoff2(readFileSync(subset)).glyphFor(0x141)).toBe(0);
    expect(readWoff2(readFileSync(full)).glyphFor(0x141)).not.toBe(0);
  });
});

/* --- A very small woff2 reader -------------------------------------------
 *
 * Enough of the format to answer "is this character in the font, and how wide is it".
 * Writing it out beats adding a font-parsing dependency to a project that has none,
 * and beats shelling out to Python from a Node test suite.
 *
 * woff2 is a table directory followed by ONE brotli stream holding every table
 * concatenated in directory order. Node can do the brotli; the directory is the only
 * fiddly part, because table tags are indexes into a fixed list and lengths are
 * variable-width integers. cmap and hmtx are never transformed, so once their offsets
 * are known they can be read as plain OpenType.
 */

// The 63 tags a woff2 directory can refer to by index; 63 itself means "4-byte tag follows".
const KNOWN_TAGS =
  'cmap head hhea hmtx maxp name OS/2 post cvt_ fpgm glyf loca prep CFF_ VORG EBDT EBLC gasp hdmx kern LTSH PCLT VDMX vhea vmtx BASE GDEF GPOS GSUB EBSC JSTF MATH CBDT CBLC COLR CPAL SVG_ sbix acnt avar bdat bloc bsln cvar fdsc feat fmtx fvar gvar hsty just lcar mort morx opbd prop trak Zapf Silf Glat Gloc Feat Sill'.split(
    ' '
  );

interface Woff2 {
  /// The glyph id for a code point, or 0 (.notdef) when the font does not have it.
  glyphFor(codePoint: number): number;
  /// Advance width in font units.
  advance(glyphId: number): number;
}

function readWoff2(file: Buffer): Woff2 {
  if (file.toString('latin1', 0, 4) !== 'wOF2') throw new Error('not a woff2 file');
  const numTables = file.readUInt16BE(12);

  let at = 48;
  /// Variable-width integer, 7 bits per byte, high bit continues.
  function base128(): number {
    let value = 0;
    for (let i = 0; i < 5; i++) {
      const byte = file[at++];
      value = value * 128 + (byte & 0x7f);
      if ((byte & 0x80) === 0) return value;
    }
    throw new Error('malformed UIntBase128');
  }

  const directory: { tag: string; offset: number; length: number; transformed: boolean }[] = [];
  let offset = 0;
  for (let i = 0; i < numTables; i++) {
    const flags = file[at++];
    const index = flags & 0x3f;
    let tag: string;
    if (index === 63) {
      tag = file.toString('latin1', at, at + 4);
      at += 4;
    } else {
      tag = KNOWN_TAGS[index].replace(/_$/, ' ');
    }
    const version = (flags >> 6) & 0x03;
    // glyf and loca invert the convention: for them version 0 IS the transform.
    const transformed = tag === 'glyf' || tag === 'loca' ? version === 0 : version !== 0;
    const originalLength = base128();
    const length = transformed ? base128() : originalLength;
    directory.push({ tag, offset, length, transformed });
    offset += length;
  }

  const tables = brotliDecompressSync(file.subarray(at));

  function table(tag: string) {
    const entry = directory.find((t) => t.tag === tag);
    if (!entry) throw new Error(`no ${tag} table`);
    return entry;
  }

  // --- cmap: prefer the full-Unicode format 12, fall back to the BMP format 4.
  const cmap = table('cmap').offset;
  let best = -1;
  let bestScore = -1;
  for (let i = 0; i < tables.readUInt16BE(cmap + 2); i++) {
    const record = cmap + 4 + i * 8;
    const platform = tables.readUInt16BE(record);
    const encoding = tables.readUInt16BE(record + 2);
    const subtable = cmap + tables.readUInt32BE(record + 4);
    const score =
      (platform === 3 && encoding === 10) || (platform === 0 && encoding >= 4)
        ? 3
        : (platform === 3 && encoding === 1) || platform === 0
          ? 2
          : 1;
    if (score > bestScore) {
      bestScore = score;
      best = subtable;
    }
  }
  if (best < 0) throw new Error('no usable cmap subtable');

  function glyphFor(codePoint: number): number {
    const format = tables.readUInt16BE(best);
    if (format === 12) {
      const groups = tables.readUInt32BE(best + 12);
      for (let i = 0; i < groups; i++) {
        const group = best + 16 + i * 12;
        const start = tables.readUInt32BE(group);
        const end = tables.readUInt32BE(group + 4);
        if (codePoint >= start && codePoint <= end) {
          return tables.readUInt32BE(group + 8) + (codePoint - start);
        }
      }
      return 0;
    }
    if (format !== 4) throw new Error(`unsupported cmap format ${format}`);
    if (codePoint > 0xffff) return 0;
    const segments = tables.readUInt16BE(best + 6) / 2;
    const ends = best + 14;
    const starts = ends + segments * 2 + 2;
    const deltas = starts + segments * 2;
    const ranges = deltas + segments * 2;
    for (let i = 0; i < segments; i++) {
      if (codePoint > tables.readUInt16BE(ends + i * 2)) continue;
      const start = tables.readUInt16BE(starts + i * 2);
      if (codePoint < start) return 0;
      const rangeOffset = tables.readUInt16BE(ranges + i * 2);
      const delta = tables.readInt16BE(deltas + i * 2);
      if (rangeOffset === 0) return (codePoint + delta) & 0xffff;
      const glyph = tables.readUInt16BE(ranges + i * 2 + rangeOffset + (codePoint - start) * 2);
      return glyph === 0 ? 0 : (glyph + delta) & 0xffff;
    }
    return 0;
  }

  // --- hmtx: advance widths, then a tail of glyphs that all share the last one.
  const hhea = table('hhea').offset;
  const metrics = tables.readUInt16BE(hhea + 34);
  const hmtx = table('hmtx');
  // The optional hmtx transform drops the side-bearing arrays and prefixes a flag byte.
  const widths = hmtx.offset + (hmtx.transformed ? 1 : 0);
  const stride = hmtx.transformed ? 2 : 4;

  function advance(glyphId: number): number {
    const index = Math.min(glyphId, metrics - 1);
    return tables.readUInt16BE(widths + index * stride);
  }

  return { glyphFor, advance };
}
