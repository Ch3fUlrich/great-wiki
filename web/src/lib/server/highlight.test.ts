import { readFileSync } from 'node:fs';
import { describe, expect, it, vi } from 'vitest';
import {
  CODE_LANGUAGES,
  FENCE_CHARACTER_LIMIT,
  LINE_CHARACTER_LIMIT,
  PAGE_BUDGET_MS,
  PAGE_FENCE_LIMIT,
  PAGE_TOKEN_LIMIT,
  highlightDocument,
  highlightFence
} from './highlight';
import { fenceKey, type Fence } from '$lib/blocks/code';
import type { Block } from '$lib/blocks/render';

// Part of this suite reads `highlight.ts` as TEXT, the way `styles/fonts.test.ts` reads
// `tokens.css`. That is deliberate rather than lazy: the two things being protected here —
// that the regex engine is the JavaScript one, and that the grammar list is a closed,
// deliberate set — are properties of how the highlighter is CONSTRUCTED, and a test that
// only called it would pass just as happily against a WebAssembly engine in a Node process
// that has WebAssembly.
const source = readFileSync(new URL('./highlight.ts', import.meta.url), 'utf8');

/** The specifier of every `import … from '…'` in the module. */
const imported = [...source.matchAll(/from '([^']+)'/g)].map((match) => match[1]);

/** Everything a highlighted fence would print, in order. */
function printed(text: string, language: unknown): string {
  const fence = highlightFence(text, language);
  return fence.kind === 'highlighted' ? fence.tokens.map((token) => token.text).join('') : '';
}

