<!--
  "Sie sehen great-wiki gerade als jemand anderes." (D-M2-17)

  Three things about this are requirements rather than styling choices.

  PERSISTENT. It is rendered by the root layout, above the header, on every page — not by
  the console that switched the mode on. Someone activates it there and then navigates away
  to look at the wiki, which is the whole point of the feature; a banner that lived on the
  console would disappear at exactly the moment it starts mattering.

  UNMISSABLE. Full width, at the very top, in the warning colour, and it pushes the page
  down rather than floating over it — an overlay can be scrolled past or covered. It names
  BOTH identities, because "Sie sehen als Gast" without saying who "Sie" are is not enough
  to act on when somebody has left a session open.

  ESCAPABLE WITHOUT JAVASCRIPT. The exit is a plain form POST, exactly like signing out. An
  administrator stuck in a read-only view because a script did not load would be the one
  failure this mode must not have — so there is no `onclick`, no `fetch`, and nothing that
  has to hydrate before the way out works.

  The `<form>` posts to the API rather than to a SvelteKit action so it is the same request
  the API's own middleware exempts: exactly `POST /api/admin/view-as/exit`, one method on
  one path. It answers 303 to `/`, so the browser lands on the home page already back in
  the administrator's own view.
-->
<script lang="ts">
  import type { Me } from '$lib/api';

  let { me }: { me: Me } = $props();

  const target = $derived(me.view_as?.target);
  const viewer = $derived(me.view_as?.viewer);
</script>

{#if me.view_as && target && viewer}
  <!-- `role="status"` with `aria-live="polite"`: a screen-reader user must learn that this
       page is somebody else's view, and learn it when the page arrives rather than by
       finding the region later. `assertive` would interrupt every navigation. -->
  <aside class="viewas no-print" role="status" aria-live="polite">
    <p class="what">
      <strong>Fremde Ansicht.</strong>
      Sie sehen great-wiki so, wie
      <strong>{target.display_name}</strong>
      <span class="mono">({target.username})</span>
      es sieht. Angemeldet sind Sie als
      <strong>{viewer.display_name}</strong>
      <span class="mono">({viewer.username})</span>.
    </p>
    <p class="rule">
      Änderungen sind in dieser Ansicht gesperrt: Jede schreibende Anfrage wird abgelehnt,
      bevor sie irgendeinen Endpunkt erreicht.
    </p>
    <form method="post" action="/api/admin/view-as/exit">
      <button type="submit">Eigene Ansicht wiederherstellen</button>
    </form>
  </aside>
{/if}

<style>
  /* Every value resolves through a token (ADR 0005). The wash is mixed from `--warn`
     rather than written as a hex, so a theme that repaints the warning colour repaints
     this with it. */
  .viewas {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-4);
    padding: var(--space-3) var(--space-6);
    border-block-end: 2px solid var(--warn);
    background: color-mix(in srgb, var(--warn) 14%, var(--bg));
    color: var(--ink);
    font-size: var(--text-sm);
  }

  p {
    margin: 0;
    max-width: 60ch;
  }

  .rule {
    color: var(--ink-muted);
    font-size: var(--text-xs);
  }

  .mono {
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--ink-muted);
  }

  form {
    margin: 0;
  }

  /* The same shape as the header's controls, so the way out does not read as a different
     kind of thing from "Abmelden" beside it. */
  button {
    display: inline-flex;
    align-items: center;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--warn);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--ink);
    font: inherit;
    font-size: var(--text-sm);
    cursor: pointer;
  }

  button:hover {
    background: var(--bg-sunken);
  }
</style>
