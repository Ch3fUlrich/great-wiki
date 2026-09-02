import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import type { Block } from '$lib/blocks/render';
import type { Formula } from '$lib/blocks/maths';
import {
  FORMULA_CHARACTER_LIMIT,
  PAGE_FORMULA_LIMIT,
  PAGE_MARKUP_LIMIT,
  katexOptions,
  typesetDocument
} from './maths';

// Part of this suite reads `maths.ts` as TEXT, the way `server/highlight.test.ts` reads its
// own module and `styles/fonts.test.ts` reads `tokens.css`. That is deliberate: the safety
// property here is that an option is NEVER PASSED, and no rendering assertion can tell the
// absence of a key from a `trust` function that happened to say no to the case being tested.
const source = readFileSync(new URL('./maths.ts', import.meta.url), 'utf8');

/** A ```math fence, as the importer stores one. */
function fence(text: string, language = 'math'): Block {
  return { kind: 'codeBlock', attrs: { language }, content: [{ kind: 'text', text }] };
}

/** A document holding the given blocks. */
function doc(...content: Block[]): Block {
  return { kind: 'doc', content };
}

/** What one fence's text became, on a page holding only that fence. */
function one(text: string): Formula {
  const formula = typesetDocument(doc(fence(text))).get(text);
  expect(formula, `nothing was recorded for ${JSON.stringify(text)}`).toBeDefined();
  return formula!;
}

/** The markup one fence produced, or the assertion failure that it produced none. */
function markup(text: string): string {
  const formula = one(text);
  expect(formula.kind).toBe('typeset');
  return formula.kind === 'typeset' ? formula.html : '';
}

describe('the options KaTeX is called with', () => {
  it('never passes `trust`, in any spelling', () => {
    // The whole safety story of this feature. `trust` defaults to FALSE, and that single
    // default disables `\href`, `\url`, `\includegraphics`, `\htmlClass`, `\htmlId`,
    // `\htmlStyle` and `\htmlData` in one go — there is nothing to configure, and the safe
    // setting is the one you get by not typing anything.
    //
    // Asserted on the object rather than only on rendered output because `trust` also takes
    // a FUNCTION, and a permissive one would pass every rendering assertion below while
    // opening all seven commands at once.
    expect(Object.keys(katexOptions())).not.toContain('trust');
    expect(source).not.toMatch(/^[^*/\n]*\btrust\b\s*:/m);
  });

  it('says where an amendment would have to be argued, because it is ADR 0007’s sentence', () => {
    // `\htmlStyle` puts author-written CSS declarations into a `style` attribute, and ADR
    // 0007 admits `style-src-attr 'unsafe-inline'` partly on the sentence "the renderer does
    // not emit authored CSS into one either way" (`web/vite.config.ts`). So turning trust on
    // is not a configuration tweak, it is an amendment to that decision — and the reason has
    // to be written where somebody about to make the change would find it.
    expect(source).toContain('0007');
  });

  it('bounds the two things a formula can otherwise make unbounded', () => {
    // `maxSize` defaults to Infinity, which makes `\rule{500em}{500em}` a layout bomb any
    // author can leave on a page; `maxExpand` is what bounds macro expansion. Both are
    // stated rather than inherited, because a default is a thing a dependency may change.
    const options = katexOptions();
    expect(Number.isFinite(options.maxSize)).toBe(true);
    expect(Number.isFinite(options.maxExpand)).toBe(true);
  });

  it('hands every call its own `macros` object', () => {
    // KaTeX writes `\gdef` definitions INTO the object it is given, so one shared object
    // would let a formula redefine what every later formula on the site means — including
    // on pages its author cannot edit, because this module is loaded once per server
    // process and would outlive the request.
    expect(katexOptions().macros).not.toBe(katexOptions().macros);

    const page = typesetDocument(doc(fence('\\gdef\\dosis{424242}\\dosis'), fence('\\dosis')));
    expect(page.get('\\gdef\\dosis{424242}\\dosis')!.kind).toBe('typeset');
    const second = page.get('\\dosis')!;
    expect(second.kind).toBe('typeset');
    if (second.kind !== 'typeset') return;
    // `\dosis` is not a KaTeX command, so on its own it renders as its own name in the
    // error colour. Had the first fence's `\gdef` leaked, it would render as 424242.
    expect(second.html).toContain('dosis');
    expect(second.html).not.toContain('424242');
  });
});

