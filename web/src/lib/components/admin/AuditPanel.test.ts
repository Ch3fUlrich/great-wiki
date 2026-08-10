import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import type { ComponentProps } from 'svelte';
import AuditPanel from './AuditPanel.svelte';
import type { AdminPrincipal, AuditPage } from '$lib/adminApi';

type Props = ComponentProps<typeof AuditPanel>;

function html(props: Props): string {
  return render(AuditPanel, { props }).body.replace(/<!--.*?-->/g, '');
}

const principals: AdminPrincipal[] = [
  {
    id: 'p1',
    kind: 'oidc',
    username: 'sergej',
    display_name: 'Sergej Maul',
    email: null,
    groups: [],
    teams: [],
    active: true
  }
];

/** Deliberately out of order, to prove the panel does not simply trust the server. */
const page: AuditPage = {
  entries: [
    {
      id: 'a2',
      at: '2026-08-09 18:00:00',
      principal_id: 'p1',
      action: 'team.create',
      target: 'redaktion',
      path: null
    },
    {
      id: 'a1',
      at: '2026-08-10 09:12:03',
      principal_id: 'p1',
      action: 'acl.grant',
      target: 'team:redaktion',
      path: '/handbuch'
    },
    {
      id: 'a3',
      at: '2026-08-08 07:00:00',
      principal_id: null,
      action: 'unbekannt.tat',
      target: null,
      path: null
    }
  ],
  truncated: true
};

function props(over: Partial<Props> = {}): Props {
  return { page, error: null, principals, limit: 50, onLimitChange: () => {}, ...over };
}

describe('AuditPanel', () => {
  it('shows the newest entry first', () => {
    const out = html(props());
    const newest = out.indexOf('10.08.2026');
    const middle = out.indexOf('09.08.2026');
    const oldest = out.indexOf('08.08.2026');
    expect(newest).toBeGreaterThan(-1);
    expect(newest).toBeLessThan(middle);
    expect(middle).toBeLessThan(oldest);
  });

  it('shows time, who, what and the path', () => {
    const out = html(props());
    expect(out).toContain('10.08.2026, 09:12');
    expect(out).toContain('Sergej Maul (sergej)');
    expect(out).toContain('Zugriff gewährt');
    expect(out).toContain('/handbuch');
  });

  it('labels the timestamp column as UTC, since that is what the API stores', () => {
    const out = html(props());
    expect(out).toContain('Zeitpunkt (UTC)');
  });

  it('shows an unrecognised action verbatim rather than hiding it', () => {
    // The log exists to make actions visible. Rendering a new action type as "Unbekannt"
    // would make exactly the entry nobody has seen before the one nobody can read.
    const out = html(props());
    expect(out).toContain('unbekannt.tat');
  });

  it('says an entry is instance-wide rather than leaving the path cell blank', () => {
    // `path: null` means the action concerns the whole instance, not that the path is
    // unknown. An empty cell would read as missing data.
    const out = html(props());
    expect(out).toContain('instanzweit');
  });

  it('shows the target beside the verb, since it is usually not a path', () => {
    const out = html(props());
    expect(out).toContain('team:redaktion');
    expect(out).toContain('redaktion');
  });

  it('attributes an entry with no principal to the system', () => {
    const out = html(props());
    expect(out).toContain('System');
  });

  it('says when older entries were cut off, and that they are kept', () => {
    // D-M2-13: entries are kept indefinitely. A truncated view must not read as
    // "that is all there ever was".
    const out = html(props());
    expect(out).toContain('neuesten 50 Einträge');
    expect(out).toContain('dauerhaft aufbewahrt');
  });

  it('renders the failure in German rather than an empty table', () => {
    const out = html(
      props({
        page: null,
        error: 'Das Protokoll konnte nicht geladen werden: Der Server hat mit 500 geantwortet.'
      })
    );
    expect(out).toContain('Protokoll nicht geladen.');
    expect(out).toContain('mit 500 geantwortet');
    expect(out).toContain('role="alert"');
    expect(out).not.toContain('<table');
  });

  it('distinguishes an empty log from a failed request', () => {
    const out = html(props({ page: { entries: [], truncated: false } }));
    expect(out).toContain('Es ist noch nichts protokolliert.');
    expect(out).not.toContain('nicht geladen');
  });

  it('gives the table a caption and a scope on every header', () => {
    const out = html(props());
    expect(out).toMatch(/<caption[^>]*>Verwaltungsvorgänge, neueste zuerst/);
    expect(out).toContain('<th scope="col">Wer</th>');
    expect(out).toMatch(/<th scope="row" class="gw-adm-mono">10\.08\.2026/);
  });
});
