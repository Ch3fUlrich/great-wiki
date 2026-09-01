import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import PageTopics from './PageTopics.svelte';
import type { Topic, TopicSummary } from '$lib/topics';

/**
 * What a page is about, shown and edited **on the page itself**.
 *
 * The owner's second decision: tagging is something you do while reading, not something that
 * requires opening the editor. So the chips sit under the title, clicking one browses that
 * topic, and the control that adds and removes is right there beside them.
 *
 * Rendered with `svelte/server`, which is the first response. That is the requirement rather
 * than a preference: every control here is a link or a real form submission, so a reader with
 * JavaScript switched off can still see what a page is about, follow a topic, and change one
 * — and a click handler could not pass a single one of these tests.
 */
const topics: Topic[] = [
  { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen' },
  { path: '/format', name: 'Format', display_path: 'Format' }
];

const vorschlaege: TopicSummary[] = [
  { path: '/format', name: 'Format', display_path: 'Format', documents: 1 },
  { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang', documents: 3 },
  { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen', documents: 1 }
];

function html(
  {
    themen = topics,
    alle = vorschlaege,
    darfSchreiben = false,
    fehler = null,
    getippt = ''
  }: {
    themen?: Topic[];
    alle?: TopicSummary[];
    darfSchreiben?: boolean;
    fehler?: string | null;
    getippt?: string;
  } = {}
): string {
  return render(PageTopics, {
    props: { themen, alle, darfSchreiben, fehler, getippt }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the chips', () => {
  it('spells each topic in full, so a nested one is not mistaken for a top-level one', () => {
    // The opposite decision from the tree, and for a stated reason: a chip has no list around
    // it to say where the topic sits, so it carries its whole ancestry.
    const out = html();
    expect(out).toContain('Rundgang/Tabellen');
  });

  it('makes every chip a link to that topic', () => {
    const out = html();
    expect(out).toContain('href="/themen/rundgang/tabellen"');
    expect(out).toContain('href="/themen/format"');
  });

  it('is a named landmark, reachable without reading down the page', () => {
    expect(html()).toMatch(/<nav[^>]*aria-label="Themen dieser Seite"/);
  });

  it('can be landed on, so a change to it is read out rather than merely drawn', () => {
    // The region is what a finished change redirects to by fragment. A live region already
    // present when the page loads announces nothing; moving focus into this one does, with
    // no script at all — the same mechanism a board move uses.
    expect(html()).toMatch(/id="gw-themen"[^>]*tabindex="-1"|tabindex="-1"[^>]*id="gw-themen"/);
  });
});

describe('what a reader who may not write this page sees', () => {
  it('sees the topics and no control that would be refused', () => {
    const out = html({ darfSchreiben: false });
    expect(out).toContain('Rundgang/Tabellen');
    expect(out).not.toContain('<form');
    expect(out).not.toContain('<input');
  });

  it('is shown nothing at all on a page with no topics', () => {
    // Furniture on every page in the wiki, paid for by every reader who never asked about a
    // topic. `Backlinks` and `Subpages` make the same call for the same reason.
    expect(html({ themen: [], darfSchreiben: false }).trim()).toBe('');
  });
});

describe('what somebody who may write this page gets', () => {
  it('gets a field that submits without a script', () => {
    const out = html({ darfSchreiben: true });
    expect(out).toMatch(/<form[^>]*method="post"[^>]*action="\?\/themaHinzufuegen"/);
    expect(out).toMatch(/<input[^>]*name="thema"/);
  });

  it('is offered the topics that already exist, filtered exactly as the index is', () => {
    // ADR 0011: a suggestion list is a disclosure surface, and it is the one that feels like
    // a convenience and therefore gets forgotten. It cannot be forgotten here — the options
    // ARE the index the sidebar renders, handed straight through, so there is no second
    // request for anybody to leave unfiltered.
    const out = html({ darfSchreiben: true });
    expect(out).toMatch(/<datalist[^>]*id="gw-themen-vorschlaege"/);
    expect(out).toMatch(/<input[^>]*list="gw-themen-vorschlaege"/);
    for (const wanted of ['Format', 'Rundgang', 'Rundgang/Tabellen']) {
      expect(out).toContain(`value="${wanted}"`);
    }
  });

  it('suggests the spelling a file states, never the key underneath it', () => {
    // What goes back to the API from this field is the stated spelling —
    // `Rundgang/Tabellen`. A canonical path (`/rundgang/tabellen`) would be REFUSED by
    // `set_document_topics`, because a leading separator makes an empty first segment, so a
    // suggestion spelled that way is one that cannot be accepted. (The removal buttons DO
    // carry the canonical path: that never goes to the API, it only says which chip.)
    const liste = /<datalist[\s\S]*?<\/datalist>/.exec(html({ darfSchreiben: true }))?.[0] ?? '';
    expect(liste).toContain('value="Rundgang/Tabellen"');
    expect(liste).not.toContain('value="/rundgang/tabellen"');
  });

  it('offers a named way to take each topic off again', () => {
    const out = html({ darfSchreiben: true });
    expect(out).toMatch(/<form[^>]*action="\?\/themaEntfernen"/);
    expect(out).toContain('aria-label="Thema »Rundgang/Tabellen« entfernen"');
    // The button carries which topic, so one form serves every chip — the same shape a
    // board's column buttons use.
    expect(out).toMatch(/name="pfad"[^>]*value="\/rundgang\/tabellen"/);
  });

  it('says »Keine Themen« on a page that has none, rather than an empty row', () => {
    const out = html({ themen: [], darfSchreiben: true });
    expect(out).toContain('Keine Themen');
    expect(out).toMatch(/<input[^>]*name="thema"/);
  });

  it('gives back what was typed when the change was refused', () => {
    const out = html({ darfSchreiben: true, getippt: 'Medizin//Darm', fehler: 'Das ist kein Thema.' });
    expect(out).toContain('value="Medizin//Darm"');
  });
});

describe('a refusal', () => {
  it('is said out loud, and is announced rather than only coloured', () => {
    const out = html({ darfSchreiben: true, fehler: 'Dafür fehlt das Schreibrecht.' });
    expect(out).toContain('Dafür fehlt das Schreibrecht.');
    expect(out).toMatch(/role="alert"/);
  });

  it('never says »Keine Themen« beside a failure to load them', () => {
    // Two different things: a page filed under nothing, and a request that did not come
    // back. Saying both at once is the conflation every empty state here refuses.
    const out = html({
      themen: [],
      darfSchreiben: true,
      fehler: 'Die Themen konnten nicht geladen werden (Fehler 500).'
    });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Keine Themen');
  });

  it('is shown even to somebody the page now believes may not write it', () => {
    // Found the hard way on /projekte: a session that expires between the render and the
    // submit withdraws the form, and with it — when the message lived inside the form — the
    // sentence explaining what had just happened.
    const out = html({ darfSchreiben: false, fehler: 'Nicht angemeldet — bitte erneut anmelden.' });
    expect(out).toContain('Nicht angemeldet');
  });
});
