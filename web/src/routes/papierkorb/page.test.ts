import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { ANONYMOUS, type Me } from '$lib/api';
import Page from './+page.svelte';
import type { PurgeReport, TrashEntry } from '$lib/trash';

/**
 * The Papierkorb, rendered exactly as the server renders it.
 *
 * Three things are asserted here that a browser check cannot reach and a type cannot express.
 *
 * **A control that will be refused is not offered.** `may_restore` is the store's own verdict
 * carried on the wire (ADR 0010), so an entry that says `false` gets no restore control — and
 * gets a sentence instead, because a control that silently is not there reads as a bug rather
 * than as an answer.
 *
 * **The purge states its consequence before it happens, by name.** The API computes the report
 * by running the purge and rolling it back, so the names in it are the names that go. This
 * page prints every one of them. "Diese Seite und 3 weitere" is precisely what it may not do.
 *
 * **No number about what was hidden.** The listing is filtered per document, and the one
 * number beside an entry is the pages this reader may see in it. A total, or an "N
 * ausgeblendet", would be a fact about pages they may not read (ADR 0011). Its absence is
 * asserted rather than left to a comment.
 */

const entries: TrashEntry[] = [
  {
    path: '/handbuch',
    title: 'Handbuch',
    deleted_at: '2026-08-30 09:00:00',
    deleted_by_name: 'Sergej',
    pages: 3,
    may_restore: true
  },
  {
    path: '/rundgang/tabellen',
    title: 'Tabellen',
    deleted_at: '2026-08-29 17:45:00',
    deleted_by_name: 'Andere Person',
    pages: 1,
    may_restore: false
  }
];

const bericht: PurgeReport = {
  committed: false,
  pages: [
    { path: '/handbuch', title: 'Handbuch' },
    { path: '/handbuch/onboarding', title: 'Onboarding' },
    { path: '/handbuch/intern', title: 'Nur intern' }
  ],
  revisions: 12,
  tasks: 3,
  projects: 1,
  links: 0,
  topic_filings: 4,
  topics: 2
};

const ANGEMELDET: Me = { ...ANONYMOUS, authenticated: true, username: 'sergej', display_name: 'Sergej' };

interface Zustand {
  me?: Me;
  entries?: TrashEntry[];
  fehler?: string | null;
  entfernen?: TrashEntry | null;
  bericht?: PurgeReport | null;
  berichtFehler?: string | null;
  wiederhergestellt?: string | null;
  geleert?: string | null;
  form?: { wo: 'wiederherstellen' | 'endgueltig'; fehler: string } | null;
}

