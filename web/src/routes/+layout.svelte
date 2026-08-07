<script lang="ts">
  import favicon from '$lib/assets/favicon.svg';

  let { children } = $props();
</script>

<svelte:head>
  <link rel="icon" href={favicon} />
</svelte:head>

<!-- Skip link first in the DOM: keyboard users must be able to bypass navigation. -->
<a class="skip" href="#content">Skip to content</a>
<header>
  <a class="brand" href="/">great-wiki</a>
</header>
{@render children()}

<style>
  /* Theme via custom properties, dark by system preference with a light override.
     Both directions are styled; neither is an afterthought. */
  :global(:root) {
    --bg: #0f1115; --panel: #161a21; --line: #2a3140;
    --ink: #e6e9ef; --ink-dim: #a9b2c3; --accent: #6ea8fe;
  }
  @media (prefers-color-scheme: light) {
    :global(:root) {
      --bg: #ffffff; --panel: #f6f7f9; --line: #d9dee7;
      --ink: #16191f; --ink-dim: #5b6473; --accent: #1a5fd0;
    }
  }
  :global(body) {
    margin: 0; background: var(--bg); color: var(--ink);
    font: 16px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif;
  }
  :global(a) { color: var(--accent); }
  .skip {
    position: absolute; left: -9999px;
  }
  .skip:focus {
    left: 1rem; top: 1rem; z-index: 10;
    background: var(--panel); padding: .5rem 1rem; border-radius: 6px;
  }
  header { border-bottom: 1px solid var(--line); padding: 1rem; }
  .brand { font-weight: 600; text-decoration: none; }
</style>
