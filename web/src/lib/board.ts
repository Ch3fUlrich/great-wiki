/**
 * The board, on the wire and in words.
 *
 * Pure on purpose, exactly like `$lib/projects`: this module is imported from two
 * `+page.server.ts` files **and** from the component that renders both placements, so it may
 * not touch `$env/dynamic/private` — importing a server-only module from a component poisons
 * the client bundle. The calls themselves live in `$lib/api`.
 *
 * **Why so much of the board is a function in here rather than markup in the component.**
 * D-12 put the board in two places — `/aufgaben`, and embedded in a project's home page —
 * and named the cost plainly: two things that must agree. They are one component fed by one
 * endpoint for that reason, and every rule that could still drift between them is a function
 * here with a test beside it: which three columns exist, whether a due date is overdue,
 * whether a card may be moved, and where a move comes back to.
 *
 * **Nothing here decides who may see or do anything.** `GET /api/board` answers only the
 * cards whose governing page the caller may read, per document (D-3), and that filtering
 * belongs to `Store::board_for`, where it is mutation-tested. What this file owns is wording
 * and two guards that keep a query string from deciding something it may not: which project
 * is being filtered on, and where a form submission is allowed to send somebody afterwards.
 */
import type { Me } from '$lib/api';
import type { Project } from '$lib/projects';

// --- What comes off the wire ---------------------------------------------------------------

/**
 * D-9's three, spelled as `gw_store::TaskStatus` spells them.
 *
 * Composed `ä` (U+00E4) and not `a` + U+0308. The store's CHECK constraint compares text
 * byte for byte, so the two spellings are two different statuses as far as SQLite is
 * concerned, and `0010_tasks.sql` says so in its own comment. Anything sent from here is one
 * of these three constants, never a string somebody typed.
 */
export type TaskStatus = 'Offen' | 'Läuft' | 'Fertig';

/** Every status there is, in board order. D-9 made the set fixed; this is that set. */
export const STATUSES: readonly TaskStatus[] = ['Offen', 'Läuft', 'Fertig'];

/**
 * The column headings. The same words as the values, deliberately: the status a card holds
 * and the column it sits in are one thing, and a separate label would be a second name for
 * it that could drift.
 */
export const STATUS_LABEL: Record<TaskStatus, string> = {
  Offen: 'Offen',
  Läuft: 'Läuft',
  Fertig: 'Fertig'
};

/**
 * A status read off something that is not this program — a query string, a form field.
 *
 * The decomposed spelling is refused rather than normalised. Everything that reaches this
 * comes from this application's own buttons, which emit the composed form; accepting a
 * second spelling here would widen what can be sent to the API without widening what the
 * three columns are, which is precisely the drift D-9's fixed set exists to prevent.
 */
export function isStatus(value: string | null | undefined): value is TaskStatus {
  return typeof value === 'string' && (STATUSES as readonly string[]).includes(value);
}

/** The page a card's line was written on. Mirrors `gw_api::routes::tasks::PageView`. */
export interface TaskPage {
  path: string;
  title: string;
}

/**
 * One card. Mirrors `gw_api::routes::tasks::TaskView`.
 *
 * `page` is `null` only for a card created on a board rather than written as a line in a
 * page — never for an anchored card the caller is being shown, because a card is on this
 * board only because the store resolved its page through the permission-checked accessor.
 */
export interface BoardTask {
  id: string;
  title: string;
  status: TaskStatus;
  /** The principal this card rests on, **by id**. The API resolves no display name. */
  assignee: string | null;
  due_at: string | null;
  position: number;
  anchored: boolean;
  page: TaskPage | null;
  /** D-8: the page no longer mentions the line that authored this card. */
  detached: boolean;
  created_at: string;
  updated_at: string;
  /**
   * Whether the caller may move this card — **not on the committed wire.**
   *
   * There is no field on `/api/board` today that says "may I write the page that governs
   * this card", the same one bit `[...path]/+page.svelte` and `/projekte` both record as
   * missing. It is declared here, optional, so that the per-card read-only marking has one
   * place to become true the day the endpoint answers it, rather than a new one. Absent, the
   * offer is made to whoever is signed in and the true answer arrives when it is used.
   */
  movable?: boolean;
}

