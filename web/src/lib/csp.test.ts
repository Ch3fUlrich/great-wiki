import { describe, expect, it } from 'vitest';
import { widenCspNonceToStyles } from './csp';

// The exact header a production build emits, taken from a running `node build/index.js`
// rather than written by hand — the point of these tests is the real string.
const REAL =
  "default-src 'self'; frame-src 'none'; connect-src 'self'; font-src 'self'; " +
  "img-src 'self' data:; object-src 'none'; script-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='; " +
  "style-src 'self'; style-src-attr 'unsafe-inline'; base-uri 'self'; form-action 'self'; " +
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
      "frame-src 'none'",
      "connect-src 'self'",
      "object-src 'none'",
      "base-uri 'self'",
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
