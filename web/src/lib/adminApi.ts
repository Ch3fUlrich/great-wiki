/**
 * The client half of the administration API.
 *
 * This module is imported from the BROWSER as well as from `+page.server.ts`, so it must
 * stay free of `$env/dynamic/private` — importing `apiGet` from `$lib/api` here would
 * poison every component that touches these types. The server load calls `apiGet`
 * itself and borrows only the types and the German failure wording from this file.
 *
 * Two rules shape everything below.
 *
 * **A failure is a value, never an exception and never a silent nothing.** Every call
 * returns either `{ ok: true, value }` or `{ ok: false, message }` with a German sentence
 * a person can act on. The console has to render something honest when the API is not
 * there at all — this milestone ships the two halves separately, so "endpoint missing"
 * is a state the interface must survive rather than a bug.
 *
 * **A mutation that changed nothing is a failure.** The API reports it (a mistyped team
 * slug, a grant that is inherited rather than defined here), and swallowing that report
 * would leave somebody believing they revoked access they did not revoke.
 */

/** Who a grant is about. Mirrors `gw_auth::Subject`, which serialises tagged. */
export type SubjectKind = 'principal' | 'team' | 'group' | 'anyone' | 'authenticated';

export interface Subject {
  kind: SubjectKind;
  /** Absent for `anyone` and `authenticated`, which name no particular subject. */
  id?: string | null;
}

/** What a grant confers. Mirrors `gw_auth::Permission`. */
export type Permission = 'read' | 'comment' | 'write' | 'admin';

export interface Grant {
  subject: Subject;
  permission: Permission;
}

export interface AdminPrincipal {
  id: string;
  /** `oidc` is a homelab account; `local` is a great-wiki guest account. */
  kind: 'oidc' | 'local';
  username: string;
  display_name: string;
  email: string | null;
  groups: string[];
  teams: string[];
  active: boolean;
}

/**
 * `TeamSummary` on the Rust side.
 *
 * `members` is a list of principal **ids**, not people — the API resolves them nowhere,
 * deliberately, so listing teams stays one query however many there are. Turning them
 * into names is this interface's job, against the principals it already has.
 */
export interface Team {
  slug: string;
  name: string;
  members: string[];
}

/**
 * `GET /api/admin/acl?path=…`.
 *
 * `effective` is what actually applies at this path; `defined_here` is the subset stored
 * against this exact path. When they differ, the grants came from `inherited_from` — and
 * the distinction is the whole point of this screen, because an inherited grant cannot be
 * revoked here.
 */
export interface AclView {
  path: string;
  effective: Grant[];
  inherited_from: string | null;
  defined_here: Grant[];
  /**
   * The nearest ancestor ABOVE this path that carries entries, and what they are.
   *
   * What would apply here if every entry on this path were removed — which is exactly
   * what revoking the last one does, here and on every page below that carries nothing of
   * its own. `inherited_from` cannot answer it: a path is its own first ancestor, so it
   * names this path as soon as this path holds a single row.
   */
  ancestor_source: string | null;
  ancestor_grants: Grant[];
}

/**
 * One row of the group-to-baseline mapping (`GET /api/admin/roles`).
 *
 * Default reach, before any entry is consulted. A group mapped to `admin` reads every
 * `restricted` page in the wiki with no entry written anywhere — which is the one thing a
 * table of entries can never show, and the reason the access panel loads this.
 */
export interface GroupRole {
  group: string;
  baseline: 'public' | 'internal' | 'admin';
}

export interface AuditEntry {
  id: string;
  /** SQLite `datetime('now')` — UTC, either `YYYY-MM-DD HH:MM:SS` or ISO-8601. */
  at: string;
  /** `null` for an action taken by nobody in particular — a migration, or the system. */
  principal_id: string | null;
  /** A dotted verb: `acl.grant`, `principal.deactivate`, `team.member.add`. */
  action: string;
  /** What the action was applied to, in whatever terms the action uses. */
  target: string | null;
  /** The subtree the action concerns. `null` means instance-wide. */
  path: string | null;
  detail?: string;
}

export interface AuditPage {
  entries: AuditEntry[];
  /** True when older entries exist beyond the requested limit. */
  truncated: boolean;
}

/** What a server load hands a panel: data, or the reason there is none. Never both null. */
export interface Loaded<T> {
  data: T | null;
  error: string | null;
}

export type Outcome<T> = { ok: true; value: T } | { ok: false; message: string };

