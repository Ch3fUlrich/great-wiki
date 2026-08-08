import { ANONYMOUS, apiGet, type Me } from '$lib/api';
import type { LayoutServerLoad } from './$types';

/**
 * Who is signed in, for the header.
 *
 * A failure here must not take the page down: if the API is unreachable the interface
 * shows nobody signed in, which is both true and harmless. It would be the wrong trade to
 * fail a public page because the identity endpoint blinked — and `/api/me` is reporting,
 * never deciding, so an anonymous fallback cannot grant anything.
 */
export const load: LayoutServerLoad = async ({ fetch, request }) => {
  try {
    const { data } = await apiGet<Me>(fetch, '/api/me', request.headers.get('cookie'));
    return { me: data ?? ANONYMOUS };
  } catch {
    return { me: ANONYMOUS };
  }
};
