import { describe, expect, it } from 'vitest';
import {
  grantReachText,
  grantReachTitle,
  inheritedReachText,
  inheritedReachTitle,
  reachRoutes,
  revokeResumeText,
  revokeResumeTitle,
  visibilityChangeText,
  visibilityConsequence
} from './reach';

/**
 * The sentences the console uses to describe how far a grant reaches.
 *
 * They are asserted here, as strings, rather than only through the component, because
 * the grant-time ones are rendered inside an Ark `Portal` — which mounts from an
 * `$effect` and therefore renders NOTHING under `svelte/server`. A test that went
 * through the component could not see them at all. `web/scripts/behaviour.mjs` Group H
 * checks that they actually reach the screen; these check that they say the right thing.
 *
 * What "the right thing" is, restated from `crates/gw-store/src/acl.rs`:
 *
 *   - `permits()` asks `can()` at `Visibility::Restricted` FIRST. A grant therefore
 *     decides on its own, whatever the document's visibility says.
 *   - `grants_for_path` returns the grants of the NEAREST ancestor that has any, and a
 *     path is its own first ancestor. Grants are never unioned.
 *
 * Together: a grant on `/a` reaches `/a/b` even when `/a/b` is `restricted`, and the
 * only thing that stops it is a grant row on `/a/b` itself (or on something between).
 */
describe('grantReach', () => {
  it('names the subtree the grant is about to reach', () => {
    expect(grantReachTitle('/handbuch')).toBe('Gilt auch für alle Seiten unter /handbuch.');
  });

  it('says that the pages below are reached whatever their visibility says', () => {
    // The one belief this text exists to correct. Marking a page `restricted` does not
    // hide it from somebody holding a grant on an ancestor, because `permits()` never
    // reaches the visibility branch once `can()` has matched a grant.
    const text = grantReachText('/handbuch');
    expect(text).toContain('erreicht damit jede Seite unterhalb von /handbuch');
    expect(text).toContain('unabhängig von deren Sichtbarkeit');
    expect(text).toContain(
      'Auch eine Seite mit der Sichtbarkeit »Eingeschränkt« wird dadurch erreichbar.'
    );
  });

  it('names the only exception there is, and does not invent a second one', () => {
    const text = grantReachText('/handbuch');
    expect(text).toContain(
      'Davon ausgenommen ist allein eine Seite, die selbst einen Zugriffseintrag trägt'
    );
    expect(text).toContain('Der nächstgelegene Eintrag gilt vollständig und allein');
    expect(text).toContain('Einen anderen Weg, einzelne Seiten auszunehmen, gibt es hier nicht.');
  });

  it('never sends anybody to the visibility control to undo a grant', () => {
    // The control now EXISTS — `/api/admin/visibility`, added in the same change as these
    // words — which makes this assertion more important than it was, not less. Pointing
    // at it here would send somebody to a real control that does not do what they were
    // told: `permits()` consults the grants before it reads the field, so lowering a page
    // to »Eingeschränkt« closes nothing this grant has opened.
    const text = `${grantReachTitle('/handbuch')} ${grantReachText('/handbuch')}`;
    expect(text).not.toContain('Sichtbarkeit ändern');
    expect(text).not.toContain('Sichtbarkeit auf');
    expect(text).not.toContain('einstellen');
    expect(text).not.toContain('festlegen');
  });
});

describe('inheritedReach', () => {
  it('says the ancestor is the reason, not the page’s own visibility', () => {
    expect(inheritedReachTitle('/handbuch', 'restricted')).toBe(
      'Erreichbar über /handbuch, nicht über die Sichtbarkeit dieser Seite.'
    );
  });

  it('names the visibility it is overriding, so the surprise is on screen', () => {
    const text = inheritedReachText('/handbuch/onboarding', '/handbuch', 'restricted');
    expect(text).toContain(
      'Die Rechte unten sind auf /handbuch eingetragen und gelten hier, weil /handbuch/onboarding darunter liegt'
    );
    expect(text).toContain(
      'unabhängig davon, dass diese Seite als »Eingeschränkt« gekennzeichnet ist'
    );
  });

  it('says where the grant can be revoked, and what replaces it here', () => {
    const text = inheritedReachText('/handbuch/onboarding', '/handbuch', 'internal');
    expect(text).toContain('Entziehen lassen sie sich nur auf /handbuch.');
    expect(text).toContain(
      'Ein eigener Eintrag auf /handbuch/onboarding ersetzt die geerbten Rechte vollständig.'
    );
  });

  it('does not claim a public page is unreachable without the grant', () => {
    // `can()` returns true for a public read before it looks at anything else, so on a
    // public page the inherited grant is what confers the EXTRA — writing, commenting —
    // and saying "reachable only because of /handbuch" would simply be false.
    expect(inheritedReachTitle('/handbuch', 'public')).toBe(
      'Die Rechte unten kommen von /handbuch.'
    );
    const text = inheritedReachText('/oeffentlich/unterseite', '/handbuch', 'public');
    expect(text).toContain('Lesen kann diese Seite ohnehin jede und jeder');
    expect(text).toContain('die Sichtbarkeit ist »Öffentlich«');
    expect(text).not.toContain('nicht über die Sichtbarkeit');
  });

  it('drops the visibility clause rather than guessing when the visibility is unknown', () => {
    // The panel takes `visibility` as an optional prop and the ACL endpoint does not
    // carry it. Missing must read as missing — never as `restricted`, which is what the
    // Rust default happens to be, and never as a value invented for the sentence.
    const text = inheritedReachText('/handbuch/onboarding', '/handbuch', null);
    expect(text).toContain('unabhängig von der Sichtbarkeit dieser Seite.');
    expect(text).not.toContain('»');
  });

  it('passes an unrecognised visibility through instead of dropping it', () => {
    const text = inheritedReachText('/a/b', '/a', 'geheim');
    expect(text).toContain('unabhängig davon, dass diese Seite als »geheim« gekennzeichnet ist');
  });
});

