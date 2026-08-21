import { error } from '@sveltejs/kit';
import { apiGet, type StoredDocument } from '$lib/api';
import {
  isView,
  selectPair,
  type RevisionDiff,
  type RevisionSource,
  type RevisionSummary
} from '$lib/history';
import type { PageServerLoad } from './$types';

/**
 * Everything the history page shows, decided here so that all of it arrives in the first
 * response.
 *
 * **The state is in the URL, not in the component.** Which two revisions are being compared,
 * which tab is open and whether a restore is being confirmed are all query parameters. That
 * buys three things at once: a comparison is a link somebody can send, the back button walks
 * back through what they looked at, and — because there is no DOM environment in this
 * project — every one of those states can be rendered and asserted by a test. German
 * parameter names, like `/graph`'s `?wurzel=`: this is a German interface and the address bar
 * is part of it.
 *
 * **Nothing here decides who may see anything.** `/api/documents` settles whether this page
 * may be read at all, and `/api/revisions/*` filters everything else through
 * `Store::document_for` — the same permission-checked accessor a page read uses. A `?von=`
 * naming a revision of another page is dropped by `selectPair` before it can be sent, which
 * is not a permission check but good manners: the API refuses it, and this page has no
 * business asking on a stranger's behalf.
 *
 * **One hazard, written down because it is invisible until it bites.** `[...path]/history` is
 * a literal segment beside a rest parameter, and SvelteKit prefers the literal — so a wiki
 * page that ever lives at a path ending in `/history` is shadowed by this route and answers
 * "Diese Seite gibt es nicht" instead of its own content. It is the same class of collision
 * `gw-api/src/routes/collab.rs` and `links.rs` document on the API side, where it was avoided
 * outright by putting the catch-all last; here the address is what the M3 plan specifies and
 * the URL space is the wiki's own, so there is no equivalent escape. Nothing in the corpus is
 * named that today. If a page ever is, this route's address has to move — not the page.
 */
function describe(status: number, clause: string): string {
  if (status === 0) return `${clause}: Die Anwendung antwortet nicht.`;
  if (status === 403) return `${clause}: Dafür fehlen die Rechte.`;
  if (status === 404) return `${clause}: Es gibt sie nicht (mehr).`;
  return `${clause} (Fehler ${status}).`;
}

export const load: PageServerLoad = async ({ params, fetch, request, url }) => {
  const cookie = request.headers.get('cookie');

  // The page first, and through the ordinary document endpoint: if it may not be read, its
  // history may not be read either, and the answer must be the same one the page itself
  // would have given rather than an empty timeline.
  const { status, data: doc } = await apiGet<StoredDocument>(
    fetch,
    `/api/documents/${params.path}`,
    cookie
  );
  if (status === 403) error(403, 'Diese Seite darfst du nicht lesen, ihren Verlauf also auch nicht.');
  if (!doc) error(404, 'Diese Seite gibt es nicht.');

  const { status: listStatus, data: list } = await apiGet<{ revisions: RevisionSummary[] }>(
    fetch,
    `/api/revisions/document/${params.path}`,
    cookie
  );
  const revisions = list?.revisions ?? [];
  // A failed request and a page with no history are different things and are never
  // conflated: "noch keine Fassungen" about a server that is down is a lie, and the same
  // distinction the graph page and the admin console both make.
  const listError = list ? null : describe(listStatus, 'Der Verlauf konnte nicht geladen werden');

  const { from, to } = selectPair(
    revisions,
    url.searchParams.get('von'),
    url.searchParams.get('bis')
  );

  const parameter = url.searchParams.get('ansicht');
  const view = isView(parameter) ? parameter : 'prosa';

  let diff: RevisionDiff | null = null;
  let diffError: string | null = null;
  if (from && to && from.id !== to.id) {
    const answer = await apiGet<RevisionDiff>(
      fetch,
      `/api/revisions/${from.id}/diff/${to.id}`,
      cookie
    );
    diff = answer.data;
    if (!diff) diffError = describe(answer.status, 'Der Vergleich konnte nicht geladen werden');
  }

  // Only for the tab that shows it. The source is rendered through the exporter's own
  // round-trip check, which is real work, and a reader looking at a prose diff never sees it.
  let source: RevisionSource | null = null;
  let sourceError: string | null = null;
  const selected = to ?? revisions[0] ?? null;
  if (view === 'quelle' && selected) {
    const answer = await apiGet<RevisionSource>(
      fetch,
      `/api/revisions/${selected.id}/source?path=${encodeURIComponent(doc.path)}`,
      cookie
    );
    source = answer.data;
    if (!source) sourceError = describe(answer.status, 'Der Quelltext konnte nicht geladen werden');
  }

  const confirming =
    revisions.find((revision) => revision.id === url.searchParams.get('wiederherstellen')) ?? null;

  return {
    doc,
    revisions,
    error: listError,
    from,
    to,
    view,
    diff,
    diffError,
    source,
    sourceError,
    confirming,
    // One instant for both renders. Ages are relative, and a server clock and a browser
    // clock would otherwise disagree about the same revision between the first response and
    // hydration — a mismatch on a value nobody would think to suspect.
    now: Date.now()
  };
};
