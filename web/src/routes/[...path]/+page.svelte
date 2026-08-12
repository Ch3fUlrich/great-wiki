<script lang="ts">
  import { browser } from '$app/environment';
  import BlockView from '$lib/components/BlockView.svelte';
  import Breadcrumb from '$lib/components/Breadcrumb.svelte';
  import PageMeta from '$lib/components/PageMeta.svelte';
  import Subpages from '$lib/components/Subpages.svelte';
  import Tree from '$lib/components/Tree.svelte';
  import { outline } from '$lib/blocks/render';
  import { breadcrumb, childrenOf } from '$lib/pagemeta';

  let { data } = $props();
  const headings = $derived(outline(data.body));

  /**
   * Whether to offer editing at all, and why the answer is this crude.
   *
   * There is no capability on the wire. `/api/documents` returns ten fields and none of
   * them is "may I write this"; `/api/me` reports groups and a baseline, and D-M2-8 is
   * explicit that no baseline confers write — an instance admin with no grant is refused
   * like anybody else. The only endpoint that knows is the collaboration socket, and it
   * cannot be asked without either a real WebSocket handshake (which allocates a room on
   * the server, on every page view) or a POST (which would publish somebody else's live
   * session as a revision). Neither belongs in a page render.
   *
   * So the control is offered to whoever is signed in, and the true answer is given the
   * moment it is pressed: `Editor.svelte` opens the session, and a refusal produces a
   * sentence rather than an editor. That is the honest arrangement available today — the
   * button can be a false offer, but it can never be a false editor.
   *
   * What would fix it is one boolean from the API; the accompanying report names it.
   * Anonymous readers are left out because nothing in this deployment grants write to
   * `anyone`, and an offer nobody can accept is worse than no offer.
   */
  const mayOfferEditing = $derived(data.me?.authenticated === true);

  /**
   * `null` means "whatever the URL said". The control is a real link to `?edit=1`, so it
   * works before hydration and survives being opened in a new tab; once hydrated its click
   * handler switches in place instead, and "Fertig" switches back — neither needs a round
   * trip, and neither needs the URL and the state to be kept in step.
   */
  let toggled = $state<boolean | null>(null);
  const editing = $derived(mayOfferEditing && (toggled ?? data.edit === true));

  const editorName = $derived(data.me?.display_name || data.me?.username || 'Unbekannt');

  /**
   * Load the editor, in the browser only.
   *
   * The `browser` guard is not defensive — it is what keeps TipTap out of the **server**
   * bundle. `$app/environment`'s `browser` is replaced with a literal at build time, so in
   * the SSR build this reads `false ? import(…) : …` and rollup drops the import entirely;
   * no server chunk is emitted and the server bundle names no `@tiptap/*` package. A bare
   * `import()` inside `{#if editing}` does NOT achieve that: the branch never executes on
   * the server, but the chunk is still emitted, and the runtime image ships no
   * `node_modules` for it to resolve against. The container build refuses such a bundle,
   * which is how this was caught — before that, a comment three lines below claimed the
   * editor was already out of the server bundle.
   *
   * The server-side branch returns a promise that never settles. It is unreachable —
   * `editing` is false while rendering — and a rejection would render the failure branch
   * into the SSR HTML for a reader who never asked to edit.
   */
  const loadEditor = () =>
    browser
      ? import('$lib/editor/Editor.svelte')
      : new Promise<typeof import('$lib/editor/Editor.svelte')>(() => {});

  // Derived here rather than in the loader on purpose: `$derived` runs during server
  // rendering too, so the markup is complete in the first response, and the tree is
  // already in the payload — computing these server-side would ship the same titles twice.
  const crumbs = $derived(breadcrumb(data.tree, data.doc));
  const subpages = $derived(childrenOf(data.tree, data.doc.path));
</script>

<svelte:head><title>{data.doc.title} — great-wiki</title></svelte:head>

