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
export function describeStatus(status: number, clause: string): string {
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
  body?: unknown
): Promise<Outcome<T>> {
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers: body === undefined ? {} : { 'content-type': 'application/json' },
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  } catch {
    return { ok: false, message: describeStatus(0, clause) };
  }

  if (!res.ok) return { ok: false, message: describeStatus(res.status, clause) };

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
