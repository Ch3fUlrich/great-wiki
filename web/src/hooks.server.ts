import type { Handle } from '@sveltejs/kit';
import { DIAGRAM_FRAME_PATH } from '$lib/blocks/diagram';
import { diagramFramePolicy, widenCspNonceToStyles } from '$lib/csp';

/**
 * The Content-Security-Policy is CONFIGURED in `vite.config.ts` (`kit.csp`) and only
 * repaired here. The repair, and why it is not a policy decision, is written out in
 * `$lib/csp` — the short version is that SvelteKit puts its per-response nonce in
 * `script-src` and, in a production build, nowhere else, so a library that is handed the
 * nonce for a `<style>` element still has it refused.
 *
 * **And one route is served a policy of its own** (D-26). `kit.csp` is a single
 * configuration for the whole application and SvelteKit offers no per-route form of it, so
 * the frame Mermaid runs in — `DIAGRAM_FRAME_PATH`, `$lib/blocks/diagram` — gets its policy
 * by replacing the generated one HERE. `$lib/csp`'s [diagramFramePolicy] says what moves
 * and why; the part that belongs in this file is that the two repairs are **alternatives
 * and never a sequence**: a nonce in a source list makes a browser ignore `'unsafe-inline'`
 * beside it, so widening the frame's `style-src` with the nonce would switch the frame's
 * whole reason for existing off, silently and in production only.
 *
 * The comparison is `===` against the whole pathname rather than a prefix test, so
 * `/_diagrammx` and `/_diagramm/etwas` are ordinary pages with the ordinary policy. There
 * is one document behind that route and it takes no parameters.
 *
 * Deliberately the whole of this hook. Header rewriting in `handle` is the kind of place
 * that accumulates unrelated jobs, and every one added here runs on every response.
 */
export const handle: Handle = async ({ event, resolve }) => {
  const response = await resolve(event);

  const policy = response.headers.get('content-security-policy');
  if (policy) {
    // `event.url` is optional only so that a test may drive this function with the minimal
    // event SvelteKit's own callers do not need to supply. In the server there is always one.
    const repariert =
      event.url?.pathname === DIAGRAM_FRAME_PATH
        ? diagramFramePolicy(policy)
        : widenCspNonceToStyles(policy);
    if (repariert !== policy) response.headers.set('content-security-policy', repariert);
  }

  return response;
};
