<!--
  Editing, in place, on the page that was just being read.

  # What this component is responsible for

  Two things, and the second is the one that matters.

  The first is the obvious one: mount TipTap over the Yjs document the server is holding, so
  that two people typing on one page both keep their work. That part is almost entirely
  configuration, and the configuration that would silently lose content lives in
  `extensions.ts` with the tests that pin it.

  The second is telling the truth about where the work is. `session.ts` holds that logic as
  pure functions because it is the part that must never be wrong; this file is the wiring
  that decides which of those states the session is actually in, and it is deliberately
  boring about it.

  # The socket is opened only by somebody who asked to edit

  Never on a page view. `GET /api/collab/{path}` requires `Action::Write`, and beyond the
  permission itself a subscriber makes a room ineligible for eviction — so a socket opened
  "just to see" would let any reader pin a document's CRDT in the server's memory for as
  long as they held the tab open. The rendered page comes from `/api/documents`; this
  component is mounted only after a person presses "Bearbeiten".

  It follows that the editor is never shown to somebody who may not edit: the editing
  surface is not created until the session is `live`, and a refusal replaces it with a
  sentence rather than with a text box that quietly throws keystrokes away.

  # Refusal looks exactly like a broken network, and is not

  A browser will not tell JavaScript the HTTP status of a rejected WebSocket upgrade. A 403
  arrives as close code 1006 — the same thing an unplugged cable produces. So the first
  failed attempt asks a second question that does have a readable answer (can this session
  still READ the page?) and reports the refusal or the outage accordingly. See
  `diagnoseFirstFailure`.

  # Reconnection has to be stopped by hand

  `y-websocket` treats only close codes 4400-4499 as permanent. `collab.rs` uses the
  standard ones — 1008 for a revoked permission and for a rate-limit trip — so the stock
  behaviour is an exponential-backoff loop against an endpoint that has just said no, for as
  long as the tab is open. `shouldReconnect` below is what stops it, and `isTerminalClose`
  is where the list of codes lives.
