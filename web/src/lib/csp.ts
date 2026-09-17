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

/**
 * The policy for the one route Mermaid runs in (D-26, ADR 0018).
 *
 * # Why a second policy exists at all
 *
 * Mermaid needs a browser DOM to measure text, and while it measures it inserts a `<style>`
 * ELEMENT into the document. `style-src 'self'` refuses that element — correctly, that is
 * the CSS-injection class — so every diagram logged four *"Refused to apply inline style"*
 * errors and mermaid measured its labels against the page's font rather than the one it was
 * about to draw with. The library has no nonce hook (verified against `mermaid@11.17.2`),
 * and `'unsafe-inline'` on the PAGE is the one loosening
 * [ADR 0007](../../../docs/decisions/0007-content-security-policy.md) refused — it would
 * also make [widenCspNonceToStyles] skip the directive and unstyle the editor in production
 * only.
 *
 * So the library was moved out of the page instead. It runs in an `<iframe>` served from
 * `DIAGRAM_FRAME_PATH` (`$lib/blocks/diagram`), and THAT response gets this policy.
 *
 * # Exactly two directives move, and both are stated rather than edited
 *
 * - **`style-src` becomes `'self' 'unsafe-inline'`.** This document holds the renderer and
 *   nothing else: no session-bearing form, no editor, no page content, no route a reader is
 *   ever sent to. What the loosening admits is one `<style>` element per render, built by
 *   mermaid from a theme the diagram itself may not set (`SECURE_CONFIG_KEYS`). Any nonce
 *   already in the list is REMOVED, because a source list containing a nonce makes browsers
 *   ignore `'unsafe-inline'` beside it — the same rule [widenCspNonceToStyles] respects from
 *   the other side, and the reason this function must be used INSTEAD of that one rather
 *   than after it.
 * - **`frame-src` becomes `'none'`.** It opens to `'self'` on the page so this frame can
 *   exist; the frame itself frames nothing, and in particular not the
 *   `<iframe src="data:text/html;base64,…">` mermaid's `securityLevel: 'sandbox'` would
 *   emit, which ADR 0018 refuses for the page and refuses here for the same reason.
 *
 * Everything else is left byte-for-byte, and that is the load-bearing part.
 * `script-src 'self' 'nonce-…'` in particular is untouched: the frame is a SAME-ORIGIN
 * document (it has to be — see `web/src/routes/_diagramm/rahmen.ts` for why an opaque origin
 * cannot load the module chunks it needs), so it holds the session cookie, and the directive
 * that keeps an injected handler from executing is the one that must not move.
 * `frame-ancestors 'self'` is likewise inherited unchanged, which is what stops anyone else
 * embedding the renderer.
 *
 * Unlike [widenCspNonceToStyles] this function may not decline when a directive is absent.
 * A `style-src` left to fall through to `default-src 'self'` is the defect still in place,
 * and a `frame-src` left to fall through is a frame that can hang another frame — so both
 * are appended when they are not already there. That is what "a policy scoped to this route"
 * means: it says its own values instead of inheriting somebody's.
 */
export function diagramFramePolicy(policy: string): string {
  return setzeDirektive(
    setzeDirektive(policy, 'style-src', "'self' 'unsafe-inline'"),
    'frame-src',
    "'none'"
  );
}

/**
 * One directive given an exact value, keeping the rest of the header as it was — and
 * appended if the policy never named it.
 *
 * The name is matched with a following space so that `style-src-attr` and `style-src-elem`
 * are different directives, exactly as they are to a browser. That distinction is the one
 * [widenCspNonceToStyles] also turns on, and getting it wrong here would take
 * `style-src-attr`'s `'unsafe-inline'` away and unstyle every server-rendered `style="…"`
 * attribute on the site.
 */
function setzeDirektive(policy: string, name: string, quellen: string): string {
  const muster = new RegExp(`^\\s*${name}\\s`);
  const teile = policy.split(';');
  const index = teile.findIndex((teil) => muster.test(teil));
  if (index === -1) return `${policy.replace(/;\s*$/, '')}; ${name} ${quellen}`;
  teile[index] = ` ${name} ${quellen}`;
  return teile.join(';').replace(/^\s+/, '');
}
