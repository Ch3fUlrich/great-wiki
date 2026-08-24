import { describe, expect, it } from 'vitest';
import { describeCreate, describeDelete, describeList, homePath } from './projects';

/**
 * The pure half of the projects page: what a person typed turned into a path, and what a
 * refusal reads as.
 *
 * Nothing here decides who may see or do anything — `/api/projects` does, per document, and
 * this file could not widen it if it tried. What it does decide is whether somebody is told
 * what went wrong, which is the requirement these tests exist for: a refusal that arrives as
 * "Fehler" is a refusal nobody can act on.
 */
describe('homePath', () => {
  it('takes the path as typed', () => {
    expect(homePath('/rundgang/tabellen')).toBe('/rundgang/tabellen');
  });

  it('adds the leading slash somebody left out', () => {
    expect(homePath('rundgang/tabellen')).toBe('/rundgang/tabellen');
  });

  it('ignores surrounding whitespace, which is what a paste brings with it', () => {
    expect(homePath('  /rundgang/tabellen \n')).toBe('/rundgang/tabellen');
  });

  it('drops a trailing slash, so two spellings of one page are one path', () => {
    expect(homePath('/rundgang/tabellen/')).toBe('/rundgang/tabellen');
  });

  it('takes the path out of a pasted address, because that is how a page is copied', () => {
    expect(homePath('https://wiki.example.org/rundgang/tabellen')).toBe('/rundgang/tabellen');
  });

  it('has nothing to offer for an empty field or for the bare root', () => {
    expect(homePath('')).toBeNull();
    expect(homePath('   ')).toBeNull();
    expect(homePath('/')).toBeNull();
  });
});

describe('what a refusal reads as', () => {
  it('says what a taken home page means and where to go instead, never just "Fehler"', () => {
    const message = describeCreate(409, '/rundgang/tabellen');
    // The fact: this page already has a project on it.
    expect(message).toContain('/rundgang/tabellen');
    expect(message).toContain('Startseite eines Projekts');
    // And the way out, which is the half that makes it actionable.
    expect(message).toMatch(/andere Seite/);
    expect(message).not.toMatch(/Fehler 409/);
  });

  it('names the missing page rather than reporting a bare 404', () => {
    const message = describeCreate(404, '/gibtsnicht');
    expect(message).toContain('/gibtsnicht');
    expect(message).not.toMatch(/Fehler 404/);
  });

  it('says a refusal is about write access on the page, not about projects', () => {
    const message = describeCreate(403, '/rundgang/tabellen');
    expect(message).toContain('/rundgang/tabellen');
    expect(message).toMatch(/Schreibrecht/);
  });

  it('separates "the application did not answer" from a status code', () => {
    expect(describeCreate(0, '/x')).toMatch(/antwortet nicht/);
    expect(describeCreate(0, '/x')).toMatch(/nichts angelegt/);
    expect(describeList(0)).toMatch(/antwortet nicht/);
  });

  it('carries the server sentence through for a status it has no wording for', () => {
    // Nothing is swallowed. An unmapped code is rare by definition, and dropping what the
    // server said about it would leave the one clue there was on the floor.
    const message = describeCreate(418, '/x', 'i am a teapot');
    expect(message).toContain('418');
    expect(message).toContain('i am a teapot');
  });

  it('promises nothing was deleted when a deletion is refused', () => {
    expect(describeDelete(403)).toMatch(/nichts gelöscht/);
    expect(describeDelete(0)).toMatch(/nichts gelöscht/);
  });
});
