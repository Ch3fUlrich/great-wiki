import { fail, redirect } from '@sveltejs/kit';
import { apiGet, apiSend, type ApiFailure } from '$lib/api';
import {
  describeCreate,
  describeDelete,
  describeList,
  homePath,
  type Project,
  type ProjectsResponse
} from '$lib/projects';
import type { Actions, PageServerLoad } from './$types';

/**
 * The projects page (D-13): the list of projects, and the form that makes a page the home of
 * a new one.
 *
 * **This page is an aggregate view, and it does not filter anything.** `/api/projects` answers
 * only the projects whose home page the caller may read — per document, because D-3 makes a
 * project span pages with different grants by design — and that filtering belongs to
 * `Store::projects_for`, where it is mutation-tested. There is exactly one request here for
 * exactly that reason: a second source of projects would be a second answer to "which
 * projects exist", and every row is a disclosure surface, so a second answer is also a second
 * chance to leak. The same property, stated for boards, is the whole first half of
 * `gw_api::routes::tasks`.
 *
 * The corollary that is easy to get wrong: **no count, and no "N ausgeblendet" hint.** A
 * number here would be a number about pages the reader may not read, which is the one thing
 * the filtering cannot take back. `server.test.ts` pins the single request; `page.test.ts`
 * pins the absence of a count.
 *
 * **Everything works before hydration.** Creating and deleting are real form actions, not
 * click handlers: the browser's own submission carries the cookie, the answer is a redirect
 * back to the list, and a refusal comes back as `fail()` and is rendered into the page. This
 * repository already says why that matters, about its own edit link — a control that needs
 * JavaScript to arrive is a control that looks live and does nothing.
 *
 * **State is in the URL.** Which deletion is being confirmed (`?loeschen=`) and which project
 * was just created (`?angelegt=`) are query parameters, German like `/graph`'s `?wurzel=`,
 * which buys the same three things it does there: the question is a link, the back button
 * walks through it, and — because there is no DOM environment in this project — every state
 * can be rendered and asserted by a test.
 *
 * **One hazard, the same one `[...path]/history` writes down.** `/projekte` is a literal
 * segment and SvelteKit prefers it over `[...path]`, so a wiki page that ever lives at
 * `/projekte` is shadowed by this route and answers this page instead of its own content.
 * Nothing in `content-example` is named that. If a page ever is, this route's address has to
 * move — not the page.
 */
export const load: PageServerLoad = async ({ fetch, request, url }) => {
  const cookie = request.headers.get('cookie');

  let projects: Project[] = [];
  let error: string | null = null;
  try {
    const answer = await apiGet<ProjectsResponse>(fetch, '/api/projects', cookie);
    // A failed request and a wiki with no projects are different things and are never
    // conflated: "hier ist kein Projekt zu sehen" about a server that is down is a lie. The
    // graph page and the admin console both make the same distinction.
    if (answer.data) projects = answer.data.projects;
    else error = describeList(answer.status);
  } catch {
    // `apiGet` throws when the request never got an answer at all.
    error = describeList(0);
  }

  // Both of these are matched against the list rather than trusted from the address bar. The
  // list is already filtered, so matching against it is what stops a hand-typed id turning
  // the URL into a second way to ask "is there a project on this page" — the API would refuse
  // the follow-up anyway, but a page that renders the question has already answered it.
  const wanted = url.searchParams.get('loeschen');
  const confirming = projects.find((project) => project.id === wanted) ?? null;

  const justCreated = url.searchParams.get('angelegt');
  const created = projects.find((project) => project.home_path === justCreated) ?? null;

  return { projects, error, confirming, created };
};

/** The path a refusal or a success comes back to. */
const LIST = '/projekte';

/** What a refused write said, when this file has no wording of its own for the status. */
function said(failure: ApiFailure | null): string | null {
  return failure?.message ?? null;
}

export const actions: Actions = {
  /**
   * Make a page the home of a new project.
   *
   * Post, redirect, get. The redirect is what keeps a reload from offering to create the
   * project a second time, and it carries the path rather than a flag so the loader can
   * confirm the creation against the list it just read instead of against the address bar.
   *
   * Nothing here decides whether the caller may do this. `POST /api/projects` needs Write on
   * the page and asks `Store::document_for` for it; this action turns its answer into a
   * German sentence and nothing more. The form is offered to anybody signed in for the same
   * reason the edit link is (see `[...path]/+page.svelte`): no field on the wire says "may I
   * write this page", so the offer can be false and the creation cannot.
   */
  anlegen: async ({ request, fetch }) => {
    const form = await request.formData();
    const typed = String(form.get('startseite') ?? '');
    const path = homePath(typed);

    if (!path) {
      return fail(400, {
        wo: 'anlegen' as const,
        fehler:
          'Bitte den Pfad der Startseite angeben — die Seite, der das Projekt gehört, ' +
          'zum Beispiel /rundgang/tabellen.',
        startseite: typed
      });
    }

    const { status, failure } = await apiSend(
      fetch,
      'POST',
      '/api/projects',
      request.headers.get('cookie'),
      { home_path: path }
    );

    if (failure) {
      // The status is passed through as the form's own, so a refusal is not reported as a
      // 200 with a sad message in it.
      return fail(status === 0 ? 503 : status, {
        wo: 'anlegen' as const,
        fehler: describeCreate(status, path, said(failure)),
        startseite: path
      });
    }

    redirect(303, `${LIST}?angelegt=${encodeURIComponent(path)}`);
  },

  /**
   * Delete a project.
   *
   * Its own standalone cards go with it — the foreign key in `0010_tasks.sql` — and the tasks
   * written into pages of the home subtree do not: those are governed by their own pages and
   * keep existing without a board. The confirmation says both, because "delete a project" is
   * otherwise a sentence somebody has to guess the scope of.
   */
  loeschen: async ({ request, fetch }) => {
    const form = await request.formData();
    const id = String(form.get('id') ?? '').trim();
    if (!id) {
      return fail(400, {
        wo: 'loeschen' as const,
        fehler: 'Es wurde kein Projekt genannt. Es wurde nichts gelöscht.',
        startseite: ''
      });
    }

    const { status, failure } = await apiSend(
      fetch,
      'DELETE',
      `/api/projects/${encodeURIComponent(id)}`,
      request.headers.get('cookie')
    );

    if (failure) {
      return fail(status === 0 ? 503 : status, {
        wo: 'loeschen' as const,
        fehler: describeDelete(status, said(failure)),
        startseite: ''
      });
    }

    redirect(303, LIST);
  }
};
