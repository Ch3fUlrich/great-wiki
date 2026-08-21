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
 *  - that a page can be excluded by changing its visibility. `/api/admin/visibility`
 *    does now write that field — the console offers the control, and this module's own
 *    `visibilityChangeText` describes it — but it would not help: `permits()` consults
 *    the grants BEFORE it reads the field, so lowering a page to »Eingeschränkt« closes
 *    nothing an entry has opened. Sending somebody there is sending them to a control
 *    that exists and does not do what they were told it does, which is worse than
 *    sending them to one that does not exist.
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

/**
 * The heading over grants that are in force here but written on `source`.
 *
 * `public` and `internal` both earn their own sentence for the same reason: on either,
 * this page's OWN visibility genuinely does grant reach, so "nicht über die Sichtbarkeit
 * dieser Seite" would be false — not merely incomplete. `restricted`, and the case where
 * this view was not told a visibility at all, are the only ones where the ancestor grant
 * really is the only route in, which is what the generic sentence claims.
 */
export function inheritedReachTitle(source: string, visibility?: string | null): string {
  if (visibility === 'public') return `Die Rechte unten kommen von ${source}.`;
  if (visibility === 'internal') {
    return (
      `Die Rechte unten kommen von ${source} — zusätzlich zur Sichtbarkeit »Intern« ` +
      'dieser Seite, die für sich schon reicht.'
    );
  }
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

// --------------------------------------------------------------------------------------
// The whole answer, not only the grants half.
// --------------------------------------------------------------------------------------
//
// Everything above describes what a GRANT does. That was never the whole question the
// panel asks, and the gap was doing real harm: a table captioned "Wer /x erreicht" that
// lists grants is silent about the two ways into a page that need no grant at all.
//
// `permits()` in `crates/gw-store/src/acl.rs` decides in this order:
//
//   1. `can(principal, action, Visibility::Restricted, grants)` — grants only. An
//      `Anyone` grant is answered here BEFORE authentication is looked at, which is what
//      makes it a public share link rather than an entry for a very large group.
//   2. anything but a read: refused. Default reach never confers writing.
//   3. `Public`   — `can()` again, which allows the read before it consults anything.
//   4. `Internal` — `baseline >= Internal`.
//   5. `Restricted` — `baseline >= Admin`.
//
// So the honest answer has four parts, and only the last of them is the table.

/** One way into this page. Rendered as a list, in the order `permits()` decides. */
export interface ReachRoute {
  key: 'share' | 'visibility' | 'baseline' | 'grants';
  /** `warn` for the two that reach beyond the people who were deliberately named. */
  tone: 'warn' | 'info';
  title: string;
  text: string;
}

export interface ReachInput {
  path: string;
  /** `public` | `internal` | `restricted`, or `null` when this view does not know. */
  visibility?: string | null;
  /**
   * German labels of the permissions any `Subject::Anyone` grant confers. Empty when
   * there is none — which is the ordinary case, and the reason the entry is conditional.
   */
  shareLink: string[];
  /** Whether any grant at all is in force here, inherited ones included. */
  hasGrants: boolean;
  /**
   * OIDC groups mapped to the `admin` and `internal` baselines.
   *
   * `null` means the mapping could not be read — `/api/admin/roles` is instance-wide, so
   * a space admin is refused it. That is a different fact from an empty list, and the
   * copy below keeps them apart: "no group is mapped" would be a claim about the
   * instance, made by a view that was not allowed to look.
   */
  adminGroups: string[] | null;
  internalGroups: string[] | null;
}

/** `['a', 'b']` → `»a«, »b«`. */
function quotedList(names: string[]): string {
  return names.map((name) => `»${name}«`).join(', ');
}

/** Which groups confer at least `internal` reach: the internal ones AND the admin ones. */
function atLeastInternal(input: ReachInput): string[] | null {
  if (input.internalGroups === null || input.adminGroups === null) return null;
  return [...new Set([...input.internalGroups, ...input.adminGroups])].sort();
}

/**
 * The sentence that names the groups behind a baseline, or says why it cannot.
 *
 * `also`, when given, is appended to WHICHEVER branch fires — not only the empty one.
 * `baseline_on` in `crates/gw-store/src/acl.rs` checks the `instance_admins` promotion
 * BEFORE it ever looks at a group, so a promoted account carries the reach regardless of
 * whether any group is mapped to it. That fact does not become less true once a group
 * IS mapped, and saying it only in the empty branch reads as "only these groups", which
 * `baseline_on`'s ordering never claimed.
 */
function groupClause(groups: string[] | null, none: string, also?: string): string {
  if (groups === null) {
    return 'Welche Gruppen das sind, zeigt diese Ansicht nicht — dafür fehlen ihr die Rechte.';
  }
  const suffix = also ? ` ${also}` : '';
  if (groups.length === 0) return `${none}${suffix}`;
  const plural = groups.length === 1 ? 'die Gruppe' : 'die Gruppen';
  return (
    `Zurzeit betrifft das ${plural} ${quotedList(groups)}; wer darin ist, entscheidet Authelia ` +
    `und nicht diese Seite.${suffix}`
  );
}

function shareLinkRoute(input: ReachInput): ReachRoute | null {
  if (input.shareLink.length === 0) return null;
  return {
    key: 'share',
    tone: 'warn',
    title: 'Freigabelink: erreichbar aus dem offenen Internet.',
    text:
      'Ein Eintrag für »Alle, auch nicht angemeldete« wirkt, bevor überhaupt geprüft wird, ' +
      `ob jemand angemeldet ist. Diese Seite ist dadurch ohne Anmeldung erreichbar: ${input.shareLink.join(
        ', '
      )}. ` +
      'Die Sichtbarkeit der Seite nimmt das nicht zurück — solange der Eintrag steht, ' +
      'gilt er. In der Tabelle unten ist es die Zeile »Alle, auch nicht angemeldete«.'
  };
}

function visibilityRoute(input: ReachInput): ReachRoute {
  const { path, visibility } = input;

  if (!visibility) {
    return {
      key: 'visibility',
      tone: 'info',
      title: 'Sichtbarkeit: hier nicht bekannt.',
      text:
        `Wie offen ${path} ist, ist dieser Ansicht gerade nicht bekannt, und sie rät nicht. ` +
        'Sie entscheidet mit darüber, wer die Seite ohne jeden Eintrag erreicht.'
    };
  }

  if (visibility === 'public') {
    return {
      key: 'visibility',
      tone: 'warn',
      title: 'Sichtbarkeit »Öffentlich«: lesen darf jede und jeder.',
      text:
        'Auch ohne Anmeldung, und ohne jeden Eintrag unten — diese Seite steht damit im ' +
        'offenen Internet. Schreiben ist davon nicht umfasst: dafür braucht es weiterhin ' +
        'einen Eintrag.'
    };
  }

  if (visibility === 'internal') {
    return {
      key: 'visibility',
      tone: 'info',
      title: 'Sichtbarkeit »Intern«: alle Angemeldeten mit interner Reichweite.',
      text:
        'Wer angemeldet und aktiv ist und über eine Gruppe mindestens interne Reichweite hat, ' +
        'liest diese Seite ohne jeden Eintrag unten. ' +
        `${groupClause(
          atLeastInternal(input),
          'Zurzeit ist dafür keine Gruppe eingetragen.',
          // `baseline_on` checks the per-account promotion before any group, and Admin
          // sits above Internal in the ordering (`Baseline::Admin > Baseline::Internal`),
          // so a promoted account reaches an internal page whether or not a group does.
          // "also erreicht so niemand die Seite" was false whenever that promotion exists.
          'Wer einzeln die Reichweite »Verwaltung« trägt, erreicht die Seite trotzdem — ' +
            'diese Reichweite schließt »Intern« ein.'
        )} ` +
        'Lesen ist alles, was das mitbringt.'
    };
  }

  if (visibility === 'restricted') {
    return {
      key: 'visibility',
      tone: 'info',
      title: 'Sichtbarkeit »Eingeschränkt«: über die Sichtbarkeit kommt niemand herein.',
      text:
        'Wer diese Seite erreicht, erreicht sie über einen Eintrag oder über die ' +
        'Reichweite »Verwaltung« — nicht über die Sichtbarkeit. Umgekehrt nimmt ' +
        '»Eingeschränkt« niemandem etwas weg, der über einen Eintrag hereinkommt.'
    };
  }

  // A value this interface does not know. Passed through rather than dropped or guessed:
  // the store falls back to `restricted` when it cannot parse the column, but saying so
  // here would be inventing a fact about a document.
  return {
    key: 'visibility',
    tone: 'info',
    title: `Sichtbarkeit »${visibility}«: dieser Wert ist hier nicht bekannt.`,
    text:
      'Was er bewirkt, sagt diese Ansicht nicht, weil sie es nicht weiß. Bekannt sind ' +
      '»Öffentlich«, »Intern« und »Eingeschränkt«.'
  };
}

function baselineRoute(input: ReachInput): ReachRoute {
  return {
    key: 'baseline',
    tone: 'info',
    title: 'Reichweite »Verwaltung«: liest jede Seite, ohne Eintrag.',
    text:
      'Wer die Reichweite »Verwaltung« hat, liest jede Seite dieses Wikis — auch eine mit ' +
      'der Sichtbarkeit »Eingeschränkt«, und ohne jeden Eintrag unten. ' +
      `${groupClause(
        input.adminGroups,
        'Zurzeit ist dafür keine Gruppe eingetragen.',
        // True whether or not a group IS mapped — `baseline_on` reads the per-account
        // promotion before it ever looks at groups — so this belongs in both branches of
        // `groupClause`, not only the one that used to carry it.
        'Einzelne Konten können die Reichweite trotzdem tragen: sie lässt sich auch pro ' +
          'Person vergeben.'
      )} ` +
      'Lesen ist alles, was das mitbringt — Schreiben braucht auch dort einen Eintrag.'
  };
}

function grantsRoute(input: ReachInput): ReachRoute {
  if (input.hasGrants) {
    return {
      key: 'grants',
      tone: 'info',
      title: 'Zugriffseinträge: die Tabelle unten.',
      text:
        'Sie gelten zusätzlich zu allem, was hier steht, und unabhängig von der ' +
        'Sichtbarkeit dieser Seite.'
    };
  }
  return {
    key: 'grants',
    tone: 'info',
    title: 'Kein Zugriffseintrag.',
    // "and on every page above it" is not a flourish: `effective` is empty exactly when
    // no ancestor carries a row either, because the nearest one with any wins outright.
    text:
      `Weder auf ${input.path} noch auf einer übergeordneten Seite ist etwas eingetragen. ` +
      'Die übrigen Wege oben gelten weiter.'
  };
}

/**
 * Every way into this page, in the order `permits()` decides them.
 *
 * Deliberately a pure function over facts the caller has already established, so that
 * what it says can be asserted without a component, a store or a browser.
 */
export function reachRoutes(input: ReachInput): ReachRoute[] {
  const share = shareLinkRoute(input);
  return [
    ...(share ? [share] : []),
    visibilityRoute(input),
    baselineRoute(input),
    grantsRoute(input)
  ];
}

// --------------------------------------------------------------------------------------
// Revoking the last entry on a path.
// --------------------------------------------------------------------------------------

/** The heading over the warning shown when a revoke would empty this path. */
export function revokeResumeTitle(source: string): string {
  return `Danach gelten hier wieder die Rechte von ${source}.`;
}

/**
 * What removing the final entry on a path actually does.
 *
 * Not a local change. `grants_for_path` returns the rows of the nearest ancestor that has
 * ANY, so a path with nothing of its own falls through to the one above — and so does
 * every page below it that carries nothing of its own. The old dialog said inherited
 * rights "bleiben davon unberührt", which is true and reads as "they stay where they are";
 * they resume applying *here*.
 */
export function revokeResumeText(path: string, source: string, resuming: string[]): string {
  return (
    `Das ist der letzte Zugriffseintrag auf ${path}. Ohne ihn trägt ${path} nichts Eigenes ` +
    `mehr, und der nächstgelegene Eintrag darüber gilt wieder: hier und auf jeder Seite ` +
    `darunter, die selbst nichts eingetragen hat. Zurück kommen damit die Rechte von ` +
    `${source} — ${resuming.join('; ')}. Entziehen lassen sie sich dann nur noch auf ` +
    `${source}; ein neuer Eintrag auf ${path} ersetzt sie hier wieder vollständig.`
  );
}

// --------------------------------------------------------------------------------------
// Changing how open a page is.
// --------------------------------------------------------------------------------------

/** The heading over the warning shown while the visibility is being changed. */
export function visibilityChangeTitle(path: string): string {
  return `Gilt nur für ${path} — und nimmt niemandem den Zugriff.`;
}

/**
 * The two things about visibility that are the opposite of what people expect, said at
 * the moment somebody is about to rely on the expectation.
 *
 * The grant warning's mirror image, and deliberately not a copy of it. There the surprise
 * is that an entry reaches DOWN; here it is that visibility does not, and that lowering it
 * closes nothing that an entry has opened — `permits()` asks `can()` about the grants
 * before it ever looks at this field.
 */
export function visibilityChangeText(path: string): string {
  return (
    'Die Sichtbarkeit entscheidet, wer diese Seite ohne jeden Zugriffseintrag erreicht. ' +
    `Sie gilt für ${path} allein: Unterseiten behalten ihre eigene, anders als ein ` +
    'Zugriffseintrag wirkt sie nicht nach unten. Und sie hebt keinen Zugriffseintrag auf — ' +
    'wer über einen Eintrag auf dieser oder einer übergeordneten Seite hereinkommt, kommt ' +
    'auch bei »Eingeschränkt« weiter herein, und wer die Reichweite »Verwaltung« hat, ' +
    'liest die Seite ohnehin.'
  );
}

/** What one particular value will do, once it is chosen. */
export function visibilityConsequence(path: string, value: string): string {
  if (value === 'public') {
    return (
      `»Öffentlich« veröffentlicht ${path} im offenen Internet: lesen darf die Seite dann ` +
      'jede und jeder, ohne Anmeldung. Schreiben bleibt davon unberührt.'
    );
  }
  if (value === 'internal') {
    return (
      `»Intern« gibt ${path} für alle frei, die angemeldet und aktiv sind und über eine ` +
      'Gruppe mindestens interne Reichweite haben — ohne weiteren Eintrag.'
    );
  }
  if (value === 'restricted') {
    return (
      `»Eingeschränkt« heißt: über die Sichtbarkeit erreicht ${path} niemand mehr. ` +
      'Bestehende Zugriffseinträge und die Reichweite »Verwaltung« bleiben davon unberührt.'
    );
  }
  return `»${value}« ist dieser Ansicht nicht bekannt; was der Wert bewirkt, sagt sie deshalb nicht.`;
}
