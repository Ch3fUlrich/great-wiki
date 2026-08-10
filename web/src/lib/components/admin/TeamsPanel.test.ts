import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import TeamsPanel from './TeamsPanel.svelte';
import type { AdminPrincipal, Team } from '$lib/adminApi';

type Props = ComponentProps<typeof TeamsPanel>;

function html(props: Props): string {
  return render(TeamsPanel, { props }).body.replace(/<!--.*?-->/g, '');
}

const principals: AdminPrincipal[] = [
  {
    id: 'p1',
    kind: 'oidc',
    username: 'sergej',
    display_name: 'Sergej Maul',
    email: null,
    groups: [],
    teams: ['redaktion'],
    active: true
  }
];

const teams: Team[] = [
  { slug: 'redaktion', name: 'Redaktion', members: ['p1'] },
  { slug: 'leser', name: 'Leser', members: [] }
];

const noop = async () => true;

function props(over: Partial<Props> = {}): Props {
  return {
    teams,
    principals,
    error: null,
    onCreateTeam: noop,
    onAddMember: noop,
    onRemoveMember: noop,
    ...over
  };
}

describe('TeamsPanel', () => {
  it('lists every team with its members', () => {
    const out = html(props());
    expect(out).toContain('Redaktion');
    expect(out).toContain('redaktion');
    expect(out).toContain('Sergej Maul');
    expect(out).toMatch(/<caption[^>]*>Mitglieder von Redaktion/);
  });

  it('resolves member ids to people, since the API returns only ids', () => {
    const out = html(props());
    expect(out).toContain('Sergej Maul');
    expect(out).toContain('sergej');
    expect(out).not.toMatch(/<th scope="row">p1<\/th>/);
  });

  it('shows a membership pointing at nobody rather than hiding it', () => {
    // That row is exactly the one somebody came here to remove; dropping it would leave
    // a team with a member that cannot be seen and therefore cannot be taken out.
    const out = html(props({ teams: [{ slug: 'geist', name: 'Geist', members: ['weg'] }] }));
    expect(out).toContain('weg');
    expect(out).toContain('unbekannt');
  });

  it('says a team is empty instead of rendering an empty table', () => {
    const out = html(props());
    expect(out).toContain('»Leser« hat noch keine Mitglieder.');
  });

  it('puts removing a member behind a confirmation dialog', () => {
    const out = html(props());
    expect(out).toMatch(/aria-haspopup="dialog"[^>]*>[^<]*<span>Entfernen<\/span>/);
  });

  it('renders the failure in German rather than an empty panel', () => {
    const out = html(
      props({
        teams: null,
        error: 'Die Teamliste konnte nicht geladen werden: Die Verwaltungs-API ist nicht erreichbar.'
      })
    );
    expect(out).toContain('Teams nicht geladen.');
    expect(out).toContain('nicht erreichbar');
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('<table');
  });

  it('distinguishes "no teams yet" from "could not load teams"', () => {
    const out = html(props({ teams: [] }));
    expect(out).toContain('Es ist noch kein Team angelegt.');
    expect(out).not.toContain('nicht geladen');
  });

  it('gives the member table a scope on every header', () => {
    const out = html(props());
    expect(out).toContain('<th scope="col">Benutzername</th>');
    expect(out).toMatch(/<th scope="row">Sergej Maul<\/th>/);
  });
});
