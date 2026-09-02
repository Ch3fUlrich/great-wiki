import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  DIAGRAM_ASPECT_LIMIT,
  DIAGRAM_CHARACTER_LIMIT,
  DIAGRAM_EDGE_LIMIT,
  DIAGRAM_STATEMENT_LIMIT,
  diagramStatements,
  istUeberbreit,
  diagramDataUri,
  diagramEdgeRefusal,
  diagramRefusal,
  diagramSize,
  isDiagramFence,
  mermaidConfig,
  SECURE_CONFIG_KEYS
} from './diagram';

// Part of this suite reads `mermaid.ts` as TEXT, the way `highlight.test.ts` reads its own
// module and `server/maths.test.ts` reads its own. That is deliberate rather than lazy: the
// things being protected — that `render` is called with two arguments and never three, that
// `bindFunctions` is never called, that the library is behind the `browser` guard — are
// properties of how the renderer is WRITTEN, and no assertion about its return value can see
// any of them. There is no DOM in this suite, so the only alternative is a browser.
const renderer = readFileSync(new URL('./mermaid.ts', import.meta.url), 'utf8');

describe('which fence is a diagram', () => {
  it('is ```mermaid and nothing that merely looks like it', () => {
    // One spelling, for `isMathFence`'s reason: the reader asks this to decide what to draw,
    // and a second spelling is a second thing to argue about later. An author who writes
    // ```graphviz is told this wiki does not know that language rather than being silently
    // handed something they did not ask for.
    for (const yes of ['mermaid', 'MERMAID', ' mermaid ', 'Mermaid']) {
      expect(isDiagramFence(yes), yes).toBe(true);
    }
    for (const no of ['mermaid-js', 'graphviz', 'dot', 'diagram', '', 'math', 'rust']) {
      expect(isDiagramFence(no), no).toBe(false);
    }
  });

  it('says no to anything that is not a string, because a block attribute is arbitrary', () => {
    // `attrs.language` is the info string's first token on the way in
    // (`crates/gw-core/src/markdown.rs`) and an arbitrary JSON value over the collaboration
    // socket; nothing between the editor and `documents.body` validates it.
    for (const nothing of [undefined, null, 42, {}, ['mermaid'], true]) {
      expect(isDiagramFence(nothing), String(nothing)).toBe(false);
    }
  });
});

