import { describe, expect, it } from 'vitest';
import type { TreeNode } from './api';
import {
  breadcrumb,
  childrenOf,
  documentTypeLabel,
  editedAtLabel,
  foreignLanguage,
  languageName,
  subpageCount,
  trail,
  visibilityStatement
} from './pagemeta';

function node(path: string, title: string, children: TreeNode[] = []): TreeNode {
  return {
    path,
    slug: path.slice(path.lastIndexOf('/') + 1),
    title,
    doc_type: 'page',
    visibility: 'restricted',
    children
  };
}

/**
 * The shape of the real corpus, minus its content: four levels deep in one branch, and —
 * this is the part that matters — a sibling whose path is a PREFIX of another's.
 * `/rundgang` and `/rundgang-alt` are exactly the pair that a lookup written with
 * `startsWith` instead of `===` gets wrong, and gets wrong silently.
 */
const tree: TreeNode[] = [
  node('/rundgang', 'Rundgang', [
    node('/rundgang/tabellen', 'Tabellen'),
    node('/rundgang/import-export', 'Import und Export', [
      node('/rundgang/import-export/heikler-text', 'Heikler Text')
    ])
  ]),
  node('/rundgang-alt', 'Alter Rundgang', [node('/rundgang-alt/notiz', 'Notiz')])
];

describe('trail', () => {
  it('reads from the root down, with the titles the tree states', () => {
    expect(trail(tree, '/rundgang/import-export/heikler-text')).toEqual([
      { path: '/rundgang', title: 'Rundgang' },
      { path: '/rundgang/import-export', title: 'Import und Export' },
      { path: '/rundgang/import-export/heikler-text', title: 'Heikler Text' }
    ]);
  });

  it('does not confuse a sibling whose path is a prefix of another', () => {
    // `/rundgang-alt` starts with `/rundgang`. A prefix comparison anywhere in the walk
    // puts the wrong branch in the breadcrumb, and every title in it is plausible.
    expect(trail(tree, '/rundgang-alt/notiz')).toEqual([
      { path: '/rundgang-alt', title: 'Alter Rundgang' },
      { path: '/rundgang-alt/notiz', title: 'Notiz' }
    ]);
  });

  it('is empty for a path the tree does not contain', () => {
    // Not hypothetical: the tree drops a whole branch whose parent this caller cannot
    // read, while `/api/documents` still serves a page they hold a direct grant on.
    expect(trail(tree, '/geheim/unterseite')).toEqual([]);
  });
});

describe('breadcrumb', () => {
  it('uses the trail when the tree has the page', () => {
    expect(breadcrumb(tree, { path: '/rundgang/tabellen', title: 'Tabellen' })).toEqual([
      { path: '/rundgang', title: 'Rundgang' },
      { path: '/rundgang/tabellen', title: 'Tabellen' }
    ]);
  });

  it('never invents an ancestor the tree did not give it', () => {
    // The tempting fallback is to cut the path into segments and title-case them. That
    // would put "Geheim" in the breadcrumb of a page whose parent the reader was
    // deliberately not shown — a guess, presented in the position facts occupy.
    const crumbs = breadcrumb(tree, { path: '/geheim/unterseite', title: 'Unterseite' });
    expect(crumbs).toEqual([{ path: '/geheim/unterseite', title: 'Unterseite' }]);
    // The titles, specifically: the page's own path is in the URL bar already and
    // discloses nothing, but a TITLE for `/geheim` would be a fact nobody supplied.
    expect(crumbs.map((c) => c.title)).toEqual(['Unterseite']);
  });
});

describe('childrenOf', () => {
  it('returns the children of a nested page', () => {
    expect(childrenOf(tree, '/rundgang/import-export').map((c) => c.path)).toEqual([
      '/rundgang/import-export/heikler-text'
    ]);
  });

  it('does not return a prefix-sharing sibling’s children', () => {
    expect(childrenOf(tree, '/rundgang').map((c) => c.path)).toEqual([
      '/rundgang/tabellen',
      '/rundgang/import-export'
    ]);
    // The direction that actually breaks: `/rundgang-alt` starts with `/rundgang`, so a
    // prefix test hands back the OTHER branch's children — a list of real titles, all of
    // them wrong, with nothing to make it look like a mistake.
    expect(childrenOf(tree, '/rundgang-alt').map((c) => c.path)).toEqual(['/rundgang-alt/notiz']);
  });

  it('is empty for a leaf and for a path that is not there', () => {
    expect(childrenOf(tree, '/rundgang/tabellen')).toEqual([]);
    expect(childrenOf(tree, '/nirgends')).toEqual([]);
  });
});

