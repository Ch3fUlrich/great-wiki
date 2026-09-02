#!/usr/bin/env node
/**
 * Copy KaTeX's stylesheet and its typefaces into this repository, self-hosted.
 *
 * Run it from `web/`, after `npm install`:
 *
 *     node scripts/vendor-katex.mjs
 *
 * It is idempotent, and `src/lib/styles/katex.test.ts` asserts that what is committed is
 * exactly what it would write — so a KaTeX version bump that changes either the stylesheet
 * or a font file turns that test red instead of shipping a maths face that no longer
 * matches the metrics the CSS was written against.
 *
 * # Why the fonts are copied at all
 *
 * The Content-Security-Policy is `font-src 'self'` (ADR 0007), and `fonts.css` records the
 * reason it is not a latency decision: a font fetched from a third party on every page load
 * hands that party the reader's IP address and the page they were reading, and this platform
 * has no lawful basis for that transfer. A CDN font under this policy does not fail loudly —
 * it fails to load, and the maths renders in whatever serif the machine has, at metrics
 * KaTeX did not compute, which is wrong in a way nobody reports.
 *
 * Only the `.woff2` cut is taken. KaTeX also ships `.woff` and `.ttf` for browsers that
 * predate woff2; this application's own faces are woff2-only already, and carrying three
 * formats would be 1.2 MB in the image to serve none of them.
 *
 * # Why the stylesheet is rewritten rather than imported
 *
 * `katex.css` addresses its fonts as `url(fonts/…)`, relative to itself inside
 * `node_modules`. Left alone, Vite would fingerprint and emit all sixty files from there —
 * the two formats nothing asks for included — and they would live under `_app/immutable/`
 * rather than beside every other typeface this application serves. Two substitutions fix
 * that, and they are the ONLY two: see `vendorCss`.
 */
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** `web/`, whatever directory this was invoked from. */
export const WEB = dirname(dirname(fileURLToPath(import.meta.url)));

/** Where the package puts what we take. */
export const UPSTREAM = join(WEB, 'node_modules', 'katex');

/** Where the fonts are served from, and the second half of every rewritten `url()`. */
export const FONT_DIR = join(WEB, 'static', 'fonts', 'katex');

/** The committed stylesheet. */
export const CSS_FILE = join(WEB, 'src', 'lib', 'styles', 'katex.css');

/** The version of KaTeX currently installed. */
export function installedVersion() {
  const meta = JSON.parse(readFileSync(join(UPSTREAM, 'package.json'), 'utf8'));
  return String(meta.version);
}

/**
 * The two substitutions, and nothing else.
 *
 * Each `@font-face` upstream lists three formats:
 *
 *     src: url(fonts/KaTeX_Main-Regular.woff2) format("woff2"),
 *          url(fonts/KaTeX_Main-Regular.woff) format("woff"),
 *          url(fonts/KaTeX_Main-Regular.ttf) format("truetype");
 *
 * and each becomes the woff2 entry alone, addressed absolutely from this origin and quoted
 * the way `fonts.css` quotes its own — which is not cosmetic: `styles/fonts.test.ts` finds
 * every shipped face by matching `url('/fonts/…')` with single quotes, and a double-quoted
 * one would silently drop out of the check that every file shipped is a file referenced.
 *
 * Everything else — every rule, every declaration, every byte of KaTeX's own layout — is
 * left exactly as published.
 *
 * @param {string} upstream the contents of `katex/dist/katex.css`
 * @returns {string}
 */
export function vendorCss(upstream) {
  return upstream.replace(
    /src: url\(fonts\/([A-Za-z0-9_-]+)\.woff2\) format\("woff2"\)[^;]*;/g,
    "src: url('/fonts/katex/$1.woff2') format('woff2');"
  );
}

/**
 * What is written above the vendored rules.
 *
 * @param {string} version
 * @returns {string}
 */
export function header(version) {
  return `/* KaTeX's own stylesheet — VENDORED, NOT WRITTEN HERE. Do not edit by hand.
 *
 * Provenance: npm \`katex@${version}\`, \`dist/katex.css\`, with exactly two substitutions
 * applied by \`web/scripts/vendor-katex.mjs\`:
 *
 *   - each \`src:\` list reduced to its woff2 entry, because that is the only cut shipped;
 *   - each face's address rewritten from a path inside the package to an absolute one
 *     under /fonts/katex/ here, because the files are served from this origin.
 *     (Written out rather than quoted: \`styles/fonts.test.ts\` finds every shipped face by
 *     matching that spelling, and an example in this comment would be a phantom file.)
 *
 * Regenerate with \`node scripts/vendor-katex.mjs\` after any version bump;
 * \`src/lib/styles/katex.test.ts\` fails if what is committed is not what that writes.
 *
 * UNLAYERED, unlike every other stylesheet in this application. Two reasons, and the
 * second is why it is safe: the \`@font-face\` rules must be unlayered anyway (app.css
 * says why — a face takes no part in the cascade and putting it in a layer would state a
 * precedence that is not true), and wrapping the rest would be a third substitution on
 * vendored bytes, which is what the sync test exists to prevent. Every rule below is
 * scoped under \`.katex\`, so it reaches nothing this application styles — with one
 * exception, \`body { counter-reset: katexEqnNo mmlEqnNo; }\` at the very end, which
 * declares the counters \`\\tag\` uses and is inert on a page without maths.
 *
 * Imported by \`MathView.svelte\` rather than by app.css, so it is fetched with the route
 * that can hold a formula and not with every page in the wiki. The typefaces themselves
 * are fetched only when a rule that names one matches something, so a page with no maths
 * downloads none of them.
 *
 * The .woff2 files are MIT, like KaTeX itself; \`static/fonts/katex/LICENSE\` is KaTeX's own
 * copy and the carve-out at the top of the root LICENSE names them. This is the one family
 * here that is not OFL, which is why \`styles/fonts.test.ts\` asks each family for the
 * licence it is actually under rather than for an OFL.txt.
 */
`;
}

/** The stylesheet exactly as it should be committed. */
export function expectedCss() {
  return header(installedVersion()) + vendorCss(readFileSync(join(UPSTREAM, 'dist/katex.css'), 'utf8'));
}

/** The woff2 files upstream ships, sorted. */
export function upstreamFonts() {
  return readdirSync(join(UPSTREAM, 'dist/fonts'))
    .filter((name) => name.endsWith('.woff2'))
    .sort();
}

function main() {
  writeFileSync(CSS_FILE, expectedCss());

  mkdirSync(FONT_DIR, { recursive: true });
  const wanted = new Set(upstreamFonts());
  // A face that upstream dropped must go, or `styles/fonts.test.ts` fails on dead weight in
  // the image — which is the point of that assertion.
  for (const name of readdirSync(FONT_DIR)) {
    if (name.endsWith('.woff2') && !wanted.has(name)) rmSync(join(FONT_DIR, name));
  }
  for (const name of wanted) {
    copyFileSync(join(UPSTREAM, 'dist/fonts', name), join(FONT_DIR, name));
  }
  copyFileSync(join(UPSTREAM, 'LICENSE'), join(FONT_DIR, 'LICENSE'));

  process.stdout.write(
    `katex@${installedVersion()}: ${wanted.size} woff2 faces and one stylesheet vendored\n`
  );
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