describe('the configuration mermaid is given', () => {
  it('is strict, and never one of the levels that would loosen it', () => {
    // The security level is the library's own half of D-19's first barrier. `loose` and
    // `antiscript` admit script into the rendered SVG; `sandbox` emits
    // `<iframe src="data:text/html;base64,…">`, and this application's `frame-src` is
    // `['none']` — loosening that to admit `data:` would hand a general XSS-hosting
    // primitive to a policy whose author wrote `frame-src 'none'` on the grounds that
    // nothing is embedded and nothing embeds this.
    for (const theme of ['default', 'dark'] as const) {
      expect(mermaidConfig(theme).securityLevel).toBe('strict');
    }
    expect(JSON.stringify(mermaidConfig('default'))).not.toContain('sandbox');
    // And it is settled HERE, once. A second configuration applied at the call site is the
    // shape this would come undone in — `initialize` merges what it is given over the site
    // config, so one extra key beside the call to it would win silently.
    expect(renderer).not.toMatch(/securityLevel\s*:/);
  });

  it('never starts the library on its own, and never leaves an error diagram behind', () => {
    // `startOnLoad` would make mermaid go looking for `.mermaid` elements in the live
    // document and render whatever it found, on its own schedule, outside every cap here.
    //
    // `suppressErrorRendering` is the one that is not obvious, and it was read out of the
    // installed package rather than assumed: with it false, a diagram whose RENDERER throws
    // takes mermaid's `errorRenderer` branch, which draws an error diagram and rethrows
    // WITHOUT reaching the cleanup — leaving that markup parked in `document.body`. With it
    // true, mermaid removes its own temporary elements before it throws.
    const config = mermaidConfig('default');
    expect(config.startOnLoad).toBe(false);
    expect(config.suppressErrorRendering).toBe(true);
  });

  it('draws a label as SVG text, because an HTML label does not survive the data URI', () => {
    // THE fix for a bug three reviewers found and nothing here could see. With `htmlLabels`
    // on — mermaid's default — a label is HTML inside a `<foreignObject>`, and mermaid
    // serialises the finished SVG with `innerHTML`, which is the HTML serialiser. So the
    // documented, canonical Mermaid line break `A[Erste Zeile<br>Zweite Zeile]` comes back
    // as a `<br>` with no closing tag: valid HTML, and NOT well-formed XML.
    //
    // `data:image/svg+xml` is parsed as strict XML, so that string does not decode and the
    // reader gets a broken-image glyph — the one outcome `diagramDataUri`'s own comment,
    // ADR 0018 and the changelog fragment all promise this feature never produces. Verified
    // in Chromium, Firefox and WebKit: two `<img>` with `naturalWidth === 0`, and
    // `DOMParser` on the decoded source answering *"Opening and ending tag mismatch: br"*.
    //
    // With this off, mermaid splits the label on `<br>` into `<tspan>`s instead: the line
    // break works AS DOCUMENTED and no HTML-only element is serialised at all. It stays in
    // `secure` so that a diagram's own `%%{init}%%` cannot turn it back on — `sanitize`
    // strips the key from a directive, and `setSiteConfig` (which `initialize` calls) is
    // deliberately not sanitised, which is what lets us set it here.
    for (const theme of ['default', 'dark'] as const) {
      expect(mermaidConfig(theme).htmlLabels).toBe(false);
    }
    expect(SECURE_CONFIG_KEYS).toContain('htmlLabels');
  });

  it('renders each theme under its own theme name, which is the whole of D-24', () => {
    expect(mermaidConfig('default').theme).toBe('default');
    expect(mermaidConfig('dark').theme).toBe('dark');
  });

  it('caps what the parser will accept, in the parser as well as before it', () => {
    const config = mermaidConfig('default');
    expect(config.maxTextSize).toBe(DIAGRAM_CHARACTER_LIMIT);
    expect(config.maxEdges).toBe(DIAGRAM_EDGE_LIMIT);
    expect(Number.isFinite(DIAGRAM_CHARACTER_LIMIT)).toBe(true);
    expect(Number.isFinite(DIAGRAM_EDGE_LIMIT)).toBe(true);
  });

  it('names every key a diagram must not be able to set on itself', () => {
    // `secure` is the list of configuration keys a `%%{init: …}%%` directive INSIDE the
    // diagram may not change (mermaid's `sanitize`, which deletes them from the directive).
    // The default list is short and covers only the library's own machinery, so the keys
    // that matter to this application are added by name:
    //
    //  - `dompurifyConfig` would let diagram text weaken the sanitiser meant to be
    //    protecting the page from it;
    //  - `themeCSS`, `themeVariables`, `fontFamily` and `altFontFamily` are the CSS-injection
    //    surface — mermaid puts a `<style>` element inside the SVG while it is still in the
    //    live document;
    //  - `htmlLabels` turns a label into markup rather than text;
    //  - `theme` is what would make both of D-24's renders come out the same, so that the
    //    image shown on a dark background is the one drawn for a light one.
    for (const key of [
      'dompurifyConfig',
      'themeCSS',
      'themeVariables',
      'htmlLabels',
      'fontFamily',
      'altFontFamily',
      'theme'
    ]) {
      expect(SECURE_CONFIG_KEYS, key).toContain(key);
    }
    // And the library's own defaults are kept rather than replaced: passing `secure` at all
    // REPLACES the default list, so anything left out of ours stops being protected.
    for (const key of [
      'secure',
      'securityLevel',
      'startOnLoad',
      'maxTextSize',
      'maxEdges',
      'suppressErrorRendering'
    ]) {
      expect(SECURE_CONFIG_KEYS, key).toContain(key);
    }
    expect(mermaidConfig('default').secure).toEqual([...SECURE_CONFIG_KEYS]);
  });
});

