import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import PageAttachments from './PageAttachments.svelte';
import type { Attachment } from '$lib/attachments';

/**
 * The `Anhänge` section, rendered exactly as the server renders it.
 *
 * Rendered with `svelte/server`, which is the first response, and that is the requirement
 * rather than a preference: the download is a link and the upload is a real form submission,
 * so a reader with JavaScript switched off can see what a page carries, fetch one of the
 * files, and add another. A click handler could not pass a single test in this file.
 *
 * Four things are pinned here that neither a type nor a browser check can reach.
 *
 * **The list is the authority (D-15).** What it shows is what is attached, whatever the prose
 * does or does not mention. So the section says what a page carries, in words, including when
 * that is nothing — and it never says »Keine Anhänge« about a request that failed.
 *
 * **A download address is the one the API sent (D-16).** Nothing here assembles one, and no
 * content address appears anywhere in the rendered page.
 *
 * **A control that would be refused is not offered, and its absence is explained.** `may_write`
 * is the store's own verdict, carried on the wire (ADR 0010); the account requirement is the
 * other half `Store::attach` checks before it consults a single grant. A silently missing
 * control reads as a fault, which is what `/papierkorb` already writes down about its own.
 *
 * **The field states no list of its own.** `gw_store::blobs::sniff` owns the accepted set and
 * it is being widened; an `accept` attribute here would be a second answer that refuses a file
 * the wiki would have taken, before the request is even made.
 */

const anhaenge: Attachment[] = [
  {
    filename: 'Befund 2024.pdf',
    media_type: 'application/pdf',
    byte_size: 1_258_291,
    uploaded_at: '2026-09-01 09:30:00',
    uploaded_by_name: 'Sergej',
    href: '/api/attachment/Befund%202024.pdf/rundgang/tabellen'
  },
  {
    filename: 'Röntgen.png',
    media_type: 'image/png',
    byte_size: 40_960,
    uploaded_at: '2026-08-30 17:45:00',
    uploaded_by_name: 'Andere Person',
    href: '/api/attachment/R%C3%B6ntgen.png/rundgang/tabellen'
  }
];

