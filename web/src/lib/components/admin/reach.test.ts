import { describe, expect, it } from 'vitest';
import {
  grantReachText,
  grantReachTitle,
  inheritedReachText,
  inheritedReachTitle
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

  it('never tells anybody to change a page’s visibility, which nothing in this system can do', () => {
    // No API route writes `visibility`: `/api/admin/acl` grants and revokes, and the
    // value comes from frontmatter at import. Copy that says "set this page to
    // restricted" would be an instruction nobody can follow — and would be wrong anyway,
    // since a grant outranks it.
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
