<script lang="ts">
  import { browser } from '$app/environment';
  import Backlinks from '$lib/components/Backlinks.svelte';
  import BlockView from '$lib/components/BlockView.svelte';
  import Board from '$lib/components/Board.svelte';
  import Breadcrumb from '$lib/components/Breadcrumb.svelte';
  import PageMeta from '$lib/components/PageMeta.svelte';
  import PageTopics from '$lib/components/PageTopics.svelte';
  import Subpages from '$lib/components/Subpages.svelte';
  import { outline } from '$lib/blocks/render';
  import { breadcrumb, childrenOf } from '$lib/pagemeta';
  import { chromeHref } from '$lib/tabs';

  let { data, form } = $props();
  const headings = $derived(outline(data.body));

  /**
   * Where a link in this page's CHROME goes: the same workspace, with the active tab
   * pointed at the new address.
   *
   * The chrome is the breadcrumb, the subpage list, the backlinks and the two controls
   * above the article — every link this file puts on the page. The links inside the
   * DOCUMENT are deliberately not touched: those are what somebody wrote, they are plain
   * addresses, and rewriting them would be a lie about the text. Following one lands on an
   * address with no workspace in it, which the shell restores from storage — see the
   * effect in `+layout.svelte` for why that is the honest split rather than a gap.
   *
   * It carries the sidebar's own choice too, for the reason `chromeHref` gives: following a
   * topic from a page while the sidebar is showing topics must not snap the sidebar back to
   * the page tree.
   *
   * With one tab open and the sidebar untouched this returns the address unchanged, so
   * nothing about a wiki nobody has opened a second tab in looks any different.
   */
  function gehZu(target: string): string {
    return chromeHref(target, data.tabHrefs ?? [], data.hier ?? data.doc.path, data.seitenleiste);
  }

  /**
   * Whether the caller may write this page — **the same verdict a write would get.**
   *
   * `/api/documents` answers `may_write` off the very authorisation that let this page be
   * read (ADR 0010), so a control offered on it and the refusal it would receive cannot come
   * apart. This file used to say there was no such bit on the wire and offer every control to
   * whoever was signed in; there has been one since 073281b, and the offer was a guess for as
   * long as this went on reading `/api/me` instead.
   *
   * `=== true`, not `!== false`: an API that says nothing is an API this cannot ask, and a
   * control offered on a missing field is a control offered on a guess. Fail closed —
   * AGENTS.md rule 3.
   */
  const darfSchreiben = $derived(data.doc.may_write === true);

  /**
   * Whether to offer editing at all: **write, AND an account.**
   *
   * The composition ADR 0010 describes, and the reason the two halves are not the same
   * question. `may_write` licenses opening the editor and changing what is there; *filing a
   * revision* needs a signed-in, active account as well, because a revision records who wrote
   * it. So an editor offered to somebody with write and no account would be an editor they
   * could type into and never publish from.
   *
   * Re-filing this page under a topic is deliberately NOT in that second group:
   * `Store::set_document_topics` writes no revision — the page's words are unchanged — so it
   * needs Write and nothing more, which is why the chips below read `darfSchreiben` alone.
   *
   * The socket is still the thing that hands over an editable document, and it still decides
   * on press. What has changed is that the offer is no longer a guess.
   */
  const mayOfferEditing = $derived(darfSchreiben && data.me?.authenticated === true);

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

<!--
  Two columns, and the division between them is the whole layout decision.

  LEFT: the document, and things that ARE the document — its title, the board of the
  project homed here. The prose inside is still capped at the measure (`.prose` in
  app.css), because a 200-character line on a wide monitor is genuinely harder to read;
  the COLUMN is not capped, so a table or a board can use the room the screen actually
  has. That is the difference between filling a screen and stretching text across it.

  RIGHT: things that are true ABOUT the document — where it sits, who may read it, what
  is under it, what points at it, what is in it. Every one of these used to be stacked
  underneath the prose, which put "Verweist hierher" below however many thousand words
  the page happened to contain. Beside it, they are visible at the moment they are useful.

  The page tree is in neither: the shell draws it now, on every view rather than only on a
  document. See `+layout.svelte`.
