import { ANONYMOUS, apiGet, type Me, type TreeNode } from '$lib/api';
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
 * **This load deliberately does not read `url`.** SvelteKit re-runs a server load whose
 * dependencies changed, and touching `url` here would make both of these requests happen
 * again on every navigation within the workspace. The part that *does* depend on the
 * address — which tabs are open — is in `+layout.ts` instead, where it costs no round
 * trip at all.
 */
export const load: LayoutServerLoad = async ({ fetch, request }) => {
  const cookie = request.headers.get('cookie');

  const [me, tree] = await Promise.all([
    apiGet<Me>(fetch, '/api/me', cookie)
      .then((answer) => answer.data ?? ANONYMOUS)
      .catch(() => ANONYMOUS),
    apiGet<TreeNode[]>(fetch, '/api/tree', cookie)
      .then((answer) => answer.data ?? [])
      .catch(() => [])
  ]);

  return { me, tree };
};
