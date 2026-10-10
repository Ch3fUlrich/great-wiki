<script lang="ts">
  /**
   * Adds a row (`row` null) or edits one. The draft lives here as form entries; saving sends
   * only validated, typed values (`submitRow`). A 409 shows the row as it is now, as text, and
   * keeps the draft so nothing typed is lost. Every value reaches the page through `{…}` only.
   */
  import { invalidateAll } from '$app/navigation';
  import { apiSendBrowser } from './browser';
  import BoolCell from './cells/BoolCell.svelte';
  import ScalarCell from './cells/ScalarCell.svelte';
  import { toRaw, type Raw } from './cells/edit';
  import { editableFields, submitRow, type Drafts } from './rowedit';
  import { cellOf, type Field, type Row } from './table';

  let {
    path,
    fields,
    row = null,
    onclose
  }: { path: string; fields: Field[]; row?: Row | null; onclose?: () => void } = $props();

  const editable = $derived(editableFields(fields));
  // The draft and the version the next save is measured against start from the row given once;
  // "Aktuelle Version übernehmen" moves the version, never the draft.
  /* svelte-ignore state_referenced_locally */
  let base = $state<Row | null>(row);
  /* svelte-ignore state_referenced_locally */
  let drafts = $state<Drafts>(
    Object.fromEntries(editableFields(fields).map((f) => [f.key, toRaw(f.kind, row?.values?.[f.key])]))
  );
  let errors = $state<Record<string, string>>({});
  let failure = $state<string | null>(null);
  let stale = $state<Row | null>(null);
  let busy = $state(false);

  const text = (key: string) => (drafts[key] as string) ?? '';
  const textOf = (f: Field, v: unknown) => {
    const c = cellOf(f, v);
    return c.kind === 'chips' ? c.items.join(', ') : c.kind === 'error' ? c.reason : c.text;
  };

  async function save(event: Event) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    errors = {};
    failure = null;
    try {
      const r = await submitRow(apiSendBrowser, path, fields, base, drafts);
      if (r.kind === 'invalid') errors = r.errors;
      else if (r.kind === 'unchanged') failure = 'Nichts geändert.';
      else if (r.kind === 'stale') stale = r.current;
      else if (r.kind === 'failed') failure = r.message;
      else {
        stale = null;
        if (!row) {
          const d: Drafts = {};
          for (const f of editable) d[f.key] = '';
          drafts = d;
        }
        onclose?.();
        await invalidateAll();
      }
    } finally {
      busy = false;
    }
  }

  function adopt() {
    base = stale;
    stale = null;
  }
</script>

<form class="zeile-editor" onsubmit={save} aria-label={row ? 'Zeile bearbeiten' : 'Zeile hinzufügen'}>
  <h3>{row ? 'Zeile bearbeiten' : 'Zeile hinzufügen'}</h3>
  {#each editable as f (f.key)}
    <label>
      {f.label}
      {#if f.kind === 'bool'}
        <BoolCell field={f} bind:value={() => text(f.key), (v) => (drafts[f.key] = v)} error={errors[f.key]} />
      {:else}
        <ScalarCell field={f} bind:value={() => text(f.key), (v) => (drafts[f.key] = v)} error={errors[f.key]} />
      {/if}
    </label>
  {/each}

  {#if stale}
    <div class="konflikt" role="alert">
      <p>Die Zeile wurde inzwischen von jemand anderem geändert. Dein Entwurf bleibt erhalten.</p>
      <dl>
        {#each editable as f (f.key)}
          <dt>{f.label}</dt>
          <dd>{textOf(f, stale.values?.[f.key])}</dd>
        {/each}
      </dl>
      <button type="button" onclick={adopt}>Aktuelle Version übernehmen</button>
    </div>
  {/if}

  <div class="knoepfe">
    <button type="submit" disabled={busy}>Speichern</button>
    {#if onclose}<button type="button" onclick={() => onclose?.()}>Abbrechen</button>{/if}
  </div>
  {#if failure}<p class="tabelle-fehler" role="alert">{failure}</p>{/if}
</form>

<style>
  @layer components {
    .zeile-editor {
      display: grid;
      gap: var(--space-2);
      max-inline-size: 28rem;
      margin-block-start: var(--space-4);
    }
    .zeile-editor label {
      display: grid;
      gap: var(--space-1);
    }
    .konflikt {
      border: 1px solid var(--border);
      padding: var(--space-2) var(--space-3);
    }
    .knoepfe {
      display: flex;
      gap: var(--space-2);
    }
    :global(.zell-fehler),
    .tabelle-fehler {
      color: var(--danger, inherit);
    }
  }
</style>
