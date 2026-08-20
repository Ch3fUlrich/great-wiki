/**
 * One repair to the Content-Security-Policy SvelteKit generates, kept as a pure function so
 * it can be tested without a server.
 *
 * # What is wrong with the generated header
 *
 * `kit.csp` (see `vite.config.ts`) is configured with `mode: 'nonce'`, and SvelteKit mints a
 * fresh nonce per response and puts it on every `<script>` it emits. It adds that nonce to
 * `script-src` — and to `style-src` ONLY IF it also emitted an inline `<style>` of its own
 * (`add_style` in `@sveltejs/kit/src/runtime/server/page/csp.js` is what appends it, and in a
 * production build with the default `inlineStyleThreshold` of 0 it is never called). So the
 * shipped header reads `script-src 'self' 'nonce-…'; style-src 'self'`, and a nonce on a
 * `<style>` element means nothing, because the policy never said that nonce was acceptable
 * there.
 *
 * That is not hypothetical. TipTap appends the ProseMirror base stylesheet as a `<style>`
 * ELEMENT when the editor is constructed, and supports being handed a nonce for exactly this
 * situation (`injectNonce`, wired up in `Editor.svelte`). Without this function the element
 * is refused, the editing surface loses `white-space: pre-wrap`, the gap cursor and the
 * hidden-selection rules, and — because SvelteKit adds `'unsafe-inline'` to `style-src` in
 * DEVELOPMENT so it can inject its own styles — it is refused *only in production*. A
 * difference that exists only in production is the worst place for one to live.
 *
 * # Why widen the policy rather than pin a hash
 *
 * The alternative is `style-src 'self' 'sha256-…'` over TipTap's stylesheet, which is
 * genuinely stricter: it admits one exact byte string instead of anything this page's own
 * code cares to inject. It was rejected because the hash is a copy of a dependency's
 * private constant. A TipTap upgrade that touches one CSS declaration invalidates it, and
 * the symptom is the production-only styling failure above — discovered by a reader, not by
 * a test. Handing the page's own nonce to a library that asks for it costs nothing an
 * attacker could not already have: writing a nonce'd `<style>` requires script execution,
 * and script execution is the thing `script-src` is there to prevent.
 *
 * # A nonce beside `'unsafe-inline'` REMOVES permission
 *
 * The one rule this function has to get right, and it was found by running the dev server
 * rather than by reading the spec: a source list containing a nonce or a hash makes browsers
 * IGNORE `'unsafe-inline'` in that same list. So widening a directive that already has
 * `'unsafe-inline'` narrows it.
 *
 * That is not hypothetical either. SvelteKit adds `'unsafe-inline'` to `style-src` in
 * DEVELOPMENT — it injects component styles as `<style>` elements it cannot nonce, and so
 * does Vite's client — and an unconditional widening turned `just dev` into fourteen
 * `Applying inline style violates …` errors and an unstyled page, while production was fine.
 * `style-src-attr` carries `'unsafe-inline'` in both, for Svelte's server-rendered
 * `style="…"` attributes.
 *
 * Hence: a directive that already permits inline content is left exactly as it is.
 */
export function widenCspNonceToStyles(policy: string): string {
  // The nonce is read back out of the header rather than passed in, because SvelteKit gives
  // the value to the page template and to nothing else — there is no hook argument carrying
  // it. `script-src` is where it is guaranteed to appear under `mode: 'nonce'`.
  const nonce = /(?:^|;)\s*script-src\s[^;]*'(nonce-[^']+)'/.exec(policy)?.[1];
  if (!nonce) return policy;

  // No `style-src` at all means the policy is relying on `default-src`, and appending a
  // directive here would silently drop that fallback's other sources — so a `part` that
  // does not match either guard below is returned untouched, and a policy with no
  // `style-src` at all comes back byte-for-byte the same as it went in. There used to be a
  // `widened` flag bookkeeping that outcome to choose between returning `parts.join(';')`
  // and returning `policy` directly, but `policy.split(';').join(';')` reconstructs
  // `policy` exactly, so both branches were always the same string; the flag tracked
  // nothing the return value could see. The configuration in vite.config.ts is what
  // decides the shape of the policy, this function only edits the parts already there.
  const parts = policy.split(';').map((part) => {
    // `style-src-attr` and `style-src-elem` are different directives and must not match:
    // the character after `style-src` has to be whitespace, not a hyphen.
    if (!/^\s*style-src\s/.test(part)) return part;
    // Already permits inline content, so adding the nonce would take that away.
    if (part.includes("'unsafe-inline'")) return part;
    return part.includes(`'${nonce}'`) ? part : `${part} '${nonce}'`;
  });

  return parts.join(';');
}
