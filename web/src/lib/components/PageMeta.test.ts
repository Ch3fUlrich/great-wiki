import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import PageMeta from './PageMeta.svelte';
import type { RevisionInfo } from '$lib/pagemeta';

interface Props {
  visibility?: string;
  language?: string;
  docType?: string;
  revision?: RevisionInfo;
}

/** A page as the corpus actually stands today: restricted, German, an ordinary page. */
function html(props: Props = {}): string {
  return render(PageMeta, {
    props: {
      visibility: 'restricted',
      language: 'de',
      docType: 'page',
      ...props
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('PageMeta', () => {
  it('always states who can read the page, in a sentence', () => {
    const out = html();
    expect(out).toContain('Sichtbarkeit');
    expect(out).toContain('Eingeschränkt');
    expect(out).toContain('Nur ausdrücklich berechtigte Personen und Teams können diese Seite lesen.');
  });

  it('leaves a public page impossible to misread as merely internal', () => {
    const out = html({ visibility: 'public' });
    expect(out).toContain('Öffentlich im Internet');
    expect(out).toContain('ohne Anmeldung');
  });

  it('marks only the public state visually, and names all three for a theme', () => {
    // Two visual states, not three: the reader's question is "can strangers read this?".
    // `data-visibility` still distinguishes internal from restricted, so a plugin can
    // paint them differently without this component shipping rules it does not use.
    expect(html({ visibility: 'public' })).toMatch(/class="[^"]*\bchip--public\b/);
    for (const visibility of ['internal', 'restricted']) {
      const out = html({ visibility });
      expect(out).not.toContain('chip--public');
      expect(out).toContain(`data-visibility="${visibility}"`);
    }
  });

  it('reports an unparseable visibility as the strictest one', () => {
    // Which is what the permission engine will do with it. Saying "unbekannt" would
    // invite the reader to assume the page is more open than the server allows.
    const out = html({ visibility: 'weltweit' });
    expect(out).toContain('Eingeschränkt');
    expect(out).toContain('data-visibility="restricted"');
  });

  // --- Rows that stay silent when they have nothing to say ----------------------------

  it('says nothing about a German page in a German interface', () => {
    expect(html({ language: 'de' })).not.toContain('Sprache');
  });

  it('names the language when the page is not in the language around it', () => {
    const out = html({ language: 'en' });
    expect(out).toContain('Sprache');
    expect(out).toContain('Englisch');
  });

  it('says nothing about the ordinary document type, and names any other', () => {
    expect(html({ docType: 'page' })).not.toContain('Dokumentart');
    const out = html({ docType: 'research' });
    expect(out).toContain('Dokumentart');
    expect(out).toContain('Recherche');
  });

  // --- The revision slot ---------------------------------------------------------------
  //
  // There is no revisions endpoint yet, so nothing constructs a `RevisionInfo` in the
  // application. These two tests are the contract for whoever lands it.

  it('shows nothing at all where the edit history would go', () => {
    // The failure this guards is not a missing feature but an invented one: a dash, an
    // "unbekannt", or today's date sitting in a panel whose other rows are facts.
    const out = html();
    expect(out).not.toContain('Zuletzt bearbeitet');
    expect(out).not.toContain('unbekannt');
    expect(out).not.toContain('<time');
    // No date-shaped text anywhere, in either notation.
    expect(out).not.toMatch(/\d{1,2}\.\s*\w+\s*\d{4}/);
    expect(out).not.toMatch(/\d{4}-\d{2}-\d{2}/);
  });

  it('renders the edit line once a revision is supplied, machine-readable date included', () => {
    const out = html({
      revision: { edited_at: '2026-07-04T09:30:00Z', edited_by: 'Sergej', number: 7 }
    });
    expect(out).toContain('Zuletzt bearbeitet');
    // The stored instant stays in `datetime`, so the machine-readable value is the exact
    // one the API sent while the human-readable one is local and German.
    expect(out).toContain('datetime="2026-07-04T09:30:00Z"');
    expect(out).toContain('4. Juli 2026 um 11:30');
    expect(out).toContain('Sergej');
    expect(out).toContain('Fassung 7');
  });

  it('is a named region rather than an anonymous box of words', () => {
    expect(html()).toMatch(/aria-label="Angaben zu dieser Seite"/);
  });
});
