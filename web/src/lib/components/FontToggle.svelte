<script lang="ts">
  import { onMount } from 'svelte';

  type Font = 'plex' | 'literata' | 'system';

  // Starts as 'plex' so server-rendered markup matches the client's first paint.
  // The real value is read in onMount; guessing here would cause a hydration mismatch.
  let font = $state<Font>('plex');

  onMount(() => {
    try {
      const stored = localStorage.getItem('gw-font');
      if (stored === 'literata' || stored === 'system') font = stored;
    } catch {
      // Private browsing can throw. The default typeface stays in charge, which is fine.
    }
  });

  function apply(next: Font) {
    font = next;
    try {
      if (next === 'plex') {
        // The default is the absence of the attribute, so choosing it means clearing
        // both — never storing 'plex'. Same shape as "follow the system" for the theme.
        localStorage.removeItem('gw-font');
        delete document.documentElement.dataset.font;
      } else {
        localStorage.setItem('gw-font', next);
        document.documentElement.dataset.font = next;
      }
    } catch {
      // Storage unavailable: the choice still applies for this page load.
    }
  }

  const OPTIONS: { value: Font; label: string; title: string }[] = [
    { value: 'plex', label: 'Plex', title: 'IBM Plex — serifenlos, die Voreinstellung' },
    { value: 'literata', label: 'Literata', title: 'Literata — Serifenschrift zum Lesen' },
    { value: 'system', label: 'System', title: 'Schrift des Betriebssystems — lädt nichts nach' }
  ];
</script>

<!--
  Same control as the theme, because it is the same kind of thing: one choice among
  three, remembered. role="radiogroup" with aria-checked says that to a screen reader;
  a row of plain buttons would only say "three buttons".

  The labels are visible rather than icons — a typeface has no glyph that means it —
  and they are deliberately NOT previewed in their own face, which would download all
  three families just to draw the switch.
-->
<div class="fonts no-print" role="radiogroup" aria-label="Schriftart">
  {#each OPTIONS as opt (opt.value)}
    <button
      type="button"
      role="radio"
      aria-checked={font === opt.value}
      title={opt.title}
      onclick={() => apply(opt.value)}
    >
      {opt.label}
    </button>
  {/each}
</div>

<style>
  .fonts {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: var(--bg-sunken);
    border: 1px solid var(--border);
    border-radius: var(--radius);
  }

  button {
    display: grid;
    place-items: center;
    block-size: 1.85rem;
    padding-inline: var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--ink-faint);
    font: inherit;
    font-size: var(--text-xs);
    line-height: 1;
    white-space: nowrap;
    cursor: pointer;
  }

  button:hover {
    color: var(--ink);
  }

  button[aria-checked='true'] {
    background: var(--bg-raised);
    color: var(--ink);
    box-shadow: var(--shadow-sm);
  }
</style>