/**
 * One German sentence per failure mode, appended to a caller-supplied clause.
 *
 * `status === 0` means the request never got an answer — offline, DNS, a dead proxy.
 * It is separated from 5xx deliberately: "not reachable" and "answered with 500" send
 * somebody to different places.
 */
export function describeStatus(
  status: number,
  clause: string,
  /**
   * A sentence to use instead of the generic one for a particular status.
   *
   * Exists for exactly one reason: the generic sentences name an HTTP status and no
   * remedy, which is right when the console cannot know what the status meant, and wrong
   * when it can. `POST /api/admin/invites` has one 409 and one only — the username is
   * taken — and "Der Server meldet einen Konflikt (409)" tells the person nothing they
   * can act on. An override is a caller saying "I know what this status means here".
   */
  overrides?: Partial<Record<number, string>>
): string {
  const override = overrides?.[status];
  if (override) return `${clause}: ${override}`;
  if (status === 0) return `${clause}: Die Verwaltungs-API ist nicht erreichbar.`;
  if (status === 401) return `${clause}: Nicht angemeldet (401). Bitte erneut anmelden.`;
  if (status === 403) return `${clause}: Dafür fehlen die Rechte (403).`;
  if (status === 404) {
    return (
      `${clause}: Dieser Endpunkt existiert nicht (404). ` +
      `Die Verwaltungs-API ist in dieser Installation noch nicht vorhanden.`
    );
  }
  if (status === 409) return `${clause}: Der Server meldet einen Konflikt (409).`;
  if (status === 400 || status === 422) {
    return `${clause}: Die Eingabe wurde abgelehnt (${status}).`;
  }
  if (status >= 500) return `${clause}: Der Server hat mit ${status} geantwortet.`;
  return `${clause}: Die Anfrage ist mit ${status} fehlgeschlagen.`;
}

/** The shape a mutating endpoint may use to report that it changed nothing. */
interface MutationReport {
  changed?: boolean;
  message?: string | null;
}

async function request<T>(
  method: string,
  path: string,
  clause: string,
  body?: unknown,
  overrides?: Partial<Record<number, string>>
): Promise<Outcome<T>> {
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  } catch {
    return { ok: false, message: describeStatus(0, clause, overrides) };
  }

  if (!res.ok) return { ok: false, message: describeStatus(res.status, clause, overrides) };

  // 204 and an empty body are both legitimate for a DELETE.
  const text = await res.text();
  if (!text) return { ok: true, value: undefined as T };

  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    return { ok: false, message: `${clause}: Die Antwort war kein JSON.` };
  }

  // A 200 that reports `changed: false` is a failure, not a success. This is the one
  // place the interface could quietly lie, so it is checked before anything else.
  const report = parsed as MutationReport | null;
  if (report && typeof report === 'object' && report.changed === false) {
    return {
      ok: false,
      message: report.message
        ? `${clause}: ${report.message}`
        : `${clause}: Der Server hat nichts geändert.`
    };
  }

  return { ok: true, value: parsed as T };
}

// --- Reads ----------------------------------------------------------------
// The console loads through `+page.server.ts` so the first paint is real, but these
// exist for anything that has to refresh without a navigation.

export function listPrincipals(): Promise<Outcome<AdminPrincipal[]>> {
  return request('GET', '/api/admin/principals', 'Die Personenliste konnte nicht geladen werden');
}

export function listTeams(): Promise<Outcome<Team[]>> {
  return request('GET', '/api/admin/teams', 'Die Teamliste konnte nicht geladen werden');
}

export function listRoles(): Promise<Outcome<GroupRole[]>> {
  return request(
    'GET',
    '/api/admin/roles',
    'Die Zuordnung von Gruppen zu Reichweiten konnte nicht geladen werden'
  );
}

export function getAcl(path: string): Promise<Outcome<AclView>> {
  return request(
    'GET',
    `/api/admin/acl?path=${encodeURIComponent(path)}`,
    `Die Zugriffsrechte für ${path} konnten nicht geladen werden`
  );
}

/**
 * The largest page the API will serve. It CLAMPS anything above this rather than
 * refusing it, so asking for more and captioning the table with the number asked for
 * would put a figure on screen that is not the number of rows below it.
 */
export const MAX_AUDIT_LIMIT = 500;

export function listAudit(limit: number): Promise<Outcome<AuditPage>> {
  return request('GET', `/api/admin/audit?limit=${limit}`, 'Das Protokoll konnte nicht geladen werden');
}