/** One of D-9's three columns, as the API answers it. */
export interface BoardColumn {
  status: TaskStatus;
  tasks: BoardTask[];
}

/**
 * A board. Mirrors `gw_api::routes::tasks::BoardResponse`, with one difference that the
 * global board forces: **`project` is nullable here.**
 *
 * `GET /api/projects/{id}/board` always belongs to exactly one project, so its `project` is
 * a value. `GET /api/board` with no filter is every card the caller may see, across every
 * project and the cards that belong to none — there is no single project for it to name.
 * Read as `null` when the key is absent, so that both answers land in one shape and one
 * component renders them.
 *
 * **There is deliberately no count, here or anywhere below.** A total, an `omitted`, a
 * "3 Karten" badge would each be a number about cards the caller was not shown, which is
 * the one thing the filtering cannot take back. `gw_api::routes::tasks` pins the same
 * absence structurally, on the keys.
 */
export interface BoardResponse {
  project: Project | null;
  columns: BoardColumn[];
}

// --- Which board ---------------------------------------------------------------------------

/** The one board endpoint. Named once so the two placements cannot drift onto two. */
export const BOARD_ENDPOINT = '/api/board';

/** Where the global board lives, and where a move comes back to when nothing else said. */
export const BOARD_PATH = '/aufgaben';

/**
 * The element a move's answer is announced in, and the fragment the redirect carries.
 *
 * A `role="status"` region that is already in the document when it loads is not reliably
 * announced by anything — a live region announces what CHANGES. The redirect after a move
 * therefore points at this id, the element carries `tabindex="-1"`, and the browser moves
 * focus to it: the sentence is then read out because it is where the reader now is. That
 * works with no JavaScript at all, which is the requirement.
 */
export const BOARD_NOTICE_ID = 'aufgaben-hinweis';

/**
 * The query parameters a board's own answer is carried in — `?verschoben=` for a card that
 * moved, `?fehler=` for one that did not. Named once because two things must agree about
 * them: the action that writes them and {@link returnTo}, which clears the stale ones.
 */
export const NOTICE_PARAMS = ['verschoben', 'fehler'] as const;

/** Which board is being asked for. D-12's three cases, and there are only three. */
export type Filter =
  | { kind: 'alle' }
  | { kind: 'projekt'; id: string }
  | { kind: 'seite'; path: string };

/**
 * The one request, with the filter bound.
 *
 * D-12 is explicit that the embedded board **is** the global board with a filter, not a
 * second endpoint: two retrieval paths are two answers, and because every card is a
 * disclosure surface, a second answer is also a second chance to leak one.
 */
export function boardPath(filter: Filter): string {
  if (filter.kind === 'alle') return BOARD_ENDPOINT;
  const query = new URLSearchParams(
    filter.kind === 'projekt' ? { projekt: filter.id } : { seite: filter.path }
  );
  return `${BOARD_ENDPOINT}?${query}`;
}

/**
 * The board's three columns, whatever the endpoint answered with.
 *
 * D-9 fixed the set, so a board has three columns even when a column holds nothing and even
 * when the answer omitted it. An empty column is a fact about the board; a missing one is a
 * hole in the interface, and a card whose status matched no rendered column would simply
 * disappear — which is the failure this whole design keeps trying to avoid.
 *
 * The order **inside** a column is left exactly as it arrived. `Store::board_for` already
 * answers in board order — column, then position, then id — and re-sorting here would be a
 * second opinion about an order that has one.
 */
export function columnsOf(board: BoardResponse): BoardColumn[] {
  return STATUSES.map((status) => ({
    status,
    tasks: board.columns.find((column) => column.status === status)?.tasks ?? []
  }));
}

// --- A due date ------------------------------------------------------------------------------

/** Overdue, due today, or due later. `null` when there is no due date to describe. */
export type DueState = 'überfällig' | 'heute' | 'offen';

