import { redirect } from '@sveltejs/kit';
import { apiGet, apiSend } from '$lib/api';
import {
  BOARD_PATH,
  boardPath,
  describeBoard,
  isStatus,
  noticeFor,
  returnTo,
  type BoardResponse,
  type Filter
} from '$lib/board';
import { describeList, type Project, type ProjectsResponse } from '$lib/projects';
import type { Actions, PageServerLoad } from './$types';

/**
 * The global board (D-12): every task the caller may see, optionally bound to one project.
 *
 * **One query with a filter, and that is the whole of D-12's cost being paid.** The decision
 * put a board here *and* on every project's home page, and said in the same breath what that
 * costs: two places that must agree. They agree by construction — one endpoint
 * (`GET /api/board`), one component (`$lib/components/Board.svelte`), and one form action for
 * a move, which the embedded board posts to from the page it is embedded in. A second
 * retrieval path would be a second answer to "which tasks exist", and because every card is a
 * disclosure surface, a second answer is also a second chance to leak one.
 *
 * **This page filters nothing.** `GET /api/board` answers only the cards whose governing page
 * the caller may read — per document, because D-3 makes a project span pages with different
 * grants by design — and that belongs to `Store::board_for`, where it is mutation-tested. The
 * corollary is the same one `/projekte` writes down: **no count and no "N ausgeblendet"**,
 * because a number here would be a number about pages the reader may not read.
 *
 * **The second request is not a second board.** `/api/projects` is asked for the filter's
 * options and for one guard: a project id in the address bar is matched against the list
 * before it is forwarded, so a hand-typed id cannot turn this page into a way of asking
 * whether a project exists. It is the same guard `/projekte` puts on its own `?loeschen=`.
 *
 * **State is in the URL.** Which project is being filtered on (`?projekt=`), which card just
 * moved (`?verschoben=`) and why one did not (`?fehler=`) are all query parameters, German
 * like `/graph`'s `?wurzel=`. That buys the same three things it does there: a filtered board
 * is a link somebody can send, the back button walks through it, and — because there is no
 * DOM environment in this project — every state can be rendered and asserted by a test.
 *
 * **One hazard, the same one `/projekte` and `[...path]/history` both write down.**
 * `/aufgaben` is a literal segment and SvelteKit prefers it over `[...path]`, so a wiki page
 * that ever lives at `/aufgaben` is shadowed by this route. Nothing in `content-example` is
 * named that. If a page ever is, this route's address has to move — not the page.
 */
export const load: PageServerLoad = async ({ fetch, request, url }) => {
  const cookie = request.headers.get('cookie');

  // The projects first: they are the filter's options, and they are what a requested filter
  // is checked against before it is forwarded to the board.
  let projects: Project[] = [];
  let projekteFehler: string | null = null;
  try {
    const answer = await apiGet<ProjectsResponse>(fetch, '/api/projects', cookie);
    if (answer.data) projects = answer.data.projects;
    else projekteFehler = describeList(answer.status);
  } catch {
    projekteFehler = describeList(0);
  }

  const wanted = url.searchParams.get('projekt');
  const projekt = wanted ? (projects.find((project) => project.id === wanted) ?? null) : null;
  // Said out loud rather than silently ignored, and said without confirming or denying that
  // such a project exists — the same conflation every empty state here makes.
  const filterUnbekannt = Boolean(wanted) && projekt === null;

  const filter: Filter = projekt ? { kind: 'projekt', id: projekt.id } : { kind: 'alle' };

  let board: BoardResponse = { project: null, columns: [] };
  let fehler: string | null = null;
  try {
    const answer = await apiGet<BoardResponse>(fetch, boardPath(filter), cookie);
    if (answer.data) {
      // `project` is absent from the unfiltered answer — there is no single project for a
      // board that spans all of them — and read as `null` so both shapes land in one.
      board = { project: answer.data.project ?? null, columns: answer.data.columns ?? [] };
    } else {
      fehler = describeBoard(answer.status);
    }
  } catch {
    // `apiGet` throws when the request never got an answer at all.
    fehler = describeBoard(0);
  }

  return {
    board,
    projects,
    projekteFehler,
    projekt,
    filterUnbekannt,
    fehler,
    hinweis: noticeFor(url.searchParams, board),
    // Where a move made on THIS board comes back to, filter and all. Built from the pieces
    // rather than from `url` so that a stale `?verschoben=` cannot ride along.
    zurueck: projekt ? `${BOARD_PATH}?projekt=${encodeURIComponent(projekt.id)}` : BOARD_PATH,
    // One instant for both renders — see `Board.svelte`'s `now`, and `[...path]/history`,
    // which captures one for the same reason.
    now: Date.now()
  };
};

export const actions: Actions = {
  /**
   * Move a card into another column.
   *
   * **This action serves both placements**, and that is the point of it living here rather
   * than once per route: the embedded board's form posts to `/aufgaben?/verschieben` too,
   * carrying the path it wants to come back to. One implementation of a move means one set
   * of hidden fields, one refusal wording and one announcement, which is what stops the two
   * boards from disagreeing about what a move does.
   *
   * **Every outcome is a redirect, refusals included** — which is a deliberate departure
   * from `/projekte`, where a refusal comes back as `fail()`. `fail()` re-renders the route
   * that owns the action, so a refused move made on a project's home page would land the
   * reader on the global board with a message about a page they had left. The answer travels
   * in the address instead: `?verschoben=` or `?fehler=<status>`, turned back into a sentence
   * by the loader of whichever board it lands on.
   *
   * Nothing here decides whether the caller may do this. `PATCH /api/tasks/{id}` needs Write
   * on the page that governs the card and asks `Store::document_for` for it; this action
   * turns its answer into a German sentence and nothing more. The buttons are offered to
   * whoever is signed in for the same reason the edit link is: no field on the wire says "may
   * I write this page", so the offer can be false and the move cannot.
   */
  verschieben: async ({ request, fetch }) => {
    const form = await request.formData();
    const karte = String(form.get('karte') ?? '').trim();
    const status = String(form.get('status') ?? '');
    const zurueck = typeof form.get('zurueck') === 'string' ? String(form.get('zurueck')) : null;

    // Only one of D-9's three, and only ever the composed spelling — the same set the
    // store's CHECK constraint holds. A status this application never rendered a button for
    // is refused here rather than forwarded, so the API is never asked a question this
    // interface already knows the answer to.
    if (!karte || !isStatus(status)) {
      redirect(303, returnTo(zurueck, { fehler: '400' }));
    }

    const { status: antwort, failure } = await apiSend(
      fetch,
      'PATCH',
      `/api/tasks/${encodeURIComponent(karte)}`,
      request.headers.get('cookie'),
      { status }
    );

    // Status 0 is "no answer at all" and is kept apart from a 5xx, exactly as `apiSend`
    // keeps them apart: "not reachable" and "answered with an error" send somebody to
    // different places.
    if (failure) redirect(303, returnTo(zurueck, { fehler: String(antwort) }));

    redirect(303, returnTo(zurueck, { verschoben: karte }));
  }
};
