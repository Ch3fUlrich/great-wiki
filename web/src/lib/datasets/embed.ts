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
 * interface that decided both would be a second way onto the data.
 */
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
