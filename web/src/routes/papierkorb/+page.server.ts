import { fail, redirect } from '@sveltejs/kit';
import { apiGet, apiSend, type ApiFailure } from '$lib/api';
import {
  describePurge,
  describePurgePreview,
  describeRestore,
  describeTrash,
  DELETED_PARAM,
  purgeApiPath,
  restoreApiPath,
  PURGE_PARAM,
  PURGED_PARAM,
  RESTORED_PARAM,
  TRASH_ENDPOINT,
  TRASH_PATH,
  TRASH_REGION_ID,
  type PurgeReport,
  type TrashEntry,
  type TrashResponse
} from '$lib/trash';
import type { Actions, PageServerLoad } from './$types';

/**
 * The Papierkorb (D-14): what has been deleted, what can come back, and the one act in this
 * system that loses data.
 *
 * **Why the trash is a place and deleting is not.** The owner split the two deliberately:
 * deleting happens where the page is — beside »Bearbeiten« and »Verlauf«, because that is
 * where you are when you decide a page should go — and *recovering* happens somewhere you can
 * find without knowing a URL. A restore control that only existed on the page it restores
 * would be unreachable by definition: the page is gone from the tree, out of the export and
 * out of the search, and the only way back to it would be an address somebody had kept.
 *
 * **This page is an aggregate view, and it does not filter anything.** `GET /api/trash`
 * authorises every entry, and every page inside it, through the same body a page read ends in
 * — a page you could not see before it was deleted is not one you can see in the trash. That
 * belongs to `Store::trash_for`, where it is mutation-tested. There is exactly one request for
 * it here for the reason `/projekte` writes down about its own: every row says a page exists
 * and what it is called, so a second source of rows would be a second chance to disclose one.
 *
 * The corollary that is easy to get wrong: **no total, and no "N ausgeblendet".** The number
 * beside an entry is the pages *this caller* may read in it, taken from the same filtered pass
 * the entry survived. A number about what the filter removed would be a fact about pages the
 * reader may not read, which is the one thing the filtering cannot take back (ADR 0011).
 * `page.test.ts` pins its absence; this loader has nowhere to compute one.
 *
 * **The preview is the purge, rolled back** (ADR 0012) — it opens a write transaction and
 * takes SQLite's write lock for its duration. That is affordable as an administrative action
 * and would not be as page furniture, so it is asked for exactly one entry, only once somebody
 * has asked to destroy that entry, and only for an entry this caller was actually shown. Never
 * once per row, and never for a path that arrived in the address bar and nowhere else.
 *
 * **Everything works before hydration.** Restoring and destroying are real form actions, not
 * click handlers: the browser's own submission carries the cookie, the answer is a 303 back to
 * this list, and a refusal comes back as `fail()` and is rendered into the page.
 *
 * **State is in the URL.** Which entry is being confirmed (`?entfernen=`), and what just
 * happened (`?geloescht=`, `?wiederhergestellt=`, `?geleert=`), are query parameters — German, like
 * `/graph`'s `?wurzel=` and `/projekte`'s `?loeschen=` — which buys the same three things it
 * does there: the question is a link, the back button walks out of it, and every state can be
 * rendered and asserted without a DOM.
 *
 * **One hazard, the same one `/themen`, `/projekte` and `/aufgaben` write down.**
 * `/papierkorb` is a literal segment and SvelteKit prefers it over `[...path]`, so a wiki page
 * that ever lives at `/papierkorb` is shadowed by this route. Nothing in `content-example` is
 * named that. If a page ever is, this route's address has to move — not the page.
 */