describe('what a formula may do', () => {
  // Asserted against the ATTRIBUTE each command would have written, never against the
  // string appearing anywhere at all: KaTeX echoes the author's TeX back inside
  // `<annotation encoding="application/x-tex">`, on purpose, so that a screen reader and a
  // copy-paste get the formula's source. `\href{https://…}` therefore does appear in the
  // answer as text, and must not appear as an `href`.
  const REFUSED: [string, RegExp][] = [
    ['\\href{https://angreifer.example}{Befund}', /href\s*=/],
    ['\\url{javascript:alert(1)}', /href\s*=/],
    ['\\htmlClass{boese}{x}', /class="[^"]*\bboese\b/],
    ['\\htmlId{boese}{x}', /id="[^"]*boese/],
    ['\\htmlStyle{background:url(https://angreifer.example/)}{x}', /style="[^"]*angreifer/],
    ['\\htmlData{boese=1}{x}', /data-boese/],
    ['\\includegraphics[height=1em]{https://angreifer.example/x.png}', /<img/]
  ];

  for (const [command, forbidden] of REFUSED) {
    it(`refuses ${command.slice(0, command.indexOf('{'))}`, () => {
      const html = markup(command);
      expect(html).not.toMatch(forbidden);
      // It renders the command's own name in KaTeX's error colour, which is the honest
      // outcome: the author sees that the wiki did not do what they asked for.
      expect(html).toContain('#cc0000');
    });
  }

  it('clamps a rule nobody could have meant', () => {
    // Without a finite `maxSize` this is a 500 em × 500 em black rectangle over the page,
    // which any author could leave behind and no reader could scroll past.
    const html = markup('\\rule{500em}{500em}');
    expect(html).not.toMatch(/style="[^"]*500em/);
    expect(html).toMatch(new RegExp(`style="[^"]*${katexOptions().maxSize}em`));
  });
});

describe('a formula that goes wrong', () => {
  it('renders its own source rather than throwing, for a parse error', () => {
    // `throwOnError: false` turns a KaTeX ParseError into a rendered error node carrying the
    // source. The page still renders, and the author sees what they typed and that it was
    // not understood.
    const html = markup('\\frac{');
    expect(html).toContain('\\frac{');
    expect(html).toContain('katex-error');
  });

  it('survives an error KaTeX throws PAST `throwOnError`, which is the one that matters', () => {
    // THE test of this step. `throwOnError: false` converts a ParseError and nothing else,
    // and KaTeX throws outside that path: deeply nested groups overflow the parser's own
    // stack and come back as a RangeError, which sails straight through the option.
    //
    // This runs on the server. An uncaught throw in a page's `load` is a 500 for the whole
    // route — and that route is also the edit surface (`web/src/routes/[...path]`), so the
    // page could not even be repaired through the editor afterwards. Hence a try/catch as
    // well as the option, and hence this test rather than a malformed-but-parseable one.
    const bomb = '{'.repeat(2000) + '}'.repeat(2000);
    expect(bomb.length).toBeLessThanOrEqual(FORMULA_CHARACTER_LIMIT);
    const formula = one(bomb);
    expect(formula.kind).toBe('source');
    if (formula.kind !== 'source') return;
    expect(formula.note).toContain('konnte nicht gesetzt werden');
  });

  it('never lets one bad formula cost the rest of the page', () => {
    const bomb = '{'.repeat(2000) + '}'.repeat(2000);
    const page = typesetDocument(doc(fence(bomb), fence('E = mc^2')));
    expect(page.get(bomb)!.kind).toBe('source');
    expect(page.get('E = mc^2')!.kind).toBe('typeset');
  });
});

