import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import AccessPanel from './AccessPanel.svelte';
import type { AclView, AdminPrincipal, Team } from '$lib/adminApi';

type Props = ComponentProps<typeof AccessPanel>;

/**
 * Server-rendered HTML, comments stripped — the same shape as `AccountMenu.test.ts`.
 *
 * Worth knowing before reading the assertions: **nothing inside an Ark `Portal` is
 * rendered on the server.** Ark's Svelte portal mounts its children from an `$effect`,
 * which never runs during server rendering, so every dialog body, dropdown list, menu and
 * tooltip is simply absent here. What IS rendered is the trigger — which is exactly what
 * these tests need, because the question they answer is whether a control is OFFERED.
 */
function html(props: Props): string {
  return render(AccessPanel, { props }).body.replace(/<!--.*?-->/g, '');
}

const principals: AdminPrincipal[] = [
  {
    id: 'p1',
    kind: 'oidc',
    username: 'sergej',
    display_name: 'Sergej Maul',
    email: null,
    groups: ['admins'],
    teams: ['redaktion'],
    active: true
  },
  {
    id: 'p2',
    kind: 'local',
    username: 'gast',
    display_name: 'Gast Konto',
    email: null,
    groups: [],
    teams: [],
    active: true
  }
];

const teams: Team[] = [
  { slug: 'redaktion', name: 'Redaktion', members: [] },
  { slug: 'leser', name: 'Leser', members: [] }
];

const inherited: AclView = {
  path: '/handbuch/onboarding',
  effective: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }],
  inherited_from: '/handbuch',
  defined_here: []
};

/**
 * `inherited_from` is the path ITSELF here, and that is not a typo in the fixture.
 *
 * `Store::effective_grants` walks ancestors nearest-first and a path is its own first
 * ancestor, so any path carrying grants of its own comes back naming itself — see
 * `a_space_admin_manages_grants_on_their_own_subtree` in `crates/gw-api/tests/admin.rs`,
 * which asserts exactly that. A fixture with `null` there described a response the API
 * cannot produce, and hid the fact that the panel called such grants "geerbt".
 */
const definedHere: AclView = {
  path: '/handbuch',
  effective: [
    { subject: { kind: 'team', id: 'redaktion' }, permission: 'write' },
    { subject: { kind: 'principal', id: 'p2' }, permission: 'read' }
  ],
  inherited_from: '/handbuch',
  defined_here: [
    { subject: { kind: 'team', id: 'redaktion' }, permission: 'write' },
    { subject: { kind: 'principal', id: 'p2' }, permission: 'read' }
  ]
};

const noop = async () => true;

function props(over: Partial<Props> = {}): Props {
  return {
    path: inherited.path,
    acl: inherited,
    visibility: 'restricted',
    error: null,
    principals,
    teams,
    onGrant: noop,
    onRevoke: noop,
    onSelectPath: () => {},
    ...over
  };
}

