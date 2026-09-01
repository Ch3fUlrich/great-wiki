<!--
  The application shell.

  This used to be a header above a centred column, and the column was the whole interface:
  every view drew its own sidebar, its own width and its own idea of where the page ended.
  On a desktop monitor that left most of the screen empty and gave a reader exactly one
  thing at a time. What is here now is a workspace — a persistent page tree, a strip of
  open tabs, and a panel that scrolls on its own — sized to the viewport rather than to the
  page inside it.

  THREE REGIONS, AND WHY THESE THREE.

  1. **The page tree, in the shell.** It was already the wiki's navigation; it was simply
     redrawn by the document view and absent everywhere else, so `/graph`, `/aufgaben` and
     `/projekte` were dead ends you had to leave by the browser's back button. It is the
     same `Tree.svelte` and the same `/api/tree` — moved, not rewritten.

  2. **The tab strip and the panel.** See `$lib/tabs`: a tab is an address, the set of them
     is in the URL, and the active tab's content is rendered by the route that owns that
     address. Nothing about the panel is special-cased per kind of view.

  3. **A context region belongs to the VIEW, not to the shell** — which is why there is no
     third column here. The things that used to stack under a document (its metadata, its
     subpages, what links to it, its outline) are facts about *that* document, and
     `[...path]/+page.svelte` puts them in a column beside it. `/graph` has no backlinks
     and `/aufgaben` has no outline; a shell-level slot would have been an empty frame on
     most views, and every view would have had to feed it.

  THE PANEL SCROLLS, NOT THE PAGE. That is what makes the tree and the tab strip stay put
  in a long document without `position: sticky` and without the header floating over the
  text. It is undone below 64rem, where a phone's own scrolling is the right answer and a
  100dvh application frame is a well-known way to fight the address bar, and undone again
  for print, where the document is the point and the frame is not.
