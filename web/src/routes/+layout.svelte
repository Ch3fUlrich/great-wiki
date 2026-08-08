<script lang="ts">
  import '$lib/styles/app.css';
  import favicon from '$lib/assets/favicon.svg';
  import ThemeToggle from '$lib/components/ThemeToggle.svelte';
  import FontToggle from '$lib/components/FontToggle.svelte';
  import AccountMenu from '$lib/components/AccountMenu.svelte';

  let { children, data } = $props();
</script>

<svelte:head>
  <link rel="icon" href={favicon} />
</svelte:head>

<!-- Skip link first in the DOM: a keyboard user must be able to bypass the navigation
     without tabbing through every tree entry on every page. -->
<a class="skip" href="#content">Zum Inhalt springen</a>

<header class="no-print">
  <a class="brand" href="/">great&#8209;wiki</a>
  <!-- Two reading preferences, side by side, because they are the same kind of thing.
       They wrap under the brand on a narrow screen rather than squeezing it. -->
  <div class="prefs">
    <AccountMenu me={data.me} />
    <FontToggle />
    <ThemeToggle />
  </div>
</header>

{@render children()}

<style>
  header {
    position: sticky;
    top: 0;
    z-index: 10;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-2) var(--space-4);
    padding: var(--space-3) var(--space-6);
    border-block-end: 1px solid var(--border);
    /* Slightly translucent so content scrolling underneath is felt rather than hidden. */
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(8px);
  }

  .prefs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .brand {
    font-weight: 650;
    letter-spacing: -0.01em;
    color: var(--ink);
    text-decoration: none;
  }

  .skip {
    position: absolute;
    inset-inline-start: -9999px;
  }

  .skip:focus {
    inset-inline-start: var(--space-4);
    inset-block-start: var(--space-4);
    z-index: 20;
    padding: var(--space-2) var(--space-4);
    background: var(--bg-raised);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
</style>