describe('AccessPanel', () => {
  it('names the ancestor the grants are inherited from', () => {
    // The one sentence the whole panel exists to say. Without it, somebody looking at a
    // page's access has no way to know that none of it is stored on that page.
    const out = html(props());
    expect(out).toContain('Geerbt von /handbuch');
    expect(out).toContain('Wer /handbuch/onboarding erreicht');
  });

  it('offers no revoke control for an inherited grant', () => {
    // Not "a disabled revoke button" — no control at all. A disabled button still reads
    // as "this is the control for this row", and revoking here would change nothing:
    // the grant lives on the ancestor and the API would refuse it.
    //
    // Asserted against the control and not against the bare word "Entziehen", which the
    // explanation above the table now uses as a verb ("Entziehen lassen sie sich nur auf
    // /handbuch") — deliberately the same word the button carries, because calling one
    // operation two things is its own confusion.
    const out = html(props());
    expect(out).not.toContain('gw-adm-trigger--danger');
    expect(out).not.toMatch(/aria-haspopup="dialog"[^>]*>[^<]*<span>Entziehen<\/span>/);
    expect(out).toContain('Redaktion (redaktion)');
  });

  it('offers a revoke control for a grant stored on this very path', () => {
    const out = html(props({ path: definedHere.path, acl: definedHere }));
    expect(out).toContain('Entziehen');
    expect(out).not.toContain('Geerbt von');
  });

  it('puts a confirmation behind revoking, rather than a bare button', () => {
    // The wording of the confirmation cannot be asserted here — it is inside Ark's
    // Portal, which renders nothing on the server. What can be asserted is that the
    // control is a dialog trigger at all: `aria-haspopup="dialog"` is the difference
    // between "asks first" and "deletes on click".
    const out = html(props({ path: definedHere.path, acl: definedHere }));
    expect(out).toMatch(/aria-haspopup="dialog"[^>]*>[^<]*<span>Entziehen<\/span>/);
  });

  it('renders the failure in German rather than an empty panel', () => {
    // The administration API ships separately from this interface, so "the endpoint is
    // not there" is a state a person will actually meet.
    const out = html(
      props({
        acl: null,
        error:
          'Die Zugriffsrechte für /handbuch konnten nicht geladen werden: Dieser Endpunkt existiert nicht (404).'
      })
    );
    expect(out).toContain('Zugriffsrechte nicht geladen.');
    expect(out).toContain('Dieser Endpunkt existiert nicht (404)');
    expect(out).toContain('role="alert"');
    // No table, and nothing that suggests something is still loading.
    expect(out).not.toContain('<table');
  });

  it('says what to do when no path has been chosen', () => {
    const out = html(props({ path: null, acl: null }));
    expect(out).toContain('Wähle links eine Seite aus');
    expect(out).not.toContain('<table');
  });

  it('distinguishes "no grants" from "not loaded"', () => {
    // Both would otherwise be an empty table, and they mean opposite things: one is a
    // configuration fact, the other is a failure.
    const out = html(
      props({
        path: '/oeffentlich',
        visibility: 'public',
        acl: {
          path: '/oeffentlich',
          effective: [],
          inherited_from: null,
          defined_here: []
        }
      })
    );
    expect(out).toContain('kein Zugriff eingetragen');
    expect(out).toContain('Sichtbarkeit: Öffentlich');
    expect(out).not.toContain('nicht geladen');
  });

  it('gives the table a caption and a scope on every header', () => {
    const out = html(props({ path: definedHere.path, acl: definedHere }));
    expect(out).toMatch(/<caption[^>]*>Wer \/handbuch erreicht/);
    expect(out).toContain('<th scope="col">Subjekt</th>');
    expect(out).toContain('<th scope="col">Berechtigung</th>');
    expect(out).toMatch(/<th scope="row"[^>]*>\s*Redaktion \(redaktion\)/);
  });

  it('resolves subject ids to names, and shows the raw id when it resolves to nobody', () => {
    // An id that no longer matches anybody is information — the principal was deleted —
    // and blanking the cell would hide the one row somebody is looking for.
    const orphaned: AclView = {
      ...definedHere,
      effective: [{ subject: { kind: 'principal', id: 'weg-damit' }, permission: 'read' }],
      defined_here: [{ subject: { kind: 'principal', id: 'weg-damit' }, permission: 'read' }]
    };
    const out = html(props({ path: orphaned.path, acl: orphaned }));
    expect(out).toContain('weg-damit');
  });

  it('says on the page itself why this page is reachable, and does not hide it in a tooltip', () => {
    // The correction this panel owes its reader. `permits()` in `crates/gw-store/src/acl.rs`
    // asks `can()` at `Visibility::Restricted` BEFORE it looks at the document's own
    // visibility, so a grant on /handbuch reaches /handbuch/onboarding even though that
    // page is marked `restricted`. Marking a page restricted does not hide it from
    // somebody holding a grant above it — which is the opposite of what most people
    // assume, so it has to be a sentence on screen and not a hover.
    const out = html(props());
    expect(out).toContain('Erreichbar über /handbuch, nicht über die Sichtbarkeit dieser Seite.');
    expect(out).toContain(
      'unabhängig davon, dass diese Seite als »Eingeschränkt« gekennzeichnet ist'
    );
    expect(out).toContain(
      'Ein eigener Eintrag auf /handbuch/onboarding ersetzt die geerbten Rechte vollständig.'
    );
    // No `Tooltip.Trigger`: everything inside Ark's `Portal` is absent from the server
    // render, so a tooltip is exactly the thing a person reading this page never sees.
    expect(out).not.toContain('gw-adm-tip-trigger');
  });

  it('does not call a grant written on this very path an inherited one', () => {
    // `inherited_from` naming the path itself is the API's normal answer for a path that
    // carries grants. Rendering "Geerbt von /handbuch" on /handbuch sends somebody up the
    // tree looking for a row that is right in front of them.
    const out = html(props({ path: definedHere.path, acl: definedHere }));
    expect(out).not.toContain('Geerbt von');
    expect(out).not.toContain('Erreichbar über');
    expect(out).toContain('Entziehen');
  });

  it('does not claim a public page would be unreachable without the inherited grant', () => {
    // A public read is the first thing `can()` allows and it needs no grant at all, so
    // "reachable because of /handbuch" would be false here. The inherited grant confers
    // the extra — writing — and the sentence has to say that instead.
    const out = html(
      props({
        path: '/oeffentlich/unterseite',
        visibility: 'public',
        acl: {
          path: '/oeffentlich/unterseite',
          effective: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }],
          inherited_from: '/oeffentlich',
          defined_here: []
        }
      })
    );
    expect(out).toContain('Die Rechte unten kommen von /oeffentlich.');
    expect(out).toContain('Lesen kann diese Seite ohnehin jede und jeder');
    expect(out).not.toContain('nicht über die Sichtbarkeit');
  });

  it('says the permissions and subject kinds in German', () => {
    const out = html(props({ path: definedHere.path, acl: definedHere }));
    expect(out).toContain('Schreiben');
    expect(out).toContain('Lesen');
    expect(out).toContain('Team');
    expect(out).toContain('Person');
    expect(out).not.toContain('>write<');
    expect(out).not.toContain('>read<');
  });
});