-->
<script lang="ts">
  import { replaceState } from '$app/navigation';
  import '$lib/styles/app.css';
  import favicon from '$lib/assets/favicon.svg';
  import ThemeToggle from '$lib/components/ThemeToggle.svelte';
  import FontToggle from '$lib/components/FontToggle.svelte';
  import AccountMenu from '$lib/components/AccountMenu.svelte';
  import TabStrip from '$lib/components/TabStrip.svelte';
  import TopicTree from '$lib/components/TopicTree.svelte';
  import Tree from '$lib/components/Tree.svelte';
  import ViewAsBanner from '$lib/components/ViewAsBanner.svelte';
  import {
    buildTabs,
    mergeStored,
    navigateHref,
    readStored,
    resolveTabs,
    tabDomId,
    withTabs,
    writeStored
  } from '$lib/tabs';
  import {
    activeTopicPath,
    TOPICS_PATH,
    withSidebar,
    type SidebarMode
  } from '$lib/topics';
  import { TRASH_PATH } from '$lib/trash';

  let { children, data } = $props();

  /** The panel the tabs control. Named once; the tabs point at it and it points back. */
  const PANEL = 'gw-panel';

  /**
   * The set restored from storage, when the address named none.
   *
   * `null` means "the address is in charge", which is the ordinary case and the rule the
   * owner set: the URL is the truth whenever it carries a tab set. This is only ever
   * populated by the effect below, in a browser, for the one case the URL cannot cover —
   * see it for what that case is.
   */
  let restored = $state<string[] | null>(null);

  const strip = $derived(
    buildTabs(restored ?? data.tabHrefs ?? [], data.hier ?? '/', data.tree ?? [])
  );
  const hrefs = $derived(strip.tabs.map((tab) => tab.href));

  /** The page the active tab is showing, so the tree can mark where you are. */
  const aktiverPfad = $derived((strip.tabs[strip.active]?.href ?? '/').split('?')[0]);

  /**
   * Which half of the sidebar is showing — the page tree, or the topics.
   *
   * The owner put browsing by topic in two places: a page of its own at `/themen`, and here,
   * so that the tree and the topics are two ways through one corpus rather than two corpora.
   * They are **one query rendered twice**: `+layout.server.ts` asks `GET /api/topics` once,
   * and `TopicTree.svelte` is the same component `/themen` mounts.
   *
   * The choice is in the address (`?seitenleiste=themen`), so it is server-rendered in the
   * first response, survives a reload, and the back button walks through it — and so it can
   * be asserted by a test in a project with no DOM. It is deliberately NOT part of a tab's
   * identity: a tab is a page, and this says what is beside the page. See `$lib/tabs`.
   */
  const modus = $derived<SidebarMode>(data.seitenleiste ?? 'seiten');

  /** The topic being looked at, when the active tab is a topic page, so it can be marked. */
  const aktivesThema = $derived(activeTopicPath(aktiverPfad));

  /**
   * Where a link in the SHELL goes: the same workspace, with the active tab pointed at the
   * new address, and the sidebar still showing what it was showing. Following a link is
   * navigation, not opening — the strip is unchanged around it, and the tab you were in now
   * shows something else.
   *
   * While a single tab is open and the sidebar is on the page tree, this returns the address
   * unchanged, so a wiki nobody has opened a second tab in — or touched the switcher in —
   * keeps exactly the links and exactly the address bar it had.
   */
  function gehZu(target: string): string {
    return gehZuMit(target, modus);
  }

  /**
   * The same, with the sidebar's choice named explicitly. The switcher's own two links are
   * the only thing that needs it: they go to the address you are already on, and differ from
   * each other in nothing but which half they ask for.
   */
  function gehZuMit(target: string, wunsch: SidebarMode): string {
    return navigateHref(withSidebar(target, wunsch), hrefs, strip.active);
  }

  /** `localStorage` can throw on the property itself, not only on its methods. */
  function speicher(): Storage | null {
    try {
      return typeof localStorage === 'undefined' ? null : localStorage;
    } catch {
      return null;
    }
  }

  /**
   * The browser as the fallback, exactly where the URL cannot reach.
   *
   * **The links inside a document belong to the document.** A wiki page links to other
   * pages by their plain addresses — that is what makes it a wiki, and rewriting every
   * `<a>` in every rendered body to carry a tab set would be both invasive and a lie about
   * what the author wrote. So following one lands on an address with no workspace in it,
   * and that is the one case where the URL cannot say what is open.
   *
   * What happens then: the last set is read back from storage and the page just landed on
   * REPLACES the tab it was reached from, rather than becoming a new tab — browsing inside
   * a tab must not grow a strip. The address bar is then corrected with `replaceState`, so
   * that the workspace is a shareable link again immediately and a reload restores it from
   * the URL rather than from storage.
   *
   * Everything about storage is wrapped, and every branch renders correctly when it is
   * empty or throws: private browsing, blocked site data and a full quota are all real,
   * and none of them is worth a broken page. Without a script this whole effect is absent
   * and the workspace collapses to the one tab the address named — which is precisely how
   * this application behaved before it had tabs at all.
   */
  $effect(() => {
    const ausUrl = data.tabHrefs ?? [];
    const hier = data.hier ?? '/';
    const store = speicher();

    if (ausUrl.length > 0) {
      restored = null;
      writeStored(store, resolveTabs(ausUrl, hier));
      return;
    }

    const stored = readStored(store);
    if (!stored || stored.hrefs.length < 2) {
      restored = null;
      writeStored(store, { hrefs: [hier], active: 0 });
      return;
    }

    const merged = mergeStored(stored, hier);
    restored = merged.hrefs;
    writeStored(store, merged);
    // Built from the address that is actually in the bar, NOT from `hier`. `hier` is the
    // tab's IDENTITY and has the one-shot parameters stripped out of it on purpose — so
    // correcting the address from it dropped whatever the reader had just asked this page
    // for. `?edit=1` was the one that existed to lose: arriving on it with a remembered
    // workspace rewrote the address to one that would not reopen the editor on reload.
    korrigiere(withTabs(location.pathname + location.search, merged.hrefs));
  });

  /**
   * Put the restored workspace into the address bar.
   *
   * **Deferred, because on a fresh page load this runs far too early**, and the fix is not
   * the obvious one. SvelteKit's `replaceState` throws `Cannot call replaceState(...)
   * before router is initialized`; the root layout's effects run during hydration, so the
   * direct call always failed — and so did a retry from `afterNavigate`, which looked like
   * the right readiness signal and is not: on the initial load the router runs its
   * after-navigate callbacks from INSIDE hydration, before it marks itself started. Both
   * failures were invisible, because the only thing wrong on screen was an address bar
   * still describing one tab while two were open. They were found by instrumenting the
   * swallowed `catch`, which is what a swallowed `catch` is for.
   *
   * A macrotask is late enough: the router sets itself started synchronously once
   * hydration resolves, so anything scheduled with `setTimeout` from within it runs after.
   * One retry, then silence — if it still fails, the strip on screen is correct and every
   * link in it carries the workspace, and only the address is behind until the next
   * navigation writes it.
   */
  function korrigiere(ziel: string, nochmal = true) {
    try {
      replaceState(ziel, {});
    } catch {
      if (nochmal) setTimeout(() => korrigiere(ziel, false), 0);
    }
  }
