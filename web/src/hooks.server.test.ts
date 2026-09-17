import { describe, expect, it } from 'vitest';
import { handle } from './hooks.server';
import type { RequestEvent } from '@sveltejs/kit';
import { DIAGRAM_FRAME_PATH } from '$lib/blocks/diagram';

/**
 * `$lib/csp.test.ts` proves `widenCspNonceToStyles` is correct in isolation. It cannot
 * prove the fix ships: nothing calls that function except this hook, and a reviewer who
 * deleted `hooks.server.ts` outright left `cargo test --workspace`, `npx vitest run`,
 * `npm run check` and `npm run build` all green while the production build lost TipTap's
 * stylesheet (docs/decisions/0007-content-security-policy.md). So this test drives the
 * real, exported `handle` — the thing that was deleted — through a stub `resolve`, the
 * same shape SvelteKit itself uses to call it.
 *
 * The header below is the production shape, not a hand-picked one: a nonce in
 * `script-src` and a nonce-free `style-src`, which is exactly what SvelteKit emits with
 * the default `inlineStyleThreshold` (see `$lib/csp` for why).
 */
const SVELTEKIT_PRODUCTION_HEADER =
	"default-src 'self'; script-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='; " +
	"style-src 'self'; style-src-attr 'unsafe-inline'";

function resolvingWith(policy: string) {
	return async () => new Response('<!doctype html>', { headers: { 'content-security-policy': policy } });
}

describe('handle (web/src/hooks.server.ts)', () => {
	it('widens the nonce into style-src on the response the real hook returns', async () => {
		const response = await handle({
			event: {} as RequestEvent,
			resolve: resolvingWith(SVELTEKIT_PRODUCTION_HEADER)
		});

		expect(response.headers.get('content-security-policy')).toContain(
			"style-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='"
		);
	});

	it('leaves a response with no content-security-policy header alone', async () => {
		const response = await handle({
			event: {} as RequestEvent,
			resolve: async () => new Response('<!doctype html>')
		});

		expect(response.headers.get('content-security-policy')).toBeNull();
	});
});

/**
 * The frame's own policy (D-26) is decided here and nowhere else, so this is where "the one
 * route Mermaid runs in is served something looser, and no other route is" has to be
 * asserted. `$lib/csp.test.ts` proves the string transformation; this proves the routing.
 */
function ereignis(pathname: string): RequestEvent {
	return { url: new URL(`https://wiki.example${pathname}`) } as RequestEvent;
}

describe('which response gets the frame policy', () => {
	it('gives the diagram frame a style-src that admits the <style> mermaid injects', async () => {
		const response = await handle({
			event: ereignis(DIAGRAM_FRAME_PATH),
			resolve: resolvingWith(SVELTEKIT_PRODUCTION_HEADER)
		});

		const policy = response.headers.get('content-security-policy') ?? '';
		expect(policy).toContain("style-src 'self' 'unsafe-inline'");
		expect(policy).toContain("frame-src 'none'");
		// And NOT the widening the rest of the site gets: a nonce beside `'unsafe-inline'`
		// makes a browser ignore the `'unsafe-inline'`, so the two repairs are alternatives
		// and never a sequence.
		expect(policy).not.toContain("style-src 'self' 'nonce-");
	});

	it('gives every other route the nonce widening and nothing looser', async () => {
		// The failure this rules out: a prefix or a `startsWith` comparison that hands the
		// looser policy to `/`, or to a page whose path merely begins with the frame's.
		for (const pfad of ['/', '/rundgang/was-schon-geht', `${DIAGRAM_FRAME_PATH}x`, `${DIAGRAM_FRAME_PATH}/mehr`]) {
			const response = await handle({
				event: ereignis(pfad),
				resolve: resolvingWith(SVELTEKIT_PRODUCTION_HEADER)
			});
			const policy = response.headers.get('content-security-policy') ?? '';
			expect(policy, pfad).toContain("style-src 'self' 'nonce-R2HOT7vwD6nTVUBT2SiUwA=='");
			expect(policy, pfad).not.toContain("'unsafe-inline'; ");
		}
	});
});
