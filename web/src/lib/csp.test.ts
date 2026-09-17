import { describe, expect, it } from 'vitest';
import { diagramFramePolicy, widenCspNonceToStyles } from './csp';

// The exact header a production build emits, taken from a running `node build/index.js`
// rather than written by hand — the point of these tests is the real string. `frame-src` is
// `'self'` since D-26, which is the one directive that has changed since this was first
// captured; it is not what `widenCspNonceToStyles` looks at, and it is updated anyway,
// because a fixture that drifts from the real header stops being evidence of anything.
const REAL =
  "default-src 'self'; frame-src 'self'; connect-src 'self'; font-src 'self'; " +
  "img-src 'self' data:; object-src 'none'; script-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='; " +
  "style-src 'self'; style-src-attr 'unsafe-inline'; base-uri 'none'; form-action 'self'; " +
  "frame-ancestors 'self'";

describe('widenCspNonceToStyles', () => {
  it('adds the response nonce to style-src, which is what makes TipTap`s stylesheet load', () => {
    const out = widenCspNonceToStyles(REAL);
    expect(out).toContain("style-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='");
  });

  it('leaves style-src-attr alone', () => {
    // A nonce beside `'unsafe-inline'` makes a browser IGNORE the `'unsafe-inline'`, which
    // would break every server-rendered `style="…"` attribute on the site at once.
    expect(widenCspNonceToStyles(REAL)).toContain("style-src-attr 'unsafe-inline'");
    expect(widenCspNonceToStyles(REAL)).not.toContain("style-src-attr 'unsafe-inline' 'nonce-");
  });

  it('changes nothing else about the policy', () => {
    const out = widenCspNonceToStyles(REAL);
    for (const directive of [
      "default-src 'self'",
      "frame-src 'self'",
      "connect-src 'self'",
      "object-src 'none'",
      "base-uri 'none'",
      "form-action 'self'",
      "frame-ancestors 'self'",
      "script-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='"
    ]) {
      expect(out).toContain(directive);
    }
    // One directive gained one source and nothing else moved.
    expect(out.length).toBe(REAL.length + " 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='".length);
  });

  it('does nothing when there is no nonce to widen with', () => {
    const hashed = "default-src 'self'; script-src 'self'; style-src 'self'";
    expect(widenCspNonceToStyles(hashed)).toBe(hashed);
  });

  it('does not invent a style-src where the policy relies on default-src', () => {
    // Appending one would silently drop every source `default-src` was covering.
    const noStyleSrc = "default-src 'self' https://cdn.example; script-src 'self' 'nonce-abc'";
    expect(widenCspNonceToStyles(noStyleSrc)).toBe(noStyleSrc);
  });

  it('is idempotent, so a double application cannot duplicate the source', () => {
    const once = widenCspNonceToStyles(REAL);
    expect(widenCspNonceToStyles(once)).toBe(once);
  });

  it('leaves a style-src that already allows unsafe-inline completely alone', () => {
    // The regression this rule exists for, and the exact header `just dev` sends: SvelteKit
    // adds `'unsafe-inline'` to `style-src` in development so it can inject component
    // styles. A nonce in the same source list makes browsers IGNORE `'unsafe-inline'`, so
    // widening here NARROWS the policy — fourteen `Applying inline style violates …` errors
    // and an unstyled page, in development only, while production stayed clean.
    const dev =
      "default-src 'self'; script-src 'self' 'nonce-abc'; " +
      "style-src 'self' 'unsafe-inline'; style-src-attr 'unsafe-inline'";
    expect(widenCspNonceToStyles(dev)).toBe(dev);
  });

  it('does not mistake a nonce in another directive for the script nonce', () => {
    const odd = "style-src 'self'; script-src 'self' 'nonce-real'; connect-src 'nonce-decoy'";
    expect(widenCspNonceToStyles(odd)).toContain("style-src 'self' 'nonce-real'");
  });
});

// The policy the one framed route answers with (D-26) — the same generated header every
// other response carries, because `kit.csp` is one configuration for the whole application
// and SvelteKit offers no per-route form of it. That is the point: the frame's policy is made
// by REPLACING directives in this string, in `hooks.server.ts`.
const RAHMEN_EINGANG = REAL;