describe('the renderer, as a piece of source text', () => {
  it('calls render with two arguments and never three', () => {
    // The three-argument form hands mermaid a container and it performs the DOM write on
    // your behalf — verified in the installed package: `svgContainingElement.innerHTML = ""`
    // is the first thing it does with it. The insertion ADR 0014 forbids would then happen
    // one stack frame down inside a dependency, where no grep for our own spellings would
    // ever see it. `scripts/check-html-sinks.sh` greps for the three-argument call in any
    // file that mentions mermaid; this asserts the same thing from the other side, so that
    // deleting either one still leaves a red test.
    expect(renderer).toMatch(/\.render\([^)]*,[^),]*\)/);
    expect(renderer).not.toMatch(/\.render\([^)]*,[^)]*,/);
  });

  it('never calls bindFunctions, which is what would wire a diagram to the page', () => {
    // The belt to `securityLevel: 'strict'`'s braces. `bindFunctions` is what attaches a
    // diagram's `click` interactions to the DOM, and the returned SVG goes into an `<img>`
    // where nothing could be attached to anyway — so calling it could only ever be a step
    // towards putting the markup in the page instead.
    expect(renderer).not.toMatch(/bindFunctions\s*\(/);
  });

  it('reaches the library only through the browser guard, so no page pays for it unasked', () => {
    // The same shape `loadEditor` uses in `[...path]/+page.svelte`, and for the same reason:
    // `$app/environment`'s `browser` is replaced with a literal at build time, so the SSR
    // build reads `false ? import(…) : …` and rollup drops the import entirely. A bare
    // `import()` inside a branch that never executes on the server does NOT achieve that —
    // the chunk is still emitted, and the production image ships no `node_modules` for it to
    // resolve against.
    expect(renderer).toMatch(/from '\$app\/environment'/);
    expect(renderer).toMatch(/browser\s*\?\s*import\('mermaid'\)/);
    // …and nowhere else. A second, unguarded `import('mermaid')` would put the whole library
    // back into the server bundle without changing anything this file can otherwise see.
    expect(renderer.match(/(?<!typeof )import\('mermaid'\)/g) ?? []).toHaveLength(1);
    expect(renderer).not.toMatch(/^import .* from 'mermaid'/m);
  });

  it('shows no picture the browser has not already decoded', () => {
    // The guarantee `diagramDataUri` cannot make on its own, and the reason the promise
    // "malformed source is never a broken image" was false until now: that function is pure
    // and sees a STRING, while whether a `data:image/svg+xml` decodes is a question only an
    // XML parser can answer. A `<br>` in a label — HTML-serialised by mermaid, and the
    // documented way to write a two-line node — produced a URI no browser could read.
    //
    // So the address is handed to the browser's own image decoder before it is handed to
    // the page: same bytes, same parser, same answer. A rejection is the German note and
    // the fence's own source, which is what the whole feature promises. Asserted as source
    // text because there is no DOM in this suite — the behaviour harness loads a diagram
    // with a `<br>` in it against a real browser, which is the other half of this.
    expect(renderer).toMatch(/\.decode\(\)/);
    expect(renderer).toMatch(/new Image\(\)/);
  });
});

describe('the size a diagram is refused at', () => {
  it('lets an ordinary diagram through and says nothing about it', () => {
    expect(diagramRefusal('graph TD;\n  A-->B;')).toBeNull();
    expect(diagramRefusal('x'.repeat(DIAGRAM_CHARACTER_LIMIT))).toBeNull();
  });

  it('names the limit, in the reader s own language, rather than failing silently', () => {
    // D-22: generous caps, and a refusal that says what the limit is. An author who wrote a
    // diagram that does not appear must be able to tell "too big" from "broken".
    const note = diagramRefusal('x'.repeat(DIAGRAM_CHARACTER_LIMIT + 1));
    expect(note).not.toBeNull();
    expect(note).toContain('10.001');
    expect(note).toContain('10.000');
  });
});

