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

const definedHere: AclView = {
  path: '/handbuch',
  effective: [
    { subject: { kind: 'team', id: 'redaktion' }, permission: 'write' },
    { subject: { kind: 'principal', id: 'p2' }, permission: 'read' }
  ],
  inherited_from: null,
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
    const out = html(props());
    expect(out).not.toContain('Entziehen');
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