export const load: PageServerLoad = async ({ fetch, request, url }) => {
  const cookie = request.headers.get('cookie');

  let entries: TrashEntry[] = [];
  let fehler: string | null = null;
  try {
    const answer = await apiGet<TrashResponse>(fetch, TRASH_ENDPOINT, cookie);
    // A failed request and an empty Papierkorb are different things and are never conflated:
    // "hier liegt nichts" about a server that is down is the lie every other view here
    // refuses to tell. `/projekte` and `/graph` make the same distinction.
    if (answer.data) entries = answer.data.entries ?? [];
    else fehler = describeTrash(answer.status);
  } catch {
    // `apiGet` throws when the request never got an answer at all.
    fehler = describeTrash(0);
  }

  // Matched against the listing rather than trusted from the address bar — the discipline
  // `/projekte` applies to its own `?loeschen=`, and here it does a second job: it is what
  // stops a hand-typed path opening a write transaction on the API.
  const wanted = url.searchParams.get(PURGE_PARAM);
  const entfernen = entries.find((entry) => entry.path === wanted) ?? null;

  let bericht: PurgeReport | null = null;
  let berichtFehler: string | null = null;
  if (entfernen) {
    try {
      const answer = await apiGet<PurgeReport>(fetch, purgeApiPath(entfernen.path), cookie);
      if (answer.data) bericht = answer.data;
      else berichtFehler = describePurgePreview(answer.status);
    } catch {
      berichtFehler = describePurgePreview(0);
    }
  }

  /**
   * A path this caller was told about and which is no longer in the trash.
   *
   * Checked against the listing that was just read, for the reason `/projekte` checks
   * `?angelegt=`: a message that says a page came back while that page is still sitting in the
   * list below it is the interface contradicting its own data. An entry that is genuinely gone
   * is the only evidence available here, and it is enough.
   */
  const gone = (path: string | null): string | null =>
    path && !entries.some((entry) => entry.path === path) ? path : null;

  /**
   * The entry that has just arrived, when a delete on a page sent the reader here.
   *
   * The mirror of {@link gone}: this one has to be *present* to be true, and it is the entry
   * itself rather than the path so the notice can name the page as the table names it — and
   * say how many pages came with it, from the same filtered number the row carries.
   */
  const angekommen = url.searchParams.get(DELETED_PARAM);

  return {
    entries,
    fehler,
    entfernen,
    bericht,
    berichtFehler,
    geloescht: entries.find((entry) => entry.path === angekommen) ?? null,
    wiederhergestellt: gone(url.searchParams.get(RESTORED_PARAM)),
    geleert: gone(url.searchParams.get(PURGED_PARAM))
  };
};

/** Where a finished act comes back to — and the region that then announces it. */
const DONE = `${TRASH_PATH}?`;

/** What a refused write said, when this file has no wording of its own for the status. */
function said(failure: ApiFailure | null): string | null {
  return failure?.message ?? null;
}

/**
 * Putting a page back, and destroying one.
 *
 * **Two actions, two permissions, and the difference between them is the whole feature.**
 * Restoring is an edit — write on every page that comes back — and is offered on
 * `TrashEntry::may_restore`, the store's own verdict carried on the wire. Destroying is not an
 * edit: it is gated by `path_admin` on the page's own path (ADR 0012), and no bit for it
 * exists on the wire at all. So the destroying control is never offered on a guess; it appears
 * only after `GET /api/trash/purge/{path}` — gated identically to the `POST` — has already
 * answered this caller with a report.
 *
 * Neither action decides anything. Both turn the API's answer into a German sentence, and both
 * are real form submissions so they work with JavaScript switched off.
 *
 * A finished act redirects to the list with a fragment, which is what makes it **announced**
 * rather than merely drawn: the browser moves focus to the region, and a region that has just
 * received focus is read out. A live region already present in the document announces nothing.
 * No script is involved — the same mechanism `TOPICS_REGION_ID` uses, and the same reason.
 */
export const actions: Actions = {
  /** Put a trash entry back, with everything that went down with it. */
  wiederherstellen: async ({ request, fetch }) => {
    const form = await request.formData();
    const pfad = String(form.get('pfad') ?? '').trim();
    if (!pfad) {
      // Refused here rather than forwarded: the API would say the same thing, and asking it a
      // question this interface already knows the answer to is a round trip for nothing.
      return fail(400, {
        wo: 'wiederherstellen' as const,
        fehler: 'Es wurde keine Seite genannt. Es wurde nichts wiederhergestellt.'
      });
    }

    const { status, failure } = await apiSend(
      fetch,
      'POST',
      restoreApiPath(pfad),
      request.headers.get('cookie')
    );

    if (failure) {
      // The status is passed through as the form's own, so a refusal is not reported as a 200
      // with a sad message in it. `failure.message` is what the API named — and for a 409 that
      // is the path of the page standing in the way, which nothing else here knows.
      return fail(status === 0 ? 503 : status, {
        wo: 'wiederherstellen' as const,
        fehler: describeRestore(status, said(failure))
      });
    }

    redirect(303, `${DONE}${RESTORED_PARAM}=${encodeURIComponent(pfad)}#${TRASH_REGION_ID}`);
  },

  /**
   * Destroy a trash entry and everything under it. There is no undo, which is why the reader
   * has already been shown the report this posts against.
   */
  endgueltigLoeschen: async ({ request, fetch }) => {
    const form = await request.formData();
    const pfad = String(form.get('pfad') ?? '').trim();
    if (!pfad) {
      return fail(400, {
        wo: 'endgueltig' as const,
        fehler: 'Es wurde keine Seite genannt. Es wurde nichts endgültig gelöscht.'
      });
    }

    const { status, failure } = await apiSend(
      fetch,
      'POST',
      purgeApiPath(pfad),
      request.headers.get('cookie')
    );

    if (failure) {
      return fail(status === 0 ? 503 : status, {
        wo: 'endgueltig' as const,
        fehler: describePurge(status, said(failure))
      });
    }

    redirect(303, `${DONE}${PURGED_PARAM}=${encodeURIComponent(pfad)}#${TRASH_REGION_ID}`);
  }
};