describe('the statement cap, which is the edge cap for every diagram that is not a flowchart', () => {
  // `maxEdges` is mermaid's, and mermaid applies it to FLOWCHARTS. A classDiagram, a
  // stateDiagram-v2, a mindmap and a stack of nested subgraphs are bounded by nothing but
  // the character limit, which is far too generous for them: measured in Chromium under the
  // production policy, 750 `C <|-- D` relations in 9 805 characters — inside every cap this
  // module had — drew in 16.8 seconds, during which seven animation frames were served
  // against a 61 fps baseline. That is not a slow tab, it is a frozen one, and ADR 0018's
  // "generous caps" section named a number that never applied to it.
  //
  // So the count is ours, it is syntax-agnostic, and it is asked before the library is
  // fetched at all — which also puts the refusal in the first response.
  it('counts what every diagram language spells one per line', () => {
    expect(diagramStatements('graph TD;\n  A-->B;')).toBe(2);
    // Blank lines and `%%` comments are not statements, and a directive is a comment.
    expect(diagramStatements('graph TD\n\n  %% ein Kommentar\n  A-->B')).toBe(2);
    expect(diagramStatements('%%{init: {"theme":"dark"}}%%\ngraph TD\n A-->B')).toBe(2);
    // A flowchart may put several on one line, which is how the checked-in example is
    // written; counting lines alone would let that shape past the cap entirely.
    expect(diagramStatements('graph TD; A-->B; B-->C; C-->D;')).toBe(4);
  });

  it('refuses a diagram past the statement cap, whatever language it is written in', () => {
    const gross = ['classDiagram']
      .concat(Array.from({ length: DIAGRAM_STATEMENT_LIMIT }, (_, i) => `  C${i} <|-- D${i}`))
      .join('\n');
    expect(gross.length).toBeLessThan(DIAGRAM_CHARACTER_LIMIT);
    const note = diagramRefusal(gross);
    expect(note).not.toBeNull();
    expect(note).toContain(`${DIAGRAM_STATEMENT_LIMIT}`);
    expect(note).toContain('Anweisungen');
  });

  it('lets a diagram of ordinary size through', () => {
    const normal = ['classDiagram']
      .concat(Array.from({ length: 40 }, (_, i) => `  C${i} <|-- D${i}`))
      .join('\n');
    expect(diagramRefusal(normal)).toBeNull();
  });
});

describe('a drawing too wide to be shrunk into the column', () => {
  it('is left at its own size, to be scrolled rather than flattened into a line', () => {
    // Measured: 750 class relations laid out to a viewBox of 63 604 × 306, which
    // `max-width: 100%` renders as roughly 700 × 3 CSS pixels — a grey line where a diagram
    // should be. Past this ratio the picture keeps its own size inside the scroll box the
    // wrapper already is, which is legible by dragging rather than illegible in place.
    expect(istUeberbreit(null)).toBe(false);
    expect(istUeberbreit({ breite: 700, hoehe: 400 })).toBe(false);
    expect(istUeberbreit({ breite: 8 * 300, hoehe: 300 })).toBe(false);
    expect(istUeberbreit({ breite: 8 * 300 + 1, hoehe: 300 })).toBe(true);
    expect(istUeberbreit({ breite: 63_604, hoehe: 306 })).toBe(true);
    expect(DIAGRAM_ASPECT_LIMIT).toBeGreaterThan(1);
  });
});

describe('the edge cap, which mermaid enforces and we translate', () => {
  it('names the number, in German, when the library says the limit was passed', () => {
    // The library's own message is *"Edge limit exceeded. 201 edges found, but the limit is
    // 200. Initialize mermaid with maxEdges set to a higher number…"* — a sentence about how
    // this wiki is configured, addressed to whoever configured it, shown to somebody who was
    // trying to draw a flowchart.
    const note = diagramEdgeRefusal(
      new Error('Edge limit exceeded. 201 edges found, but the limit is 200.')
    );
    expect(note).not.toBeNull();
    expect(note).toContain('200');
    expect(note).toContain('Verbindungen');
  });

  it('answers null for everything else, so no other failure is explained wrongly', () => {
    // The match is on a dependency's English text and will one day stop matching. That costs
    // the general sentence instead of the specific one — it must never cost a wrong one.
    for (const other of [
      new Error('Parse error on line 2'),
      new Error('no diagram type detected'),
      new RangeError('Maximum call stack size exceeded'),
      'Edge limit exceeded',
      null,
      undefined,
      { message: 'Edge limit exceeded' }
    ]) {
      expect(diagramEdgeRefusal(other), String(other)).toBeNull();
    }
  });
});

