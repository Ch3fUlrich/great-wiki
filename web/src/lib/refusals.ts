/**
 * What a reader is told when a page is not theirs, or is not there.
 *
 * In German, addressed as »Sie«. These were the last English sentences left on a
 * reader-facing path — "You do not have access to this page.", "Page not found." — and they
 * were found by walking the invitation flow as a second person rather than by reading the
 * code. An invited relative follows a link to a page they were not granted, and this is the
 * screen they get; it is the first thing the wiki ever says to them about a boundary, and
 * it was saying it in a language they may not read.
 *
 * Here rather than beside the loader that throws them, for two reasons. SvelteKit refuses
 * any export from a `+page.server.ts` that is not one of its own (`load`, `actions`, …), so
 * a constant a test can import cannot live there at all — a build error, not a type error,
 * which is why `just build` and not `npm run check` is what catches it. And the page loader
 * and the history loader beside it must not drift apart: the same refusal reached by two
 * routes should read the same way.
 *
 * **Neither sentence names the page.** `/api/documents` answers 403 with no body, so there
 * is nothing here to leak by accident, and keeping these as constants keeps it that way — a
 * refusal that named the title would hand over exactly what the permission filter exists to
 * withhold. The 404 and the 403 stay distinguishable, which is the same deliberate split
 * `gw-api/src/routes/docs.rs` documents at the endpoint: collapsing them would hide a
 * misconfiguration behind "you spelled it wrong".
 */
export const GERMAN_REFUSALS = {
  forbidden:
    'Diese Seite ist nicht für Sie freigegeben. Bitten Sie um Zugriff, wenn Sie ihn brauchen.',
  missing: 'Diese Seite gibt es nicht. Vielleicht wurde sie verschoben oder gelöscht.',
  /**
   * The history of a page that may not be read.
   *
   * Its own sentence rather than `forbidden`, because the reader asked a different
   * question and »diese Seite« alone would read as though they had mistyped the address.
   */
  forbiddenHistory: 'Diese Seite ist nicht für Sie freigegeben, ihr Verlauf also auch nicht.'
} as const;