// --- Writes ---------------------------------------------------------------

/**
 * One outstanding or finished invitation, as `GET /api/admin/invites` lists it.
 *
 * Mirrors `gw_store::InviteSummary`, and — like it — has **no token field and no digest
 * field**, deliberately. The plaintext is never stored; the digest in a listing would be
 * as good as the token to anybody who could also write to the database. The link exists
 * exactly once, in the answer to the POST that created it, and nowhere else ever again.
 */
export interface Invite {
  id: string;
  username: string;
  email: string | null;
  invited_by: string | null;
  /** Resolved now, and `null` when the inviter's account is gone — an invite outlives them. */
  invited_by_name: string | null;
  path: string | null;
  permission: Permission | null;
  /** A team slug, which is what the rest of this API speaks. */
  team: string | null;
  created_at: string;
  expires_at: string;
  state: InviteState;
  accepted_principal_id: string | null;
}

export type InviteState = 'pending' | 'accepted' | 'revoked' | 'expired';

/**
 * How the link reached the person. Mirrors `gw_api::routes::admin::Delivery`, a tagged
 * enum with one variant — the seam where `emailed` will appear once anything in this
 * stack can send mail.
 */
export interface Delivery {
  how: 'shown-once';
  /**
   * **Origin-relative.** The API refuses to build an absolute URL, because the only origin
   * it could derive one from is the `Host` header and any client can set that — a link
   * built from it could be made to point at somebody else's server and collect the token.
   * The console is already on the right origin, so it prepends its own.
   */
  url: string;
}

/** What the POST answers: the invitation, and the one and only copy of its link. */
export interface CreatedInvite extends Invite {
  delivery: Delivery;
}

/**
 * What an invitation is asked to carry.
 *
 * `path` + `permission` travel together or not at all, and a `team` puts the whole request
 * behind instance administration rather than path administration — both rules are the
 * API's (D-M2-2) and the panel mirrors them in what it offers, so that the interface never
 * asks for something that is going to be refused.
 */
export interface NewInvite {
  username: string;
  email?: string;
  path?: string;
  permission?: Permission;
  team?: string;
}

export function listInvites(): Promise<Outcome<Invite[]>> {
  return request('GET', '/api/admin/invites', 'Die Einladungen konnten nicht geladen werden');
}

/**
 * Create an invitation and hand back its link.
 *
 * The 409 is overridden because this endpoint has exactly one, it is not a race and it is
 * not a server problem: somebody already holds that username. "Der Server meldet einen
 * Konflikt (409)" would send a person to the logs for something they fix by typing a
 * different word.
 */
export function createInvite(input: NewInvite): Promise<Outcome<CreatedInvite>> {
  return request(
    'POST',
    '/api/admin/invites',
    `Die Einladung für »${input.username}« konnte nicht erstellt werden`,
    {
      username: input.username,
      email: input.email?.trim() ? input.email.trim() : undefined,
      path: input.path || undefined,
      permission: input.path ? input.permission : undefined,
      team: input.team || undefined
    },
    {
      409:
        'Diesen Benutzernamen gibt es hier schon. Wählen Sie einen anderen, oder vergeben Sie ' +
        'den Zugriff unter »Zugriff« an das vorhandene Konto.'
    }
  );
}

/**
 * Withdraw an invitation that has not been redeemed.
 *
 * A 404 covers four things at once — no such invite, already spent, already withdrawn, or
 * not this caller's to see — and the API answers it that way on purpose, so the console
 * must not translate it into "gone" and leave somebody believing a live link is closed.
 * An already-accepted invitation needs the ACCOUNT deactivating, which is a different tab,
 * and the sentence says so.
 */
export function revokeInvite(id: string, username: string): Promise<Outcome<unknown>> {
  return request(
    'DELETE',
    `/api/admin/invites/${encodeURIComponent(id)}`,
    `Die Einladung für »${username}« konnte nicht zurückgezogen werden`,
    undefined,
    {
      404:
        'Sie ist nicht mehr offen — sie wurde bereits benutzt, bereits zurückgezogen, oder sie ' +
        'ist abgelaufen. Ein bereits benutzter Link wird nicht zurückgezogen, sondern das ' +
        'entstandene Konto unter »Personen« deaktiviert.'
    }
  );
}

export interface NewPrincipal {
  username: string;
  display_name: string;
  email?: string;
  password: string;
}

