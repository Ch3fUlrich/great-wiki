import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

// The interface addresses its reader as »Sie« (invite walkthrough, 2026-09-17). This keeps a
// »du« from coming back in any text a reader can see: string literals in scripts and the
// markup of components. Pronouns and possessives only — imperative du-verbs (»klicke«,
// »lade«) collide with too many ordinary words to check mechanically.

const SRC = fileURLToPath(new URL('..', import.meta.url));

// `\b` is not Unicode-aware in JavaScript, so the boundaries are lookarounds on letters,
// digits and `_` — which also keeps identifiers such as `content_dir` out.
const DU =
  /(?<![\p{L}\p{N}_])(?:du|dich|dir|dein|deine|deinen|deinem|deiner|Du|Dich|Dir|Dein|Deine|Deinen|Deinem|Deiner)(?![\p{L}\p{N}_])/u;

/** A deliberate exception names its file, the text and why. None so far. */
const ALLOWLIST: Array<{ file: string; text: string; reason: string }> = [];

function sources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return sources(path);
    if (/\.test\.ts$|\.d\.ts$/.test(entry.name)) return [];
    return /\.(ts|svelte)$/.test(entry.name) ? [path] : [];
  });
}

const stripComments = (code: string) =>
  code.replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:'"`])\/\/.*$/gm, '$1');

const literals = (code: string) =>
  [
    ...stripComments(code).matchAll(/'(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*"|`(?:[^`\\]|\\.)*`/g)
  ].map((m) => m[0]);

/** Everything a reader could see in a file: script literals, and markup text for components. */
export function readerText(file: string, source: string): string[] {
  if (!file.endsWith('.svelte')) return literals(source);
  const scripts = [...source.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/g)].map((m) => m[1]);
  const markup = source
    .replace(/<script\b[^>]*>[\s\S]*?<\/script>/g, '')
    .replace(/<style\b[^>]*>[\s\S]*?<\/style>/g, '')
    .replace(/<!--[\s\S]*?-->/g, '');
  return [...scripts.flatMap(literals), markup];
}

export function informal(texts: string[]): string[] {
  return texts.map((t) => t.match(DU)?.[0]).filter((w): w is string => w !== undefined);
}

describe('the interface says Sie', () => {
  it('finds »du« where a reader would see it, and nowhere else', () => {
    expect(
      informal(readerText('a.ts', `const a = 'Du kannst'; const b = "bevor du gehst";`))
    ).toEqual(['Du', 'du']);
    expect(informal(readerText('a.svelte', '<p>Deine Seite</p>'))).toEqual(['Deine']);
    expect(
      informal(
        readerText(
          'a.ts',
          `// du in a comment\nconst content_dir = 'Dudelsack, dual, directory: Sie dürfen';`
        )
      )
    ).toEqual([]);
    expect(
      informal(
        readerText('a.svelte', '<script>// dir\n</script><style>.dir{}</style><p>Ihre Seite</p>')
      )
    ).toEqual([]);
  });

  it('no component or module addresses the reader as »du«', () => {
    const files = sources(SRC);
    // A guard that reads nothing passes for the wrong reason: there are well over a
    // hundred sources, so a broken path shows up here rather than as a silent green.
    expect(files.length).toBeGreaterThan(100);
    const found = files.flatMap((file) => {
      const rel = relative(SRC, file);
      return informal(readerText(file, readFileSync(file, 'utf8')))
        .filter((text) => !ALLOWLIST.some((a) => a.file === rel && a.text === text))
        .map((text) => `${rel}: »${text}«`);
    });
    expect(found).toEqual([]);
  });
});