/**
 * A stored due date as three instants: the deadline itself, and the day it falls on.
 *
 * `due_at` is a bare text column — `0010_tasks.sql` puts no format on it — and the store's
 * own tests write `2026-09-01`. A date with no time is a **whole day**, not midnight: read
 * as an instant, everything due today would be overdue from one second past midnight. So a
 * date-only deadline ends when its day does.
 *
 * Everything is UTC, like every other timestamp this interface renders. A zone would have to
 * come from somewhere, and the two places it could come from — the server's clock and the
 * reader's browser — are exactly the two that must not disagree between the first render and
 * hydration.
 */
function parseDue(due: string): { deadline: number; dayStart: number; dayEnd: number } | null {
  const m = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2}))?)?/.exec(due.trim());
  if (!m) return null;
  const [, year, month, day, hour, minute, second] = m;
  const dayStart = Date.UTC(+year, +month - 1, +day);
  const dayEnd = dayStart + 24 * 3_600_000 - 1;
  const deadline =
    hour === undefined ? dayEnd : Date.UTC(+year, +month - 1, +day, +hour, +minute, +(second ?? 0));
  return { deadline, dayStart, dayEnd };
}

/**
 * Where a card's due date stands against `now`.
 *
 * `now` is passed in rather than read from the clock, for the reason `relativeTime` in
 * `$lib/history` gives: the loader captures one instant and both the server render and the
 * hydrated one use it, so the same card cannot be overdue on the server and due today in the
 * browser — a hydration mismatch on a value nobody would think to suspect.
 */
export function dueState(due: string | null | undefined, now: number): DueState | null {
  if (!due) return null;
  const parsed = parseDue(due);
  if (!parsed) return null;
  if (now > parsed.deadline) return 'überfällig';
  if (now >= parsed.dayStart && now <= parsed.dayEnd) return 'heute';
  return 'offen';
}

/**
 * `2026-09-01` → `01.09.2026`, and `2026-09-01 14:30:00` → `01.09.2026, 14:30`.
 *
 * Formatted by hand rather than through `Intl`, the same decision `formatInstant` in
 * `$lib/adminApi` records: two ICU builds and two time zones between the server render and
 * hydration is a mismatch on a value nobody would think to suspect. A string that is not a
 * date comes back unchanged — nonsense a reader can see beats `NaN.NaN.NaN`.
 */
export function formatDue(due: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2}))?/.exec(due.trim());
  if (!m) return due;
  const [, year, month, day, hour, minute] = m;
  const date = `${day}.${month}.${year}`;
  return hour === undefined ? date : `${date}, ${hour}:${minute}`;
}

/**
 * The due date **in words**, overdue included.
 *
 * This is the requirement, not a flourish, and this codebase holds the line everywhere: the
 * diff views mark every change with a symbol and a word as well as a colour, and say why.
 * A card told apart from its neighbours by a red border alone is a card that is not told
 * apart at all when the page is printed, read aloud, or looked at by somebody who does not
 * see that red. The colour on the card is the redundant channel; this sentence is the first.
 */
export function describeDue(due: string | null | undefined, now: number): string {
  if (!due) return '';
  const state = dueState(due, now);
  const when = formatDue(due);
  if (state === 'überfällig') return `Überfällig seit ${when}`;
  if (state === 'heute') return `Fällig heute (${when})`;
  // Includes a due date this cannot read: saying "Fällig <whatever was stored>" keeps the
  // fact that there IS one, which dropping it would not.
  return `Fällig ${when}`;
}

// --- A card you may see but not move -----------------------------------------------------

/** Why a card is read-only. Two reasons, and they need different sentences. */
export type ReadOnlyReason = 'anmelden' | 'schreibrecht';