export function createPrincipal(input: NewPrincipal): Promise<Outcome<AdminPrincipal>> {
  return request('POST', '/api/admin/principals', `»${input.username}« konnte nicht angelegt werden`, {
    username: input.username,
    display_name: input.display_name,
    // An empty field is no address at all; sending "" would store an empty string.
    email: input.email?.trim() ? input.email.trim() : undefined,
    password: input.password
  });
}

export function setPrincipalActive(
  id: string,
  name: string,
  active: boolean
): Promise<Outcome<unknown>> {
  const clause = active ? `»${name}« konnte nicht aktiviert werden` : `»${name}« konnte nicht deaktiviert werden`;
  return request('POST', `/api/admin/principals/${encodeURIComponent(id)}/active`, clause, { active });
}

export function createTeam(slug: string, name: string): Promise<Outcome<Team>> {
  return request('POST', '/api/admin/teams', `Das Team »${slug}« konnte nicht angelegt werden`, {
    slug,
    name
  });
}

export function addTeamMember(slug: string, principalId: string): Promise<Outcome<unknown>> {
  return request(
    'POST',
    `/api/admin/teams/${encodeURIComponent(slug)}/members`,
    `Das Mitglied konnte dem Team »${slug}« nicht hinzugefügt werden`,
    { principal_id: principalId }
  );
}

export function removeTeamMember(slug: string, principalId: string): Promise<Outcome<unknown>> {
  return request(
    'DELETE',
    `/api/admin/teams/${encodeURIComponent(slug)}/members/${encodeURIComponent(principalId)}`,
    `Das Mitglied konnte aus dem Team »${slug}« nicht entfernt werden`
  );
}

export function addGrant(
  path: string,
  subject: Subject,
  permission: Permission
): Promise<Outcome<unknown>> {
  return request('POST', '/api/admin/acl', `Der Zugriff auf ${path} konnte nicht gewährt werden`, {
    path,
    subject,
    permission
  });
}

export function removeGrant(
  path: string,
  subject: Subject,
  permission: Permission
): Promise<Outcome<unknown>> {
  return request('DELETE', '/api/admin/acl', `Der Zugriff auf ${path} konnte nicht entzogen werden`, {
    path,
    subject,
    permission
  });
}

/**
 * Change how open one page is.
 *
 * The only thing in the system that writes `documents.visibility`. `seed --update`
 * deliberately refuses to — a stray `visibility: public` in a bulk file drop would
 * publish a page with nobody watching — so this is the one deliberate, audited act, made
 * by a person on one path.
 *
 * A `changed: false` answer means the page already had that value, and `request` reports
 * it as a failure. That is right: the control is disabled for the current value, so the
 * only way to see it is that somebody else got there first, which is worth knowing.
 */
export function setVisibility(path: string, visibility: string): Promise<Outcome<unknown>> {
  return request(
    'POST',
    '/api/admin/visibility',
    `Die Sichtbarkeit von ${path} konnte nicht geändert werden`,
    { path, visibility }
  );
}

// --- Vocabulary -----------------------------------------------------------
// German for everything the API says in English. Kept beside the types so a new
// `Permission` variant fails to compile here rather than rendering as a raw keyword.

export const PERMISSION_LABEL: Record<Permission, string> = {
  read: 'Lesen',
  comment: 'Kommentieren',
  write: 'Schreiben',
  admin: 'Verwalten'
};

/**
 * German for every action the audit log records.
 *
 * The API writes dotted English verbs — `acl.grant`, `invite.create` — and the Protokoll is
 * read by the person who runs the wiki, in German, beside entries that were already
 * translated. Eleven of the twenty the backend emits had no entry here and were rendered
 * verbatim, so half the log read as machine output: »Zugriff gewährt« on one line and
 * `attachment.attach` on the next.
 *
 * The panel still falls back to the raw name for anything unrecognised, deliberately — an
 * action type nobody has translated yet must stay visible in the one screen that exists to
 * make actions visible, rather than becoming "Unbekannt". That fallback is for a verb added
 * tomorrow, not a substitute for translating the ones that exist.
 *
 * `Record<string, string>` and not a closed union: the key set lives in Rust, and a type
 * that claimed to enumerate it here would be a claim this file cannot check.
 */