describe('the address the image is loaded from', () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><text>Größe &amp; Maß</text></svg>';

  it('is a data URI, which is what the policy already admits', () => {
    // `img-src ['self', 'data:']` (`web/vite.config.ts`), so no directive moves for this
    // feature. The scheme is the one ADR 0014 requires for an SVG that came from somebody
    // else: a browser executes no script in an `<img>`, whatever the bytes are.
    const uri = diagramDataUri(svg);
    expect(uri).not.toBeNull();
    expect(uri?.startsWith('data:image/svg+xml;charset=utf-8,')).toBe(true);
  });

  it('survives the round trip, umlauts and all', () => {
    expect(decodeURIComponent(diagramDataUri(svg)!.split(',').slice(1).join(','))).toBe(svg);
  });

  it('escapes the characters that would end the attribute or the URI', () => {
    // The value is bound as an `src` ATTRIBUTE, so Svelte escapes it — but a `#` inside an
    // unescaped data URI would truncate it at the fragment, which is a broken image rather
    // than an escape, and a broken image is the one outcome this feature must never produce.
    const uri = diagramDataUri('<svg id="a#b" style="fill:#fff"></svg>')!;
    expect(uri).not.toContain('#');
    expect(uri).not.toContain('"');
    expect(uri).not.toContain('<');
  });

  it('refuses anything that is not an SVG, rather than making an image of it', () => {
    // Nothing observed makes mermaid return a non-SVG string, and that is exactly why this
    // is here: if it ever does, the reader must fall back to the diagram's own source. An
    // `<img>` whose bytes are not an image is a broken-image icon, which reads as a network
    // fault and sends whoever investigates to the wrong place.
    for (const not of ['', '   ', 'nichts', '<html><body>x</body></html>', '<div>x</div>']) {
      expect(diagramDataUri(not), not).toBeNull();
    }
    expect(diagramDataUri('  \n<svg></svg>')).not.toBeNull();
    expect(diagramDataUri('<?xml version="1.0"?><svg></svg>')).not.toBeNull();
  });
});

describe('how big the drawing says it is', () => {
  it('reads the viewBox, because the width mermaid writes means something else in an <img>', () => {
    // Mermaid emits `width="100%"` with `style="max-width: 365px"` — in a document that reads
    // "as wide as there is room for, no wider than natural size". Inside an image it reads as
    // no intrinsic width at all, so the browser stretches a three-node diagram across the whole
    // column. Observed against a production build, not reasoned about.
    const svg =
      '<svg id="x" width="100%" xmlns="http://www.w3.org/2000/svg" ' +
      'style="max-width: 365px;" viewBox="0 0 365 405.95965576171875"><g/></svg>';
    expect(diagramSize(svg)).toEqual({ breite: 365, hoehe: 406 });
  });

  it('accepts the separators an SVG is allowed to use', () => {
    expect(diagramSize('<svg viewBox="0,0,100,50"/>')).toEqual({ breite: 100, hoehe: 50 });
    expect(diagramSize('<svg viewBox=" 0 0  100   50 "/>')).toEqual({ breite: 100, hoehe: 50 });
  });

  it('answers null rather than a guess for anything it cannot trust', () => {
    // These numbers become layout, and they are computed from text somebody with write access
    // to one page typed. `null` puts the browser back in charge, which is merely the stretching
    // this function exists to avoid — a bad number is a page pushed off its own screen.
    for (const bad of [
      '<svg/>',
      '<svg viewBox=""/>',
      '<svg viewBox="0 0 100"/>',
      '<svg viewBox="0 0 100 50 20"/>',
      '<svg viewBox="0 0 nan 50"/>',
      '<svg viewBox="0 0 -5 50"/>',
      '<svg viewBox="0 0 0 50"/>',
      '<svg viewBox="0 0 1e9 50"/>',
      '<svg viewBox="0 0 Infinity 50"/>'
    ]) {
      expect(diagramSize(bad), bad).toBeNull();
    }
  });
});
