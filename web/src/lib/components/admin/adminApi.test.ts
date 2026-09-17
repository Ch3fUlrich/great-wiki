import { describe, expect, it, vi, afterEach } from 'vitest';
import {
  createInvite,
  describeStatus,
  formatInstant,
  parseSubjectKey,
  AUDIT_ACTION_LABEL,
  INVITE_EMAIL_HELP,
  INVITE_STATE_LABEL,
  isMatchableEmail,
  removeGrant,
  revokeInvite,
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
    oidc_username: null,
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

// ---------------------------------------------------------------------------------------
//  Invitations.
//
//  The invite API existed before the console did: `POST /api/admin/invites`,
//  `GET /api/admin/invites` and `DELETE /api/admin/invites/{id}` shipped with 42 Rust
//  tests and no caller at all, which is how the flow went a milestone without anybody
//  walking it. These are the client half.
// ---------------------------------------------------------------------------------------

describe('invitations', () => {
  it('hands back the one and only copy of the link', async () => {
    stubFetch(
      new Response(
        JSON.stringify({
          id: 'i1',
          username: 'oma',
          email: null,
          invited_by: 'p1',
          invited_by_name: 'Sergej Maul',
          path: '/familie',
          permission: 'write',
          team: null,
          created_at: '2026-09-17 10:00:00',
          expires_at: '2026-10-17 10:00:00',
          state: 'pending',
          accepted_principal_id: null,
          delivery: { how: 'shown-once', url: '/auth/invite/tok' }
        }),
        { status: 201, headers: { 'content-type': 'application/json' } }
      )
    );
    const result = await createInvite({
      username: 'oma',
      email: 'oma@example.de',
      path: '/familie',
      permission: 'write'
    });
    expect(result.ok).toBe(true);
    if (result.ok) {
      expect(result.value.delivery.url).toBe('/auth/invite/tok');
      expect(result.value.username).toBe('oma');
    }
  });

  it('says in plain German what a 409 means rather than "Konflikt (409)"', async () => {
    // The generic 409 sentence names an HTTP status and no remedy. This endpoint has two
    // conflicts — the username, and (since ADR 0021) the e-mail address — and they share
    // one remedy: give the account that already exists access, rather than inviting the
    // same person a second time.
    stubFetch(new Response('', { status: 409 }));
    const result = await createInvite({
      username: 'oma',
      email: 'oma@example.de',
      path: '/familie',
      permission: 'read'
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain('Benutzernamen');
      expect(result.message).toContain('E-Mail-Adresse');
      expect(result.message).toContain('Zugriff');
      expect(result.message).not.toContain('Konflikt');
    }
  });

  it('explains a refused address rather than saying "Die Eingabe wurde abgelehnt (400)"', async () => {
    // The address is the merge key, not a note, so "abgelehnt" with no reason leaves
    // somebody guessing at a field the whole decision rests on.
    stubFetch(new Response('', { status: 400 }));
    const result = await createInvite({
      username: 'oma',
      email: 'oma',
      path: '/familie',
      permission: 'read'
    });
    expect(result.ok).toBe(false);
    if (!result.ok) {
      expect(result.message).toContain('name@beispiel.de');
      expect(result.message).not.toContain('(400)');
    }
  });

  it('sends the address the owner typed, trimmed, and never omits it', async () => {
    // It is required: an invitation with no address can only ever become a SECOND account
    // for one person, and nothing repairs that afterwards.
    let sent: Record<string, unknown> = {};
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init: RequestInit) => {
        sent = JSON.parse(init.body as string);
        return new Response('{}', { status: 201, headers: { 'content-type': 'application/json' } });
      })
    );
    await createInvite({
      username: 'oma',
      email: '  Oma@Example.de  ',
      path: '/familie',
      permission: 'read'
    });
    expect(sent.email).toBe('Oma@Example.de');
  });

  it('reports a withdrawal that withdrew nothing as a failure', async () => {
    // 404 is what the API answers for an invite that is already spent, already revoked,
    // or not the caller's to see. An administrator must not be told the link is closed.
    stubFetch(new Response('', { status: 404 }));
    const result = await revokeInvite('i1', 'oma');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toContain('»oma«');
  });

  it('turns an unreachable API into a sentence, not an exception', async () => {
    stubFetch(new TypeError('Failed to fetch'));
    const result = await revokeInvite('i1', 'oma');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.message).toContain('nicht erreichbar');
  });

  it('gives every invite state a German word', () => {
    expect(INVITE_STATE_LABEL.pending).toBe('Offen');
    expect(INVITE_STATE_LABEL.accepted).toBe('Angenommen');
    expect(INVITE_STATE_LABEL.revoked).toBe('Zurückgezogen');
    expect(INVITE_STATE_LABEL.expired).toBe('Abgelaufen');
  });
});