export const AUDIT_ACTION_LABEL: Record<string, string> = {
  'acl.grant': 'Zugriff gewährt',
  'acl.revoke': 'Zugriff entzogen',
  'attachment.attach': 'Datei angehängt',
  'attachment.detach': 'Datei entfernt',
  // Not a person's action: the sweep that frees blobs no attachment points at any more.
  'blobs.reclaim': 'Speicher freigegeben',
  'document.purge': 'Seite endgültig gelöscht',
  'document.restore': 'Seite wiederhergestellt',
  'document.trash': 'Seite in den Papierkorb gelegt',
  'document.visibility': 'Sichtbarkeit geändert',
  // Three different facts about one link, and the log is where somebody checks which.
  'invite.accept': 'Einladung angenommen',
  'invite.create': 'Einladung erstellt',
  'invite.revoke': 'Einladung zurückgezogen',
  'principal.activate': 'Konto aktiviert',
  'principal.create': 'Konto angelegt',
  'principal.deactivate': 'Konto deaktiviert',
  // The per-account instance-admin promotion of ADR 0006 — the fallback for an instance
  // whose Authelia administrators are gone. »Verwaltung« is the word the access panel uses
  // for the same reach, so the two screens agree.
  'principal.demote': 'Verwaltungsrecht entzogen',
  'principal.promote': 'Verwaltungsrecht erteilt',
  'team.create': 'Team angelegt',
  'team.member.add': 'Mitglied hinzugefügt',
  'team.member.remove': 'Mitglied entfernt',
  // Not emitted by anything today; kept because the sign-in path is the obvious next thing
  // to audit and the words should not have to be invented under pressure.
  'session.start': 'Anmeldung',
  'session.end': 'Abmeldung'
};

export const INVITE_STATE_LABEL: Record<InviteState, string> = {
  pending: 'Offen',
  accepted: 'Angenommen',
  revoked: 'Zurückgezogen',
  expired: 'Abgelaufen'
};

export const SUBJECT_KIND_LABEL: Record<SubjectKind, string> = {
  principal: 'Person',
  team: 'Team',
  group: 'Gruppe',
  anyone: 'Alle',
  authenticated: 'Angemeldete'
};

export const VISIBILITY_LABEL: Record<string, string> = {
  public: 'Öffentlich',
  internal: 'Intern',
  restricted: 'Eingeschränkt'
};

/** Where a principal came from. great-wiki never writes Authelia's user database. */
export const SOURCE_LABEL: Record<AdminPrincipal['kind'], string> = {
  oidc: 'Authelia',
  local: 'Lokal'
};

/**
 * A stable string for one subject, used as a list key and for equality.
 *
 * `anyone` and `authenticated` carry no id, so they key on their kind alone — two
 * `Anyone` grants at the same permission are the same grant.
 */
export function subjectKey(subject: Subject): string {
  return subject.id ? `${subject.kind}:${subject.id}` : subject.kind;
}

export function parseSubjectKey(key: string): Subject {
  const at = key.indexOf(':');
  if (at === -1) return { kind: key as SubjectKind };
  return { kind: key.slice(0, at) as SubjectKind, id: key.slice(at + 1) };
}

/**
 * What to call a subject on screen.
 *
 * The API stores ids; a person reading the table needs names. Resolution is
 * best-effort and falls back to the raw id rather than to an empty cell — an
 * unresolvable id is information ("this principal no longer exists"), and blanking it
 * would hide exactly the row somebody is looking for.
 */
export function subjectLabel(
  subject: Subject,
  principals: AdminPrincipal[],
  teams: Team[]
): string {
  if (subject.kind === 'anyone') return 'Alle, auch nicht angemeldete';
  if (subject.kind === 'authenticated') return 'Alle angemeldeten Personen';
  const id = subject.id ?? '';
  if (subject.kind === 'principal') {
    const found = principals.find((p) => p.id === id || p.username === id);
    return found ? `${found.display_name} (${found.username})` : id;
  }
  if (subject.kind === 'team') {
    const found = teams.find((t) => t.slug === id);
    return found ? `${found.name} (${found.slug})` : id;
  }
  return id;
}

/**
 * `2026-08-10 09:12:03` → `10.08.2026, 09:12`.
 *
 * Formatted by hand rather than through `Intl`, on purpose. This string is rendered on
 * the server and then hydrated in the browser; `Intl` would consult two different time
 * zones and two different ICU builds and produce a hydration mismatch on a value nobody
 * would think to suspect. The timestamps are UTC, and the column says so.
 */
export function formatInstant(at: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})/.exec(at);
  if (!m) return at;
  const [, year, month, day, hour, minute] = m;
  return `${day}.${month}.${year}, ${hour}:${minute}`;
}
