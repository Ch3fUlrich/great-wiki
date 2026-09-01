import { error, fail, redirect } from '@sveltejs/kit';
import { apiGet, apiSend, parseBody, type Backlink, type DocumentView } from '$lib/api';
import { boardPath, describeEmbeddedBoard, noticeFor, type BoardResponse } from '$lib/board';
import {
  describeSetTopics,
  describeTopics,
  documentTopicsApiPath,
  TOPICS_REGION_ID,
  type DocumentTopicsResponse,
  type Topic
} from '$lib/topics';
import type { Actions, PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, fetch, request, url }) => {
  const cookie = request.headers.get('cookie');
  const { status, data } = await apiGet<DocumentView>(
    fetch,
    `/api/documents/${params.path}`,
    cookie
  );

  if (status === 403) error(403, 'You do not have access to this page.');
  if (!data) error(404, 'Page not found.');

  // Own endpoint, own prefix (`/api/links/backlinks/{*path}`, not a suffix under
  // `/api/documents`) — see `gw-api/src/routes/links.rs` for why. Already filtered to what
  // THIS caller may read; `data` ?? [] only for a request that failed outright (network,
  // 5xx), never as a second permission filter — the store already carried that out.
  const { data: backlinks } = await apiGet<{ backlinks: Backlink[] }>(
    fetch,
    `/api/links/backlinks/${params.path}`,
    cookie
  );

  // D-12's second placement: the board of the project homed at THIS page.
  //
  // **The same endpoint the global board asks, with the filter bound** — that is the whole
  // of what D-12 permitted. It put a board here as well as at `/aufgaben` because a
  // project's own page is where you look when you are thinking about that project, and it
  // named the cost in the same breath: two places that must agree. They agree by being one
  // query and one component (`$lib/components/Board.svelte`), and a move made here posts to
  // `/aufgaben?/verschieben` like every other one. A second retrieval path would be a second
  // answer to "which tasks exist", and because every card is a disclosure surface, a second
  // answer is also a second chance to leak one.
  //
  // **Most pages are nobody's home**, and that is not an error. The endpoint says which by
  // naming a project or not; no project means no board, silently, because a notice on every
  // page in the wiki would be furniture paid for by every reader who never asked about one.
  // A 404 is read the same way, for the same reason.
  //
  // **A real failure is stated, and stated without saying whether this page is a home.** It
  // has to be: "there is nothing here" about a server that is down is the lie every other
  // view in this interface refuses to tell. The wording carries no claim either way.
  //
  // The board never fails this page. It is an addition to a page, never a precondition for
  // one — an API that cannot answer about tasks must not take the wiki's reading surface
  // down with it.
  let board: BoardResponse | null = null;
  let boardFehler: string | null = null;
  try {
    const answer = await apiGet<BoardResponse>(
      fetch,
      boardPath({ kind: 'seite', path: data.path }),
      cookie
    );
    if (answer.data) {
      const project = answer.data.project ?? null;
      if (project) board = { project, columns: answer.data.columns ?? [] };
    } else if (answer.status !== 404) {
      boardFehler = describeEmbeddedBoard(answer.status);
    }
  } catch {
    boardFehler = describeEmbeddedBoard(0);
  }

  // The owner's second decision: a page's topics are shown and edited ON THE PAGE. So they
  // are read here, beside the page itself, rather than fetched by a component after
  // hydration — tagging is something you do while reading, and a chip row that appeared a
  // second late would be furniture that flickers.
  //
  // **Its own prefix, not a suffix under `/api/documents`**, for the reason
  // `gw-api/src/routes/topics.rs` gives: matchit prefers a literal segment over a catch-all,
  // so `/api/documents/{*path}/topics` would be shadowed by a real page slugged `topics`.
  //
  // Already filtered — the endpoint answers nothing at all to somebody who may not read this
  // page, and this page has already been read by then. A failure is stated rather than
  // rendered as "this page is about nothing", and never fails the page: chips are an addition
  // to a page, exactly as the board is.
  let seitenThemen: Topic[] = [];
  let seitenThemenFehler: string | null = null;
  try {
    const answer = await apiGet<DocumentTopicsResponse>(
      fetch,
      documentTopicsApiPath(data.path),
      cookie
    );
    if (answer.data) seitenThemen = answer.data.topics ?? [];
    else seitenThemenFehler = describeTopics(answer.status);
  } catch {
    seitenThemenFehler = describeTopics(0);
  }

  // Read here rather than from `$app/state` in the component, for two reasons: the flag is
  // then part of the page's data and a server-render test can set it, and the component
  // does not have to reach for a SvelteKit runtime that only exists inside a request.
  //
  // It asks for the editor; it does not decide anything. Whether this caller may actually
  // write is settled by the collaboration socket, which is the only thing that knows.
  return {
    doc: data,
    body: parseBody(data),
    // The tree is NOT fetched here any more: the shell renders it on every view, so
    // `+layout.server.ts` asks for it once and this page reads the same answer through the
    // merged `data`. Two requests for one filtered tree was two chances for the breadcrumb
    // and the sidebar to disagree about which pages exist.
    backlinks: backlinks?.backlinks ?? [],
    seitenThemen,
    seitenThemenFehler,
    edit: url.searchParams.get('edit') === '1',
    board,
    boardFehler,
    // What just happened on the board, checked against the board itself — the same function
    // `/aufgaben` calls, so the two placements cannot say different things about one move.
    hinweis: board ? noticeFor(url.searchParams, board) : null,
    // A move made here comes back HERE, not to the global board.
    zurueck: data.path,
    // One instant for both renders, so a card cannot be overdue on the server and due today
    // in the browser — the same reason `[...path]/history` captures one.
    now: Date.now()
  };
};

