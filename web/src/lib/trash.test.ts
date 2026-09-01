import { describe, expect, it } from 'vitest';
import {
  confirmPurgeHref,
  deleteHref,
  describeDelete,
  describePurge,
  describePurgePreview,
  describeRestore,
  describeTrash,
  documentApiPath,
  pagesText,
  purgeApiPath,
  purgeLines,
  restoreApiPath,
  TRASH_ENDPOINT,
  TRASH_PATH,
  type PurgeReport
} from './trash';

/**
 * The Papierkorb, on the wire and in words.
 *
 * What is asserted here is the half of this feature that is neither a permission nor a
 * request: where a thing lives, and what a refusal says. Both are worth pinning because both
 * are the kind of thing that quietly goes wrong — an address assembled a second way, or a
 * refusal that loses the one sentence that said what to do about it.
 *
 * Nothing here decides who may do anything. `gw_store::trash` does, per document, and
 * `gw_api::routes::trash` chooses the status codes; these functions turn those answers into
 * German and nothing more.
 */

const bericht: PurgeReport = {
  committed: false,
  pages: [
    { path: '/handbuch', title: 'Handbuch' },
    { path: '/handbuch/onboarding', title: 'Onboarding' }
  ],
  revisions: 12,
  tasks: 3,
  projects: 1,
  links: 7,
  topic_filings: 4,
  topics: 2
};

