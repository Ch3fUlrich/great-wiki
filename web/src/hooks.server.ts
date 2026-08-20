import type { Handle } from '@sveltejs/kit';
import { widenCspNonceToStyles } from '$lib/csp';

/**
 * The Content-Security-Policy is CONFIGURED in `vite.config.ts` (`kit.csp`) and only
 * repaired here. The repair, and why it is not a policy decision, is written out in
 * `$lib/csp` — the short version is that SvelteKit puts its per-response nonce in
 * `script-src` and, in a production build, nowhere else, so a library that is handed the
 * nonce for a `<style>` element still has it refused.
 *
 * Deliberately the whole of this hook. Header rewriting in `handle` is the kind of place
 * that accumulates unrelated jobs, and every one added here runs on every response.
 */
export const handle: Handle = async ({ event, resolve }) => {
  const response = await resolve(event);

  const policy = response.headers.get('content-security-policy');
  if (policy) {
    const widened = widenCspNonceToStyles(policy);
    if (widened !== policy) response.headers.set('content-security-policy', widened);
  }

  return response;
};