describe('visibilityStatement', () => {
  it('says "im Internet" and "ohne Anmeldung" for a public page', () => {
    // The whole reason this function exists rather than a three-word lookup table. In an
    // intranet "öffentlich" routinely means "everyone in the organisation"; here it means
    // strangers, and a reader who takes the first reading writes something into a page
    // that the internet can read. Both halves of the correction are asserted.
    const it_ = visibilityStatement('public');
    expect(it_.tone).toBe('public');
    expect(it_.label).toContain('Internet');
    expect(it_.detail).toContain('ohne Anmeldung');
  });

  it('does not promise anonymous reading for internal or restricted', () => {
    for (const raw of ['internal', 'restricted']) {
      const statement = visibilityStatement(raw);
      expect(statement.label).not.toContain('Internet');
      expect(statement.detail).not.toContain('ohne Anmeldung');
    }
    expect(visibilityStatement('internal').label).toBe('Intern');
    expect(visibilityStatement('restricted').label).toBe('Eingeschränkt');
  });

  it('fails closed on a value it does not know', () => {
    // `Visibility::from_str(…).unwrap_or_default()` in gw-store gives Restricted for
    // anything unparseable, so this is not merely cautious — it is what the permission
    // engine will actually do with the page.
    for (const raw of ['', 'world', 'öffentlich', 'PUBLIC-ish']) {
      expect(visibilityStatement(raw).tone).toBe('restricted');
    }
  });

  it('ignores surrounding whitespace and case', () => {
    expect(visibilityStatement('  Public ').tone).toBe('public');
  });
});

describe('documentTypeLabel', () => {
  it('says nothing for the default type', () => {
    // A row reading "Dokumentart: Seite" on every page of the wiki is noise, and noise in
    // a panel of facts is what teaches people to stop reading it.
    expect(documentTypeLabel('page')).toBeNull();
    expect(documentTypeLabel('')).toBeNull();
  });

  it('names the three other types in German', () => {
    expect(documentTypeLabel('research')).toBe('Recherche');
    expect(documentTypeLabel('project')).toBe('Projekt');
    expect(documentTypeLabel('dataset')).toBe('Datensatz');
  });

  it('passes through a type it does not know rather than hiding it', () => {
    expect(documentTypeLabel('runbook')).toBe('runbook');
  });
});

describe('foreignLanguage', () => {
  it('says nothing when the page is in the language of the interface', () => {
    expect(foreignLanguage('de')).toBeNull();
    expect(foreignLanguage('de-AT')).toBeNull();
    expect(foreignLanguage('')).toBeNull();
  });

  it('names a different language in German', () => {
    expect(foreignLanguage('en')).toBe('Englisch');
    expect(foreignLanguage('en-GB')).toBe('Englisch');
    expect(foreignLanguage('FR')).toBe('Französisch');
  });

  it('falls back to the tag itself rather than dropping the row', () => {
    expect(languageName('zz')).toBe('zz');
    expect(languageName('!!')).toBe('!!');
  });
});

describe('subpageCount', () => {
  it('agrees with itself in the singular', () => {
    expect(subpageCount(1)).toBe('1 Unterseite');
    expect(subpageCount(2)).toBe('2 Unterseiten');
  });
});

describe('editedAtLabel', () => {
  // Nothing calls this yet — there is no revisions endpoint. It is written and tested now
  // so the slot has no open questions left in it; see the note on `RevisionInfo`.

  it('formats a stored instant as a German date in a stated zone', () => {
    // 09:30 UTC is 11:30 in Berlin in July. Naming the zone is what makes this assertion
    // possible at all: with the host's zone the expected string changes with `TZ`.
    expect(editedAtLabel('2026-07-04T09:30:00Z')).toBe('4. Juli 2026 um 11:30');
  });

  it('refuses an instant it cannot parse instead of printing "Invalid Date"', () => {
    // Which is what `Intl` produces without the guard, and which reads as a bug in the
    // page rather than in the data.
    expect(editedAtLabel('gestern')).toBeNull();
    expect(editedAtLabel('')).toBeNull();
  });
});