describe('where things are', () => {
  it('spells the interface route and the endpoint once each', () => {
    expect(TRASH_PATH).toBe('/papierkorb');
    expect(TRASH_ENDPOINT).toBe('/api/trash');
  });

  it('builds the three addresses the API actually serves', () => {
    expect(restoreApiPath('/handbuch')).toBe('/api/trash/restore/handbuch');
    expect(purgeApiPath('/handbuch/onboarding')).toBe('/api/trash/purge/handbuch/onboarding');
    expect(documentApiPath('/handbuch')).toBe('/api/documents/handbuch');
  });

  it('takes a path with or without its leading slash and answers the same address', () => {
    expect(restoreApiPath('handbuch')).toBe(restoreApiPath('/handbuch'));
    expect(purgeApiPath('handbuch')).toBe(purgeApiPath('/handbuch'));
  });

  it('encodes each segment, so a slug carrying a slash cannot become a different page', () => {
    // A no-op on every path the slugger emits today. It is here for the day it is not: an
    // unencoded `/` inside a segment would name another page entirely, in an address that
    // looked right.
    expect(purgeApiPath('/a b/c%d')).toBe('/api/trash/purge/a%20b/c%25d');
    expect(purgeApiPath('/a b/c%d')).not.toContain(' ');
  });

  it('asks the question in the address, so it is a link and the back button walks out of it', () => {
    expect(confirmPurgeHref('/handbuch')).toContain('entfernen=%2Fhandbuch');
    expect(confirmPurgeHref('/handbuch')).toContain(TRASH_PATH);
    // Focus lands on the confirmation rather than at the top of the page: a block that is
    // merely rendered is not announced, and this is the one act that cannot be undone.
    expect(confirmPurgeHref('/handbuch')).toMatch(/#\S+$/);
  });

  it('asks before deleting on the page itself, in that page‘s own address', () => {
    expect(deleteHref('/handbuch')).toBe('/handbuch?loeschen=1#gw-loeschen');
    // The fragment does the same job it does before a purge: it moves focus to the question,
    // so the question is announced rather than merely drawn.
    expect(deleteHref('/handbuch')).toMatch(/#\S+$/);
  });
});

describe('how many pages moved', () => {
  it('agrees with itself in the singular', () => {
    expect(pagesText(1)).toBe('1 Seite');
    expect(pagesText(4)).toBe('4 Seiten');
    expect(pagesText(0)).toBe('0 Seiten');
  });
});

describe('what a purge would destroy, in words', () => {
  it('names every kind of thing that goes, including the ones that are none', () => {
    const lines = purgeLines(bericht);
    expect(lines.map((line) => line.zahl)).toEqual([12, 3, 1, 7, 4, 2]);
    // A missing line reads as "not counted" just as easily as "none", and this is the one
    // confirmation in the system where that difference cannot be checked afterwards.
    expect(purgeLines({ ...bericht, links: 0 }).map((line) => line.zahl)).toContain(0);
    expect(purgeLines({ ...bericht, links: 0 })).toHaveLength(lines.length);
  });

  it('renders a count the API grew that this interface has never heard of', () => {
    // `PurgeReport` grows whenever the system grows something a purge cascades away. A
    // hand-written list of fields would quietly stop mentioning the newest one, which is a
    // confirmation under-reporting what it destroys — in the one place nobody can check
    // afterwards. An ugly label is the acceptable failure; a missing line is not.
    const gewachsen = { ...bericht, anhaenge: 5 } as unknown as PurgeReport;
    const lines = purgeLines(gewachsen);
    expect(lines).toHaveLength(purgeLines(bericht).length + 1);
    expect(lines.at(-1)).toEqual({
      was: 'Weiteres, das der Papierkorb »anhaenge« nennt',
      zahl: 5
    });
  });

  it('counts nothing that is not a number, so the page list is never a line', () => {
    expect(purgeLines(bericht).some((line) => line.was.includes('pages'))).toBe(false);
    expect(purgeLines(bericht).some((line) => line.was.includes('committed'))).toBe(false);
  });

  it('says what each number is about, not merely what table it came from', () => {
    const was = purgeLines(bericht).map((line) => line.was);
    expect(was.join(' ')).toContain('Versionen');
    expect(was.join(' ')).toContain('Karten');
    expect(was.join(' ')).toContain('Projekte');
    expect(was.join(' ')).toContain('Verweise');
    expect(was.join(' ')).toContain('Themen');
    // Every label is a German noun phrase, never a column name.
    expect(was.join(' ')).not.toMatch(/topic_filings|revisions|links/);
  });
});

describe('why the Papierkorb is not there', () => {
  it('never says it is empty about a server that did not answer', () => {
    expect(describeTrash(0)).toContain('antwortet nicht');
    expect(describeTrash(500)).toContain('Fehler 500');
    for (const status of [0, 500, 502]) {
      expect(describeTrash(status)).not.toMatch(/ist leer|nichts gelöscht worden/i);
    }
  });
});

describe('why a page was not deleted', () => {
  it('promises, in every branch, that nothing was', () => {
    for (const status of [0, 401, 403, 404, 409, 500]) {
      expect(describeDelete(status, null)).toContain('Es wurde nichts gelöscht.');
    }
  });

  it('names the way out of the one refusal that has one', () => {
    // The API's 409 has exactly one shape: a subpage the caller may not write. The way out
    // is somebody else's to take, and saying so is what stops the reader pressing again.
    const message = describeDelete(409, null);
    expect(message).toMatch(/Unterseite|darunter/);
    expect(message).not.toMatch(/Fehler 409/);
  });

  it('sends a missing write right and a missing session to different places', () => {
    expect(describeDelete(403, null)).toContain('Schreibrecht');
    expect(describeDelete(401, null)).toContain('angemeldet');
    expect(describeDelete(0, null)).toContain('antwortet nicht');
  });

  it('keeps whatever the server said about a status it has no wording for', () => {
    expect(describeDelete(500, 'database is locked')).toContain('database is locked');
  });
});

describe('why nothing came back', () => {
  it('promises, in every branch, that nothing was restored', () => {
    for (const status of [0, 401, 403, 404, 409, 500]) {
      expect(describeRestore(status, null)).toMatch(/nichts wiederhergestellt/);
    }
  });

  it('carries the refusal that names the parent, because only the API knows which page it is', () => {
    // `restore_document` refuses a page whose parent is still in the trash and NAMES that
    // parent. Dropping that sentence turns an answerable refusal into "Fehler 409".
    const said = '/handbuch is still in the trash: restore it first';
    expect(describeRestore(409, said)).toContain('/handbuch');
    expect(describeRestore(409, said)).toContain(said);
  });

  it('says so plainly when the API named no reason at all', () => {
    expect(describeRestore(409, null)).not.toContain('undefined');
    expect(describeRestore(409, null)).not.toContain('null');
  });
});

describe('why a purge was not described, and why one did not happen', () => {
  it('says who may ask, without pretending the page is not there', () => {
    // `path_admin`, not write: being able to edit a page is structurally not being able to
    // destroy it (ADR 0012). The sentence has to send somebody to the right person.
    expect(describePurgePreview(403, null)).toMatch(/verwalt/i);
    expect(describePurge(403, null)).toMatch(/verwalt/i);
  });

  it('says that a purge only reaches what is already in the Papierkorb', () => {
    expect(describePurgePreview(404, null)).toMatch(/Papierkorb/);
    expect(describePurgePreview(409, null)).toMatch(/Papierkorb/);
  });

  it('promises that nothing was destroyed when the destruction itself was refused', () => {
    for (const status of [0, 403, 404, 409, 500]) {
      expect(describePurge(status, null)).toContain('Es wurde nichts endgültig gelöscht.');
    }
  });

  it('does not make that promise about a preview, which was never going to destroy anything', () => {
    expect(describePurgePreview(403, null)).not.toContain('Es wurde nichts endgültig gelöscht.');
  });
});