/**
 * Whether this card may be moved from here — and if not, which of the two reasons it is.
 *
 * **A read-only card is shown, marked, never hidden.** That is the owner's decision and it
 * is the right way round: the checkbox is already visible on the page the line was written
 * on, so hiding the card would not hide anything, and a task that silently vanishes from a
 * board is the failure this whole design exists to prevent.
 *
 * The answer is as crude as the wire allows, and the crudeness is recorded rather than
 * hidden. Moving a card needs Write on the page that governs it; nothing on `/api/board`
 * says whether the caller has it, `/api/me` reports groups and a baseline, and D-M2-8 is
 * explicit that no baseline confers write. So: somebody who is not signed in cannot write
 * anything here (nothing in this deployment grants write to `anyone`) and every card is
 * marked read-only for them; for anybody else the control is offered and the true answer
 * arrives the moment it is used, as a sentence rather than a silent nothing. `movable`,
 * should the endpoint ever send it, is believed over the offer.
 *
 * It is the same arrangement — and the same missing bit — as the edit link on a page and the
 * create form on `/projekte`. The offer can be false; the move cannot.
 */
export function readOnly(task: BoardTask, me: Me | null | undefined): ReadOnlyReason | null {
  if (task.movable === false) return 'schreibrecht';
  if (me?.authenticated !== true) return 'anmelden';
  return null;
}

/** Why this card cannot be moved, said to the person looking at it. */
export function readOnlyText(reason: ReadOnlyReason): string {
  return reason === 'anmelden'
    ? 'Nur lesbar — zum Verschieben bitte anmelden.'
    : 'Nur lesbar — hier fehlt das Schreibrecht.';
}

/**
 * What a detached card says about itself (D-8).
 *
 * In words, because the whole reason such a card is kept rather than deleted is the due date
 * and the assignee somebody set — deleting it would discard those silently. A marker that
 * only said "stale" would leave the reader to guess whether the card still counts. It does.
 */
export function detachedText(task: BoardTask): string {
  const where = task.page
    ? `»${task.page.title}« nennt diese Zeile nicht mehr`
    : 'die Zeile, aus der diese Karte kam, steht nirgends mehr';
  return `Abgelöst: ${where}. Fälligkeit und Zuständigkeit bleiben.`;
}

// --- Where a move comes back to ------------------------------------------------------------

/**
 * A path this application is allowed to send somebody to, or `null`.
 *
 * The value arrives in a form field, so it is whatever anybody put there — and a board is
 * exactly the kind of page somebody is sent a link to. Only a path within this site is
 * accepted: an absolute address, a protocol-relative `//host`, the `/\host` that some
 * browsers also treat as protocol-relative, and anything carrying a control character are
 * all refused outright rather than repaired.
 */
function safePath(value: string | null | undefined): string | null {
  if (!value) return null;
  const text = value.trim();
  if (!text.startsWith('/')) return null;
  if (text.startsWith('//') || text.startsWith('/\\')) return null;
  if (/[\u0000-\u001f\u007f]/.test(text)) return null;
  return text;
}

/**
 * Where a move returns to, carrying its announcement.
 *
 * Post, redirect, get — the pattern `/projekte` already ships, and here it does one more
 * job: the answer has to come back to **whichever** of D-12's two placements the card was
 * moved on. The form carries that path, this validates it, and the parameters are merged
 * into whatever was already in the address so that a project filter survives a move.
 *
 * The fragment is what makes the announcement work without JavaScript — see
 * {@link BOARD_NOTICE_ID}.
 */
export function returnTo(
  zurueck: string | null | undefined,
  params: Record<string, string>
): string {
  const target = safePath(zurueck) ?? BOARD_PATH;
  const withoutHash = target.split('#')[0];
  const cut = withoutHash.indexOf('?');
  const path = cut === -1 ? withoutHash : withoutHash.slice(0, cut);
  const query = new URLSearchParams(cut === -1 ? '' : withoutHash.slice(cut + 1));
  // Whatever this address already said about a move is stale the moment another one is
  // made. Without this, a success would land on a page still carrying `?fehler=403` from
  // the attempt before it, saying two contradictory things about one card.
  for (const stale of NOTICE_PARAMS) query.delete(stale);
  for (const [key, value] of Object.entries(params)) query.set(key, value);
  const search = query.toString();
  return `${path}${search ? `?${search}` : ''}#${BOARD_NOTICE_ID}`;
}

// --- Refusals ------------------------------------------------------------------------------

