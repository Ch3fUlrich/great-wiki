import { describe, expect, it } from 'vitest';
import { safeHref } from '$lib/blocks/render';
import { normalizeLinkAddress } from './linkAddress';

const ORIGIN = 'https://wiki.example.org';

describe('normalizeLinkAddress', () => {
  it('turns a same-origin absolute URL into its path', () => {
    // The exact shape the prompt used to invite: "paste the address bar". `wiki_path` on
    // the server only ever resolves a scheme-less, authority-less reference, so an absolute
    // self-link recorded no edge at all until this ran at insert time instead.
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', `${ORIGIN}/darm/labor`)).toBe(
      '/darm/labor'
    );
  });

  it('keeps the query and the fragment of a same-origin absolute URL', () => {
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', `${ORIGIN}/ziel?von=hier#abschnitt`)).toBe(
      '/ziel?von=hier#abschnitt'
    );
  });

  it('leaves a foreign absolute URL untouched', () => {
    // Not this wiki's page to claim an edge for, exactly as `wiki_path` treats it.
    const foreign = 'https://example.org/ziel-a';
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', foreign)).toBe(foreign);
  });

  it('resolves a relative reference against the CURRENT page, not the root', () => {
    // The probe that found the server-side half of this bug: `nachbar`, written on
    // `/rundgang/tabellen`, is a click to `/rundgang/nachbar` — never to `/nachbar` — because
    // there is no `<base>` in `web/src/app.html`. This is the point this function exists for.
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', 'nachbar')).toBe(
      '/rundgang/nachbar'
    );
  });

  it('resolves `.` and `..` against the current page as a browser would', () => {
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', '../ziel-a')).toBe('/ziel-a');
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', './nachbar')).toBe(
      '/rundgang/nachbar'
    );
  });

  it('root-anchors a relative reference that was already absolute-looking, i.e. leading /', () => {
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', '/ziel-a')).toBe('/ziel-a');
  });

  it('leaves an empty or whitespace-only address alone', () => {
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', '')).toBe('');
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', '   ')).toBe('');
  });

  it('does not itself defeat safeHref: a javascript: address is still rejected downstream', () => {
    // This function's job is address SHAPE, not safety — `safeHref` is the sink both the
    // editor and the renderer already go through, and a scheme address has its own origin
    // (`"null"` for a non-special scheme), so it is left alone here rather than silently
    // resolved into something that passes.
    const typed = 'javascript:alert(1)';
    const normalized = normalizeLinkAddress(ORIGIN, '/rundgang', typed);
    expect(normalized).toBe(typed);
    expect(safeHref(normalized)).toBeNull();
  });

  it('leaves a mailto: address unchanged, same as any other foreign scheme', () => {
    const typed = 'mailto:jemand@example.org';
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', typed)).toBe(typed);
  });

  it('leaves a PROTOCOL-RELATIVE address on a foreign host alone instead of making it internal', () => {
    // The bug: `//evil.example.com/phish` carries no scheme, so `new URL()` with no base
    // throws and it used to fall into the relative branch — where resolving it against the
    // current page produced `https://evil.example.com/phish`, and taking `.pathname` off
    // that threw the HOST away. The address collapsed to `/phish`: a link the author wrote
    // to somewhere else silently became a link to a different page of THIS wiki, and the
    // owner would then publish it.
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', '//evil.example.com/phish')).toBe(
      '//evil.example.com/phish'
    );
    // Also on a nested current page, where the old resolution would have produced a
    // different — but equally wrong — internal path.
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', '//evil.example.com/phish')).toBe(
      '//evil.example.com/phish'
    );
    // Query and fragment go with it, untouched.
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', '//evil.example.com/p?a=1#b')).toBe(
      '//evil.example.com/p?a=1#b'
    );
  });

  it('still turns a protocol-relative address that names THIS origin into its path', () => {
    // Scheme-less but not foreign. `//wiki.example.org/ziel` addresses the same origin the
    // absolute form does, so it gets the same answer the absolute form gets — the rule is
    // "which origin does this address", not "does it spell out a scheme".
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', '//wiki.example.org/ziel')).toBe('/ziel');
  });

  it('agrees with safeHref about protocol-relative: both treat it as an external address', () => {
    // `safeHref` resolves against a placeholder base, so `//evil.example` takes that base's
    // https scheme and is ALLOWED — deliberately, because an outright `https://evil.example`
    // is allowed too and nothing is granted by the shorter spelling. The two functions have
    // to agree on what the address MEANS, and now they do: external stays external.
    const typed = '//evil.example/phish';
    expect(normalizeLinkAddress(ORIGIN, '/rundgang', typed)).toBe(typed);
    expect(safeHref(typed)).toBe(typed);
  });

  it('leaves a relative reference alone, which is what the origin comparison must not break', () => {
    // A genuinely relative reference can never resolve to another origin, so the guard added
    // for protocol-relative addresses is inert for every one of these.
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', '#abschnitt')).toBe(
      '/rundgang/tabellen#abschnitt'
    );
    expect(normalizeLinkAddress(ORIGIN, '/rundgang/tabellen', '?von=hier')).toBe(
      '/rundgang/tabellen?von=hier'
    );
  });
});
