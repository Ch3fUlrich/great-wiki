import { describe, expect, it } from 'vitest';
import { handle } from './hooks.server';
import type { RequestEvent } from '@sveltejs/kit';

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
