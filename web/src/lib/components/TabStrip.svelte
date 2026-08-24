<!--
  The workspace's tab strip: what is open, which one you are looking at, and the controls
  that open, close and rearrange them.

  **EVERY CONTROL IS A LINK.** Switching, closing, reordering and opening are all ordinary
  `<a href>`s to addresses `$lib/tabs` computed on the server, so the whole strip works
  with JavaScript switched off, in the first response, before any bundle arrives. That is
  not a nicety here: this repository already records, about its own edit button, that a
  control which only works once a script has loaded is a control that looks live and does
  nothing. A tab strip is the most obviously clickable thing on the screen, so it would be
  the worst possible place to break that rule.

  **WHAT HYDRATION ADDS IS ONLY THE KEYBOARD PATTERN.** The ARIA tabs pattern asks for a
  roving tabindex — one stop for the whole tablist, arrow keys to move within it — and a
  roving tabindex means `tabindex="-1"` on every unselected tab. Server-rendering that
  would take every tab but one out of the keyboard's reach for anybody whose script never
  arrives, which trades a real failure for a stylistic one. So the roving tabindex is
  applied ON MOUNT: before that every tab is an ordinary focusable link, which is operable
  if slightly more verbose; after it, the pattern is the one the guidance describes.

  **MANUAL ACTIVATION**, also deliberately. Arrow keys move focus and Enter follows the
  link; focus alone does not switch tabs. Automatic activation is only appropriate when
  showing a panel is free, and here it is a navigation that fetches a page.

  **THE CLOSE CONTROL SITS INSIDE A `role="presentation"` WRAPPER** beside its tab rather
  than inside it. A `tablist` owns `tab`s, and an extra link among them would be an
  invalid child; a focusable element *inside* a `tab` is the other way this is usually
  done and is no better. The presentational wrapper is what lets the two live together —
  the tab keeps its role and its name, and the close link keeps its own name, which spells
  out which tab it closes so that a list of links is not eleven identical "schließen"s.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { moveHref, newTabHref, tabDomId, type Tab } from '$lib/tabs';

  interface Props {
    tabs: Tab[];
    active: number;
    /** The id of the panel these tabs control — the shell's routed view. */
    panelId: string;
  }

  let { tabs, active, panelId }: Props = $props();

  const hrefs = $derived(tabs.map((tab) => tab.href));
  const current = $derived(tabs[active]);

  /**
   * Reordering is offered for the ACTIVE tab only, and that is a decision about the strip
   * rather than a limitation. Two move controls on every tab is four extra links per tab
   * on a row that is already the busiest in the interface; one pair, next to the tab they
   * act on, says the same thing and can actually be read. Moving a background tab means
   * switching to it first, which is one click and is what you were about to do anyway.
   */
  const nachLinks = $derived(active > 0 ? moveHref(hrefs, active, active - 1) : null);
  const nachRechts = $derived(
    active < hrefs.length - 1 ? moveHref(hrefs, active, active + 1) : null
  );
  const neu = $derived(newTabHref(hrefs));

  /** See the note above: the roving tabindex exists only once a browser is driving. */
  let roving = $state(false);
  onMount(() => {
    roving = true;
  });

  /**
   * Arrow keys, Home and End move focus within the strip; Delete closes the focused tab.
   *
   * Focus is read from the event target rather than tracked in state, so this stays
   * correct however focus got where it is — a click, a Tab press, or the browser
   * restoring it after a navigation.
   */
  function onkeydown(event: KeyboardEvent) {
    const strip = event.currentTarget as HTMLElement;
    const items = [...strip.querySelectorAll<HTMLElement>('[role="tab"]')];
    const from = items.indexOf(document.activeElement as HTMLElement);
    if (from === -1) return;

    if (event.key === 'Delete') {
      const close = items[from].parentElement?.querySelector<HTMLAnchorElement>('.zu');
      if (!close) return;
      event.preventDefault();
      close.click();
      return;
    }

    let to: number | null = null;
    if (event.key === 'ArrowRight') to = (from + 1) % items.length;
    else if (event.key === 'ArrowLeft') to = (from - 1 + items.length) % items.length;
    else if (event.key === 'Home') to = 0;
    else if (event.key === 'End') to = items.length - 1;
    if (to === null) return;

    event.preventDefault();
    items[to].focus();
  }
</script>