/**
 * The panel's answer to its own question.
 *
 * A table of grants is not that answer, and for three years' worth of wrong assumptions
 * it has been captioned as if it were. `permits()` in `crates/gw-store/src/acl.rs`
 * decides in this order, and only the last of them is a grant:
 *
 *   1. `can()` at `Visibility::Restricted` — grants, and `Anyone` grants BEFORE the
 *      caller is even known to be signed in.
 *   2. `Visibility::Public` — everyone, including anonymous.
 *   3. `Visibility::Internal` — baseline `internal` or better.
 *   4. `Visibility::Restricted` — baseline `admin`, on READS only.
 *
 * So an instance admin reads every restricted page in the corpus with no row anywhere,
 * and no grants table will ever show it. These routes are what the panel says instead.
 */
describe('reachRoutes', () => {
  const base = {
    path: '/handbuch',
    shareLink: [] as string[],
    hasGrants: false,
    adminGroups: ['admins'],
    internalGroups: ['users']
  };

  it('always names the admin baseline, which no grants table can show', () => {
    const routes = reachRoutes({ ...base, visibility: 'restricted' });
    const admin = routes.find((route) => route.key === 'baseline');
    expect(admin).toBeDefined();
    expect(admin?.title).toContain('Verwaltung');
    expect(admin?.text).toContain('jede Seite');
    expect(admin?.text).toContain('ohne jeden Eintrag');
    // Read only. `permits()` returns false for every other action once no grant matches.
    expect(admin?.text).toContain('Schreiben');
  });

  it('names the groups that carry that reach, and says who decides membership', () => {
    const routes = reachRoutes({ ...base, visibility: 'restricted' });
    const admin = routes.find((route) => route.key === 'baseline');
    expect(admin?.text).toContain('»admins«');
    expect(admin?.text).toContain('Authelia');
  });

  it('says the mapping is unknown rather than implying no group has it', () => {
    // A space admin is refused `/api/admin/roles`, which is instance-wide. "No group is
    // mapped" and "this console may not read the mapping" are opposite facts.
    const routes = reachRoutes({
      ...base,
      visibility: 'restricted',
      adminGroups: null,
      internalGroups: null
    });
    const admin = routes.find((route) => route.key === 'baseline');
    expect(admin?.text).toContain('zeigt diese Ansicht nicht');
    expect(admin?.text).not.toContain('keine Gruppe');
  });

  it('distinguishes "no group is mapped" from "the mapping is unknown"', () => {
    const routes = reachRoutes({ ...base, visibility: 'restricted', adminGroups: [] });
    const admin = routes.find((route) => route.key === 'baseline');
    expect(admin?.text).toContain('keine Gruppe');
    expect(admin?.text).toContain('Einzelne Konten');
  });

  it('says a public page is readable by everyone, before any entry is consulted', () => {
    const routes = reachRoutes({ ...base, visibility: 'public' });
    const visibility = routes.find((route) => route.key === 'visibility');
    expect(visibility?.tone).toBe('warn');
    expect(visibility?.title).toContain('Öffentlich');
    expect(visibility?.text).toContain('ohne Anmeldung');
    expect(visibility?.text).toContain('offenen Internet');
  });

  it('names who an internal page reaches, which is not "everyone signed in"', () => {
    // `can()` alone would give an internal read to anybody authenticated. `permits()`
    // narrows it to baseline >= internal, which is a group question.
    const routes = reachRoutes({ ...base, visibility: 'internal' });
    const visibility = routes.find((route) => route.key === 'visibility');
    expect(visibility?.text).toContain('angemeldet');
    expect(visibility?.text).toContain('»users«');
    // The admin baseline is >= internal, so those groups read it too.
    expect(visibility?.text).toContain('»admins«');
  });

  it('says plainly that a restricted page lets nobody in through its visibility', () => {
    const routes = reachRoutes({ ...base, visibility: 'restricted' });
    const visibility = routes.find((route) => route.key === 'visibility');
    expect(visibility?.text).toContain('niemand');
  });

  it('marks an Anyone grant as the open internet, above everything else', () => {
    const routes = reachRoutes({
      ...base,
      visibility: 'restricted',
      shareLink: ['Lesen'],
      hasGrants: true
    });
    expect(routes[0].key).toBe('share');
    expect(routes[0].tone).toBe('warn');
    expect(routes[0].text).toContain('ohne Anmeldung');
    expect(routes[0].text).toContain('Lesen');
    // It is decided before authentication, so no visibility setting takes it back.
    expect(routes[0].text).toContain('Sichtbarkeit');
  });

  it('never says that with no entries nothing else applies', () => {
    // The sentence this whole exercise is about. "Es gilt allein die Sichtbarkeit der
    // Seite" is true about grants and reads as "nobody else gets in" — while an
    // administrator reads the page and an internal page is open to a whole group.
    const routes = reachRoutes({ ...base, visibility: 'restricted', hasGrants: false });
    const grants = routes.find((route) => route.key === 'grants');
    expect(grants?.title).toContain('Kein Zugriffseintrag');
    expect(grants?.text).toContain('Die übrigen Wege oben gelten weiter');
    const all = routes.map((route) => `${route.title} ${route.text}`).join(' ');
    expect(all).not.toContain('allein die Sichtbarkeit');
    expect(all).not.toContain('gilt allein');
  });

  it('says the visibility is unknown rather than leaving it out silently', () => {
    const routes = reachRoutes({ ...base, visibility: null });
    const visibility = routes.find((route) => route.key === 'visibility');
    expect(visibility?.text).toContain('nicht bekannt');
  });
});