describe('the caps, which are generous and are still caps', () => {
  it('refuses a fence past the character limit, and names the limit and the reason', () => {
    const huge = 'x + '.repeat(FORMULA_CHARACTER_LIMIT);
    expect(huge.length).toBeGreaterThan(FORMULA_CHARACTER_LIMIT);
    const formula = one(huge);
    expect(formula.kind).toBe('source');
    if (formula.kind !== 'source') return;
    expect(formula.note).toContain('20.000 Zeichen');
    expect(formula.note).toContain('5.000');
    // The reason, not only the number: this is paid for on the shared server rather than in
    // the reader's own tab, which is why the limit exists at all.
    expect(formula.note).toContain('Server');
  });

  it('refuses past the per-page count, and names that limit instead', () => {
    // Distinct fences, because identical ones are typeset once and looked up thereafter.
    const many = Array.from({ length: PAGE_FORMULA_LIMIT + 5 }, (_, i) => fence(`x^{${i}}`));
    const page = typesetDocument(doc(...many));
    expect(page.get('x^{0}')!.kind).toBe('typeset');
    const over = page.get(`x^{${PAGE_FORMULA_LIMIT + 4}}`)!;
    expect(over.kind).toBe('source');
    if (over.kind !== 'source') return;
    expect(over.note).toContain(`${PAGE_FORMULA_LIMIT} Formeln`);
    expect(over.note).toContain('Server');
  });

  it('counts a repeated formula once, because it is typeset once', () => {
    const repeated = Array.from({ length: PAGE_FORMULA_LIMIT + 5 }, () => fence('E = mc^2'));
    const page = typesetDocument(doc(...repeated, fence('a^2 + b^2 = c^2')));
    expect(page.get('E = mc^2')!.kind).toBe('typeset');
    expect(page.get('a^2 + b^2 = c^2')!.kind).toBe('typeset');
    expect(page.size).toBe(2);
  });

  it('refuses past the markup a page may carry, which the input limit does not bound', () => {
    // Measured rather than assumed: KaTeX amplifies. `x + ` repeated comes back at roughly
    // 320 characters of markup per SOURCE character, so a fence well inside the character
    // limit can be more than a megabyte of response on its own. The input cap therefore
    // bounds the parser's work and not the page's size, and this is the cap that does.
    const dense = 'x + '.repeat(1000);
    expect(dense.length).toBeLessThan(FORMULA_CHARACTER_LIMIT);
    const page = typesetDocument(doc(...Array.from({ length: 4 }, (_, i) => fence(`${dense} ${i}`))));

    const refused = [...page.values()].filter((formula) => formula.kind === 'source');
    expect(refused.length).toBeGreaterThan(0);
    expect(refused[0].kind === 'source' && refused[0].note).toContain('1.000.000 Zeichen');

    const set = [...page.values()]
      .map((formula) => (formula.kind === 'typeset' ? formula.html.length : 0))
      .reduce((a, b) => a + b, 0);
    expect(set).toBeLessThanOrEqual(PAGE_MARKUP_LIMIT);
  });

  it('stops at the markup budget rather than skipping past it', () => {
    // The budget bounded the SIZE of the answer and not the WORK: a formula refused for
    // crossing the line did not add to the total, so the total never rose, so every one of
    // the hundred allowed renders still ran. A page could therefore spend seconds of the
    // shared server's only thread and keep exactly one formula — measured at 7.3 s for a
    // hundred `\begin{array}` fences, all of them inside the per-formula character limit.
    //
    // "Did not render" has to be observable to be testable, and the shape that makes it
    // observable is a formula that CANNOT be rendered: KaTeX throws past `throwOnError` on
    // deep nesting, and a throw has its own note. So if the bomb below comes back with the
    // budget's note it was never handed to KaTeX, and if it comes back with the failure's
    // note it was. No timing assertion, and nothing that changes with the machine.
    const dense = 'x + '.repeat(1000);
    const bomb = '{'.repeat(2000) + '}'.repeat(2000);
    const page = typesetDocument(
      doc(...Array.from({ length: 4 }, (_, i) => fence(`${dense} ${i}`)), fence(bomb))
    );

    const refused = page.get(bomb)!;
    expect(refused.kind).toBe('source');
    if (refused.kind !== 'source') return;
    expect(refused.note).not.toContain('konnte nicht gesetzt werden');
    expect(refused.note).toContain('1.000.000 Zeichen');
  });

  it('stops at the page’s share of the server’s time, which the markup budget does not bound', () => {
    // The other half, and the reason the stop above is not the whole answer. Markup and CPU
    // are not proportional: `\text{a a a …}` measures at about 20 ms for 15 kB of markup on
    // this machine, so sixty-odd of them stay inside PAGE_MARKUP_LIMIT while costing well
    // over a second of the one thread every reader's page load is queued behind.
    //
    // Counted as time actually spent inside KaTeX, so a slow machine typesets fewer
    // formulas rather than taking longer — the budget belongs to the deployment and not to
    // the page.
    //
    // That ratio is what this test rests on, so it is written down: `\text{}` is the worst
    // cost-per-byte shape found (127 ms per 100 kB of markup, against 8–28 for arrays,
    // fractions and nested delimiters), which puts the time budget at about a fifth of the
    // markup budget. On a machine five times faster than this one the markup stop would
    // fire first and this test would go red naming the wrong limit — which is a diagnosable
    // failure and the reason the number is here rather than in a commit message.
    const slow = (n: number) => `\\text{${'a '.repeat(2400)}${n}}`;
    const page = typesetDocument(doc(...Array.from({ length: 100 }, (_, i) => fence(slow(i)))));

    expect(page.get(slow(0))!.kind).toBe('typeset');
    const notes = [...page.values()]
      .filter((formula) => formula.kind === 'source')
      .map((formula) => (formula.kind === 'source' ? formula.note : ''));
    expect(notes.length).toBeGreaterThan(0);
    expect(notes.some((note) => note.includes('Rechenzeit'))).toBe(true);
    // And the budget is spent on the formulas rather than on discovering it is spent: the
    // stop means the hundredth fence costs a map lookup, not a render.
    expect(notes.some((note) => note.includes('konnte nicht gesetzt werden'))).toBe(false);
  });
});