describe('the audit log vocabulary', () => {
  /**
   * Every action name `gw-store` and `gw-api` actually write, as of this walkthrough.
   *
   * Listed here rather than derived, because there is no way for a TypeScript test to read
   * the Rust source — so this is the seam, and it is a deliberate one: a new audited action
   * has to be added in two places, and this test is what says so out loud when only one of
   * them was done. Regenerate with:
   *
   *   grep -rhoE '"(acl|document|team|principal|invite|attachment|blobs|identity)\.[a-z._]+"' crates/ | sort -u
   */
  const EMITTED = [
    'acl.grant',
    'acl.revoke',
    'attachment.attach',
    'attachment.detach',
    'blobs.reclaim',
    'document.purge',
    'document.restore',
    'document.trash',
    'document.visibility',
    'identity.merge',
    'invite.accept',
    'invite.create',
    'invite.revoke',
    'principal.activate',
    'principal.create',
    'principal.deactivate',
    'principal.demote',
    'principal.promote',
    'team.create',
    'team.member.add',
    'team.member.remove'
  ];

  it('has a German word for every action the backend records', () => {
    // Eleven of these had none, so the German console showed `invite.create`,
    // `attachment.attach` and `document.trash` verbatim beside »Zugriff gewährt« — found
    // by reading the Protokoll after walking an invitation through it. The panel falls
    // back to the raw name on purpose (a new action must not become invisible), and that
    // fallback had quietly become the normal case for half the log.
    const missing = EMITTED.filter((action) => !(action in AUDIT_ACTION_LABEL));
    expect(missing).toEqual([]);
  });

  it('says what happened, not which endpoint was called', () => {
    // A person reading the Protokoll is asking "what was done to my wiki", not "which
    // handler ran". No label may be the dotted verb with a space in it.
    for (const [action, label] of Object.entries(AUDIT_ACTION_LABEL)) {
      expect(label).not.toContain('.');
      expect(label.toLowerCase()).not.toBe(action.replace('.', ' '));
      expect(label).toMatch(/[a-zäöüß]/);
    }
  });

  it('names the three halves of an invitation apart from one another', () => {
    // Created, accepted and withdrawn are three different facts about one link, and the
    // log is where somebody checks which of them happened.
    const words = [
      AUDIT_ACTION_LABEL['invite.create'],
      AUDIT_ACTION_LABEL['invite.accept'],
      AUDIT_ACTION_LABEL['invite.revoke']
    ];
    expect(new Set(words).size).toBe(3);
    for (const word of words) expect(word).toMatch(/Einladung/);
  });
});

describe('the address an invitation is matched on (ADR 0021)', () => {
  it('accepts an ordinary address whatever its case or surrounding space', () => {
    for (const good of ['oma@example.de', 'Oma@Example.DE', '  oma@example.de  ']) {
      expect(isMatchableEmail(good)).toBe(true);
    }
  });

  it('refuses everything the merge cannot key on, rather than guessing', () => {
    // The same list `gw_store::canonical_email` refuses. Two of these matter especially:
    // an internationalised domain is a DIFFERENT registration from the one it resembles,
    // and a Cyrillic `о` renders identically to a Latin one — folding either would hand
    // one person's grants to whoever can register the other.
    for (const bad of ['', '   ', 'oma', 'oma@', '@example.de', 'a@b@c.de', 'oma @example.de',
                       'oma@exämple.de', 'оma@example.de']) {
      expect(isMatchableEmail(bad), bad).toBe(false);
    }
  });

  it('tells the owner what the field is for, not merely that it is required', () => {
    // A required field with no reason on screen is a field people fill in with anything,
    // and what is at stake is whether one person ends up with two accounts.
    expect(INVITE_EMAIL_HELP).toContain('Authelia');
    expect(INVITE_EMAIL_HELP).toContain('dasselbe Konto');
    expect(INVITE_EMAIL_HELP).toContain('nachträglich nicht mehr zusammenlegen');
    // And it still says the wiki sends no mail, which was the old field's only job.
    expect(INVITE_EMAIL_HELP).toContain('keine Post');
  });
});
