import { error } from '@sveltejs/kit';
import { apiGet, parseBody, type Backlink, type StoredDocument } from '$lib/api';
import { boardPath, describeEmbeddedBoard, noticeFor, type BoardResponse } from '$lib/board';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, fetch, request, url }) => {
  const cookie = request.headers.get('cookie');
  const { status, data } = await apiGet<StoredDocument>(
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