-->
<script lang="ts">
  import { onMount, tick } from 'svelte';
  import * as Y from 'yjs';
  import { WebsocketProvider } from 'y-websocket';
  import * as authProtocol from 'y-protocols/auth';
  import { Editor } from '@tiptap/core';
  import Collaboration from '@tiptap/extension-collaboration';
  import CollaborationCaret from '@tiptap/extension-collaboration-caret';
  import { Clipboard } from '@ark-ui/svelte/clipboard';
  import { Field } from '@ark-ui/svelte/field';
  import BlockView from '$lib/components/BlockView.svelte';
  import type { Block } from '$lib/blocks/render';
  import { CONTENT_FIELD, contentExtensions } from './extensions';
  import {
    HISTORY_WARNING,
    collabTarget,
    describePublish,
    describeSession,
    diagnoseFirstFailure,
    hasUnsavedWork,
    isTerminalClose,
    mayType,
    type PublishDescription,
    type SessionState
  } from './session';
  import EditorToolbar from './EditorToolbar.svelte';

  interface Props {
    /** The document's stored path, leading slash included. */
    path: string;
    title: string;
    /** The body as the server rendered it, shown until the session is live. */
    body: Block;
    /** The document's own language, so the editing surface reads as the page does. */
    language: string;
    /** Whoever is editing, for the presence caret. */
    editorName: string;
    /** Back to reading. */
    onLeave: () => void;
  }

  let { path, title, body, language, editorName, onLeave }: Props = $props();

  let session = $state<SessionState>('connecting');
  let ready = $state(false);
  let publishNote = $state<PublishDescription | null>(null);
  let summary = $state('');
  let publishing = $state(false);
  let peers = $state<string[]>([]);
  /** Local edits made since the last confirmed sync — the ones the room has NOT got. */
  let pending = $state(false);
  /** Any local edit at all this session. Once a session is dead, all of them are at risk. */
  let touched = $state(false);
  /** The plain text, kept current so it can be rescued when a session ends badly. */
  let rescueText = $state('');

  let host = $state<HTMLDivElement | null>(null);
  // Reactive on purpose: the toolbar cannot subscribe to an editor it is never told about,
  // and this one does not exist until the session is live.
  let editor = $state<Editor | null>(null);

  // `$derived` rather than a one-off: `path` is a prop, and a value read once at setup is a
  // value that silently belongs to whichever page happened to mount first.
  const target = $derived(
    collabTarget(typeof location === 'undefined' ? 'http://localhost' : location.origin, path)
  );

  const description = $derived(describeSession(session));
  const unsaved = $derived(hasUnsavedWork(session, session === 'offline' ? pending : touched));
  /** Publishing takes what is in the ROOM, so it is only meaningful while one is joined. */
  const canPublish = $derived(session === 'live' && !publishing);

  /**
   * The caret colour, as a token rather than a value.
   *
   * `@tiptap/extension-collaboration-caret` inlines whatever it is given as
   * `style="border-color: …"`, and a custom property resolves there like anywhere else — so
   * a peer's colour repaints with the theme instead of being a hex literal that only works
   * in one of them. Four is enough: beyond four people on one paragraph the names matter
   * more than the hues, and every one of these carries meaning elsewhere in the interface,
   * which is a cost worth paying to keep the palette a theme can reach.
   */
  const CARET_TOKENS = ['var(--accent)', 'var(--ok)', 'var(--warn)', 'var(--danger)'];

  function caretColour(name: string): string {
    let hash = 0;
    for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)!) % 9973;
    return CARET_TOKENS[hash % CARET_TOKENS.length];
  }

  /**
   * Can this session still read the page it just failed to open for editing?
   *
   * The differential diagnosis behind `diagnoseFirstFailure`. It is a plain GET with a
   * status this code can actually see, sent as the same session with the same cookies.
   */
  async function pageStillReadable(): Promise<boolean> {
    try {
      const response = await fetch(`/api/documents/${target.room}`, {
        headers: { accept: 'application/json' }
      });
      return response.ok;
    } catch {
      return false;
    }
  }

  onMount(() => {
    const doc = new Y.Doc();
    /** Whether an upgrade has ever succeeded. Distinguishes "refused" from "dropped". */
    let everConnected = false;
    /** Whether the server sent `permissionDenied` before closing. */
    let denied = false;

    const provider = new WebsocketProvider(target.serverUrl, target.room, doc, {
      shouldReconnect: (event) => everConnected && !isTerminalClose(event.code)
    });

    // `y-websocket`'s own handler for `messageAuth` is a `console.warn` and nothing else,
    // and it is a module-level constant with no option to replace it. The per-instance
    // handler table is public, though, so this is the supported-enough way to find out that
    // the server said no — which is the difference between "your permission ended" and a
    // connection that mysteriously stopped.
    provider.messageHandlers[2] = (_encoder, decoder) => {
      authProtocol.readAuthMessage(decoder, doc, () => {
        denied = true;
      });
    };

    provider.on('status', ({ status }) => {
      if (status === 'connected') {
        everConnected = true;
      } else if (status === 'disconnected' && everConnected && !denied && session !== 'ended') {
        // Not a refusal and not the end: y-websocket will retry, the CRDT will merge
        // whatever was typed meanwhile, and the person may keep writing.
        session = 'offline';
      }
    });

    provider.on('sync', (isSynced: boolean) => {
      if (!isSynced) return;
      pending = false;
      session = 'live';
      void mount(doc, provider);
    });

    provider.on('closed', async ({ code }) => {
      if (denied) session = 'revoked';
      else if (!everConnected) session = diagnoseFirstFailure(await pageStillReadable());
      else session = isTerminalClose(code) ? 'ended' : 'offline';
    });

    // "Pending" means an update the socket was not up to carry. `origin === provider` is a
    // remote update; anything else was made here.
    doc.on('update', (_update: Uint8Array, origin: unknown) => {
      if (origin === provider) return;
      touched = true;
      if (!provider.wsconnected) pending = true;
      rescueText = editor?.getText({ blockSeparator: '\n\n' }) ?? rescueText;
    });

    provider.awareness.on('change', () => {
      peers = [...provider.awareness.getStates().entries()]
        .filter(([client]) => client !== doc.clientID)
        .map(([, entry]) => (entry as { user?: { name?: string } })?.user?.name)
        .filter((name): name is string => typeof name === 'string' && name.length > 0);
    });

    return () => {
      editor?.destroy();
      editor = null;
      provider.destroy();
      doc.destroy();
    };
  });

  /**
   * The nonce this page's Content-Security-Policy was issued with, or `undefined` where
   * there is no policy at all.
   *
   * TipTap injects the ProseMirror base stylesheet as a STYLE ELEMENT at editor
   * construction, and `kit.csp` in vite.config.ts sends `style-src 'self'` — no
   * `unsafe-inline` — so without a nonce that element is refused and the editing surface
   * loses `white-space: pre-wrap`, the gap cursor and the hidden-selection rules. It is not
   * a blank page, which is what makes it worth writing down: it looks like a styling bug,
   * and only the console says otherwise. `style-src-attr` does NOT cover it — that
   * directive is about `style="…"` attributes, and this is an element.
   *
   * (Spelled out in words rather than as markup on purpose. Svelte's parser scans the WHOLE
   * file for the start of the style block, comments included, so a literal opening style tag
   * anywhere in here makes `svelte-check` report `<script> was left open` at the last line
   * of the file and nothing anywhere near the actual text.)
   *
   * Read off the DOM because SvelteKit gives the value to the template and not to client
   * code: `%sveltekit.nonce%` renders into the scripts it emits, and the HTML parser then
   * BLANKS the content attribute while keeping the real value on the IDL property. So
   * `[nonce]` still matches as a selector, `getAttribute('nonce')` returns `""`, and
   * `.nonce` returns the value — all three verified in Chromium against this build. Any
   * script that could read this already runs, and a nonce only has to be unguessable to
   * code that does not.
   */
  function cspNonce(): string | undefined {
    if (typeof document === 'undefined') return undefined;
    return document.querySelector<HTMLScriptElement>('script[nonce]')?.nonce || undefined;
  }

  /** Build the editing surface. Only ever called once the session is live. */
  async function mount(doc: Y.Doc, provider: WebsocketProvider) {
    if (editor) return;
    await tick();
    if (!host) return;

    editor = new Editor({
      element: host,
      // Carried into the `data-tiptap-style` element TipTap appends to the document head.
      // `undefined` leaves it off, which is the right answer where no policy is in force.
      injectNonce: cspNonce(),
      extensions: [
        ...contentExtensions(),
        // `field` is the whole reason this line is spelled out. The extension's default is
        // `'default'`; `gw-collab`'s `CONTENT_FIELD` is `'content'`. Leave it off and both
        // ends work perfectly against two different, permanently empty fragments — no
        // error, no warning, an editor that opens blank and a page that never changes.
        Collaboration.configure({ document: doc, field: CONTENT_FIELD }),
        CollaborationCaret.configure({
          provider,
          user: { name: editorName, color: caretColour(editorName) }
        })
      ],
      editorProps: {
        attributes: {
          // `.prose` so the text is set exactly as it is when read — that is what makes
          // this editing in place rather than a form that resembles the page.
          class: 'gw-ed-surface prose',
          id: 'gw-ed-surface',
          role: 'textbox',
          'aria-multiline': 'true',
          'aria-label': `Inhalt der Seite „${title}“`,
          lang: language
        }
      }
    });
    rescueText = editor.getText({ blockSeparator: '\n\n' });
    ready = true;
  }

  // Editable exactly when a keystroke has somewhere to go. `offline` still does — Yjs
  // merges it on reconnect — and every other non-live session does not, so the surface goes
  // read-only rather than accepting input it cannot keep.
  $effect(() => {
    editor?.setEditable(mayType(session));
  });

  // The leave guard, on only while there is something to lose. Always-on would train people
  // to dismiss it, which is worse than not having it: in `live` the room already has every
  // keystroke and the sweep writes it out whether or not this tab is still here.
  $effect(() => {
    if (!unsaved) return;
    const warn = (event: BeforeUnloadEvent) => event.preventDefault();
    window.addEventListener('beforeunload', warn);
    return () => window.removeEventListener('beforeunload', warn);
  });

  async function publish() {
    publishing = true;
    publishNote = null;
    try {
      const response = await fetch(`/api/collab/${target.room}`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ summary: summary.trim() || null })
      });
      publishNote = describePublish(response.status);
      if (response.ok) summary = '';
    } catch {
      // A network failure is not a status, and it is not a publish either. Saying so beats
      // leaving the button looking like it worked.
      publishNote = describePublish(0);
    } finally {
      publishing = false;
    }
  }
