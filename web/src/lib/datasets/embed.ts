/**
 * The one answer a `dataset-view` block's embed read can come back as, read as data.
 *
 * `GET /api/datasets/embed/{dataset_doc}/{view}` returns exactly one of three things and
 * the whole job of this file is to trust that shape and nothing else: what the block renders
 * is decided from a value that has already survived being checked against it. See
 * `crates/gw-store/src/datasets/embed.rs` for the server's side and ADR 0022 for why every
 * refusal is the same bytes; this is the reader's half of the same contract.
 *
 * Pure — no fetch, no DOM, no `$lib/api`. Nothing here reaches the network or reads a page:
 * the rows are a separate read (`loadDataset`), made under the caller's own identity, and an
 * interface that decided both would be a second way onto the data. {@link resolveEmbed} below
 * does orchestrate a read, but it is handed the fetch and the loader rather than importing
 * them, so this module stays free of the server-only API layer and every one of its
 * decisions can be tested against a stand-in.
 */
import type { DatasetLoad } from './load';
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
 * What a `dataset-view` embed shows, as one value the component renders from. `EmbeddedView`
 * starts at `loading` (which is also all SSR ever emits — the fetch is client-only) and the
 * effect replaces it once the read settles.
 */
export type EmbedState =
  | { status: 'loading' }
  | { status: 'unavailable' }
  | { status: 'view_missing' }
  /** `ok`, but the view kind is not one that may be embedded (a form, or an unknown kind). */
  | { status: 'not_embeddable' }
  /** `ok` and embeddable, with the rows read under the caller's own identity. */
  | { status: 'ready'; path: string; load: DatasetLoad }
  /** The request itself failed — a transport error, a non-2xx, a body that is not JSON. */
  | { status: 'error' };

/** The row read: `loadDataset`'s own shape, injected so this module never imports `$lib/api`. */
type Loader = (
  fetchFn: typeof fetch,
  path: string,
  params: URLSearchParams,
  cookie: string | null
) => Promise<DatasetLoad>;

/**
 * The client's whole embed read: fetch the answer, decide what it may show, and — only for an
 * embeddable view the caller is entitled to — load its rows. It returns an {@link EmbedState}
 * and touches no module-level fetch or API: the caller hands it both, so a test can drive every
 * branch with a stand-in and the server bundle never reaches here.
 *
 * The order is the security rule. The answer's `state` is decided before any rows are asked
 * for, and the rows are read through `load` under the CALLER's identity — the embed endpoint
 * deliberately carries none (ADR 0022). A `unavailable` therefore loads nothing and its state
 * value carries no title, no path and no row: two different reasons for the same refusal
 * produce a byte-identical result, because there is nothing in it to differ.
 *
 * `signal` is the caller's abort: an in-flight read whose props moved on is stopped, and its
 * rejection is reported as `error` for the component to discard on its own staleness token.
 */
export async function resolveEmbed(
  fetchFn: typeof fetch,
  datasetDoc: string,
  view: string,
  load: Loader,
  signal?: AbortSignal
): Promise<EmbedState> {
  let answer: EmbedAnswer;
  try {
    const res = await fetchFn(embedApiPath(datasetDoc, view), { signal });
    if (!res.ok) return { status: 'error' };
    answer = parseEmbedAnswer(await res.json());
  } catch {
    return { status: 'error' };
  }

  if (answer.state === 'unavailable') return { status: 'unavailable' };
  if (answer.state === 'view_missing') return { status: 'view_missing' };
  if (!embeddable(answer.view.kind)) return { status: 'not_embeddable' };

  try {
    const loaded = await load(fetchFn, answer.path, new URLSearchParams({ view: answer.view.id }), null);
    return { status: 'ready', path: answer.path, load: loaded };
  } catch {
    return { status: 'error' };
  }
}