/**
 * Why the board is not there.
 *
 * Never conflated with "there are no cards". A board that failed to load and a board with
 * nothing on it are different things, and "hier ist keine Aufgabe zu sehen" about a server
 * that is down is a lie — the same distinction `/projekte`, `/graph` and the admin console
 * all make.
 */
export function describeBoard(status: number): string {
  if (status === 0) return 'Die Aufgaben konnten nicht geladen werden: Die Anwendung antwortet nicht.';
  return `Die Aufgaben konnten nicht geladen werden (Fehler ${status}).`;
}

/**
 * The same, for the board **embedded in a page** — and deliberately hedged.
 *
 * `/aufgaben` is a board, so "Die Aufgaben konnten nicht geladen werden" is a true sentence
 * there. On an ordinary page it would not be: nearly every page in the wiki is nobody's
 * project home, and a failed request cannot tell which kind this one is. Saying it plainly
 * would therefore announce a board on pages that have none — a claim the request never
 * established — so the sentence says "if there is one" and leaves the question open, which
 * is exactly what is known.
 */
export function describeEmbeddedBoard(status: number): string {
  const why = status === 0 ? 'Die Anwendung antwortet nicht.' : `Fehler ${status}.`;
  return `Falls zu dieser Seite eine Aufgabentafel gehört, konnte sie nicht geladen werden: ${why}`;
}

/**
 * Why a card did not move — and, in every branch, that it did not.
 *
 * The promise is the point, exactly as it is for a refused deletion on `/projekte`: a move
 * that half happened is the thing somebody would go and check for. The API decides before it
 * writes, so it is a promise this interface may actually make.
 */
export function describeMove(status: number): string {
  if (status === 0) return 'Die Anwendung antwortet nicht. Die Karte wurde nicht verschoben.';
  if (status === 400) return 'Diesen Status gibt es nicht. Die Karte wurde nicht verschoben.';
  if (status === 401) {
    return 'Nicht angemeldet — bitte erneut anmelden. Die Karte wurde nicht verschoben.';
  }
  if (status === 403) {
    return (
      'Dafür fehlt das Schreibrecht auf der Seite, zu der die Karte gehört. ' +
      'Die Karte wurde nicht verschoben.'
    );
  }
  if (status === 404) return 'Diese Karte gibt es nicht (mehr). Es wurde nichts verschoben.';
  return `Die Karte konnte nicht verschoben werden (Fehler ${status}).`;
}

/** What was just moved, said so it can be announced. Named here so both placements agree. */
export function movedText(task: BoardTask): string {
  return `»${task.title}« steht jetzt in ${STATUS_LABEL[task.status]}.`;
}

/**
 * The one sentence a board says about what just happened on it, if anything.
 *
 * Built by the loader — from the board it just read, never from the address bar — and handed
 * to the component, which is what keeps the two placements saying the same thing about the
 * same event. `art` decides `role="status"` against `role="alert"`; both are announced, and
 * an alert interrupts, which is right for a refusal and wrong for a success.
 */
export interface BoardNotice {
  art: 'ok' | 'fehler';
  text: string;
}

/**
 * What just happened on this board, read off the address and **checked against the board**.
 *
 * Here rather than in either loader, because it is precisely the kind of rule D-12 warns
 * about: two placements that must say the same thing about the same event. Both call this.
 *
 * A success is confirmed against the cards that were actually answered for this reader, never
 * against the address bar — a hand-typed `?verschoben=` names a card only if that card is
 * genuinely on this board, the same guard `/projekte` puts on its own `?angelegt=`. A refusal
 * is a status code and nothing else: it names no card, so there is nothing for it to
 * disclose, and a code that is not a number produces no notice rather than "Fehler NaN".
 */
export function noticeFor(query: URLSearchParams, board: BoardResponse): BoardNotice | null {
  const moved = query.get('verschoben');
  if (moved) {
    const card = board.columns.flatMap((column) => column.tasks).find((task) => task.id === moved);
    return card ? { art: 'ok', text: movedText(card) } : null;
  }

  const failed = query.get('fehler');
  if (failed && /^\d+$/.test(failed)) return { art: 'fehler', text: describeMove(Number(failed)) };
  return null;
}