/**
 * The two ways a page's topics change, both on the page itself.
 *
 * **Real form actions, and no `use:enhance`.** The browser submits, the server answers 303
 * back to this page, and the whole thing works with JavaScript switched off — which is the
 * requirement, not a fallback: this repository already records, about its own edit link, that
 * a control which needs a bundle to arrive is a control that looks live and does nothing.
 *
 * **The endpoint takes the whole set**, deliberately (`PUT`, not `PATCH`): "these are the
 * topics" is what a frontmatter line says and what a file drop has to be able to mean. So both
 * actions are read–modify–write, and both **read fresh** rather than trusting a hidden field
 * with the set the reader was shown — a stale one would put back a topic somebody else had
 * just removed, silently, and the reader who pressed »entfernen« would be the one who did it.
 *
 * **Nothing here decides whether the caller may do this.** `PUT /api/topics/document/{path}`
 * needs Write on the page and asks `Store::document_for` for it; these actions turn its answer
 * into a German sentence and nothing more. The control is offered on `may_write`, which is the
 * same verdict — so the offer and the refusal cannot disagree — and the offer can still be
 * stale where the refusal never is.
 *
 * **A refusal comes back as `fail()`**, not as a redirect: `fail()` re-renders the route that
 * owns the action, and that route is this page — the one the reader is standing on. (The
 * board's move action is the opposite case and says so: it serves two placements, so a
 * refusal there has to travel in the address to reach whichever board it happened on.)
 */
export const actions: Actions = {
  /** File this page under one more topic. */
  themaHinzufuegen: async ({ params, request, fetch }) => {
    const form = await request.formData();
    const typed = String(form.get('thema') ?? '').trim();
    if (!typed) {
      // Refused here rather than forwarded: the API would say the same thing, and asking it a
      // question this interface already knows the answer to is a round trip for nothing.
      return fail(400, {
        fehler: 'Bitte ein Thema angeben. Die Themen dieser Seite wurden nicht geändert.',
        getippt: ''
      });
    }
    return setzeThemen({ params, request, fetch }, (jetzt) => [...spellings(jetzt), typed], typed);
  },

  /** Take one topic off this page. */
  themaEntfernen: async ({ params, request, fetch }) => {
    const form = await request.formData();
    const pfad = String(form.get('pfad') ?? '').trim();
    if (!pfad) {
      return fail(400, {
        fehler: 'Es wurde kein Thema genannt. Die Themen dieser Seite wurden nicht geändert.',
        getippt: ''
      });
    }
    // Matched on the canonical path, which is the topic's identity — never on the spelling,
    // where `Darm` and `darm` are one topic wearing two strings.
    return setzeThemen(
      { params, request, fetch },
      (jetzt) => spellings(jetzt.filter((topic) => topic.path !== pfad)),
      ''
    );
  }
};

/** The strings the API takes: what a file states, never the canonical path. */
function spellings(topics: Topic[]): string[] {
  return topics.map((topic) => topic.display_path);
}

/** The parts of an action event both topic actions read. */
interface TopicEvent {
  params: { path: string };
  request: Request;
  fetch: typeof fetch;
}

/**
 * Read this page's topics, decide the new set from them, and put it back.
 *
 * One function for both actions, so there is one place that knows the order of the two calls,
 * one wording for a refusal, and one place a change comes back to. Two copies of this would be
 * two chances for a removal and an addition to disagree about what "the whole set" is.
 */
async function setzeThemen(
  { params, request, fetch }: TopicEvent,
  next: (current: Topic[]) => string[],
  getippt: string
) {
  const cookie = request.headers.get('cookie');
  const path = `/${params.path}`;
  const endpoint = documentTopicsApiPath(path);

  let jetzt: Topic[];
  try {
    const answer = await apiGet<DocumentTopicsResponse>(fetch, endpoint, cookie);
    if (!answer.data) {
      return fail(answer.status === 0 ? 503 : answer.status, {
        fehler: describeSetTopics(answer.status, null),
        getippt
      });
    }
    jetzt = answer.data.topics ?? [];
  } catch {
    return fail(503, { fehler: describeSetTopics(0, null), getippt });
  }

  const { status, failure } = await apiSend(fetch, 'PUT', endpoint, cookie, {
    topics: next(jetzt)
  });

  if (failure) {
    // The status is passed through as the form's own, so a refusal is not reported as a 200
    // with a sad message in it. `failure.message` is what the API named — a 400 here says
    // which string it would not take and why, and dropping that turns a typo into "Fehler
    // 400", which is a refusal nobody can act on.
    return fail(status === 0 ? 503 : status, {
      fehler: describeSetTopics(status, failure.message),
      getippt
    });
  }

  // Post, redirect, get — so a reload does not offer to file the topic a second time. The
  // fragment is what makes the change ANNOUNCED rather than merely drawn: the browser moves
  // focus to the topics region, and a region that has just received focus is read out. A live
  // region already present in the document announces nothing. No script is involved, which is
  // the same mechanism `BOARD_NOTICE_ID` uses and the same reason.
  redirect(303, `${path}#${TOPICS_REGION_ID}`);
}
