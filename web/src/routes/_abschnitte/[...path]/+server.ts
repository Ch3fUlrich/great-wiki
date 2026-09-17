import { json } from '@sveltejs/kit';
import { apiGet, parseBody, type DocumentView } from '$lib/api';
import { outline } from '$lib/blocks/render';
import type { RequestHandler } from './$types';

/**
 * The headings of one page, so the editor's embed dialog can offer a section to quote.
 *
 * # Why this exists at all, and why it is a proxy rather than an answer
 *
 * An embed may name one section of a page (D-27), anchored by the heading's **stable id** —
 * which is stored in that page's body and nowhere else. The dialog therefore has to read the
 * target's body, and the target is not the page being edited: the editor holds a Y.Doc of
 * *this* page and has never seen that one.
 *
 * So this asks `GET /api/documents/{path}` with the caller's own cookie and reduces the answer
 * to a list of headings. **It decides nothing.** The API's own authorisation is the whole
 * check — a page this caller may not read answers 403, one that does not exist answers 404,
 * and this hands both back untouched, exactly as the page route does. There is no unfiltered
 * variant it could reach for by mistake, and nothing here is a second answer to a permission
 * question: it is the same answer, with the words thrown away.
 *
 * Only `text` and `anchor` come back. The words are what a person picks by; the anchor is what
 * is stored. A heading with no anchor — a page that has not been published since stable ids
 * existed — is **left out rather than offered**, because offering it would mean writing an
 * embed with no anchor to write, and D-29's orphan frame is a state to reach by deletion, not
 * by construction.
 *
 * `_abschnitte` rather than `api`, for `_behaviour`'s and `_diagramm`'s reason: a literal
 * first segment wins over `[...path]`, so a segment that could be a real page's slug would
 * shadow that page.
 */
export const GET: RequestHandler = async ({ params, fetch, request }) => {
  const cookie = request.headers.get('cookie');
  const { status, data } = await apiGet<DocumentView>(
    fetch,
    `/api/documents/${params.path}`,
    cookie
  );
  if (!data) return json({ headings: [] }, { status });

  const headings = outline(parseBody(data))
    .filter((heading) => typeof heading.anchor === 'string')
    .map((heading) => ({ text: heading.text, level: heading.level, anchor: heading.anchor }));
  return json({ headings });
};
