import { error } from '@sveltejs/kit';
import { apiGet } from '$lib/api';
import {
  describeTopic,
  topicApiPath,
  topicPathFromRoute,
  type TopicPageResponse
} from '$lib/topics';
import type { PageServerLoad } from './$types';

/**
 * One topic: the pages filed under it, and the topics inside it.
 *
 * D-4 made topics invisible in the graph and named the consequence — a topic page listing its
 * documents is the ONLY way topics are reachable — so this is the view that decision was
 * about.
 *
 * **It filters nothing, and it must not.** `GET /api/topics/tagged/{path}` answers only the
 * documents whose page the caller may read, per document, and that belongs to
 * `Store::topic_for`, where it is mutation-tested. One request, for the reason `/projekte`
 * writes down about its own: every row here says that a page exists and what it is called, so
 * a second source of rows would be a second chance to disclose one.
 *
 * **Listing a topic means that topic AND everything inside it.** That is not this file's
 * decision and is not re-derived here: `gw-store/src/topics.rs` records why, and the short
 * version is that a `Medizin` showing two documents while forty sat under `Medizin/Darm` would
 * be a browsing dead end for somebody with no other route to those forty.
 *
 * **404 is the whole of the disclosure rule at this layer.** ADR 0011: a topic the caller may
 * see no page of answers exactly as a topic nobody ever typed, because a refusal that differed
 * from an absence would confirm the name — and a topic's name is the thing being kept back.
 * The API conflates them deliberately; this loader passes that through untouched and its
 * message is a sentence that is true of both. A 500 is NOT folded into it: a server that is
 * down and a topic that is not there are different things, and saying the second about the
 * first is the lie every other view here refuses to tell.
 */
export const load: PageServerLoad = async ({ params, fetch, request }) => {
  const cookie = request.headers.get('cookie');
  const path = topicPathFromRoute(params.pfad);

  let status: number;
  let data: TopicPageResponse | null;
  try {
    ({ status, data } = await apiGet<TopicPageResponse>(fetch, topicApiPath(path), cookie));
  } catch {
    // `apiGet` throws when the request never got an answer at all. 503 rather than 500,
    // because "not reachable" and "answered with an error" send somebody to different
    // places — the same split `$lib/api` makes with its status 0.
    error(503, describeTopic(0));
  }

  if (!data) {
    // `error()` accepts 400–599 only, and `apiGet` reports anything that is not 2xx — so a
    // redirect from the API, which this endpoint never sends, would otherwise fail inside
    // SvelteKit with a message about the status code rather than about the topic. The real
    // number stays in the sentence either way.
    error(status >= 400 && status <= 599 ? status : 502, describeTopic(status));
  }

  return { thema: data };
};
