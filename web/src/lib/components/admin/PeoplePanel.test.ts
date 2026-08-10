import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import PeoplePanel from './PeoplePanel.svelte';
import type { AdminPrincipal } from '$lib/adminApi';

type Props = ComponentProps<typeof PeoplePanel>;

function html(props: Props): string {
  return render(PeoplePanel, { props }).body.replace(/<!--.*?-->/g, '');
}

const principals: AdminPrincipal[] = [
  {
    id: 'p1',
    kind: 'oidc',
    username: 'sergej',
    display_name: 'Sergej Maul',
    email: null,
    groups: ['admins', 'users'],
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
    active: false
  }
];

const noop = async () => true;

function props(over: Partial<Props> = {}): Props {
  return { principals, error: null, onCreate: noop, onSetActive: noop, ...over };
}

describe('PeoplePanel', () => {
  it('names the source of every account', () => {
    // The distinction that matters operationally: an Authelia row is a mirror of a
    // homelab account, a local one is great-wiki's own. Deactivating them means
    // different things.
    const out = html(props());
    expect(out).toContain('Authelia');
    expect(out).toContain('Lokal');
    expect(out).not.toContain('>oidc<');
  });

  it('says that homelab accounts are not managed here', () => {
    // ADR 0002: great-wiki never writes Authelia's user database. Somebody will look for
    // "create a homelab user" on this tab, and it has to be answered before they try.
    const out = html(props());
    expect(out).toContain('Homelab-Konten werden in der Konten-App verwaltet');
    expect(out).toContain('niemals in die Benutzerdatenbank von Authelia');
  });

  it('shows the active state as words, not only as a switch position', () => {
    // A control whose meaning is carried entirely by which end a knob sits at is
    // unreadable for anybody who cannot see it.
    const out = html(props());
    expect(out).toContain('Aktiv');
    expect(out).toContain('Deaktiviert');
  });

  it('renders the failure in German rather than an empty table', () => {
    const out = html(
      props({
        principals: null,
        error: 'Die Personenliste konnte nicht geladen werden: Dafür fehlen die Rechte (403).'
      })
    );
    expect(out).toContain('Personen nicht geladen.');
    expect(out).toContain('Dafür fehlen die Rechte (403)');
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('<table');
  });

  it('distinguishes an empty instance from a failed request', () => {
    const out = html(props({ principals: [] }));
    expect(out).toContain('Es ist noch niemand angelegt');
    expect(out).not.toContain('nicht geladen');
  });

  it('gives the table a caption and a scope on every header', () => {
    const out = html(props());
    expect(out).toMatch(/<caption[^>]*>Alle Personen/);
    expect(out).toContain('<th scope="col">Quelle</th>');
    expect(out).toContain('<th scope="col">Gruppen</th>');
    expect(out).toContain('<th scope="col">Status</th>');
    expect(out).toMatch(/<th scope="row">\s*Sergej Maul/);
  });

  it('puts creating an account behind a dialog', () => {
    const out = html(props());
    expect(out).toMatch(/aria-haspopup="dialog"[^>]*>[^<]*<span>Person anlegen<\/span>/);
  });

  it('shows an em dash rather than an empty cell for someone with no groups', () => {
    // An empty cell reads as "not loaded"; a dash reads as "none", which is what it is.
    const out = html(props({ principals: [principals[1]] }));
    expect(out).toContain('—');
  });
});
