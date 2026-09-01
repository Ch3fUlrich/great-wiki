/**
 * The Papierkorb (D-14), on the wire and in words.
 *
 * Pure on purpose, exactly like `$lib/topics`, `$lib/board` and `$lib/projects`: this module
 * is imported from `+page.server.ts` files **and** from the components that render the two
 * placements — the trash itself and the delete control on a page — so it may not touch
 * `$env/dynamic/private`. Importing a server-only module from a component poisons the client
 * bundle. The calls themselves live in `$lib/api`.
 *
 * **Nothing here decides who may do anything, and nothing here filters.** Three different
 * permissions meet in this feature and all three are answered before the interface sees
 * anything:
 *
 * - *Deleting* is an edit: write on the page and on every page under it, decided by
 *   `Store::trash_document`. The page already carries the same verdict as `may_write`
 *   (ADR 0010), which is what a control may be offered on.
 * - *Restoring* is the same permission asked about a trashed row, and the answer travels as
 *   `TrashEntry::may_restore` — the store's own verdict, carried rather than recomputed. An
 *   interface that recomputed it would be a second answer, and the second answer is the one
 *   that gets it wrong.
 * - *Purging* is not an edit at all. It is gated by `path_admin` on the page's own path
 *   (ADR 0012), and **there is no bit for it on the wire**. That absence is what shapes the
 *   whole purge flow here: see {@link confirmPurgeHref}.
 *
 * **The listing is an aggregate view and carries exactly one number**: `TrashEntry::pages`,
 * the pages in that entry *this caller may read*, taken from the same filtered pass the entry
 * survived. There is no total and there is nowhere for one to arrive — {@link pagesText}
 * takes a single count, exactly as `$lib/topics`'s `countText` does, and for the reason
 * ADR 0011 gives.
 */

import { formatInstant } from '$lib/adminApi';

export { formatInstant };

// --- What comes off the wire ---------------------------------------------------------------

/**
 * One act in the Papierkorb. Mirrors `gw_api::routes::trash::TrashEntryView`.
 *
 * `path` names the **entry** — the page somebody deleted — and never one of the pages that
 * went down with it. Those are not separately addressable, which is the whole reason a
 * restore puts back everything that went in together.
 */
export interface TrashEntry {
  path: string;
  title: string;
  /** As SQLite writes it (`YYYY-MM-DD HH:MM:SS`, UTC). Rendered through `formatInstant`. */
  deleted_at: string;
  /** Who deleted it, as they were called then — a snapshot, exactly as a byline is. */
  deleted_by_name: string;
  /** Pages in this entry **this caller may read**. Never a total. See the module comment. */
  pages: number;
  /** Whether this caller may put it back. The store's verdict, carried (ADR 0010). */
  may_restore: boolean;
}

/** `GET /api/trash`. Already filtered per document — see the module comment. */
export interface TrashResponse {
  entries: TrashEntry[];
}

/** One page a purge names. Mirrors `gw_api::routes::trash::PurgedPageView`. */
export interface PurgedPage {
  path: string;
  title: string;
}

/**
 * What a purge destroyed, or would. Mirrors `gw_api::routes::trash::PurgeReportView`.
 *
 * Every number in it was measured across the DELETE itself, in the same transaction, so a
 * preview and the purge it describes cannot report different totals (ADR 0012). That is what
 * licenses this interface to show the preview's numbers as *the* numbers rather than as an
 * estimate — and what makes summarising them a loss rather than a tidy-up.
 */
export interface PurgeReport {
  /** Whether this actually happened. `false` for a preview. */
  committed: boolean;
  /** Every page that goes, by name, in path order. */
  pages: PurgedPage[];
  revisions: number;
  tasks: number;
  projects: number;
  links: number;
  topic_filings: number;
  topics: number;
}

// --- Where things are ------------------------------------------------------------------------

/** The one listing endpoint, named once so two placements cannot drift onto two spellings. */
export const TRASH_ENDPOINT = '/api/trash';

/**
 * Where the Papierkorb lives in this interface.
 *
 * **One hazard, the same one `/themen`, `/projekte` and `/aufgaben` write down.**
 * `/papierkorb` is a literal segment and SvelteKit prefers it over `[...path]`, so a wiki page
 * that ever lives at `/papierkorb` is shadowed by this route. Nothing in `content-example` is
 * named that. If a page ever is, this route's address has to move — not the page.
 */
export const TRASH_PATH = '/papierkorb';