describe('revokeResume', () => {
  it('says which ancestor resumes, and that it covers the whole subtree', () => {
    // `grants_for_path` returns the rows of the NEAREST ancestor that has any. Removing
    // the last row here does not remove access — it hands the page back to the ancestor,
    // and every page below that carries nothing of its own with it.
    expect(revokeResumeTitle('/handbuch')).toBe(
      'Danach gelten hier wieder die Rechte von /handbuch.'
    );
    const text = revokeResumeText('/handbuch/onboarding', '/handbuch', [
      'Redaktion (redaktion): Schreiben'
    ]);
    expect(text).toContain('letzte Zugriffseintrag auf /handbuch/onboarding');
    expect(text).toContain('auf jeder Seite darunter');
    expect(text).toContain('Redaktion (redaktion): Schreiben');
    expect(text).toContain('/handbuch');
  });

  it('does not promise the change can be undone here', () => {
    const text = revokeResumeText('/a/b', '/a', ['Team (t): Lesen']);
    expect(text).not.toContain('rückgängig');
    expect(text).toContain('ersetzt');
  });
});

describe('visibilityChange', () => {
  it('says the setting stops at this page, unlike a grant', () => {
    const text = visibilityChangeText('/handbuch');
    expect(text).toContain('Unterseiten behalten ihre eigene');
    expect(text).toContain('nicht nach unten');
  });

  it('says it takes nobody’s access away, which is the belief that gets people hurt', () => {
    // The mirror image of the grant warning. Somebody sets `restricted` believing it
    // closes the page; `permits()` consults grants first, so a grant on an ancestor
    // walks straight past it.
    const text = visibilityChangeText('/handbuch');
    expect(text).toContain('hebt keinen Zugriffseintrag auf');
    expect(text).toContain('»Eingeschränkt«');
    expect(text).toContain('Verwaltung');
  });

  it('spells out what each value actually does', () => {
    expect(visibilityConsequence('/handbuch', 'public')).toContain('offenen Internet');
    expect(visibilityConsequence('/handbuch', 'public')).toContain('ohne Anmeldung');
    expect(visibilityConsequence('/handbuch', 'internal')).toContain('angemeldet');
    expect(visibilityConsequence('/handbuch', 'restricted')).toContain('niemand');
    expect(visibilityConsequence('/handbuch', 'restricted')).toContain('bleiben davon unberührt');
  });

  it('passes an unrecognised value through rather than describing the wrong one', () => {
    expect(visibilityConsequence('/handbuch', 'geheim')).toContain('»geheim«');
  });
});
