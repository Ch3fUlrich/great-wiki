import { error, fail, redirect } from '@sveltejs/kit';
import { apiGet, apiSend, apiUpload, parseBody, type Backlink, type DocumentView } from '$lib/api';
import {
  attachmentApiPath,
  attachmentsApiPath,
  describeAttachments,
  describeUpload,
  ATTACHMENTS_REGION_ID,
  UPLOADED_PARAM,
  type Attachment,
  type AttachmentsResponse
} from '$lib/attachments';
import {
  describeDelete,
  documentApiPath,
  DELETE_PARAM,
  DELETED_PARAM,
  TRASH_PATH,
  TRASH_REGION_ID
} from '$lib/trash';
import { boardPath, describeEmbeddedBoard, noticeFor, type BoardResponse } from '$lib/board';
import { typesetDocument } from '$lib/server/maths';
import { highlightDocument } from '$lib/server/highlight';
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

  const body = parseBody(data);

  /**
   * The page's ` ```math ` fences, typeset by KaTeX — **here, and not in the component that
   * draws them.**
   *
   * A Svelte component renders on the server and again in the browser while it hydrates, so
   * a component that called KaTeX would put the whole library — about 272 kB — into every
   * reader's bundle in order to re-derive markup that reader was already sent. Doing it in
   * this load puts KaTeX under `$lib/server/`, which SvelteKit refuses to let any
   * client-reachable module import: the guarantee that no maths library reaches a reader is
   * a build error rather than a promise. The formula is in the first response, typeset, and
   * works with JavaScript switched off.
   *
   * It never fails the page. Every call is capped and caught inside `typesetDocument` —
   * this is the shared server, and `Store::open` holds `max_connections(1)`, so a slow load
   * is a lever on the whole deployment rather than on one tab. A formula that could not be
   * set renders as its own source with a line saying which limit stopped it.
   */
  const formeln = typesetDocument(body);

  /**
   * The page's fenced code blocks, tokenised by Shiki — **here, and not in the component
   * that draws them**, for both of `formeln`'s reasons and a third.
   *
   * The library and its eight grammars are 609 kB raw. A component that called the
   * highlighter would render on the server and again while hydrating, so every reader of
   * every page — this wiki is overwhelmingly prose — downloaded all of it and re-derived
   * tokens they had already been sent, on their own main thread. `$lib/server/` is what
   * makes that a build error rather than a promise.
   *
   * And the caps only mean something here. Per-fence limits cannot see how many fences a
   * page has: measured before this moved, a page of five 20 000-character fences answered
   * in 51.98 s and held every other reader's page load behind it for the same time, on a
   * server whose `Store::open` keeps `max_connections(1)`. `highlightDocument` bounds the
   * page rather than the block, and never fails it: over a limit, a fence renders as
   * ordinary code with a line saying which one.
   */
  const codeBloecke = highlightDocument(body);

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

  // D-15's list: what this page carries besides its words.
  //
  // **The list is the authority on what is attached**, and nothing is derived from the
  // document's body — cutting a picture out of a paragraph leaves the file exactly where it
  // was, which is the whole reason there is a list at all. `gw_store::attachments` states the
  // same rule from the other side.
  //
  // **One request, deciding nothing.** `GET /api/attachments/{path}` authorises the list
  // through the same body a page read ends in, and answers `may_write` off that very
  // authorisation (ADR 0010) — so the control the section offers and the refusal that would
  // follow pressing it are one verdict rather than two that agree today. Neither number is
  // recomputed here and there is nowhere for a second answer to arrive.
  //
  // A failure is stated rather than rendered as "this page carries nothing", and never fails
  // the page: attachments are an addition to a page, exactly as the board and the chips are.
  let anhaenge: Attachment[] = [];
  let anhaengeDarfSchreiben = false;
  let anhaengeFehler: string | null = null;
  try {
    const answer = await apiGet<AttachmentsResponse>(fetch, attachmentsApiPath(data.path), cookie);
    if (answer.data) {
      anhaenge = answer.data.attachments ?? [];
      // `=== true`, not `!== false`: an API that says nothing is an API this cannot ask, and a
      // control offered on a missing field is a control offered on a guess. Fail closed —
      // AGENTS.md rule 3.
      anhaengeDarfSchreiben = answer.data.may_write === true;
    } else {
      anhaengeFehler = describeAttachments(answer.status);
    }
  } catch {
    anhaengeFehler = describeAttachments(0);
  }

  /**
   * The file that has just been attached, named in the address by the action that attached it.
   *
   * Matched against the listing above rather than trusted from the query string — the
   * discipline `/papierkorb` applies to its own `?geloescht=`. A notice saying a file is
   * attached while the list beside it does not show that file is the interface contradicting
   * its own data, and it is the LIST that is the authority (D-15).
   */
  const angekommen = url.searchParams.get(UPLOADED_PARAM);

  // Read here rather than from `$app/state` in the component, for two reasons: the flag is
  // then part of the page's data and a server-render test can set it, and the component
  // does not have to reach for a SvelteKit runtime that only exists inside a request.
  //
  // It asks for the editor; it does not decide anything. Whether this caller may actually
  // write is settled by the collaboration socket, which is the only thing that knows.
  return {
    doc: data,
    body,
    formeln,
    fences: codeBloecke,
    // The tree is NOT fetched here any more: the shell renders it on every view, so
    // `+layout.server.ts` asks for it once and this page reads the same answer through the
    // merged `data`. Two requests for one filtered tree was two chances for the breadcrumb
    // and the sidebar to disagree about which pages exist.
    backlinks: backlinks?.backlinks ?? [],
    seitenThemen,
    seitenThemenFehler,
    anhaenge,
    anhaengeDarfSchreiben,
    anhaengeFehler,
    // Matched against the listing that was just read — see `angekommen` above.
    hochgeladen: angekommen
      ? (anhaenge.find((anhang) => anhang.filename === angekommen) ?? null)
      : null,
    edit: url.searchParams.get('edit') === '1',
    // The question before a delete, asked in the address so the page renders it in the first
    // response — no dialog waiting for a bundle, and a state a test can assert with no DOM.
    // See `$lib/trash`'s `deleteHref`.
    loeschen: url.searchParams.get(DELETE_PARAM) === '1',
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
        wo: 'thema' as const,
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
        wo: 'thema' as const,
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
  },

  /**
   * Put this page in the Papierkorb — the owner's first decision, and the reason it lives
   * here rather than in an administrative console: deleting happens where the page is.
   *
   * **`DELETE` on the document itself**, not a fifth route under `/api/trash`. It is the same
   * resource `GET /api/documents/{path}` reads and the verb already says which operation this
   * is; `gw_api::routes::trash`'s header records that choice.
   *
   * **The page takes everything under it**, which is stated in the question this posts from
   * rather than discovered afterwards. `Store::tree` matches each row's parent against a
   * parent it has already emitted, so a live page left under a deleted one is not filtered out
   * of the navigation — it is unreachable in it, absent from the export, and still readable at
   * its own address. The subtree therefore moves as one, and the price is that a delete needs
   * write on every page that moves.
   *
   * **Nothing here decides whether the caller may do this.** `Store::trash_document` needs
   * write on every page in the subtree and a signed-in, active account; this action turns its
   * answer into a German sentence and nothing more. The control is offered on `may_write`
   * composed with `authenticated`, which is the same pair — so the offer and the refusal
   * cannot disagree about the ordinary case, and the one they can disagree about (a subpage
   * that is somebody else's) comes back as a 409 that says so.
   *
   * **A success goes to the Papierkorb, not back here.** This address answers 404 now. The
   * Papierkorb is where the page is and where it can be brought back from, and landing there
   * is what makes the delete feel undoable rather than final — which it is.
   */
  loeschen: async ({ params, request, fetch }) => {
    const path = `/${params.path}`;
    const { status, failure } = await apiSend(
      fetch,
      'DELETE',
      documentApiPath(path),
      request.headers.get('cookie')
    );

    if (failure) {
      // `fail()` re-renders the route that owns the action, and that route is this page — the
      // one the reader is standing on, which still exists precisely because the delete did
      // not happen. `wo` is what keeps this refusal out of the topic field, which is a
      // different control with a different permission and its own sentence.
      return fail(status === 0 ? 503 : status, {
        wo: 'loeschen' as const,
        fehler: describeDelete(status, failure.message),
        getippt: ''
      });
    }

    redirect(
      303,
      `${TRASH_PATH}?${DELETED_PARAM}=${encodeURIComponent(path)}#${TRASH_REGION_ID}`
    );
  },

  /**
   * Attach a file to this page — D-15's `Anhänge` list, added to.
   *
   * **A real multipart form submission, unpacked here and forwarded as bytes.** The API takes
   * the file and nothing else: no multipart, no JSON envelope, no declared type, with the name
   * in the address. `gw_api::routes::attachments` gives the reason and it is not minimalism —
   * a `Content-Type` in the request is a type the *uploader* chose, and the only way to be
   * sure it is never echoed back is for there to be nowhere to put one. So the browser's
   * envelope stops here and `apiUpload` sends the bytes on.
   *
   * **Nothing here decides whether the caller may do this.** `Store::attach` needs write on
   * the page **and** a signed-in, active account — the account first, before it consults a
   * single grant, because the row records who put the file there. This action turns its answer
   * into a German sentence and nothing more. The control is offered on the pair, which is the
   * same pair, so the offer and the refusal cannot disagree.
   *
   * **Nothing here decides which files are acceptable either**, and that is deliberate:
   * `gw_store::blobs::sniff` owns the allowlist, `MAX_ATTACHMENT_BYTES` owns the cap, and both
   * are the server's to change. A copy of either in this file would be a second answer that
   * refuses a file the wiki would have taken — before the request is made, with nothing in any
   * log to say why. What is refused here is only what needs no server to know: no file chosen,
   * and a file with no bytes in it.
   *
   * **A refusal comes back as `fail()`**, not as a redirect, because the reader is standing on
   * the page that still carries exactly what it carried before. `wo: 'anhang'` is what keeps
   * that sentence out of the topic field, which is a different control with a different
   * permission and its own wording.
   *
   * **THE DEPLOYMENT NOTE THIS FEATURE CANNOT ENFORCE FROM HERE.** `@sveltejs/adapter-node`
   * refuses a request body over `BODY_SIZE_LIMIT`, which **defaults to 512 kB**, before
   * SvelteKit routes it — so in a container this action never runs for anything larger and the
   * reader gets the adapter's own error page rather than the German sentence below. The dev
   * server applies no such limit, so `just behaviour` cannot see it either. The API's own cap
   * is 250 MB (D-17), and the compose file must set `BODY_SIZE_LIMIT` on `gw-web` to at least
   * that for the two to agree.
   */
  anhaengen: async ({ params, request, fetch }) => {
    const form = await request.formData();
    const datei = form.get('datei');
    const name = datei instanceof File ? datei.name.trim() : '';

    if (!(datei instanceof File) || name === '') {
      // Refused here rather than forwarded: the API would say the same thing, and asking it a
      // question this interface already knows the answer to is a round trip for nothing.
      return fail(400, {
        wo: 'anhang' as const,
        fehler: 'Bitte eine Datei auswählen. Es wurde nichts angehängt.',
        getippt: ''
      });
    }
    if (datei.size === 0) {
      // The one property of the file itself this interface may judge, because it is not a
      // policy: there is nothing to attach. `Store::attach` would answer the same, in English.
      return fail(400, {
        wo: 'anhang' as const,
        fehler: `»${name}« enthält keine Daten. Es wurde nichts angehängt.`,
        getippt: ''
      });
    }

    const path = `/${params.path}`;
    const { status, failure } = await apiUpload(
      fetch,
      attachmentApiPath(path, name),
      request.headers.get('cookie'),
      datei
    );

    if (failure) {
      // The status is passed through as the form's own, so a refusal is not reported as a 200
      // with a sad message in it. `failure.message` is what the API named — for a 415 that is
      // which types this wiki stores, for a 413 the size it stops at, and for a 409 the name
      // that is already taken. Dropping any of them turns a refusal that names the way out
      // into "Fehler 415", which is a refusal nobody can act on.
      return fail(status === 0 ? 503 : status, {
        wo: 'anhang' as const,
        fehler: describeUpload(status, failure.message),
        getippt: ''
      });
    }

    // Post, redirect, get — so a reload does not offer to attach the file a second time (and
    // the API would refuse the second one by name, which is a confusing way to learn that).
    // The fragment is what makes the change ANNOUNCED rather than merely drawn: the browser
    // moves focus to the section and a region that has just received focus is read out. A live
    // region already present in the document announces nothing, and no script is involved.
    redirect(
      303,
      `${path}?${UPLOADED_PARAM}=${encodeURIComponent(name)}#${ATTACHMENTS_REGION_ID}`
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
        wo: 'thema' as const,
        fehler: describeSetTopics(answer.status, null),
        getippt
      });
    }
    jetzt = answer.data.topics ?? [];
  } catch {
    return fail(503, { wo: 'thema' as const, fehler: describeSetTopics(0, null), getippt });
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
      wo: 'thema' as const,
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