/**
 * The region a change comes back to — the anchor that makes it *announced* rather than merely
 * drawn.
 *
 * A `role="status"` region already in the document when it loads announces nothing; a live
 * region announces what changes. So a finished restore or purge redirects to this id, the
 * region carries `tabindex="-1"`, and the browser moves focus there: the reader lands on the
 * sentence that says what happened and it is read out. No JavaScript is involved, which is
 * the requirement. The same mechanism, and the same reason, as `TOPICS_REGION_ID`.
 */
export const TRASH_REGION_ID = 'gw-papierkorb';

/** The confirmation block, for the same reason — see {@link confirmPurgeHref}. */
export const PURGE_REGION_ID = 'gw-endgueltig';

/** The question before a delete, on the page itself — see {@link deleteHref}. */
export const DELETE_REGION_ID = 'gw-loeschen';

/**
 * The parameter the purge question rides in, and the one the delete question rides in.
 *
 * In the address rather than in a store, for the four things `$lib/tabs` lists and which all
 * apply here: the question is server-rendered in the first response, it survives a reload, the
 * back button walks out of it, and — because there is no DOM environment in this project —
 * every state can be rendered and asserted by a test. German, like `?wurzel=`, `?loeschen=`
 * and `?seitenleiste=`: this is a German interface and the address bar is part of it.
 */
export const PURGE_PARAM = 'entfernen';
/** Which page is being asked about, on the page itself. */
export const DELETE_PARAM = 'loeschen';
/** What just came back, echoed so the reader is told rather than left to notice. */
export const RESTORED_PARAM = 'wiederhergestellt';
/**
 * What has just arrived here, carried from the page it was deleted on.
 *
 * A delete leaves the reader on an address that now answers 404, so it lands them here
 * instead — where the entry is, and where the control that undoes it is. The parameter is what
 * lets the Papierkorb say which of its rows is the one that just appeared.
 */
export const DELETED_PARAM = 'geloescht';
/** What was just destroyed. */
export const PURGED_PARAM = 'geleert';

/** Percent-encode each segment of a path, and nothing else. */
function encodeSegments(path: string): string {
  return path.split('/').map(encodeURIComponent).join('/');
}

function withLeadingSlash(path: string): string {
  return path.startsWith('/') ? path : `/${path}`;
}

/**
 * `/handbuch` → `/api/documents/handbuch`. The page itself; `DELETE` puts it in the trash.
 *
 * It answers a summary of what moved, and this interface deliberately drops it: the reader is
 * sent to the Papierkorb, where the entry's own row carries the number — and that one is the
 * pages *they* may read, from the filtered pass the listing already made. Two numbers for one
 * act would be two chances to disagree, and the summary's is the wrong one to show here.
 */
export function documentApiPath(path: string): string {
  return `/api/documents${encodeSegments(withLeadingSlash(path))}`;
}

/** `/handbuch` → `/api/trash/restore/handbuch`. */
export function restoreApiPath(path: string): string {
  return `/api/trash/restore${encodeSegments(withLeadingSlash(path))}`;
}

/**
 * `/handbuch` → `/api/trash/purge/handbuch`. **`GET` describes it, `POST` performs it.**
 *
 * One address for both, deliberately, and this interface must keep asking it that way: the
 * description is the destruction rolled back, so the numbers a reader confirms cannot be a
 * different number from the one that happens (ADR 0012).
 */
export function purgeApiPath(path: string): string {
  return `/api/trash/purge${encodeSegments(withLeadingSlash(path))}`;
}

/**
 * Where the question "shall this really be destroyed?" is asked.
 *
 * **A link to a question, never a control that destroys.** There is no `may_purge` on the
 * wire — a trash entry says whether *you* may restore it and nothing about who administers
 * the path — so the listing cannot know whether to offer this, and it does not pretend to.
 * What it can do is refuse to offer it to somebody it *knows* would be refused (`path_admin`
 * demands a signed-in, active account before it looks at anything else), and then let the
 * API's own gate answer the rest: `GET /api/trash/purge/{path}` is gated exactly as the
 * `POST` is, so the destroying control appears only after that gate has already said yes to
 * this caller. The button that destroys is therefore never a guess, which is the property
 * that actually matters.
 *
 * The fragment is not decoration: it is what moves focus to the confirmation so a screen
 * reader hears it. See {@link PURGE_REGION_ID}.
 */
export function confirmPurgeHref(path: string): string {
  return `${TRASH_PATH}?${PURGE_PARAM}=${encodeURIComponent(path)}#${PURGE_REGION_ID}`;
}