describe('the regex engine', () => {
  it('is not WebAssembly — highlighting works with WebAssembly unreachable', async () => {
    // THE test of this step, and the only one that fails for the right reason. Shiki
    // defaults to Oniguruma compiled to WebAssembly, and instantiating that needs
    // `'wasm-unsafe-eval'` in `script-src`, which this application's policy does not have
    // (`web/vite.config.ts`, `docs/decisions/0007-content-security-policy.md`). A silent
    // fall back to the default would work in development, pass every test that does not
    // run in a browser, and leave every code block unstyled in production only.
    //
    // So WebAssembly is made unreachable the way the policy makes it unreachable — any
    // touch throws — and the module is imported FRESH underneath that, because the copy at
    // the top of this file was built while WebAssembly still existed.
    const real = Reflect.getOwnPropertyDescriptor(globalThis, 'WebAssembly');
    Object.defineProperty(globalThis, 'WebAssembly', {
      configurable: true,
      get() {
        throw new Error('refused by script-src: no wasm-unsafe-eval');
      }
    });
    try {
      vi.resetModules();
      const fresh = await import('./highlight');
      const fence = fresh.highlightFence('fn main() { let x = 1; }', 'rust');
      expect(fence.kind).toBe('highlighted');
      // Not merely "did not throw": a throw inside the highlighter is caught and answered
      // with a plain fence, so the assertion has to be that colour actually came out.
      if (fence.kind !== 'highlighted') return;
      expect(fence.tokens.some((token) => token.light !== null)).toBe(true);
    } finally {
      if (real) Object.defineProperty(globalThis, 'WebAssembly', real);
      vi.resetModules();
    }
  });

  it('is configured explicitly, and no WebAssembly engine is imported at all', () => {
    expect(imported).toContain('shiki/engine/javascript');
    expect(source).toMatch(/engine:\s*createJavaScriptRegexEngine\(/);
    // The engine is an OPTION, so leaving it out is a one-line regression rather than an
    // exotic one. Anything reaching for the WASM build — `shiki/engine/oniguruma`,
    // `shiki/wasm`, the `shiki` root bundle whose shorthands default to it — is refused
    // here by specifier, so that the prose above may name Oniguruma freely.
    expect(imported.filter((specifier) => /wasm|oniguruma/i.test(specifier))).toEqual([]);
    expect(imported).not.toContain('shiki');
  });
});

describe('the grammar set', () => {
  it('is exactly the eight languages that were chosen', () => {
    expect([...CODE_LANGUAGES]).toEqual([
      'json',
      'markdown',
      'python',
      'rust',
      'shellscript',
      'sql',
      'typescript',
      'yaml'
    ]);
  });

  it('says beside itself that adding a language is a deliberate act', () => {
    // The sentence is the point of the list being in one named place: a ninth grammar
    // costs a download every reader of this wiki pays, so it is added on purpose or not
    // at all. Pinned because a sentence in a comment is exactly what a tidy-up deletes.
    expect(source).toContain('adding a language is a deliberate act');
  });

  it('answers to each grammar’s own aliases, and to nothing else', () => {
    // The aliases are read off the grammars rather than typed out again here, so `sh`,
    // `yml` and `ts` are known for the same reason `shellscript`, `yaml` and `typescript`
    // are. A language that is not in the set stays unknown however plausible it looks.
    for (const alias of ['sh', 'bash', 'zsh', 'shell', 'yml', 'ts', 'rs', 'md', 'py']) {
      expect(highlightFence('x', alias).kind, alias).toBe('highlighted');
    }
    for (const stranger of ['kotlin', 'js', 'javascript', 'html', 'mermaid', 'math']) {
      expect(highlightFence('x', stranger).kind, stranger).toBe('plain');
    }
  });
});

describe('what a fence renders as', () => {
  it('highlights a language it knows', () => {
    const fence = highlightFence('SELECT id FROM documents;', 'sql');
    expect(fence.kind).toBe('highlighted');
    if (fence.kind !== 'highlighted') return;
    expect(new Set(fence.tokens.map((token) => token.light)).size).toBeGreaterThan(1);
  });

  it('says nothing at all about a fence that states no language', () => {
    // D-25: a fence with no language is not an unknown language. The author said nothing
    // and the page must not argue with them.
    for (const nothing of [undefined, null, '', '   ', 42, { language: 'rust' }]) {
      expect(highlightFence('x = 1', nothing)).toEqual({ kind: 'plain', note: null });
    }
  });

  it('names the language it does not know, on the block', () => {
    // D-25: an author who writes ```kotlin and sees no colour cannot otherwise tell
    // whether the wiki does not know Kotlin, whether they misspelled it, or whether
    // highlighting is broken. Naming it answers all three.
    const fence = highlightFence('fun main() {}', 'kotlin');
    expect(fence).toEqual({ kind: 'plain', note: 'Unbekannte Sprache: kotlin' });
  });

  it('treats a deliberate `text` fence as deliberate rather than unknown', () => {
    // ```text and ```plain are the escape hatch D-18 reserves — they never highlight and
    // (once step 4 and 5 land) never draw. Labelling them "unknown" would be the page
    // arguing with an author who said exactly what they meant.
    for (const plain of ['text', 'plain', 'plaintext', 'txt']) {
      expect(highlightFence('x', plain), plain).toEqual({ kind: 'plain', note: null });
    }
  });

  it('caps and flattens the language it prints, because nothing validates that string', () => {
    // `attrs.language` is the info string's first token on the way in (`markdown.rs`) and
    // an arbitrary string over the collab socket — nothing between the editor and
    // `documents.body` validates it. It is printed as TEXT, so Svelte escapes it; what is
    // left to get wrong is length and line breaks.
    const long = highlightFence('x', 'k'.repeat(500));
    expect(long.kind).toBe('plain');
    if (long.kind !== 'plain') return;
    expect(long.note!.length).toBeLessThanOrEqual(64);

    const multiline = highlightFence('x', 'kot\nlin');
    expect(multiline).toEqual({ kind: 'plain', note: 'Unbekannte Sprache: kot lin' });
  });

  it('refuses to tokenise a fence past the character limit', () => {
    // Highlighting runs during server rendering, on the single connection every other
    // reader is queued behind, so an enormous fence is an availability lever rather than
    // a slow page. Over the limit it renders as ordinary code and says so.
    const huge = 'let x = 1;\n'.repeat(FENCE_CHARACTER_LIMIT);
    expect(huge.length).toBeGreaterThan(FENCE_CHARACTER_LIMIT);
    const fence = highlightFence(huge, 'rust');
    expect(fence).toEqual({ kind: 'plain', note: 'Zu lang für die Hervorhebung' });
  });

  it('prints back every character it was given, or gives up', () => {
    // The whole point of step 1 was that a fence's whitespace IS its content. A
    // highlighter that silently dropped a character would be that bug again, arriving
    // through a dependency this time, so the reconstruction is checked at run time and a
    // fence that does not survive it renders plain.
    for (const text of ['fn f() {}', 'a\n\n  b\n', '\tx\n', 'ä ö ü 🚀 = 1', '', '\n\n']) {
      expect(printed(text, 'rust'), JSON.stringify(text)).toBe(text);
    }
  });

  it('answers a highlighter that throws with an uncoloured block, not with a 500', async () => {
    // This runs during SERVER rendering, and an uncaught throw inside a Svelte component
    // there is a 500 for the whole route — which is also the edit surface, so the page
    // that caused it could not be repaired through the editor either. A grammar that
    // throws must therefore cost this one block its colour and nothing else.
    vi.resetModules();
    vi.doMock('shiki/core', async (importOriginal) => {
      const real = await importOriginal<typeof import('shiki/core')>();
      return {
        ...real,
        createHighlighterCoreSync: ((
          ...args: Parameters<typeof real.createHighlighterCoreSync>
        ) => ({
          ...real.createHighlighterCoreSync(...args),
          codeToTokens: () => {
            throw new Error('a grammar this suite planted');
          }
        })) as typeof real.createHighlighterCoreSync
      };
    });
    try {
      const fresh = await import('./highlight');
      expect(fresh.highlightFence('fn main() {}', 'rust')).toEqual({
        kind: 'plain',
        note: 'Hervorhebung nicht möglich'
      });
    } finally {
      vi.doUnmock('shiki/core');
      vi.resetModules();
    }
  });

  it('gives up on a fence whose line endings the tokeniser would rewrite', () => {
    // Shiki splits on `\n` and hands back lines with the `\r` gone. Dropping it would be
    // an improvement to most eyes and a change to stored bytes either way, and this
    // renderer does not make that decision on the author's behalf.
    expect(highlightFence('a\r\nb', 'rust')).toEqual({ kind: 'plain', note: null });
  });

  it('takes every colour from the theme and never from the fence', () => {
    // ADR 0007 justifies `style-src-attr 'unsafe-inline'` partly on the sentence "the
    // renderer does not emit authored CSS into one either way". A token colour is bound
    // through Svelte's `style:` directive, so it lands in exactly such an attribute — and
    // this is what keeps that sentence true: a hex literal is admitted and nothing else
    // is, whatever the theme or a future version of it hands back.
    const hostile = 'let x = "red;background:url(https://angreifer.example/)";\n/* #zzzzzz */';
    const fence = highlightFence(hostile, 'rust');
    expect(fence.kind).toBe('highlighted');
    if (fence.kind !== 'highlighted') return;
    for (const token of fence.tokens) {
      for (const colour of [token.light, token.dark]) {
        if (colour !== null) expect(colour).toMatch(/^#[0-9a-fA-F]{3,8}$/);
      }
    }
  });

  it('carries a colour for each theme, because one image cannot match both', () => {
    // The site has a light/dark control and the reader may also be following the system
    // preference, so the choice cannot be made here: both colours ride along as custom
    // properties and the stylesheet picks.
    const fence = highlightFence('fn main() {}', 'rust');
    expect(fence.kind).toBe('highlighted');
    if (fence.kind !== 'highlighted') return;
    const coloured = fence.tokens.filter((token) => token.light !== null);
    expect(coloured.length).toBeGreaterThan(0);
    expect(coloured.every((token) => token.dark !== null)).toBe(true);
    expect(coloured.some((token) => token.light !== token.dark)).toBe(true);
  });
});

// --- the page, which is what the caps are actually about ---------------------------------
//
// `highlightFence` bounds ONE fence and cannot see how many a page has. That was the whole
// of the protection when tokenising lived in `CodeView.svelte`, and it is not protection at
// all: a page of five 20 000-character markdown fences answered in 51.98 s against a
// production build, with an unrelated page requested two seconds into that render waiting
// 48.85 s behind it, on an SSR process that is one thread and a store that is one SQLite
// connection.

/** A fence as `gw_core::markdown` imports one. */
function fence(text: string, language?: string): Block {
  return {
    kind: 'codeBlock',
    attrs: language ? { language } : undefined,
    content: [{ kind: 'text', text }]
  };
}

/** A document holding the given blocks. */
function doc(...content: Block[]): Block {
  return { kind: 'doc', content };
}

/** `n` characters of the given language, in lines this module is willing to tokenise. */
function listing(unit: string, characters: number): string {
  const width = LINE_CHARACTER_LIMIT;
  const line = unit.repeat(Math.ceil(width / unit.length)).slice(0, width);
  const lines = Math.max(1, Math.floor(characters / (width + 1)));
  return Array.from({ length: lines }, () => line).join('\n');
}

/** Everything the page decided about one fence. */
function of(page: Map<string, Fence>, text: string, language?: string): Fence {
  const fence = page.get(fenceKey(text, language ?? ''));
  expect(fence, `nothing was recorded for a ${language ?? 'bare'} fence`).toBeDefined();
  return fence!;
}

describe('walking a page for its fences', () => {
  it('finds a fence wherever it is nested, under the key the reader looks it up by', () => {
    const page = highlightDocument(
      doc({
        kind: 'blockquote',
        content: [{ kind: 'listItem', content: [fence('SELECT 1;', 'sql')] }]
      })
    );
    expect(of(page, 'SELECT 1;', 'sql').kind).toBe('highlighted');
  });

  it('keys on the language as well as the text, because one page can hold both', () => {
    // ```text is D-18's escape hatch and must stay uncoloured even when the identical
    // listing appears highlighted higher up the page. One key per (language, text).
    const page = highlightDocument(doc(fence('SELECT 1;', 'sql'), fence('SELECT 1;', 'text')));
    expect(of(page, 'SELECT 1;', 'sql').kind).toBe('highlighted');
    expect(of(page, 'SELECT 1;', 'text')).toEqual({ kind: 'plain', note: null });
  });

  it('leaves a formula and a diagram alone', () => {
    // `MathView` and `DiagramView` draw those, and show the fence's own source through
    // `CodeView` with `language="text"` when they cannot. Tokenising them would be work
    // nobody ever reads — and would spend the page's budget on it.
    const page = highlightDocument(doc(fence('E = mc^2', 'math'), fence('graph TD;', 'mermaid')));
    expect(page.size).toBe(0);
  });

  it('tokenises a repeated fence once', () => {
    const page = highlightDocument(doc(...Array.from({ length: 20 }, () => fence('x = 1', 'python'))));
    expect(page.size).toBe(1);
  });

  it('answers an empty page, a missing body and a malformed one with an empty set', () => {
    expect(highlightDocument(null).size).toBe(0);
    expect(highlightDocument(undefined).size).toBe(0);
    expect(highlightDocument(doc()).size).toBe(0);
  });
});

describe('the caps that belong to the page rather than to one block', () => {
  it('refuses a fence whose LONGEST LINE is past the limit, which is what the cost follows', () => {
    // The measurement that justifies this being the cap: 20 000 characters of TypeScript
    // costs 350 ms in 200-character lines and 1 003 ms as ONE line; 20 000 characters of
    // Markdown costs 6 ms against 11 286 ms for the same two shapes. Size is nearly free
    // and line length is not, so a pasted minified blob is the shape that hurts.
    const line = 'const x = 1; '.repeat(200).slice(0, LINE_CHARACTER_LIMIT + 1);
    const out = highlightFence(`fn main() {}\n${line}\n`, 'typescript');
    expect(out.kind).toBe('plain');
    if (out.kind !== 'plain') return;
    expect(out.note).toContain(`${LINE_CHARACTER_LIMIT + 1} Zeichen`);
    // And a fence made of many short lines is not refused for being long overall.
    expect(highlightFence(listing('const x = 1; ', 20_000), 'typescript').kind).toBe('highlighted');
  });

  it('stops at the number of fences one page may hold', () => {
    const many = Array.from({ length: PAGE_FENCE_LIMIT + 5 }, (_, i) => fence(`x = ${i}`, 'python'));
    const page = highlightDocument(doc(...many));
    expect(of(page, 'x = 0', 'python').kind).toBe('highlighted');
    const over = of(page, `x = ${PAGE_FENCE_LIMIT + 4}`, 'python');
    expect(over.kind).toBe('plain');
    if (over.kind !== 'plain') return;
    expect(over.note).toContain(`${PAGE_FENCE_LIMIT} Blöcke`);
  });

  it('stops at the tokens one page may carry, and carries no more than that plus one fence', () => {
    // The bound on the RESPONSE: the tokens travel twice, once as the rendered spans and
    // once as the page data SvelteKit serialises for hydration. Asserted as the property
    // rather than as a note, because the time budget can reach a page like this first and
    // either stop is the same guarantee — what must never happen is an unbounded page.
    const dicht = listing('{"a": 1} ', 20_000);
    const seiten = Array.from({ length: 8 }, (_, i) => fence(`${dicht}\n{"i": ${i}}`, 'json'));
    const page = highlightDocument(doc(...seiten));

    const tokens = [...page.values()]
      .map((fence) => (fence.kind === 'highlighted' ? fence.tokens.length : 0))
      .reduce((a, b) => a + b, 0);
    expect(tokens).toBeGreaterThan(0);
    expect(tokens).toBeLessThanOrEqual(PAGE_TOKEN_LIMIT * 2);

    const notes = [...page.values()]
      .filter((fence) => fence.kind === 'plain')
      .map((fence) => (fence.kind === 'plain' ? fence.note : null));
    expect(notes.length).toBeGreaterThan(0);
    expect(notes.every((note) => note?.includes('dieser Seite'))).toBe(true);
  });

  it('stops at the page’s share of the server’s time', () => {
    // A page may hold fences that are each inside every character limit and still cost the
    // one thread every other reader is queued behind more than it can spare. Two fences
    // like these measure at about 600 ms each, so the budget is spent inside the first —
    // and the second must then cost a lookup rather than a tokenisation.
    const teuer = (n: number) => `${listing('const x = 1; ', 20_000)}\n// ${n}`;
    const page = highlightDocument(doc(fence(teuer(1), 'typescript'), fence(teuer(2), 'typescript')));

    expect(of(page, teuer(1), 'typescript').kind).toBe('highlighted');
    const zweite = of(page, teuer(2), 'typescript');
    expect(zweite.kind).toBe('plain');
    if (zweite.kind !== 'plain') return;
    expect(zweite.note).toContain('Rechenzeit');
    expect(zweite.note).toContain(`${PAGE_BUDGET_MS}`);
  });

  it('tells a fence with nothing to explain nothing, even on an exhausted page', () => {
    // A page that has spent its budget still says nothing about a fence that states no
    // language, or `text`, or a language this wiki does not know: those have their own
    // answers already, and the page's budget is none of their business.
    const teuer = listing('const x = 1; ', 20_000);
    const page = highlightDocument(
      doc(
        fence(teuer, 'typescript'),
        fence('x', 'kotlin'),
        fence('x', 'text'),
        fence('x')
      )
    );
    expect(of(page, 'x', 'kotlin')).toEqual({ kind: 'plain', note: 'Unbekannte Sprache: kotlin' });
    expect(of(page, 'x', 'text')).toEqual({ kind: 'plain', note: null });
    expect(of(page, 'x')).toEqual({ kind: 'plain', note: null });
  });
});