function html(
  {
    liste = anhaenge,
    darfSchreiben = false,
    angemeldet = false,
    fehler = null,
    hochgeladen = null
  }: {
    liste?: Attachment[];
    darfSchreiben?: boolean;
    angemeldet?: boolean;
    fehler?: string | null;
    hochgeladen?: Attachment | null;
  } = {}
): string {
  return render(PageAttachments, {
    props: { anhaenge: liste, darfSchreiben, angemeldet, fehler, hochgeladen }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('what a page carries', () => {
  it('names the section as this interface names it everywhere else', () => {
    expect(html()).toContain('Anhänge');
  });

  it('lists every file, by the name it carries on this page', () => {
    const out = html();
    expect(out).toContain('Befund 2024.pdf');
    expect(out).toContain('Röntgen.png');
  });

  it('says how big each file is and what kind it is, in text', () => {
    // Not an icon and not a colour: the size and the type are facts somebody needs before
    // deciding to fetch 1,2 MB over a phone connection, and a picture of a document says
    // neither of them to a screen reader.
    const out = html();
    expect(out).toContain('1,2 MB');
    expect(out).toContain('40,0 kB');
    expect(out).toContain('PDF');
    expect(out).toContain('Bild');
    // And the exact type beside the word, because »Bild« does not tell a PNG from an AVIF.
    expect(out).toContain('application/pdf');
    expect(out).toContain('image/png');
  });

  it('says who attached it and when', () => {
    const out = html();
    expect(out).toContain('Hochgeladen von');
    expect(out).toContain('Sergej');
    expect(out).toContain('Andere Person');
    expect(out).toContain('01.09.2026');
  });

  it('says a page carries nothing rather than showing an empty box', () => {
    // To somebody who could attach one, which is who the sentence is for — see the test
    // below for why it is not printed under every page in the wiki.
    const out = html({ liste: [], darfSchreiben: true, angemeldet: true });
    expect(out).toContain('Keine Anhänge');
  });

  it('never says that about a request that failed', () => {
    // A list that did not load and a page nobody has attached anything to are different
    // things. Saying the second about the first is a false claim about what a page carries.
    const out = html({ liste: [], fehler: 'Die Anhänge dieser Seite konnten nicht geladen werden (Fehler 500).' });
    expect(out).not.toContain('Keine Anhänge');
    expect(out).toContain('Fehler 500');
  });

  it('renders nothing at all on a page with no files that this reader cannot write', () => {
    // Most pages in this wiki carry no attachment and most readers may not write them. A
    // »Keine Anhänge« block under every one of them is furniture paid for by every reader who
    // never asked — the call `Backlinks`, `Subpages` and `PageTopics` all make.
    expect(html({ liste: [] })).toBe('');
  });
});

describe('fetching one', () => {
  it('is a link, so it works before hydration and under a right-click', () => {
    const out = html();
    expect(out).toContain('href="/api/attachment/Befund%202024.pdf/rundgang/tabellen"');
    expect(out).toMatch(/<a[^>]*href="\/api\/attachment\//);
  });

  it('uses the address the API sent and never one it assembled', () => {
    // D-16: a download is authorised against the page it was reached through, which is only
    // true while the page is in the address. The API builds it; this component prints it.
    const out = html({
      liste: [{ ...anhaenge[0], href: '/api/attachment/andere.pdf/woanders' }]
    });
    expect(out).toContain('href="/api/attachment/andere.pdf/woanders"');
    expect(out).not.toContain('/rundgang/tabellen');
  });

  it('shows no content address anywhere, because there is none to show', () => {
    const out = html();
    expect(out).not.toMatch(/[0-9a-f]{40,}/);
    expect(out.toLowerCase()).not.toContain('sha');
  });
});

describe('adding one', () => {
  it('offers a real form submission, not a control waiting for a bundle', () => {
    const out = html({ darfSchreiben: true, angemeldet: true });
    expect(out).toMatch(/<form[^>]*method="post"[^>]*action="\?\/anhaengen"/);
    // multipart, because the body is a file. The action unpacks it and sends the bytes on.
    expect(out).toContain('enctype="multipart/form-data"');
    expect(out).toContain('Hochladen');
  });

  it('labels the file field properly, rather than leaving a bare button', () => {
    const out = html({ darfSchreiben: true, angemeldet: true });
    expect(out).toMatch(/<label[^>]*for="gw-anhang-datei"/);
    expect(out).toMatch(/<input[^>]*id="gw-anhang-datei"[^>]*type="file"/);
    expect(out).toMatch(/<input[^>]*name="datei"/);
  });

  it('states no list of file types of its own', () => {
    // The accepted set lives in `gw_store::blobs::sniff` and is being widened. An `accept`
    // attribute here would refuse a file the wiki would have taken — before the request is
    // made, with nothing in any log to say why.
    const out = html({ darfSchreiben: true, angemeldet: true });
    expect(out).not.toContain('accept=');
  });

  it('withholds the control from somebody who may not write, and says why', () => {
    // `may_write` is the verdict the very authorisation that produced this list reached, so a
    // control withheld on it is one that would have been refused (ADR 0010).
    const out = html({ darfSchreiben: false, angemeldet: true });
    expect(out).not.toContain('?/anhaengen');
    expect(out).toContain('bearbeiten darf');
  });

  it('withholds it from somebody who may write but is not signed in, and says that instead', () => {
    // The other half `Store::attach` checks, and it checks it FIRST: the row records who put
    // the file there, and "nobody" is not an answer. A path carrying `anyone: write` makes a
    // page editable by somebody who has not said who they are.
    const out = html({ darfSchreiben: true, angemeldet: false });
    expect(out).not.toContain('?/anhaengen');
    expect(out).toContain('angemeldet');
  });

  it('says which file arrived, in a region a reader is sent to', () => {
    // Announced rather than merely drawn: the redirect carries a fragment, the browser moves
    // focus to this section, and a region that has just received focus is read out. A live
    // region already present in the document announces nothing.
    const out = html({ darfSchreiben: true, angemeldet: true, hochgeladen: anhaenge[0] });
    expect(out).toContain('id="gw-anhaenge"');
    expect(out).toContain('tabindex="-1"');
    expect(out).toMatch(/role="status"/);
    expect(out).toContain('Befund 2024.pdf');
  });

  it('puts a refusal in words, announced, and keeps the list beside it', () => {
    const out = html({
      darfSchreiben: true,
      angemeldet: true,
      fehler: 'Dieser Dateityp wird hier nicht gespeichert. Es wurde nichts angehängt.'
    });
    expect(out).toMatch(/role="alert"/);
    expect(out).toContain('Es wurde nichts angehängt.');
    // The refusal did not remove anything: the list is still what the page carries.
    expect(out).toContain('Befund 2024.pdf');
  });
});