/**
 * Where the question "shall this page go to the Papierkorb?" is asked: on the page itself.
 *
 * The owner's decision, and the reason it is a link rather than a button: deleting happens
 * where the page is, and a control that only works once a bundle has arrived is a control
 * that looks live and does nothing — which is what `[...path]/+page.svelte` already records
 * about its own edit link.
 *
 * **Deleting is recoverable, so the question it asks is a plain one** — no danger styling, no
 * second confirmation. What it must say is the part nobody would guess: the page does not go
 * on its own, it takes everything under it, because a page whose parent is in the trash is
 * not filtered out of the navigation but *unreachable* in it. That is why the question exists
 * at all rather than a bare button.
 *
 * The fragment does the same job it does before a purge — it moves focus to the question, so
 * a reader who cannot see it is told it is there. See {@link DELETE_REGION_ID}.
 */
export function deleteHref(path: string): string {
  return `${path}?${DELETE_PARAM}=1#${DELETE_REGION_ID}`;
}

// --- Words ------------------------------------------------------------------------------------

/**
 * How many pages are in an entry, in words.
 *
 * **One parameter, and that is the point.** ADR 0011 licenses exactly one number here — the
 * pages *this* caller may read in the entry — and forbids any number about what the filter
 * removed. A signature that also took a total is where an "N ausgeblendet" would one day be
 * written; there is nowhere for it to go.
 */
export function pagesText(pages: number): string {
  return pages === 1 ? '1 Seite' : `${pages} Seiten`;
}

/** One kind of thing a purge destroys, and how many of them. */
export interface PurgeLine {
  /** A German noun phrase saying what these are, never the name of a table. */
  was: string;
  zahl: number;
}

/** What each counted kind is called, for the kinds this interface knows about. */
const PURGE_LABELS: Record<string, string> = {
  revisions: 'Versionen aus der Versionsgeschichte dieser Seiten',
  tasks: 'Karten von den Tafeln dieser Seiten',
  projects: 'Projekte, die auf einer dieser Seiten zu Hause sind',
  links: 'Verweise mit einem Ende auf einer dieser Seiten',
  topic_filings: 'Themenzuordnungen — »diese Seite handelt von …«',
  topics: 'Themen, die danach keine Seite mehr trägt'
};

/**
 * Everything a purge destroys besides the pages themselves, each with its own number.
 *
 * **Every line always, including the ones that are none.** An absent line reads as "not
 * counted" exactly as easily as "none", and this is the single confirmation in the system
 * whose reader cannot check afterwards which it was. `0 Verweise` is a fact worth telling
 * somebody who is about to destroy a page.
 *
 * **And every line the API sends, including ones this file has never heard of.** The report is
 * walked rather than spelled out field by field, and a number with no wording here is rendered
 * with its own name rather than dropped. That is not defensive habit: `PurgeReport` grows
 * whenever the system grows something a purge cascades away, and a hand-written list would
 * silently stop mentioning the newest one — a confirmation that under-reports what it is about
 * to destroy, in the one place where the reader cannot find out afterwards. An ugly German
 * sentence naming a field is a bad outcome; a missing line is the outcome this whole
 * confirmation exists to prevent. The order is the API's own, which serde fixes.
 *
 * The pages are deliberately *not* in here: the API names them, so this interface names them,
 * one by one. Folding them into "und 3 weitere" would throw away the only part of the report
 * that says *which* page — including, per ADR 0012, a page fenced off with its own grants that
 * the purge reaches anyway.
 */
export function purgeLines(report: PurgeReport): PurgeLine[] {
  return Object.entries(report as unknown as Record<string, unknown>)
    .filter(([, value]) => typeof value === 'number')
    .map(([key, value]) => ({
      was: PURGE_LABELS[key] ?? `Weiteres, das der Papierkorb »${key}« nennt`,
      zahl: value as number
    }));
}

/** The sentence for a status nothing else has wording for. Nothing the server said is lost. */
function generic(status: number, clause: string, server: string | null): string {
  const detail = server ? ` Der Server meldet: ${server}` : '';
  if (status === 0) return `${clause}: Die Anwendung antwortet nicht.`;
  return `${clause} (Fehler ${status}).${detail}`;
}

/**
 * Why the Papierkorb is not there.
 *
 * Never conflated with "it is empty". A listing that failed to load and a wiki nobody has
 * deleted anything in are different things, and "der Papierkorb ist leer" about a server that
 * is down is the lie every other view here refuses to tell.
 */
export function describeTrash(status: number): string {
  return generic(status, 'Der Papierkorb konnte nicht geladen werden', null);
}

/**
 * Why a page did not go to the Papierkorb — and, in every branch, that it did not.
 *
 * The promise is the point, and the API lets this interface make it: `trash_document` decides
 * the whole subtree before it writes a single row, so a delete that half happened is not a
 * state that exists.
 *
 * **The 409 has exactly one shape** — a subpage the caller may not write — so it is said in
 * German rather than quoted. The way out is somebody else's to take, and saying whose is what
 * stops the reader pressing the same control again.
 */