function html(zustand: Zustand = {}): string {
  const { form = null, ...data } = zustand;
  return render(Page, {
    props: {
      data: {
        me: ANGEMELDET,
        tree: [],
        tabHrefs: [],
        hier: '/papierkorb',
        seitenleiste: 'seiten',
        themen: [],
        themenFehler: null,
        entries,
        fehler: null,
        entfernen: null,
        bericht: null,
        berichtFehler: null,
        wiederhergestellt: null,
        geleert: null,
        ...data
      },
      form
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
  } as any).body.replace(/<!--.*?-->/g, '');
}

describe('the page', () => {
  it('is a place you can arrive at and understand', () => {
    const out = html();
    expect(out).toMatch(/<h1[^>]*>Papierkorb<\/h1>/);
    // The same sentence /themen, /projekte and /aufgaben carry, and it is what licenses the
    // number beside an entry: the reader is told the list is theirs.
    expect(out).toContain('Es erscheint nur, was Sie auch lesen dürfen.');
  });

  it('says who deleted it and when, in the interface language', () => {
    const out = html();
    expect(out).toContain('Gelöscht von');
    expect(out).toContain('Sergej');
    // Formatted by hand, never through Intl: the string is rendered on the server and then
    // hydrated, and two ICU builds would disagree about it.
    expect(out).toContain('30.08.2026, 09:00');
    expect(out).toContain('UTC');
  });

  it('names each entry and the address that tells two same-named pages apart', () => {
    const out = html();
    expect(out).toContain('Handbuch');
    expect(out).toContain('/rundgang/tabellen');
  });

  it('says how many pages are in an entry, and never how many are not', () => {
    const out = html();
    expect(out).toContain('3 Seiten');
    expect(out).toContain('1 Seite');
    expect(out).not.toMatch(/weitere|insgesamt|ausgeblendet|verborgen|von \d/i);
  });
});

describe('putting one back', () => {
  it('offers the control only for an entry the API said may come back', () => {
    const out = html();
    expect(out).toMatch(/<form[^>]*action="\?\/wiederherstellen"/);
    // Named per row, because »Wiederherstellen« repeated is the same control twice to
    // anybody reading the controls on their own.
    expect(out).toContain('»Handbuch« wiederherstellen');
    expect(out).not.toContain('»Tabellen« wiederherstellen');
  });

  it('says why it is not offered, rather than leaving a gap that reads as a fault', () => {
    const out = html({ entries: [entries[1]] });
    expect(out).not.toMatch(/action="\?\/wiederherstellen"/);
    expect(out).toMatch(/bearbeiten/i);
  });

  it('is a real form submission, so it works before any script arrives', () => {
    const out = html();
    expect(out).toMatch(/<form[^>]*method="post"[^>]*action="\?\/wiederherstellen"/);
    expect(out).toContain('value="/handbuch"');
  });

  it('states a refusal in words, announced, and never as a colour alone', () => {
    const out = html({ form: { wo: 'wiederherstellen', fehler: '/handbuch liegt noch im Papierkorb.' } });
    expect(out).toMatch(/role="alert"/);
    expect(out).toContain('/handbuch liegt noch im Papierkorb.');
  });
});

describe('asking to destroy one', () => {
  it('offers the question to somebody signed in, as a link to a question', () => {
    const out = html();
    expect(out).toContain('entfernen=%2Fhandbuch');
    expect(out).toContain('#gw-endgueltig');
    expect(out).toContain('»Handbuch« endgültig löschen');
  });

  it('withholds it from a reader who could not possibly be allowed', () => {
    // `path_admin` refuses before it looks at anything else unless the caller is a signed-in,
    // active account. That is the one half of the gate this interface can know, so it is the
    // one half it acts on.
    const out = html({ me: ANONYMOUS });
    expect(out).not.toContain('entfernen=');
    expect(out).not.toMatch(/Endgültig löschen/);
  });

  it('makes that link a real navigation, so the question is announced and not only drawn', () => {
    // The fragment moves focus only on a real page load; the client-side router navigates and
    // leaves focus where it was. Found in a browser, on identical markup:
    // `document.activeElement` was the body after a hydrated click and the region after a
    // full load. `data-sveltekit-reload` is what makes the two paths agree.
    expect(html()).toMatch(/data-sveltekit-reload/);
  });

  it('never puts the destroying control in the list itself', () => {
    // The list offers a question. The act lives behind it, after the API's own gate has
    // answered — so nothing in a row can destroy anything by being clicked.
    const out = html();
    expect(out).not.toContain('?/endgueltigLoeschen');
  });
});

describe('the confirmation, which is the only thing standing in front of a loss', () => {
  const gefragt = { entfernen: entries[0], bericht };

  it('names every page that goes, and does not summarise them into a count', () => {
    const out = html(gefragt);
    for (const page of bericht.pages) {
      expect(out).toContain(page.title);
      expect(out).toContain(page.path);
    }
    expect(out).not.toMatch(/und \d+ weitere/i);
  });

  it('gives every other number the report carried, including the ones that are none', () => {
    const out = html(gefragt);
    expect(out).toContain('Versionen');
    expect(out).toContain('12');
    expect(out).toContain('Karten');
    expect(out).toContain('Themenzuordnungen');
    // `links: 0` — an absent line reads as "not counted" as easily as "none", and this is the
    // one confirmation whose reader cannot check afterwards which it was.
    expect(out).toContain('Verweise');
    expect(out).toMatch(/Verweise[\s\S]{0,200}>0</);
  });

  it('says that it cannot be undone, before the control that does it', () => {
    const out = html(gefragt);
    const satz = out.search(/nicht rückgängig|endgültig und nicht/i);
    const knopf = out.indexOf('?/endgueltigLoeschen');
    expect(satz).toBeGreaterThan(-1);
    expect(knopf).toBeGreaterThan(satz);
  });

  it('is announced rather than merely drawn', () => {
    // The link that opens it carries `#gw-endgueltig`; the region takes focus, and a region
    // that has just received focus is read out. No script is involved.
    const out = html(gefragt);
    expect(out).toMatch(/id="gw-endgueltig"[^>]*tabindex="-1"|tabindex="-1"[^>]*id="gw-endgueltig"/);
  });

  it('offers a way out that is as reachable as the way through', () => {
    const out = html(gefragt);
    expect(out).toContain('Abbrechen');
    expect(out).toContain('href="/papierkorb"');
  });

  it('is a real form, carrying the page it described and nothing else', () => {
    const out = html(gefragt);
    expect(out).toMatch(/<form[^>]*method="post"[^>]*action="\?\/endgueltigLoeschen[^"]*"/);
    expect(out).toContain('value="/handbuch"');
  });

  it('offers no destroying control at all when the API refused to describe the purge', () => {
    const out = html({
      entfernen: entries[0],
      bericht: null,
      berichtFehler: 'Endgültig löschen darf nur, wer diese Seite verwaltet.'
    });
    expect(out).not.toContain('?/endgueltigLoeschen');
    expect(out).toContain('Endgültig löschen darf nur, wer diese Seite verwaltet.');
    expect(out).toMatch(/role="alert"/);
  });

  it('says which grants survive it, because a purge is not a change to who may be here', () => {
    // ADR 0012: an `acl` row is a fact about a path, not about a document. A page created
    // later at a purged path inherits whatever the path still carries.
    expect(html(gefragt)).toMatch(/Zugriffsrecht|Rechte/);
  });
});

describe('what just happened', () => {
  it('says a page came back, in a region that is read out', () => {
    const out = html({ wiederhergestellt: '/andere' });
    expect(out).toMatch(/role="status"/);
    expect(out).toContain('/andere');
  });

  it('says a page was destroyed, and does not pretend it can be undone', () => {
    const out = html({ geleert: '/andere' });
    expect(out).toContain('/andere');
    expect(out).toMatch(/endgültig/i);
  });
});

describe('nothing to show', () => {
  it('says the same thing about an empty Papierkorb and one that is none of yours', () => {
    const out = html({ entries: [] });
    expect(out).toMatch(/Hier liegt nichts/);
    expect(out).not.toMatch(/dürfen nicht|keine Berechtigung/i);
  });

  it('never says that about a request that failed', () => {
    const out = html({ entries: [], fehler: 'Der Papierkorb konnte nicht geladen werden (Fehler 500).' });
    expect(out).toContain('Fehler 500');
    expect(out).not.toMatch(/Hier liegt nichts/);
  });
});
