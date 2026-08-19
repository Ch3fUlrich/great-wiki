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
});
