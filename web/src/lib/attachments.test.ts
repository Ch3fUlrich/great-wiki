import { describe, expect, it } from 'vitest';
import {
  attachmentApiPath,
  attachmentsApiPath,
  describeAttachments,
  describeUpload,
  kindText,
  sizeText,
  type Attachment
} from '$lib/attachments';

/**
 * The `Anhänge` list on the wire and in words.
 *
 * Three things are pinned here that nothing else can pin.
 *
 * **No address is ever assembled from a digest.** D-16 makes a download authorised against
 * the page it was reached through, and that is only true while the page is in the address.
 * The API builds the download address and this interface uses the one it was handed; the two
 * functions below build the *list* address and the *upload* address, both of which name a
 * page, and there is nowhere in this module for a content address to appear.
 *
 * **The server decides which files it takes, and this file does not repeat the answer.** The
 * accepted set is `gw_store::blobs::sniff`'s allowlist and it is being widened; a list of
 * extensions here would be a second answer that goes stale silently. So a refused type is
 * rendered by carrying the server's own words inside a German sentence, exactly as
 * `describeRestore` carries a 409 the Papierkorb alone can explain.
 *
 * **A failed request is never rendered as "Keine Anhänge".** That is the lie every other view
 * in this interface refuses to tell, and here it would be a claim about what a page carries.
 */

const anhang: Attachment = {
  filename: 'Befund 2024.pdf',
  media_type: 'application/pdf',
  byte_size: 1_258_291,
  uploaded_at: '2026-09-01 09:30:00',
  uploaded_by_name: 'Sergej',
  href: '/api/attachment/Befund%202024.pdf/rundgang/tabellen'
};

describe('where the attachments of a page are', () => {
  it('names the page in the listing address and nothing else', () => {
    expect(attachmentsApiPath('/rundgang/tabellen')).toBe('/api/attachments/rundgang/tabellen');
    // A loader has the path with its leading slash; a route parameter does not. Both spell
    // one address.
    expect(attachmentsApiPath('rundgang/tabellen')).toBe('/api/attachments/rundgang/tabellen');
  });

  it('puts the filename BEFORE the page in an upload address, as the API routes it', () => {
    // `{*path}` must be the last segment of a route, so anything else has to precede it.
    // Getting this backwards produces an address that 404s and looks like a permission bug.
    expect(attachmentApiPath('/rundgang/tabellen', 'befund.pdf')).toBe(
      '/api/attachment/befund.pdf/rundgang/tabellen'
    );
  });

  it('encodes a name and a path so a space or an umlaut survives being an address', () => {
    expect(attachmentApiPath('/rundgang/größe', 'Röntgen links.png')).toBe(
      '/api/attachment/R%C3%B6ntgen%20links.png/rundgang/gr%C3%B6%C3%9Fe'
    );
  });

  it('never builds a download address itself — that one comes off the wire', () => {
    // The plainest statement of D-16 this interface can make: the thing that fetches a file
    // is the `href` the API sent, which contains the page and does not contain the bytes.
    expect(anhang.href).toContain('/rundgang/tabellen');
    expect(anhang.href).not.toMatch(/[0-9a-f]{40,}/);
  });
});

describe('how big a file is, in words', () => {
  it('counts in bytes while there are few of them', () => {
    expect(sizeText(0)).toBe('0 B');
    expect(sizeText(512)).toBe('512 B');
    expect(sizeText(1023)).toBe('1023 B');
  });

  it('climbs a unit at a time, with a German decimal comma', () => {
    expect(sizeText(1024)).toBe('1,0 kB');
    expect(sizeText(1536)).toBe('1,5 kB');
    expect(sizeText(1024 * 1024)).toBe('1,0 MB');
    expect(sizeText(1_258_291)).toBe('1,2 MB');
    expect(sizeText(1024 * 1024 * 1024)).toBe('1,0 GB');
  });

  it('reaches the largest file the API will take without falling back to kilobytes', () => {
    // `$lib/history`'s own byte formatter stops at kB, which is right for a revision's growth
    // and wrong here: D-17 allows 250 MB, and "256000,0 kB" is a number nobody can read.
    expect(sizeText(250 * 1024 * 1024)).toBe('250,0 MB');
  });
});

describe('what kind of file it is, in words', () => {
  it('says so from the top-level type, so a widened allowlist cannot make it wrong', () => {
    // Derived, never a table of the types the server accepts: `gw_store::blobs::sniff` owns
    // that list and it is being widened. A type nobody here has heard of reads as "Datei"
    // beside its exact media type, which is a worse label and never a false one.
    expect(kindText('image/png')).toBe('Bild');
    expect(kindText('image/avif')).toBe('Bild');
    expect(kindText('video/mp4')).toBe('Video');
    expect(kindText('audio/ogg')).toBe('Audio');
    expect(kindText('application/pdf')).toBe('PDF');
    expect(kindText('application/zip')).toBe('Datei');
    expect(kindText('application/x-something-new')).toBe('Datei');
  });
});

describe('why the list is not there', () => {
  it('says the list failed, never that the page carries nothing', () => {
    expect(describeAttachments(500)).toContain('konnten nicht geladen werden');
    expect(describeAttachments(500)).not.toMatch(/Keine Anhänge/);
    expect(describeAttachments(0)).toContain('antwortet nicht');
  });
});

describe('why a file was not attached', () => {
  it('promises, in every branch, that nothing was attached', () => {
    for (const status of [0, 401, 403, 404, 409, 413, 415, 400, 503, 500]) {
      expect(describeUpload(status, 'weil')).toContain('Es wurde nichts angehängt.');
    }
  });

  it('sends somebody to the right permission, and names the account requirement', () => {
    // `Store::attach` refuses an unauthenticated caller before it consults a single grant,
    // because the row records who put the file there. A path carrying `anyone: write` is a
    // public share link, and that is not the same act.
    const said = describeUpload(403);
    expect(said).toContain('Schreibrecht');
    expect(said).toContain('angemeldet');
  });

  it('carries the server s own words for a type it will not store', () => {
    // The accepted set lives in the API and is being widened. Quoting it is the only way this
    // sentence cannot go stale — and "Fehler 415" is a refusal nobody can act on.
    const said = describeUpload(415, 'this wiki stores images, PDFs, ZIP archives');
    expect(said).toContain('this wiki stores images, PDFs, ZIP archives');
    expect(said).not.toMatch(/^Fehler/);
  });

  it('carries the server s own words for a file that is too large', () => {
    // The limit is `MAX_ATTACHMENT_BYTES`, on the server. Repeating "250 MB" here would be a
    // second answer to a question only one side owns.
    const said = describeUpload(413, 'an attachment may be at most 250 MB');
    expect(said).toContain('an attachment may be at most 250 MB');
    expect(said).toMatch(/zu groß/);
  });

  it('carries the server s own words for a name it will not take', () => {
    // A 409 has two shapes — a name that could not be an address, and a name already on this
    // page — and only the API knows which, and which name.
    const said = describeUpload(409, '`befund.pdf` is already attached to this page');
    expect(said).toContain('`befund.pdf` is already attached to this page');
  });

  it('says so plainly when the server named no reason at all', () => {
    expect(describeUpload(409)).toContain('nennt keinen Grund');
  });

  it('never renders a bare status code as the whole explanation', () => {
    const said = describeUpload(500, 'internal error');
    expect(said).toContain('500');
    expect(said).toContain('internal error');
  });
});