</script>

<section class="gw-ed" aria-label="Seite bearbeiten">
  <div class="gw-ed-bar">
    <div class="gw-ed-bar-row">
      <EditorToolbar {editor} enabled={ready && mayType(session)} {path} />

      <div class="gw-ed-actions">
        {#if peers.length > 0}
          <p class="gw-ed-peers">
            Auch hier: {peers.join(', ')}
          </p>
        {/if}
        <button type="button" class="gw-ed-btn" onclick={onLeave}>Fertig</button>
      </div>
    </div>

    <!-- Not a dismissible toast. D-M2-9 leaves history readable by everyone who may read
         the page, and D-M3-5 spells out the consequence: deleting a sentence removes it
         from the page and not from the record. The moment somebody needs to know that is
         the moment they are removing something, so it stays on screen the whole time. -->
    <p class="gw-ed-history">{HISTORY_WARNING}</p>

    <p
      class="gw-ed-status gw-ed-status--{description.tone}"
      role={description.tone === 'fail' ? 'alert' : 'status'}
      aria-live={description.tone === 'fail' ? 'assertive' : 'polite'}
    >
      <span class="gw-ed-status-head">{description.headline}</span>
      <span class="gw-ed-status-detail">{description.detail}</span>
    </p>

    {#if unsaved}
      <!-- The rescue. A session that ended badly leaves work in this browser and nowhere
           else, and "sorry" is not a feature. Ark's Clipboard because copying to the
           clipboard has a permissions model and a failure mode, and neither is worth
           reimplementing here. -->
      <Clipboard.Root value={rescueText} class="gw-ed-rescue">
        <Clipboard.Trigger class="gw-ed-btn">
          Text in die Zwischenablage kopieren
        </Clipboard.Trigger>
        <Clipboard.Indicator copied={copiedNote} class="gw-ed-rescue-note">
          Damit geht nichts verloren, was hier noch nicht gespeichert ist.
        </Clipboard.Indicator>
      </Clipboard.Root>
    {/if}

    {#if session === 'live'}
      <div class="gw-ed-publish">
        <Field.Root class="gw-ed-field">
          <Field.Label class="gw-ed-label">Zusammenfassung (optional)</Field.Label>
          <Field.Input
            class="gw-ed-input"
            bind:value={summary}
            placeholder="Was hat sich geändert?"
            maxlength={200}
          />
          <Field.HelperText class="gw-ed-help">
            Steht später neben dieser Fassung im Verlauf und beantwortet dort die einzige
            Frage, die sich stellt: warum. Ohne Angabe wird die Fassung nur mit Zeitpunkt
            und Name geführt.
          </Field.HelperText>
        </Field.Root>
        <button type="button" class="gw-ed-btn gw-ed-btn--primary" disabled={!canPublish} onclick={publish}>
          {publishing ? 'Wird veröffentlicht …' : 'Veröffentlichen'}
        </button>
      </div>
    {/if}

    {#if publishNote}
      <p
        class="gw-ed-note gw-ed-note--{publishNote.tone}"
        role={publishNote.tone === 'fail' ? 'alert' : 'status'}
        aria-live={publishNote.tone === 'fail' ? 'assertive' : 'polite'}
      >
        {publishNote.text}
      </p>
    {/if}
  </div>

  <!-- The surface exists from the first render so TipTap has something to mount into, and
       is `hidden` until it has. Until then the reader's own markup stays on screen, so a
       refused session never blanks the page it was going to edit. -->
  <div class="gw-ed-host" bind:this={host} hidden={!ready}></div>

  {#if !ready}
    <article class="prose" lang={language}>
      <BlockView block={body} />
    </article>
  {/if}
</section>

{#snippet copiedNote()}Kopiert. Der Text liegt jetzt in der Zwischenablage.{/snippet}

<!--
  `:global` throughout, and for two different reasons.

  The editing surface and everything inside it is built by ProseMirror after mount, so
  Svelte's scoping attribute never reaches any of it — the same situation `Dialog.svelte`
  documents for Ark's portal. The `gw-ed-` prefix stands in for the scoping, so a theme has
  predictable names to target.

  The other reason is the specificity trap `Dialog.svelte` found the hard way. Ark closes
  things with the `hidden` attribute and leans on the user-agent rule `[hidden]
  { display: none }`, which almost anything outranks. Nothing below sets `display` on
  `.gw-ed-host` at all, which is the simplest way to stay out of that fight; where a rule
  here does set `display` on something Ark can hide, it carries the `:not([hidden])` guard.
-->
<style>
  :global(.gw-ed-bar) {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow: var(--shadow-sm);
    margin-block-end: var(--space-6);
  }

  :global(.gw-ed-bar-row) {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
  }

  :global(.gw-ed-actions) {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-inline-start: auto;
  }

  :global(.gw-ed-peers) {
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  :global(.gw-ed-history) {
    color: var(--ink-muted);
    font-size: var(--text-sm);
    border-inline-start: 3px solid var(--warn);
    padding-inline-start: var(--space-3);
  }

  :global(.gw-ed-status) {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-2);
    align-items: baseline;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-sm);
    border: 1px solid var(--border);
    background: var(--bg-sunken);
  }

  :global(.gw-ed-status-head) {
    font-weight: 650;
  }

  :global(.gw-ed-status-detail) {
    color: var(--ink-muted);
  }

  /* Tone carries on the border and on the heading's colour, never on hue alone: the
     headline says what has happened in words, and these only make it visible at a glance. */
  :global(.gw-ed-status--ok) {
    border-color: var(--ok);
  }
  :global(.gw-ed-status--ok .gw-ed-status-head) {
    color: var(--ok);
  }
  :global(.gw-ed-status--warn) {
    border-color: var(--warn);
  }
  :global(.gw-ed-status--warn .gw-ed-status-head) {
    color: var(--warn);
  }
  :global(.gw-ed-status--fail) {
    border-color: var(--danger);
  }
  :global(.gw-ed-status--fail .gw-ed-status-head) {
    color: var(--danger);
  }

  :global(.gw-ed-rescue) {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }

  :global(.gw-ed-rescue-note) {
    color: var(--ink-muted);
    font-size: var(--text-sm);
  }

  :global(.gw-ed-publish) {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    gap: var(--space-3);
  }

  :global(.gw-ed-field) {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: 1 1 18rem;
  }

  :global(.gw-ed-label) {
    font-size: var(--text-sm);
    font-weight: 650;
  }

  :global(.gw-ed-input) {
    font: inherit;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--ink);
    inline-size: 100%;
  }

  :global(.gw-ed-help) {
    color: var(--ink-faint);
    font-size: var(--text-xs);
  }

  :global(.gw-ed-note) {
    font-size: var(--text-sm);
  }
  :global(.gw-ed-note--ok) {
    color: var(--ok);
  }
  :global(.gw-ed-note--fail) {
    color: var(--danger);
  }

  :global(.gw-ed-btn) {
    font: inherit;
    font-size: var(--text-sm);
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    background: var(--bg);
    color: var(--ink);
    cursor: pointer;
  }

  :global(.gw-ed-btn:hover:not(:disabled)) {
    background: var(--bg-sunken);
  }

  :global(.gw-ed-btn:disabled) {
    opacity: 0.55;
    cursor: default;
  }

  /* The guard from Dialog.svelte's second lesson, applied where it is actually needed:
     `:not()` takes the weight of its argument, so the modifier has to repeat whatever
     guard the base rule carries or it silently loses. Here the base carries none, so the
     modifier carries none either — stated because the temptation is to add one "for
     safety" and thereby break exactly this rule. */
  :global(.gw-ed-btn--primary) {
    background: var(--accent);
    color: var(--accent-ink);
    border-color: var(--accent);
    font-weight: 650;
  }

  :global(.gw-ed-btn--primary:hover:not(:disabled)) {
    background: color-mix(in srgb, var(--accent) 88%, var(--ink));
  }

  /* --- The editing surface ------------------------------------------------
     It carries `.prose` as well, so every rule in app.css's `@layer content` applies and
     the text is set exactly as it is when read. Only what ProseMirror adds is styled here.

     NOTE the layer situation: a component `<style>` block is UNLAYERED, which outranks
     every layered rule in app.css regardless of specificity. That is why nothing below
     restates a `.prose` rule — doing so would win, and the editing surface would drift
     away from the reading surface one property at a time. */
  :global(.gw-ed-surface) {
    outline: none;
    min-block-size: 12rem;
  }

  :global(.gw-ed-surface:focus-visible) {
    outline: 2px solid var(--focus);
    outline-offset: 4px;
  }

  /* ProseMirror's own placeholder for the caret between two block nodes. Without a visible
     one, putting the caret after a trailing table looks like nothing happened. */
  :global(.gw-ed-surface .ProseMirror-gapcursor) {
    position: relative;
  }

  :global(.gw-ed-surface .ProseMirror-gapcursor::after) {
    content: '';
    display: block;
    border-block-start: 2px solid var(--accent);
    inline-size: 2rem;
  }

  /* A peer's caret and their name. The colour arrives as a custom property from
     `caretColour`, inlined by the extension, so both of these repaint with the theme. */
  :global(.collaboration-carets__caret) {
    position: relative;
    margin-inline: -1px;
    border-inline: 1px solid;
    border-color: var(--accent);
    word-break: normal;
    pointer-events: none;
  }

  :global(.collaboration-carets__label) {
    position: absolute;
    inset-block-start: -1.4em;
    inset-inline-start: -1px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-sm);
    color: var(--accent-ink);
    font-size: var(--text-xs);
    font-weight: 650;
    line-height: 1.4;
    white-space: nowrap;
    user-select: none;
  }

  /* A table being edited needs its cell edges visible; the reader's version can rely on
     row rules alone because nothing there has to be aimed at. */
  :global(.gw-ed-surface td),
  :global(.gw-ed-surface th) {
    border: 1px solid var(--border);
  }

  :global(.gw-ed-surface .selectedCell::after) {
    content: '';
    position: absolute;
    inset: 0;
    background: var(--accent-soft);
    pointer-events: none;
  }

  :global(.gw-ed-surface .selectedCell) {
    position: relative;
  }
</style>