</script>

<svelte:head>
  <link rel="icon" href={favicon} />
</svelte:head>

<!-- Skip link first in the DOM: a keyboard user must be able to bypass the navigation
     without tabbing through every tree entry and every tab on every page. It is a
     fragment, not a place, so it never carries the workspace. -->
<a class="skip" href="#content">Zum Inhalt springen</a>

<div class="shell">
  <div class="chrome">
    <!-- Before the header, and outside it (D-M2-17). Above everything, on every page,
         because the mode is entered in the console and then used everywhere else — and
         inside the sticky header it would compete with the brand for the one row that
         never scrolls away. It renders nothing at all when no substitution is active. -->
    <ViewAsBanner me={data.me} />

    <header class="no-print">
      <!-- The brand and the views that are about the whole wiki rather than one page. The
           graph is here rather than on a page because that is what it is: every page at
           once. Projects are here for the same reason and for one more: D-13 put the list
           of projects on a page of its own precisely so that "what projects exist" has
           somewhere to be asked, and a page nothing links to is a page nobody finds.

           Aufgaben is here for exactly that last reason, and D-12 makes it sharper. There
           is a board on every project's home page too — but a task that belongs to no
           project has no home page to appear on, and the global board is the only place it
           exists. A link nobody can find is how such a to-do goes missing, which is the
           failure D-6 exists to prevent. It comes first because it is the one of the three
           you would open daily.

           Each of them navigates the ACTIVE TAB, like any other link in the shell. Opening
           one in a tab of its own is the strip's »Neuer Reiter« and then this link. -->
      <nav class="brand-group" aria-label="Hauptbereiche">
        <a class="brand" href={gehZu('/')}>great&#8209;wiki</a>
        <a class="section" href={gehZu('/aufgaben')}>Aufgaben</a>
        <a class="section" href={gehZu('/projekte')}>Projekte</a>
        <!-- Themen belongs here for the reason Projekte does, only more so. D-4 kept topics
             out of the graph and named the consequence: a topic page listing its documents is
             the ONLY way topics are reachable. A page nothing links to is a page nobody
             finds, so an index nothing linked to would make the whole feature unreachable
             rather than merely inconvenient. -->
        <a class="section" href={gehZu(TOPICS_PATH)}>Themen</a>
        <!-- The Papierkorb is here for the reason Themen is, and for one of its own. D-14
             puts the delete control on the page, where you are when you decide a page should
             go — but a page in the trash is out of the navigation below, out of the markdown
             export and out of the search, so there is nothing left to click through to reach
             it. Recovering therefore has to have a place, and a place nothing links to is a
             place nobody finds: without this link the only way back to a deleted page would
             be an address somebody happened to have kept. -->
        <a class="section" href={gehZu(TRASH_PATH)}>Papierkorb</a>
        <a class="section" href={gehZu('/graph')}>Graph</a>
      </nav>
      <!-- Two reading preferences, side by side, because they are the same kind of thing.
           They wrap under the brand on a narrow screen rather than squeezing it. -->
      <div class="prefs">
        <AccountMenu me={data.me} />
        <FontToggle />
        <ThemeToggle />
      </div>
    </header>
  </div>

  <!-- The wiki's navigation, on every view rather than only on a document — and now two of
       them, because the owner decided the topics are a second way through the same corpus
       rather than a separate feature you go somewhere else for.

       The switcher is two LINKS, not a toggle: it works in the first response, the back
       button walks through it, and the address it produces is one somebody can send. The
       page tree's `aria-label` is unchanged and deliberately so — it is the same landmark it
       always was, and a rename would break every test and every habit that names it. -->
  <div class="seitenleiste no-print">
    <nav class="umschalter" aria-label="Seitenleiste">
      <!-- `aria-current` is the fact; the weight and the background are the second channel.
           Two identical words with only a colour between them would say nothing to a reader
           who cannot tell the two hues apart. -->
      <a
        class="halb"
        href={gehZuMit(data.hier ?? '/', 'seiten')}
        aria-current={modus === 'seiten' ? 'true' : undefined}>Seiten</a
      >
      <a
        class="halb"
        href={gehZuMit(data.hier ?? '/', 'themen')}
        aria-current={modus === 'themen' ? 'true' : undefined}>Themen</a
      >
    </nav>

    {#if modus === 'themen'}
      <!-- The SAME component `/themen` renders, fed by the SAME single request the root
           layout made — which is the whole of what the owner's decision permitted. A second
           implementation would be a second answer to "which topics exist", and because a
           topic's own name is a disclosure (ADR 0011), a second answer is also a second
           chance to leak one. -->
      <TopicTree
        topics={data.themen ?? []}
        titel="Themen"
        ebene={2}
        fehler={data.themenFehler ?? null}
        current={aktivesThema ?? undefined}
        hrefFor={gehZu}
      />
    {:else}
      <nav class="seitenbaum" aria-label="Seitenbaum">
        <Tree nodes={data.tree ?? []} current={aktiverPfad} hrefFor={gehZu} />
      </nav>
    {/if}
  </div>

  <div class="arbeit">
    <TabStrip tabs={strip.tabs} active={strip.active} panelId={PANEL} />

    <!-- The panel, and the shell's only scrollport on a desktop screen. `tabindex="-1"`
         so the panel can be given focus programmatically without becoming a tab stop of
         its own; its name comes from whichever tab is selected. -->
    <div
      class="panel"
      id={PANEL}
      role="tabpanel"
      aria-labelledby={tabDomId(strip.active)}
      tabindex="-1"
    >
      {@render children()}
    </div>
  </div>
</div>

<style>
  /* --- The frame -----------------------------------------------------------------------
   *
   * `100dvh`, not `100vh`: on a phone the two differ by the address bar, and `vh` is the
   * larger of the two — an application frame sized to it is one that always overflows.
   * The mobile block at the end takes the frame apart anyway, but the value is right for
   * a small tablet in landscape, which is exactly where the two units disagree and the
   * media query does not fire.
   */
  .shell {
    block-size: 100dvh;
    display: grid;
    grid-template-columns: clamp(12rem, 17vw, 20rem) minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
    grid-template-areas:
      'chrome chrome'
      'baum arbeit';
    /* The frame itself never scrolls; the two regions inside it do. This is also what
       makes a long tab strip impossible to feel as a sideways page scroll. */
    overflow: hidden;
  }

  .chrome {
    grid-area: chrome;
  }

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
    /* Slightly translucent so content scrolling underneath is felt rather than hidden.
       Inert on a desktop screen, where nothing passes beneath it; it earns its keep in
       the single-column layout below 64rem, where the header is the one row that stays. */
    background: color-mix(in srgb, var(--bg) 88%, transparent);
    backdrop-filter: blur(8px);
  }

  .prefs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  .brand-group {
    display: flex;
    /* Wraps, like `header` and `.prefs` around it. It did not, and the omission was latent
       until the Papierkorb became the fifth entry: at 390px the row then ran to 456px and the
       whole document scrolled sideways — which D7 and F4 caught, on two pages that have
       nothing to do with the header. A navigation is a list of links and there is no width at
       which pushing the page sideways is better than using a second line. */
    flex-wrap: wrap;
    align-items: baseline;
    /* Row gap smaller than the column gap: two wrapped lines want to read as one block, and
       --space-4 between them opens a gap wide enough to look like a divider. */
    gap: var(--space-1) var(--space-4);
    min-inline-size: 0;
  }

  .brand {
    font-weight: 650;
    letter-spacing: -0.01em;
    color: var(--ink);
    text-decoration: none;
  }

  .section {
    color: var(--ink-muted);
    text-decoration: none;
    font-size: var(--text-sm);
  }

  .section:hover,
  .section:focus-visible {
    color: var(--ink);
    text-decoration: underline;
  }

  /* --- The sidebar: the switcher, and whichever half it is showing -------------------- */

  .seitenleiste {
    grid-area: baum;
    min-inline-size: 0;
    overflow: auto;
    overscroll-behavior: contain;
    padding: var(--space-4) var(--space-3);
    border-inline-end: 1px solid var(--border);
    font-size: var(--text-sm);
  }

  .seitenleiste > * + * {
    margin-block-start: var(--space-3);
  }

  /* Two halves of one control, so they read as a pair rather than as two links that happen
     to be adjacent. The border is the frame; `aria-current` is what says which one you are
     on, and the fill below is its second channel. */
  .umschalter {
    display: flex;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    overflow: hidden;
  }

  .halb {
    flex: 1 1 0;
    padding: var(--space-1) var(--space-2);
    color: var(--accent);
    text-align: center;
    text-decoration: none;
    font-size: var(--text-xs);
  }

  .halb + .halb {
    border-inline-start: 1px solid var(--border);
  }

  .halb:hover,
  .halb:focus-visible {
    background: var(--bg-sunken);
    text-decoration: underline;
    text-underline-offset: 0.15em;
  }

  .halb[aria-current='true'] {
    background: var(--accent-soft);
    color: var(--ink);
    font-weight: 650;
  }

  .seitenbaum {
    min-inline-size: 0;
  }

  /* --- The tab strip and the panel ----------------------------------------------------- */

  .arbeit {
    grid-area: arbeit;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr);
    /* Without this a wide table or a long tab strip would widen the whole grid column
       instead of scrolling inside itself, and the page would scroll sideways. */
    min-inline-size: 0;
  }

  .panel {
    overflow: auto;
    min-block-size: 0;
    /* An anchor lands below the top edge of the panel rather than flush against it. The
       equivalent rule on `html` in app.css is what covers the single-column layout. */
    scroll-padding-block-start: var(--space-4);
  }

  /* The panel is given focus programmatically; it must not then draw a ring around the
     entire view. Anything inside it still gets its own. */
  .panel:focus {
    outline: none;
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

  /* --- One column, and the page's own scrolling, below 64rem ---------------------------
   *
   * A 100dvh application frame on a phone is a well-known way to fight the browser's own
   * chrome, and a fixed side column is a way to have no room left. So the frame is taken
   * apart entirely: normal document flow, the header sticky as it has always been, and the
   * tree AFTER the panel — you want it when you are leaving this page anyway, and putting
   * it first would mean scrolling past every page in the wiki to reach the one you opened.
   */
  @media (max-width: 64rem) {
    .shell {
      display: flex;
      flex-direction: column;
      block-size: auto;
      overflow: visible;
    }

    .arbeit {
      order: 1;
      display: block;
    }

    .panel {
      overflow: visible;
    }

    .seitenleiste {
      order: 2;
      overflow: visible;
      border-inline-end: 0;
      border-block-start: 1px solid var(--border);
      padding: var(--space-4);
    }
  }

  /* --- Print ---------------------------------------------------------------------------
   *
   * A printed page is the document, not the application around it. Without this the frame
   * would clip everything to one screen's worth and print a single page of a document of
   * any length — the failure mode that makes "save as PDF" quietly useless. The chrome
   * itself is already `.no-print`.
   */
  @media print {
    .shell {
      display: block;
      block-size: auto;
      overflow: visible;
    }

    .seitenleiste {
      display: none;
    }

    .panel {
      overflow: visible;
    }
  }
</style>
