<script lang="ts">
  import { onMount } from 'svelte';

  type Theme = 'system' | 'light' | 'dark';

  // Starts as 'system' so server-rendered markup matches the client's first paint.
  // The real value is read in onMount; guessing here would cause a hydration mismatch.
  let theme = $state<Theme>('system');

  onMount(() => {
    try {
      const stored = localStorage.getItem('gw-theme');
      if (stored === 'light' || stored === 'dark') theme = stored;
    } catch {
      // Private browsing can throw. System preference stays in charge, which is fine.
    }
  });

  function apply(next: Theme) {
    theme = next;
    try {
      if (next === 'system') {
        localStorage.removeItem('gw-theme');
        delete document.documentElement.dataset.theme;
      } else {
        localStorage.setItem('gw-theme', next);
        document.documentElement.dataset.theme = next;
      }
    } catch {
      // Storage unavailable: the choice still applies for this page load.
    }
  }

  const OPTIONS: { value: Theme; label: string; icon: string }[] = [
    { value: 'light', label: 'Hell', icon: '☀' },
    { value: 'dark', label: 'Dunkel', icon: '☾' },
    { value: 'system', label: 'System', icon: '◐' }
  ];
</script>

<!--
  A radio group, not a toggle button. Three states do not fit a two-state control, and
  "follow the system" is a genuinely different choice from picking dark — someone who
  chooses it wants their evening switch to keep working.

  role="radiogroup" with aria-checked tells a screen reader this is one choice among
  three, which a row of buttons would not convey.
-->
<div class="themes no-print" role="radiogroup" aria-label="Farbschema">
  {#each OPTIONS as opt (opt.value)}
    <button
      type="button"
      role="radio"
      aria-checked={theme === opt.value}
      title={opt.label}
      onclick={() => apply(opt.value)}
    >
      <span aria-hidden="true">{opt.icon}</span>
      <span class="sr-only">{opt.label}</span>
    </button>
  {/each}
</div>

<style>
  .themes {
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
    inline-size: 1.85rem;
    block-size: 1.85rem;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--ink-faint);
    font: inherit;
    font-size: var(--text-sm);
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

  /* Visible to a screen reader, invisible on screen. `display: none` would hide it
     from both, which defeats the purpose of the label. */
  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
