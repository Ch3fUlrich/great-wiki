import { describe, expect, it, vi, afterEach } from 'vitest';
import {
  describeStatus,
  formatInstant,
  parseSubjectKey,
  removeGrant,
  setPrincipalActive,
  subjectKey,
  subjectLabel,
  type AdminPrincipal,
  type Team
} from '$lib/adminApi';

const principals: AdminPrincipal[] = [
  {
    id: 'p1',
    kind: 'local',
    username: 'gast',
    display_name: 'Gast Konto',
    email: null,
    groups: [],
    teams: [],
    active: true
  }
];
const teams: Team[] = [{ slug: 'redaktion', name: 'Redaktion', members: [] }];

afterEach(() => {
  vi.unstubAllGlobals();
});

function stubFetch(response: Response | Error) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => {
      if (response instanceof Error) throw response;
      return response;
    })
  );
}

describe('describeStatus', () => {
  it('separates "no answer at all" from "answered with an error"', () => {
    // They send somebody to different places: one is the network, the other the server.
    expect(describeStatus(0, 'X')).toContain('nicht erreichbar');
    expect(describeStatus(500, 'X')).toContain('mit 500 geantwortet');
  });

  it('says plainly that a 404 means the endpoint is not there yet', () => {
    // This milestone ships the API and this interface separately, so 404 is the state a
    // person meets first — and "not found" on its own would read as a bug in the console.
    expect(describeStatus(404, 'X')).toContain('noch nicht vorhanden');
  });

  it('names the permission problem for 403 and the session for 401', () => {
    expect(describeStatus(403, 'X')).toContain('fehlen die Rechte');
    expect(describeStatus(401, 'X')).toContain('Nicht angemeldet');
  });
});

describe('subjects', () => {
  it('round-trips a subject through its key', () => {
    for (const subject of [
      { kind: 'principal' as const, id: 'p1' },
      { kind: 'team' as const, id: 'redaktion' },
      { kind: 'group' as const, id: 'admins' }
    ]) {
      expect(parseSubjectKey(subjectKey(subject))).toEqual(subject);
    }
  });

  it('keys the two subjects that name nobody in particular on their kind alone', () => {
    expect(subjectKey({ kind: 'anyone' })).toBe('anyone');
    expect(parseSubjectKey('authenticated')).toEqual({ kind: 'authenticated' });
  });

  it('resolves ids to names and falls back to the raw id', () => {
    expect(subjectLabel({ kind: 'principal', id: 'p1' }, principals, teams)).toBe(
      'Gast Konto (gast)'
    );
    expect(subjectLabel({ kind: 'team', id: 'redaktion' }, principals, teams)).toBe(
      'Redaktion (redaktion)'
    );
    // Not an empty string: an unresolvable id means the principal is gone, which is
    // exactly the row somebody is looking for.
    expect(subjectLabel({ kind: 'principal', id: 'weg' }, principals, teams)).toBe('weg');
  });
});

describe('formatInstant', () => {
  it('formats both the SQLite and the ISO spelling of the same instant', () => {
    expect(formatInstant('2026-08-10 09:12:03')).toBe('10.08.2026, 09:12');
    expect(formatInstant('2026-08-10T09:12:03Z')).toBe('10.08.2026, 09:12');
  });

  it('returns anything it does not understand unchanged', () => {
    // Better a raw timestamp than "Invalid Date" in an audit log.
    expect(formatInstant('irgendwann')).toBe('irgendwann');
  });
});

describe('mutations', () => {
  it('treats a 200 that changed nothing as a failure', async () => {
    // The single most important line in this module. The API reports when an operation
    // was a no-op — an inherited grant that cannot be revoked here, a mistyped slug —
    // and swallowing that would leave somebody believing they revoked access.
    stubFetch(new Response(JSON.stringify({ changed: false }), { status: 200 }));
    const result = await removeGrant('/handbuch', { kind: 'team', id: 'redaktion' }, 'read');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toContain('nichts geändert');
  });

  it('passes the explanation from the server through when it gives one', async () => {
    stubFetch(
      new Response(JSON.stringify({ changed: false, message: 'Dieser Eintrag wird vererbt.' }), {
        status: 200
      })
    );
    const result = await removeGrant('/handbuch', { kind: 'team', id: 'redaktion' }, 'read');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toContain('Dieser Eintrag wird vererbt.');
  });

  it('accepts an empty body, which is a legitimate answer to a DELETE', async () => {
    stubFetch(new Response(null, { status: 204 }));
    expect((await removeGrant('/x', { kind: 'anyone' }, 'read')).ok).toBe(true);
  });

  it('turns a thrown request into a message rather than an exception', async () => {
    stubFetch(new TypeError('Failed to fetch'));
    const result = await setPrincipalActive('p1', 'Gast Konto', false);
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toContain('nicht erreichbar');
  });

  it('names what failed, and in which direction, when deactivating', async () => {
    stubFetch(new Response('', { status: 403 }));
    const result = await setPrincipalActive('p1', 'Gast Konto', false);
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain('»Gast Konto« konnte nicht deaktiviert werden');
      expect(result.message).toContain('403');
    }
  });
});