describe('walking the page for formulas', () => {
  it('finds a fence wherever it is nested', () => {
    const page = typesetDocument(
      doc(
        { kind: 'blockquote', content: [fence('a^2')] },
        { kind: 'bulletList', content: [{ kind: 'listItem', content: [fence('b^2')] }] },
        {
          kind: 'table',
          content: [{ kind: 'tableRow', content: [{ kind: 'tableCell', content: [fence('c^2')] }] }]
        }
      )
    );
    expect([...page.keys()].sort()).toEqual(['a^2', 'b^2', 'c^2']);
  });

  it('leaves every other fence alone', () => {
    const page = typesetDocument(
      doc(fence('let x = 1;', 'rust'), fence('graph TD;', 'mermaid'), fence('x', 'text'), {
        kind: 'codeBlock',
        content: [{ kind: 'text', text: 'ohne Sprache' }]
      })
    );
    expect(page.size).toBe(0);
  });

  it('reads a fence exactly as the reader will, split leaves and all', () => {
    // The key IS `codeText(block)`, which is what `BlockView` hands the leaf component. One
    // fence can reach the reader as several text leaves; a walker that joined them any
    // differently would key the map on a string the reader never asks for, and every
    // formula on the page would silently fall through to its own source.
    const split: Block = {
      kind: 'codeBlock',
      attrs: { language: 'math' },
      content: [
        { kind: 'text', text: 'a^2 +' },
        { kind: 'text', text: ' b^2' }
      ]
    };
    expect(typesetDocument(doc(split)).get('a^2 + b^2')!.kind).toBe('typeset');
  });

  it('answers an empty page, a missing body and a malformed one with an empty set', () => {
    expect(typesetDocument(doc()).size).toBe(0);
    expect(typesetDocument(null).size).toBe(0);
    expect(typesetDocument(undefined).size).toBe(0);
  });
});

describe('what reaches the page', () => {
  it('is HTML and MathML, so a screen reader gets the formula and not a picture of it', () => {
    const html = markup('E = mc^2');
    expect(html).toContain('<math');
    expect(html).toContain('<annotation encoding="application/x-tex">E = mc^2</annotation>');
    expect(html).toContain('class="katex-display"');
  });

  it('escapes the source it echoes back, so a fence cannot close KaTeX’s own tags', () => {
    // The one place in this reader where a STRING becomes markup, so the escaping is
    // KaTeX's rather than Svelte's and is asserted rather than assumed — including on the
    // path that echoes the author's own text back at them, which is where an unescaped `<`
    // would otherwise arrive.
    const html = markup('</span><img src=x onerror=alert(1)>');
    expect(html).not.toContain('<img');
    // Inside a TAG, which is the only place an event handler would be one. The words are
    // in the answer as text — KaTeX echoes the author's TeX inside `<annotation>` on
    // purpose — and that is exactly the difference this asserts.
    expect(html).not.toMatch(/<[^>]*\son[a-z]+\s*=/);
    // The angle brackets come back escaped in both places KaTeX echoes them: as maths, and
    // as the TeX annotation a screen reader reads.
    expect(html).toContain('&lt;/span&gt;&lt;img src=x onerror=alert(1)&gt;');
  });

  it('needs no DOM, which is what lets it run in a page load at all', () => {
    // KaTeX builds markup as a string; `renderToString` touches no document. Stated as a
    // test because it is the property this whole placement rests on — the moment a version
    // of KaTeX reached for `document`, this would fail here rather than in production.
    const real = Reflect.getOwnPropertyDescriptor(globalThis, 'document');
    Object.defineProperty(globalThis, 'document', {
      configurable: true,
      get() {
        throw new Error('there is no DOM in a server load');
      }
    });
    try {
      expect(one('\\sum_{i=1}^{n} i').kind).toBe('typeset');
    } finally {
      if (real) Object.defineProperty(globalThis, 'document', real);
      else Reflect.deleteProperty(globalThis, 'document');
    }
  });
});
