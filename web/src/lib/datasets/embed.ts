/**
 * The one answer a `dataset-view` block's embed read can come back as, read as data.
 *
 * `GET /api/datasets/embed/{dataset_doc}/{view}` returns exactly one of three things and
 * the whole job of this file is to trust that shape and nothing else: what the block renders
 * is decided from a value that has already survived being checked against it. See
 * `crates/gw-store/src/datasets/embed.rs` for the server's side and ADR 0022 for why every
 * refusal is the same bytes; this is the reader's half of the same contract.
 *
 * **Server-only since M8d D2W.** The read is made by `+page.server.ts` under the reader's
 * own cookie — a browser cannot make it, because `apiGet` carries the server-only API base
 * and the `X-GW-Proxy` attestation from private env. Anything that wants one of these values
 * in a component imports the *type* only (`import type { EmbedState }`), which leaves no
 * runtime edge to this module and keeps the whole thing out of the client bundle. `parseEmbedAnswer`,
 * `embedApiPath` and `embeddable` stay pure and independently testable; `resolveEmbedOnServer`
 * is the only part that reaches the network.
 */
import { apiGet } from '$lib/api';
import { loadDataset, type DatasetLoad } from './load';
import type { DatasetView } from './views';

/**
 * What the embed endpoint answered, narrowed to the three states it can be in. `ok` carries
 * the resolved page, the title and the saved view; the other two carry nothing, because what
 * a refusal may not say is the point of it (ADR 0022).
 */
export type EmbedAnswer =
  | { state: 'ok'; path: string; title: string; view: DatasetView }
  | { state: 'view_missing' }
  | { state: 'unavailable' };

/** The one value every malformed input collapses to, so a bad answer can never leak detail. */
const UNAVAILABLE: EmbedAnswer = { state: 'unavailable' };

/** A non-empty string, or nothing at all. */
const str = (value: unknown): string | null =>
  typeof value === 'string' && value !== '' ? value : null;

/**
 * The block's own address: the two ids the reader found in the page body, percent-encoded
 * into path segments. `encodeURIComponent` leaves `.` alone but escapes `/`, `?` and `#`, so
 * an id carrying them — which is an id the server will refuse as `unavailable` — cannot
 * escape the one segment it belongs to or turn the query into another request.
 */
export function embedApiPath(datasetDoc: string, view: string): string {
  return `/api/datasets/embed/${encodeURIComponent(datasetDoc)}/${encodeURIComponent(view)}`;
}

/**
 * Whether a view kind is safe to draw inside somebody else's page. `table`, `board`,
 * `calendar` and `gallery` are read-only presentations of rows the caller already has a
 * claim on. `form` is not: it writes, and a form in a quoted page would put a control onto a
 * dataset the reader may not even see. A `timeline` does not exist yet and is refused on the
 * same rule as anything unknown — a kind this build has not reasoned about is not embeddable.
 */
const EMBEDDABLE = new Set(['table', 'board', 'calendar', 'gallery']);
export function embeddable(kind: string): boolean {
  return EMBEDDABLE.has(kind);
}

/**
 * The endpoint's JSON read as an {@link EmbedAnswer}, or `unavailable` for anything that is
 * not one. It never throws and never trusts a field it did not check: not an object, an
 * unknown `state`, an `ok` with no string `path` or `title`, or an `ok` whose `view` is not an
 * object with a string `id` and `kind` all map to the uniform refusal. That is deliberate —
 * a malformed answer is indistinguishable from a refusal to the reader, and treating it as
 * anything else would let a broken response become a signal about what the wiki holds.
 */
export function parseEmbedAnswer(json: unknown): EmbedAnswer {
  if (typeof json !== 'object' || json === null || Array.isArray(json)) return UNAVAILABLE;
  const answer = json as Record<string, unknown>;
  if (answer.state === 'view_missing') return { state: 'view_missing' };
  if (answer.state !== 'ok') return UNAVAILABLE;

  const path = str(answer.path);
  const title = typeof answer.title === 'string' ? answer.title : null;
  const rawView = answer.view;
  if (path === null || title === null || typeof rawView !== 'object' || rawView === null) {
    return UNAVAILABLE;
  }
  const view = rawView as Record<string, unknown>;
  const id = str(view.id);
  const kind = str(view.kind);
  if (id === null || kind === null) return UNAVAILABLE;

  return {
    state: 'ok',
    path,
    title,
    view: {
      id,
      name: typeof view.name === 'string' ? view.name : '',
      kind,
      config: view.config,
      position: typeof view.position === 'number' ? view.position : 0
    }
  };
}

/**
 * What a `dataset-view` embed shows, as one value the component renders from. There is no
 * `loading` state any more (M8d D2W): the read settles before the markup exists, and a
 * block with no resolution at all is handed `{ status: 'unavailable' }` by the renderer —
 * the same value a refusal produces, so an unresolved frame cannot be told from an
 * unreadable one.
 */
export type EmbedState =
  | { status: 'unavailable' }
  | { status: 'view_missing' }
  /** `ok`, but the view kind is not one that may be embedded (a form, or an unknown kind). */
  | { status: 'not_embeddable' }
  /** `ok` and embeddable, with the rows read under the reader's own identity. */
  | { status: 'ready'; path: string; load: DatasetLoad }
  /** The request itself failed — a transport error, a non-2xx, a body that is not JSON. */
  | { status: 'error' };

/**
 * The server's whole embed read for one block: fetch the answer under the reader's cookie,
 * decide what it may show, and — only for an embeddable view the reader is entitled to —
 * load its rows, again under that same cookie. It returns an {@link EmbedState} and never
 * throws: an embed that cannot be answered is a state the frame draws, never a failed page.
 *
 * The order is the security rule. The answer's `state` is decided before any rows are asked
 * for, and every call carries exactly the READER's cookie — the dataset's ACL decides what
 * may be shown, not the embedding page's, and no service credential or page principal
 * travels on these requests. A `unavailable` therefore loads nothing and its state value
 * carries no title, no path and no row: two different reasons for the same refusal produce
 * a byte-identical result, because there is nothing in it to differ.
 */
export async function resolveEmbedOnServer(
  fetchFn: typeof fetch,
  cookie: string | null,
  datasetDoc: string,
  view: string
): Promise<EmbedState> {
  let answer: EmbedAnswer;
  try {
    const { status, data } = await apiGet<unknown>(
      fetchFn,
      embedApiPath(datasetDoc, view),
      cookie
    );
    if (status < 200 || status >= 300) return { status: 'error' };
    answer = parseEmbedAnswer(data);
  } catch {
    return { status: 'error' };
  }

  if (answer.state === 'unavailable') return { status: 'unavailable' };
  if (answer.state === 'view_missing') return { status: 'view_missing' };
  if (!embeddable(answer.view.kind)) return { status: 'not_embeddable' };

  try {
    const load = await loadDataset(
      fetchFn,
      answer.path,
      new URLSearchParams({ view: answer.view.id }),
      cookie
    );
    return { status: 'ready', path: answer.path, load };
  } catch {
    return { status: 'error' };
  }
}