<div class="shell">
  <nav class="sidebar no-print" aria-label="Seitenbaum">
    <Tree nodes={data.tree} current={data.doc.path} />
  </nav>

  <!-- `.prose` moved off `<main>` and onto the article, and `lang` with it. Both were
       right while `<main>` held nothing but the document, and both became wrong the
       moment it grew chrome around one.

       `.prose` is scoped to rendered document content so its rules never reach the
       interface, and one of those rules cannot be overridden by a component at all: the
       print block at the end of app.css is UNLAYERED on purpose, so
       `.prose a::after { content: ' (' attr(href) ')' }` outranks every layered rule
       regardless of specificity. With `.prose` on `<main>`, a printed page would have had
       its own URL spelled out after every crumb and every subpage link.

       `lang` on `<main>` claimed the German metadata panel was written in the document's
       language. On the 29 English pages of the corpus a screen reader would have
       announced "Sichtbarkeit" with English phonemes; the document's language belongs on
       the document. -->
  <main id="content" class="page">
    <Breadcrumb {crumbs} />
    <h1>{data.doc.title}</h1>
    <PageMeta
      visibility={data.doc.visibility}
      language={data.doc.language}
      docType={data.doc.doc_type}
    />

    <!-- Offered only to somebody signed in, and it is a LINK: before hydration it navigates
         to `?edit=1`, which renders the same page with the editor asked for. A button would
         be a control that looks live and does nothing until the bundle arrives. -->
    {#if mayOfferEditing && !editing}
      <p class="editbar no-print">
        <a
          class="edit-start"
          href="?edit=1"
          onclick={(event) => {
            event.preventDefault();
            toggled = true;
          }}>Bearbeiten</a
        >
      </p>
    {/if}

    {#if editing}
      <!-- Imported here and nowhere else. TipTap, ProseMirror and Yjs together are a few
           hundred kilobytes, and a reader — which is nearly every visit — must not pay for
           them.

           Loaded through `loadEditor` rather than a bare `import()` so that it is out of
           the SERVER bundle too, which a bare one is not. `{#if editing}` is false when the
           server renders, so the import never executes there — but Vite still EMITS a
           server chunk for it, carrying bare `@tiptap/*` specifiers, and the runtime image
           ships no `node_modules`. The container build refuses exactly that, which is how
           this was found: the comment here used to claim the editor was out of the server
           bundle, and it was not. See `loadEditor` for why the guard is what removes it.

           Every branch below still renders the document. That is the requirement: the page
           content is in the first response exactly as it is now, and the editor is what
           arrives afterwards. A blank page while a bundle loads is a regression for every
           reader, including the one who asked to edit. -->
      {#await loadEditor()}
        <p class="editor-loading" role="status">Der Editor wird geladen …</p>
        <article class="prose" lang={data.doc.language}>
          <BlockView block={data.body} />
        </article>
      {:then module}
        {@const Editor = module.default}
        <Editor
          path={data.doc.path}
          title={data.doc.title}
          body={data.body}
          language={data.doc.language}
          {editorName}
          onLeave={() => (toggled = false)}
        />
      {:catch}
        <p class="editor-loading" role="alert">
          Der Editor konnte nicht geladen werden. Die Seite selbst ist unverändert.
        </p>
        <article class="prose" lang={data.doc.language}>
          <BlockView block={data.body} />
        </article>
      {/await}
    {:else}
      <article class="prose" lang={data.doc.language}>
        <BlockView block={data.body} />
      </article>
    {/if}

    <Subpages nodes={subpages} />
  </main>

  {#if headings.length > 1}
    <nav class="outline no-print" aria-label="Auf dieser Seite">
      <p class="outline-title">Auf dieser Seite</p>
      <ul>
        {#each headings as h (h.id)}
          <li style:padding-inline-start={`${(h.level - 2) * 0.7}rem`}>
            <a href={`#${h.id}`}>{h.text}</a>
          </li>
        {/each}
      </ul>
    </nav>
  {/if}
</div>

<style>
  .shell {
    display: grid;
    gap: var(--space-8) var(--space-12);
    padding: var(--space-8) var(--space-6);
    /* The centre column is sized to the measure rather than to leftover space.
       With `1fr` the column stretched to the viewport while the text inside stayed
       capped at 68 characters, so on a wide screen the prose hugged the left edge
       with a band of dead space beside it — which reads as a bug, because it is one. */
    grid-template-columns:
      minmax(11rem, 15rem)
      minmax(0, var(--measure))
      minmax(9rem, 13rem);
    justify-content: center;
    max-width: 100rem;
    margin-inline: auto;
    align-items: start;
  }

  /* Both side columns stick, so navigation stays reachable in a long document.
     Offset by the sticky header's height so nothing hides underneath it. */
  .sidebar,
  .outline {
    position: sticky;
    top: calc(var(--space-12) + var(--space-2));
    max-block-size: calc(100vh - var(--space-12) * 2);
    overflow-y: auto;
    font-size: var(--text-sm);
  }

  .outline-title {
    margin-block-end: var(--space-2);
    color: var(--ink-faint);
    font-size: var(--text-xs);
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .outline ul {
    list-style: none;
    margin: 0;
    padding: 0;
    border-inline-start: 1px solid var(--border);
  }

  .outline li {
    padding-block: 2px;
  }

  .outline a {
    display: block;
    padding-inline-start: var(--space-3);
    margin-inline-start: -1px;
    border-inline-start: 2px solid transparent;
    /* The accent, not muted body ink. These were --ink-muted, which is the colour of
       de-emphasised TEXT, so the on-this-page list read as a set of labels rather than
       as jumps you can take. */
    color: var(--accent);
    text-decoration: none;
    line-height: 1.35;
    /* A wrapped entry keeps its indent on the second line. Without this the
       continuation returns to the left rail and the nesting stops reading. */
    text-indent: 0;
    hanging-punctuation: none;
  }

  .outline a:hover,
  .outline a:focus-visible {
    text-decoration: underline;
    text-underline-offset: 0.15em;
    border-inline-start-color: var(--accent);
  }

  /* The page column: chrome, then the document, then the children.
     `.prose > * + *` in `@layer content` no longer reaches any of this — it applies
     inside the article — so the rhythm between the parts is stated here, and stated
     unevenly on purpose. The breadcrumb belongs to the title, so the gap above the
     heading is small; the subpage list is a different thing from the document, so the
     gap above it is the largest on the page. */
  .page > * + * {
    margin-block-start: var(--space-6);
  }

  .page > h1 {
    font-size: var(--text-4xl);
    line-height: var(--leading-tight);
    letter-spacing: -0.02em;
    margin-block-start: var(--space-3);
    text-wrap: balance;
  }

  .page > :global(.subpages) {
    margin-block-start: var(--space-12);
  }

  /* Tight to the metadata panel above it rather than a block of its own: it is a thing you
     can do to this page, like the panel is a thing that is true of it. `.page > * + *`
     would otherwise put a full --space-6 between two rows of chrome. */
  .editbar {
    margin-block-start: var(--space-3);
  }

  .edit-start {
    display: inline-block;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    color: var(--accent);
    text-decoration: none;
    font-size: var(--text-sm);
  }

  .edit-start:hover,
  .edit-start:focus-visible {
    background: var(--accent-soft);
  }

  .editor-loading {
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  /* One column below 64rem.
     Ordering matters more than it looks. Putting both navigations above the text
     means scrolling past two blocks of links to reach the article — on a phone that
     is most of a screen of nothing you came for. So: the outline first, because it
     is short and is the fastest way through a long document; then the article; then
     the site tree, which you only want when you are leaving this page anyway. */
  @media (max-width: 64rem) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-6);
      padding: var(--space-6) var(--space-4);
    }

    .sidebar,
    .outline {
      position: static;
      max-block-size: none;
    }

    .outline {
      order: -1;
      padding-block-end: var(--space-4);
      border-block-end: 1px solid var(--border);
    }

    .sidebar {
      order: 1;
      padding-block-start: var(--space-6);
      border-block-start: 1px solid var(--border);
    }
  }
</style>
