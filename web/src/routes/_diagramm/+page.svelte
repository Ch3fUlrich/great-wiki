<script lang="ts">
  /**
   * The document Mermaid runs in (D-26). Nobody is meant to look at this page.
   *
   * It exists because a diagram is drawn by a library that needs a DOM to measure text and
   * injects a style ELEMENT while it does — which the wiki's own `style-src 'self'`
   * refuses, correctly, once per render. Rather than loosening the page's policy (the one
   * loosening [ADR 0007](../../../../docs/decisions/0007-content-security-policy.md)
   * refused), the library was given a document of its own with a policy scoped to this one
   * route: `$lib/csp`'s `diagramFramePolicy`, applied in `hooks.server.ts`.
   *
   * `./rahmen.ts` is the whole of the mechanism and carries the reasoning — in particular
   * why the sandbox the page applies must include `allow-same-origin`, and what that does
   * and does not buy. See also
   * [ADR 0018](../../../../docs/decisions/0018-how-a-diagram-reaches-the-page.md).
   *
   * **A route rather than an inline-document or an `about:blank` frame**, and not by
   * preference: a frame with a local URL INHERITS its embedder's policy, so `style-src
   * 'self'` would follow mermaid into it and nothing would have changed. Only a response of
   * this application's own can carry a policy of its own. (The attribute that would hold
   * such a document inline is separately one of the spellings `scripts/check-html-sinks.sh`
   * refuses outright — named rather than written, because that check greps this directory.)
   *
   * **No `+page.server.ts` and no `dev` guard**, unlike `/_behaviour`: that fixture must not
   * exist in production and this one must. What a visitor who navigates here directly gets
   * is an empty page that listens to nothing — `rahmenStarten` returns immediately when the
   * document is not framed.
   */
  import { rahmenStarten } from './rahmen';

  // An effect, so nothing runs during server rendering: `$app/environment`'s `browser` is
  // what keeps mermaid out of the server bundle, and an effect is what keeps this route's
  // own code from being asked to run where there is no `window` to ask about.
  $effect(() => rahmenStarten());
</script>

<svelte:head>
  <title>Diagrammzeichner</title>
  <!-- Nothing here should ever be indexed or offered as a search result: it is machinery,
       it has no content, and its address is not a page of this wiki. -->
  <meta name="robots" content="noindex, nofollow" />
</svelte:head>

<!-- Deliberately empty of anything a reader could act on. The frame is sized to nothing and
     hidden from assistive technology by the page that creates it ($lib/blocks/mermaid); this
     paragraph is for the one person who will ever open the address by hand, wondering what
     it is. -->
<p class="rahmen-hinweis">
  Diese Seite zeichnet Diagramme für andere Seiten dieses Wikis. Sie hat keinen eigenen
  Inhalt.
</p>

<style>
  .rahmen-hinweis {
    margin: var(--space-4, 1rem);
    color: var(--ink-faint, #666);
    font-size: var(--text-sm, 0.875rem);
  }
</style>