<div class="leiste no-print">
  <!--
    The compiler sees a keyboard handler on an interactive role and asks for a `tabindex`
    on the container. That is wrong for this one role: a `tablist` is a COMPOSITE widget,
    and the pattern it belongs to puts the tab stop on the tabs themselves and keeps the
    container out of the tab order entirely. Giving the container a `tabindex` would add a
    focus stop that lands on nothing and announces nothing — the opposite of what the rule
    is protecting. (The directive below has to be a comment of its own: Svelte only reads
    one that BEGINS with `svelte-ignore`, so an explanation in front of it silently
    disables the suppression rather than the warning.)
  -->
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="strip"
    role="tablist"
    aria-label="Geöffnete Reiter"
    aria-orientation="horizontal"
    {onkeydown}
  >
    {#each tabs as tab, index (tab.href)}
      <span class="stueck" class:aktiv={index === active} role="presentation">
        <a
          class="reiter"
          id={tabDomId(index)}
          role="tab"
          href={tab.switchHref}
          aria-selected={index === active}
          aria-controls={panelId}
          tabindex={roving && index !== active ? -1 : undefined}
          data-art={tab.kind}
        >
          <span class="titel">{tab.label}</span>
        </a>
        {#if tab.closeHref}
          <a class="zu" href={tab.closeHref} aria-label={`Reiter »${tab.label}« schließen`}>
            <span aria-hidden="true">×</span>
          </a>
        {/if}
      </span>
    {/each}
  </div>

  <div class="werkzeuge">
    {#if tabs.length > 1 && current}
      <!-- Rendered either way, so the row does not shift as a tab reaches an end: a link
           when the move is possible, an inert glyph when it is not. `aria-hidden` on the
           inert one rather than `aria-disabled`, because there is nothing there to
           announce — the move is not temporarily unavailable, it does not exist. -->
      {#if nachLinks}
        <a
          class="wz"
          href={nachLinks}
          aria-label={`Reiter »${current.label}« nach links verschieben`}
        >
          <span aria-hidden="true">‹</span>
        </a>
      {:else}
        <span class="wz wz--aus" aria-hidden="true">‹</span>
      {/if}
      {#if nachRechts}
        <a
          class="wz"
          href={nachRechts}
          aria-label={`Reiter »${current.label}« nach rechts verschieben`}
        >
          <span aria-hidden="true">›</span>
        </a>
      {:else}
        <span class="wz wz--aus" aria-hidden="true">›</span>
      {/if}
    {/if}

    <!-- A new tab opens the START PAGE, which is the one page that lists every other one —
         the same thing a browser's own new tab does, and the reason this needs no picker
         and no menu. You navigate from there and the tab keeps its place in the strip. -->
    <a class="wz wz--neu" href={neu} aria-label="Neuen Reiter öffnen">
      <span aria-hidden="true">+</span><span class="wort">Neuer Reiter</span>
    </a>
  </div>
</div>

<style>
  /* `@layer components`, the plugin contract (ADR 0005): a plugin's unlayered rules beat
     every rule here regardless of specificity, so a theme can restyle the strip without an
     `!important` anywhere. */
  @layer components {
    .leiste {
      display: flex;
      align-items: stretch;
      gap: var(--space-2);
      /* The strip is chrome sitting on the sunken ground; the active tab is the piece of
         it that is raised into the panel below. */
      background: var(--bg-sunken);
      border-block-end: 1px solid var(--border);
      padding-inline: var(--space-2);
      /* Nothing in here may make the PAGE scroll sideways — that is what the strip's own
         scroll box below is for. */
      min-inline-size: 0;
    }

    /* A long workspace scrolls INSIDE the strip. The page body never scrolls horizontally;
       the same rule a wide table and a wide code block already follow here. */
    .strip {
      display: flex;
      align-items: stretch;
      gap: var(--space-1);
      flex: 1 1 auto;
      min-inline-size: 0;
      overflow-x: auto;
      overscroll-behavior-inline: contain;
      scrollbar-width: thin;
    }

    .stueck {
      display: flex;
      align-items: stretch;
      flex: 0 1 auto;
      min-inline-size: 0;
      border-block-start: 2px solid transparent;
      border-inline: 1px solid transparent;
      border-radius: var(--radius) var(--radius) 0 0;
    }

    /* The active tab differs by SHAPE and WEIGHT as well as hue: it is the only one raised
       to the panel's own background, and the only one with a rule across its top. Colour
       is the third channel, never the only one. */
    .stueck.aktiv {
      background: var(--bg);
      border-block-start-color: var(--accent);
      border-inline-color: var(--border);
    }

    .reiter {
      display: flex;
      align-items: center;
      min-inline-size: 0;
      padding: var(--space-2) var(--space-2) var(--space-2) var(--space-3);
      color: var(--ink-muted);
      text-decoration: none;
      font-size: var(--text-sm);
      line-height: 1.2;
    }

    .stueck.aktiv .reiter {
      color: var(--ink);
      font-weight: 650;
    }

    .reiter:hover,
    .reiter:focus-visible {
      color: var(--ink);
    }

    /* A long page title is trimmed rather than allowed to set the strip's width. The full
       one is still the link's text, so a screen reader reads all of it. */
    .titel {
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      max-inline-size: 14rem;
    }

    .zu,
    .wz {
      display: flex;
      align-items: center;
      justify-content: center;
      flex: none;
      color: var(--ink-faint);
      text-decoration: none;
      /* A 2.25rem target rather than the glyph's own size: a 12px × is not something to
         ask anybody to hit. */
      min-inline-size: 1.75rem;
      padding-inline: var(--space-1);
      font-size: var(--text-sm);
    }

    .zu {
      border-start-end-radius: var(--radius);
    }

    .zu:hover,
    .zu:focus-visible,
    .wz:hover,
    .wz:focus-visible {
      color: var(--ink);
      background: var(--accent-soft);
    }

    .werkzeuge {
      display: flex;
      align-items: center;
      flex: none;
      gap: var(--space-1);
    }

    .wz--aus {
      opacity: 0.35;
    }

    .wz--neu {
      gap: var(--space-1);
      color: var(--accent);
      padding-inline: var(--space-2);
    }

    /* The word is the label on a wide screen and the glyph carries it on a narrow one.
       The link's accessible name is the `aria-label` either way, so it never changes. */
    @media (max-width: 60rem) {
      .wort {
        position: absolute;
        inline-size: 1px;
        block-size: 1px;
        overflow: hidden;
        clip-path: inset(50%);
        white-space: nowrap;
      }
    }
  }
</style>
