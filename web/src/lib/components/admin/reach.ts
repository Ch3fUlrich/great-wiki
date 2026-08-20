/**
 * What a grant reaches, said in German, in one place.
 *
 * These sentences are the interface's half of a rule that until now only existed as a
 * comment in `crates/gw-store/src/acl.rs`:
 *
 *   > A grant decides on its own, at any visibility.
 *
 * `permits()` asks `can()` with the document presented at `Visibility::Restricted`
 * BEFORE it looks at what the document's visibility actually is. So a matching grant
 * returns true and the visibility branch is never reached. And `grants_for_path` returns
 * the grants of the NEAREST ancestor that carries any — a path being its own first
 * ancestor — and never unions them up the tree.
 *
 * Put together, and this is the part people get backwards: **marking a page
 * `restricted` does not hide it from somebody who holds a grant on an ancestor.** The
 * grant wins, and the only thing that stops it is a grant row on the page itself (or on
 * something between it and the ancestor), which then replaces the inherited set
 * entirely rather than adding to it.
 *
 * Two things these strings must never say, because neither is true here:
 *
 *  - that a page can be excluded by changing its visibility. Nothing in this system
 *    writes `visibility` at all — `/api/admin/acl` grants and revokes, and the value
 *    arrives from frontmatter at import — and it would not help if it did.
 *  - that a public page owes its readability to a grant. `can()` allows a public read
 *    before it consults anything, so on a public page an inherited grant confers the
 *    extra (comment, write, admin) and nothing about being able to read it.
 *
 * A module rather than inline template strings because the grant-time text is rendered
 * inside an Ark `Portal`, which renders nothing under `svelte/server` — as a function it
 * can be asserted directly (`reach.test.ts`), while `behaviour.mjs` Group H proves the
 * component actually puts it on screen.
 */

import { VISIBILITY_LABEL } from '$lib/adminApi';

/** `restricted` → `»Eingeschränkt«`; an unknown value passes through rather than vanishing. */
function quoted(visibility: string): string {
  return `»${VISIBILITY_LABEL[visibility] ?? visibility}«`;
}

/** The heading of the warning shown while somebody is making a grant. */
export function grantReachTitle(path: string): string {
  return `Gilt auch für alle Seiten unter ${path}.`;
}

/**
 * The consequence of the grant, at the moment it is being made.
 *
 * Deliberately names the exception and then closes the subject: "there is no other way"
 * is what stops somebody going looking for a narrowing control that does not exist.
 */
export function grantReachText(path: string): string {
  return (
    `Wer hier eingetragen wird, erreicht damit jede Seite unterhalb von ${path} – ` +
    'unabhängig von deren Sichtbarkeit. ' +
    'Auch eine Seite mit der Sichtbarkeit »Eingeschränkt« wird dadurch erreichbar. ' +
    'Davon ausgenommen ist allein eine Seite, die selbst einen Zugriffseintrag trägt: ' +
    'Der nächstgelegene Eintrag gilt vollständig und allein, geerbte Rechte gelten dort ' +
    'nicht mehr. Einen anderen Weg, einzelne Seiten auszunehmen, gibt es hier nicht.'
  );
}

/** What one grant, once chosen, will actually do. The abstract rule made concrete. */
export function grantConsequence(
  path: string,
  subjectName: string,
  permissionLabel: string
): string {
  return `${subjectName} erhält »${permissionLabel}« auf ${path} und auf jeder Seite darunter.`;
}

/** The heading over grants that are in force here but written on `source`. */
export function inheritedReachTitle(source: string, visibility?: string | null): string {
  if (visibility === 'public') return `Die Rechte unten kommen von ${source}.`;
  return `Erreichbar über ${source}, nicht über die Sichtbarkeit dieser Seite.`;
}

/** Why they apply here, where they can be removed, and what would replace them. */
export function inheritedReachText(
  path: string,
  source: string,
  visibility?: string | null
): string {
  const tail =
    `Entziehen lassen sie sich nur auf ${source}. ` +
    `Ein eigener Eintrag auf ${path} ersetzt die geerbten Rechte vollständig.`;

  if (visibility === 'public') {
    return (
      `Sie gelten hier, weil ${path} unter ${source} liegt. ` +
      'Lesen kann diese Seite ohnehin jede und jeder – die Sichtbarkeit ist ' +
      `${quoted('public')}; die Rechte unten gehen darüber hinaus. ${tail}`
    );
  }

  // No visibility to name is said as no visibility to name. Guessing `restricted`
  // because that is the Rust default would be inventing a fact about this document.
  const trotzdem = visibility
    ? `unabhängig davon, dass diese Seite als ${quoted(visibility)} gekennzeichnet ist`
    : 'unabhängig von der Sichtbarkeit dieser Seite';

  return (
    `Die Rechte unten sind auf ${source} eingetragen und gelten hier, weil ${path} ` +
    `darunter liegt – ${trotzdem}. ${tail}`
  );
}
