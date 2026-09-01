import { ANONYMOUS, apiGet, type Me, type TreeNode } from '$lib/api';
import {
  describeTopics,
  TOPICS_ENDPOINT,
  type TopicSummary,
  type TopicsResponse
} from '$lib/topics';
import type { LayoutServerLoad } from './$types';

/**
 * What the shell is made of: who is signed in, and the page tree.
 *
 * **Neither failure may take a page down.** If the API is unreachable the interface shows
 * nobody signed in and an empty tree, which is both true and harmless. It would be the
 * wrong trade to fail a public page because the identity endpoint blinked — and `/api/me`
 * is reporting, never deciding, so an anonymous fallback cannot grant anything. The tree
 * is the same: it is navigation, never a precondition for the page beside it.
 *
 * **The tree moved here from the two pages that used to fetch it**, because it is now
 * shell furniture: the sidebar renders it on every view, not only on a document. One
 * request per page render rather than two, and one place that knows how a failure is
 * rendered. It is already filtered in the retriever to what this caller may read
 * (AGENTS.md rule 2) — nothing here filters it again, and nothing here may.
 *
 * **The topics are here for the same reason the tree is, and for one more.** The sidebar can
 * show either the page tree or the topics — the owner put browsing by topic in the shell as
 * well as on a page of its own — so the index is shell furniture exactly as the tree is. The
 * one more reason is the one that matters: `/themen`, the sidebar and the suggestion list
 * beside every page's chips are **one query rendered three times**, not three requests. ADR
 * 0011 makes a topic's name the disclosure and says in as many words that an autocomplete is
 * a disclosure surface; a second endpoint call for "just the topics, to suggest from" is
 * precisely the one that would get written without the filter. There is none, and
 * `layout.server.test.ts` counts the requests rather than trusting this paragraph.
 *
 * **This load deliberately does not read `url`.** SvelteKit re-runs a server load whose
 * dependencies changed, and touching `url` here would make all three of these requests happen
 * again on every navigation within the workspace. The parts that *do* depend on the
 * address — which tabs are open, and which half of the sidebar is showing — are in
 * `+layout.ts` instead, where they cost no round trip at all.
 */
export const load: LayoutServerLoad = async ({ fetch, request }) => {
  const cookie = request.headers.get('cookie');

  const [me, tree, topics] = await Promise.all([
    apiGet<Me>(fetch, '/api/me', cookie)
      .then((answer) => answer.data ?? ANONYMOUS)
      .catch(() => ANONYMOUS),
    apiGet<TreeNode[]>(fetch, '/api/tree', cookie)
      .then((answer) => answer.data ?? [])
      .catch(() => []),
    // A failure here is *stated*, not swallowed into an empty list: a wiki nobody has filed
    // anything in and a request that did not come back are different things, and "Keine
    // Themen" about a server that is down is the lie every other view here refuses to tell.
    // It still never fails the page — the sidebar's second half is an addition to the shell,
    // never a precondition for the view inside it.
    apiGet<TopicsResponse>(fetch, TOPICS_ENDPOINT, cookie)
      .then((answer) =>
        answer.data
          ? { themen: answer.data.topics ?? [], themenFehler: null }
          : { themen: [] as TopicSummary[], themenFehler: describeTopics(answer.status) }
      )
      .catch(() => ({ themen: [] as TopicSummary[], themenFehler: describeTopics(0) }))
  ]);

  return { me, tree, ...topics };
};