-->
<div class="ansicht">
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
    <Breadcrumb {crumbs} hrefFor={gehZu} />
    <h1>{data.doc.title}</h1>

    <!-- The owner's second decision, and its whole point is the position: what this page is
         about sits UNDER ITS TITLE, where you are already looking, not behind an editor and
         not in the panel of facts beside the document. Tagging is something you do while
         reading. Clicking a chip browses that topic, which is the only way topics are
         reachable at all (D-4).

         The suggestion list is the shell's own single `GET /api/topics` answer, handed
         straight through — so it is filtered exactly as the index is, structurally rather
         than by anybody remembering to. ADR 0011 warns that this is the surface that gets
         forgotten precisely because it feels like a convenience. -->
    <PageTopics
      themen={data.seitenThemen ?? []}
      alle={data.themen ?? []}
      darfSchreiben={darfSchreiben}
      fehler={form?.fehler ?? data.seitenThemenFehler ?? null}
      getippt={form?.getippt ?? ''}
      hrefFor={gehZu}
    />

    <!-- Bearbeiten is offered only to somebody signed in, and it is a LINK: before hydration
         it navigates to `?edit=1`, which renders the same page with the editor asked for. A
         button would be a control that looks live and does nothing until the bundle arrives.

         Verlauf is offered to EVERYBODY who can see the page, which is the whole difference
         between the two: reading the history follows reading the page (D-M3-5), while
         editing needs an explicit grant nothing here can check. And it has to be offered
         somewhere — a history nothing links to is a history nobody finds, which is the
         complaint this feature exists to answer. -->
    {#if !editing}
      <p class="editbar no-print">
        {#if mayOfferEditing}
          <a
            class="edit-start"
            href={gehZu(`${data.doc.path}?edit=1`)}
            onclick={(event) => {
              event.preventDefault();
              toggled = true;
            }}>Bearbeiten</a
          >
        {/if}
        <a class="edit-start" href={gehZu(`${data.doc.path}/history`)}>Verlauf</a>
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

    <!-- D-12's second placement: this page's own board, when this page is a project's home.
         It is the SAME component `/aufgaben` renders, fed by the SAME endpoint with the
         filter bound to this path — which is how the two boards are kept from disagreeing,
         and the whole of what the decision permitted. A move made here posts to
         `/aufgaben?/verschieben` like every other one and comes back to this page.

         Above the subpage list and the backlinks, because on a project's home page the
         tasks are what you came for; below the document, because the document is still what
         the page is. Rendered at all only when the endpoint named a project: nearly every
         page in this wiki is nobody's home, and furniture on all of them is not a cost D-12
         asked anybody to pay. -->
    {#if data.board}
      <Board
        board={data.board}
        me={data.me}
        now={data.now}
        zurueck={data.zurueck}
        titel="Aufgaben"
        ebene={2}
        hinweis={data.hinweis}
        fehler={null}
      />
    {:else if data.boardFehler}
      <!-- Hedged on purpose — see `describeEmbeddedBoard`. A failed request cannot tell a
           project's home page from any other, so the sentence must not claim this page has
           a board; it says only that if one belongs here, it is not here now. -->
      <p class="tafel-fehler" role="alert">{data.boardFehler}</p>
    {/if}

  </main>

  <!-- A plain `<div>`, not an `<aside>`. Everything in it is already its own landmark with
       its own name — the outline is a `nav`, the metadata panel is a labelled `aside`, the
       subpage list and the backlinks are labelled `nav`s — and wrapping four named
       landmarks in a fifth adds a level to walk through and no information at all.

       The outline comes first because it is about the document you are reading right now;
       the rest are about the document's place in the wiki, and you want those when you are
       finished with it. -->
  <div class="kontext no-print">
    {#if headings.length > 1}
      <nav class="outline" aria-label="Auf dieser Seite">
        <p class="kontext-titel">Auf dieser Seite</p>
        <ul>
          {#each headings as h (h.id)}
            <li style:padding-inline-start={`${(h.level - 2) * 0.7}rem`}>
              <a href={`#${h.id}`}>{h.text}</a>
            </li>
          {/each}
        </ul>
      </nav>
    {/if}

    <PageMeta
      visibility={data.doc.visibility}
      language={data.doc.language}
      docType={data.doc.doc_type}
    />

    <Subpages nodes={subpages} hrefFor={gehZu} />
    <Backlinks backlinks={data.backlinks} hrefFor={gehZu} />
  </div>
</div>

<style>
  .ansicht {
    display: grid;
    gap: var(--space-8) var(--space-12);
    padding: var(--space-8) var(--space-6);
    /* The reading column takes the room that is there; the PROSE inside it is what stays
       capped, by `.prose` in app.css. That distinction is the whole point: the column had
       to be `minmax(0, var(--measure))` while the tree and the outline were in this grid
       and the whole thing was centred in the viewport, and it meant a table or a board was
       squeezed to 58 characters too. The shell owns the frame now, so this can stop
       pretending to be one. */
    grid-template-columns: minmax(0, 1fr) minmax(0, 19rem);
    /* Deliberately NOT `align-items: start`. The context column has to be as tall as the
       row for the outline inside it to stick for the whole of a long document — a sticky
       element only travels within its own containing block, and a column sized to its
       contents gives it a few hundred pixels to travel in. Both columns stretch; the
       content inside each still starts at the top. */
  }

  /* The reading column is a query container so that something genuinely wide — a table —
     can ask for the column's width rather than the prose's. `100cqi` below is that ask,
     and it is the one way a child escapes its parent's `max-width` without negative
     margins or a second grid inside the article. */
  .page {
    container-type: inline-size;
    min-inline-size: 0;
  }

  /* The one exception to the reading measure, and the reason it is stated here rather than
     in `.prose` itself: a table is data, not a sentence. Widening `.prose` would widen the
     paragraphs with it, and turning `.prose` into a grid would apply grid layout to the
     EDITOR's surface too — `Editor.svelte` puts `.prose` on the contenteditable element,
     and re-laying-out a ProseMirror document is not a change to make blind.

     The table still scrolls inside its own box (`TableView.css`), so this widens the box
     and never the page: a document that scrolls sideways is unreadable, which is what that
     box exists to prevent. */
  .page :global(article.prose > .gw-tbl) {
    inline-size: 100cqi;
    max-inline-size: 100cqi;
    /* Recentred on the reading rail's own axis. The rail is centred in the column (see
       below), so a child that is `100cqi` wide would otherwise start at the rail's left
       edge and hang off the right of the column. Half the difference, negative, puts the
       two on one centre line — and resolves to zero on a narrow screen, where the rail is
       already as wide as the column. */
    margin-inline-start: calc((100% - 100cqi) / 2);
  }

  /* The column of facts about the page.
     NOT sticky itself, and that is a correction rather than an omission: it was, capped at
     `max-block-size: 100%`, and a percentage there resolves against the grid ROW — so on a
     page with a long list of backlinks the column could be taller than the screen, stick at
     the top, and put its own bottom permanently out of reach. Only the outline sticks now
     (below), which is the part that is short by nature and the part you actually want to
     keep. */
  .kontext {
    min-inline-size: 0;
    font-size: var(--text-sm);
    /* `gap` rather than `> * + *`: three of the four children are components whose roots
       Svelte's CSS pruning cannot see through, and the sibling rule was dropped as unused.
       A gap is also simply right here — a panel that renders nothing (no subpages, no
       backlinks; the common case) then contributes no space either, where a margin on the
       next sibling would have been the wrong one to blame. */
    display: flex;
    flex-direction: column;
    gap: var(--space-6);
  }

  .kontext-titel {
    margin-block-end: var(--space-2);
    color: var(--ink-faint);
    font-size: var(--text-xs);
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  /* The outline alone travels with the reader. Capped in viewport units rather than in
     percent, for the reason above — and approximate on purpose: it is a cap, nothing is
     laid out against it, and the chrome it is subtracting is a header whose height depends
     on how far the preferences have wrapped. The same approximation this file has always
     made about its sticky columns. */
  .outline {
    position: sticky;
    top: 0;
    max-block-size: calc(100dvh - var(--space-12) * 2);
    overflow-y: auto;
    overscroll-behavior: contain;
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

  /* The reading rail: the parts of this column that are SENTENCES, held to the measure so
     that a wide screen widens the room rather than the lines. The board below is
     deliberately not in this list — a three-column board squeezed into 58 characters was
     the layout bug this whole change is about. */
  .page > :global(.crumbs),
  .page > h1,
  .page > :global(.themen),
  .page > .editbar,
  .page > .editor-loading,
  .page > .tafel-fehler,
  .page > :global(article.prose) {
    max-inline-size: var(--measure);
    /* CENTRED in the column, not flush left. This file already recorded the reason, about
       the layout that came before this one: prose capped at the measure inside a column
       sized to the viewport "hugged the left edge with a band of dead space beside it —
       which reads as a bug, because it is one". The rail is centred and the wide things —
       a board, a table — span the whole column around the same axis, which is what makes
       one line down the middle of the view instead of two ragged edges. */
    margin-inline: auto;
  }

  /* Both of these belong to the title rather than being blocks of their own — one says what
     the page is about, the other what you can do to it — so `.page > * + *`'s full --space-6
     between rows of chrome is wrong for them. */
  .page > :global(.themen) {
    margin-block-start: var(--space-3);
  }

  .editbar {
    margin-block-start: var(--space-3);
    /* Two controls now, and a flex row rather than inline text so the gap between them is
       stated rather than inherited from a newline in the markup. */
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
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

  .tafel-fehler {
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-inline-start: 3px solid var(--danger);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    font-size: var(--text-sm);
    max-width: var(--measure);
  }

  /* One column below 64rem, and the context column moves BELOW the document rather than
     above it. That is a deliberate change from what this file used to do: the outline used
     to be hoisted above the article on a narrow screen, on the reasoning that it is the
     fastest way through a long document. It shares a column with three other panels now,
     and hoisting the whole group would put the metadata, the subpages and the backlinks
     between the title and the first sentence — most of a phone screen of things you did
     not come for. The outline alone was worth the hoist; the group is not. */
  @media (max-width: 64rem) {
    .ansicht {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--space-6);
      padding: var(--space-6) var(--space-4);
    }

    .kontext {
      padding-block-start: var(--space-6);
      border-block-start: 1px solid var(--border);
    }

    /* Nothing sticks on a phone: the page itself is the scrollport there, and an outline
       pinned to the top of it would sit on the text. */
    .outline {
      position: static;
      max-block-size: none;
      overflow: visible;
    }
  }

  /* Printing is the document, not the room it was read in. The context column is already
     `.no-print`; this is the grid that would otherwise leave a column's worth of blank
     paper down the right-hand side of every page. */
  @media print {
    .ansicht {
      display: block;
      padding: 0;
    }

    /* app.css's print block lifts the measure off `.prose` — on paper the column width is
       the paper, and there is no screen to be too wide for. That rule is unlayered but its
       specificity is one class; the centring rule above is scoped by Svelte and outranks
       it, so the cap has to be lifted here too or a printed page would keep a 58-character
       column with white paper either side of it. */
    .page > :global(article.prose) {
      max-inline-size: none;
      margin-inline: 0;
    }
  }
</style>