export function describeDelete(status: number, server: string | null = null): string {
  const nothing = 'Es wurde nichts gelöscht.';
  if (status === 0) return `Die Anwendung antwortet nicht. ${nothing}`;
  if (status === 401) return `Nicht angemeldet — bitte erneut anmelden. ${nothing}`;
  if (status === 403) return `Dafür fehlt das Schreibrecht auf dieser Seite. ${nothing}`;
  if (status === 404) return `Diese Seite gibt es nicht (mehr). ${nothing}`;
  if (status === 409) {
    return (
      'Unter dieser Seite liegt eine Unterseite, die Sie nicht bearbeiten dürfen — und eine ' +
      'Seite kommt mit allem darunter in den Papierkorb. Wer diese Unterseite verwaltet, ' +
      `muss sie zuerst löschen. ${nothing}`
    );
  }
  return `${generic(status, 'Die Seite konnte nicht gelöscht werden', server)} ${nothing}`;
}

/**
 * Why nothing came back — and, in every branch, that nothing did.
 *
 * **The 409 is quoted, and that is deliberate.** It has three shapes and two of them name the
 * page that is in the way: a parent still in the trash, or a parent that no longer exists at
 * all. Only the API knows which, and only the API knows its path. Telling the three apart
 * would mean matching English sentences in a German interface — fragile in a way that fails
 * silently — so the refusal is framed in German and the API's own words are carried inside
 * it. Dropping them would turn a refusal that names the way out into "Fehler 409", which is a
 * refusal nobody can act on.
 */
export function describeRestore(status: number, server: string | null = null): string {
  const nothing = 'Es wurde nichts wiederhergestellt.';
  if (status === 0) return `Die Anwendung antwortet nicht. ${nothing}`;
  if (status === 401) return `Nicht angemeldet — bitte erneut anmelden. ${nothing}`;
  if (status === 403) {
    return (
      'Dafür fehlt das Schreibrecht auf einer der Seiten, die zurückkämen. ' + nothing
    );
  }
  if (status === 404) return `Im Papierkorb liegt dort nichts (mehr). ${nothing}`;
  if (status === 409) {
    const grund = server
      ? `Der Papierkorb nennt den Grund: ${server}`
      : 'Der Papierkorb nennt keinen Grund.';
    return `So wie es steht, kann diese Seite nicht zurück. ${grund} ${nothing}`;
  }
  return `${generic(status, 'Die Seite konnte nicht wiederhergestellt werden', server)} ${nothing}`;
}

/**
 * The branches a purge and its description share.
 *
 * One body for both, so the description cannot explain a refusal differently from the thing it
 * describes — which is the interface's half of the API's own "one gated body, two verbs".
 */
function purgeReason(status: number, server: string | null): string {
  if (status === 0) return 'Die Anwendung antwortet nicht.';
  if (status === 401 || status === 403) {
    // `path_admin`, not write. ADR 0012: being able to edit a page is structurally not being
    // able to destroy it, so this must send somebody to the right person rather than suggest
    // that a write right would do.
    return (
      'Endgültig löschen darf nur, wer diese Seite verwaltet. Schreibrecht genügt dafür ' +
      'nicht — das ist der Unterschied zwischen Löschen und endgültig Löschen.'
    );
  }
  if (status === 404) {
    return 'Im Papierkorb liegt dort nichts. Endgültig gelöscht wird nur, was schon im Papierkorb liegt.';
  }
  if (status === 409) {
    const detail = server ? ` Der Papierkorb nennt den Grund: ${server}` : '';
    return (
      'Unter dieser Seite liegt noch eine Seite, die nicht im Papierkorb ist. Endgültig ' +
      `gelöscht wird nur, was schon gelöscht wurde.${detail}`
    );
  }
  return generic(status, 'Der Papierkorb konnte die Frage nicht beantworten', server);
}

/**
 * Why this interface cannot say what a purge would destroy.
 *
 * It makes **no promise about nothing having been destroyed**, because a preview was never
 * going to destroy anything — and a sentence that reassured about an act that was not
 * attempted is a sentence that teaches the reader to skim the one that matters.
 */
export function describePurgePreview(status: number, server: string | null = null): string {
  return purgeReason(status, server);
}

/** Why a purge did not happen — and, in every branch, that nothing was destroyed. */
export function describePurge(status: number, server: string | null = null): string {
  return `${purgeReason(status, server)} Es wurde nichts endgültig gelöscht.`;
}