describe('diagramFramePolicy', () => {
  it('lets mermaid inject the <style> it measures text with, which is the whole point', () => {
    // The defect: mermaid inserts a `<style>` ELEMENT into the document while it measures a
    // label, and `style-src 'self'` refuses it — four console errors per diagram, and text
    // measured against the page's font rather than the one it is drawn with. Loosening this
    // on the PAGE is the one thing ADR 0007 refused; loosening it on a document that holds
    // nothing but the renderer is what D-26 buys.
    const out = diagramFramePolicy(RAHMEN_EINGANG);
    expect(out).toContain("style-src 'self' 'unsafe-inline'");
  });

  it('carries no nonce in style-src, because a nonce there would switch the loosening off', () => {
    // A source list containing a nonce makes browsers IGNORE `'unsafe-inline'` in the same
    // list — the rule `widenCspNonceToStyles` exists to respect. So this policy must reach
    // the browser WITHOUT going through that function, and must not grow a nonce of its own.
    const out = diagramFramePolicy(RAHMEN_EINGANG);
    const styleSrc = out.split(';').find((part) => /^\s*style-src\s/.test(part)) ?? '';
    expect(styleSrc).not.toContain('nonce-');
  });

  it('strips a nonce a caller had already widened style-src with', () => {
    // Belt and braces for the same rule: if these two functions are ever applied in the
    // wrong order, the frame must still be a frame that works rather than one where
    // `'unsafe-inline'` is silently inert.
    const schon = widenCspNonceToStyles(RAHMEN_EINGANG);
    const out = diagramFramePolicy(schon);
    expect(out).toContain("style-src 'self' 'unsafe-inline'");
    expect(out.split(';').find((part) => /^\s*style-src\s/.test(part))).not.toContain('nonce-');
  });

  it('closes frame-src again, so the frame frames nothing itself', () => {
    // `frame-src` opens to `'self'` on the PAGE so that the diagram frame can exist at all.
    // The frame inherits that from the same configuration and has no use for it: a document
    // whose only job is to run one library must not be a place to hang another frame —
    // including the `<iframe src="data:text/html;base64,…">` that mermaid's
    // `securityLevel: 'sandbox'` would emit, which ADR 0018 refuses on the page and refuses
    // here for the same reason.
    expect(diagramFramePolicy(RAHMEN_EINGANG)).toContain("frame-src 'none'");
    expect(diagramFramePolicy(RAHMEN_EINGANG)).not.toContain("frame-src 'self'");
  });

  it('keeps the frame un-embeddable by anyone but this origin', () => {
    // The other half of `frame-src 'self'`: this origin may frame the renderer, and nobody
    // else may. `frame-ancestors` is the directive that says so, and it is the one a
    // reverse proxy's X-Frame-Options cannot express per-route.
    expect(diagramFramePolicy(RAHMEN_EINGANG)).toContain("frame-ancestors 'self'");
  });

  it('keeps script-src exactly as strict as the page, nonce included', () => {
    // The frame is a same-origin document holding the session cookie, so the directive that
    // matters most is untouched: no `'unsafe-inline'`, no `'unsafe-eval'`, `'self'` for the
    // module chunks and the response nonce for app.html's pre-paint theme script.
    const out = diagramFramePolicy(RAHMEN_EINGANG);
    expect(out).toContain("script-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='");
    expect(out).not.toContain("'unsafe-eval'");
    const scriptSrc = out.split(';').find((part) => /^\s*script-src\s/.test(part)) ?? '';
    expect(scriptSrc).not.toContain("'unsafe-inline'");
  });

  it('changes nothing else at all', () => {
    const out = diagramFramePolicy(RAHMEN_EINGANG);
    for (const directive of [
      "default-src 'self'",
      "connect-src 'self'",
      "font-src 'self'",
      "img-src 'self' data:",
      "object-src 'none'",
      "style-src-attr 'unsafe-inline'",
      "base-uri 'none'",
      "form-action 'self'"
    ]) {
      expect(out).toContain(directive);
    }
  });

  it('is idempotent', () => {
    const once = diagramFramePolicy(RAHMEN_EINGANG);
    expect(diagramFramePolicy(once)).toBe(once);
  });

  it('states both directives even when the policy left them to default-src', () => {
    // Unlike `widenCspNonceToStyles`, this one may not decline: a frame whose `style-src`
    // fell through to `default-src 'self'` is a frame with the defect still in it, and a
    // frame whose `frame-src` fell through to `'self'` is one that can hang another frame.
    // Saying the value outright is the point of a route-scoped policy.
    const knapp = "default-src 'self'; script-src 'self' 'nonce-abc'";
    const out = diagramFramePolicy(knapp);
    expect(out).toContain("style-src 'self' 'unsafe-inline'");
    expect(out).toContain("frame-src 'none'");
    expect(out).toContain("default-src 'self'");
    expect(out).toContain("script-src 'self' 'nonce-abc'");
  });
});
