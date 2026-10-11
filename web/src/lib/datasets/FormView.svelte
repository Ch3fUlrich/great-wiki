<script lang="ts">
  /**
   * A saved FORM view of a dataset (M8c C6): the view's own questions, answered as one new
   * row. The editors are the row editor's cells — one set of controls for one kind of data —
   * and submitting goes through `submitForm`, so the same subset and requiredness the server
   * re-checks are checked here first. Script-driven: without a bundle the form shows itself
   * but has no submit path; a POST needs the typed values this component assembles.
   * Every value reaches the page through `{…}` only.
   */
  import { apiSendBrowser } from './browser';
  import ChoiceCell from './cells/ChoiceCell.svelte';
  import BoolCell from './cells/BoolCell.svelte';
  import ScalarCell from './cells/ScalarCell.svelte';
  import RelationCell from './cells/RelationCell.svelte';
  import { toRaw, type Raw } from './cells/edit';
  import { formFields, submitForm, type Drafts } from './form';
  import type { Field } from './table';
  import type { DatasetView } from './views';

  let {
    path,
    fields,
    view,
    mayWrite
  }: { path: string; fields: Field[]; view: DatasetView; mayWrite: boolean } = $props();

  const items = $derived(formFields(view, fields));
  const title = $derived.by(() => {
    const t = (view.config as { title?: unknown } | null)?.title;
    return typeof t === 'string' && t.trim() !== '' ? t : view.name;
  });

  /* svelte-ignore state_referenced_locally */
  let drafts = $state<Drafts>(
    Object.fromEntries(formFields(view, fields).map((i) => [i.field.key, toRaw(i.field.kind, null)]))
  );
  let errors = $state<Record<string, string>>({});
  let failure = $state<string | null>(null);
  let saved = $state(false);
  let busy = $state(false);

  const text = (key: string) => (drafts[key] as string) ?? '';
  const raw = (key: string): Raw => drafts[key] ?? '';

  async function submit(event: Event) {
    event.preventDefault();
    if (busy) return;
    busy = true;
    errors = {};
    failure = null;
    saved = false;
    try {
      const r = await submitForm(apiSendBrowser, path, view.id, items, drafts);
      if (r.kind === 'invalid') errors = r.errors;
      else if (r.kind === 'failed') failure = r.message;
      else {
        const d: Drafts = {};
        for (const i of items) d[i.field.key] = i.field.kind === 'multi_select' ? [] : '';
        drafts = d;
        saved = true;
      }
    } finally {
      busy = false;
    }
  }
</script>

{#if !mayWrite}
  <p class="form-status" role="status">Zum Ausfüllen ist Schreibrecht nötig.</p>
{:else}
  <form class="datensatz-formular" aria-label="Formular" onsubmit={submit}>
    <h2>{title}</h2>
    {#each items as item (item.field.key)}
      {@const key = item.field.key}
      <label>
        {item.label}{#if item.required}<span aria-hidden="true">*</span><span class="nur-lesen"> (Pflichtfeld)</span>{/if}
        <span
          class="feld"
          role="group"
          aria-label={item.label}
          aria-describedby={errors[key] ? `${key}-fehler` : undefined}
        >
          {#if item.field.kind === 'bool'}
            <BoolCell field={item.field} bind:value={() => text(key), (v) => (drafts[key] = v)} />
          {:else if item.field.kind === 'select' || item.field.kind === 'multi_select' || item.field.kind === 'tags'}
            <ChoiceCell field={item.field} bind:value={() => raw(key), (v) => (drafts[key] = v)} />
          {:else if item.field.kind === 'relation'}
            <RelationCell field={item.field} bind:value={() => raw(key), (v) => (drafts[key] = v)} />
          {:else}
            <ScalarCell field={item.field} bind:value={() => text(key), (v) => (drafts[key] = v)} />
          {/if}
        </span>
        {#if errors[key]}
          <span id={`${key}-fehler`} class="zell-fehler" role="alert">{errors[key]}</span>
        {/if}
      </label>
    {/each}
    <div class="knoepfe">
      <button type="submit" disabled={busy}>Absenden</button>
    </div>
    {#if failure}<p class="formular-fehler" role="alert">{failure}</p>{/if}
    {#if saved}<p class="form-status" role="status">Gespeichert.</p>{/if}
  </form>
{/if}

<style>
  @layer components {
    .datensatz-formular {
      display: grid;
      gap: var(--space-2);
      max-inline-size: 28rem;
      margin-block-start: var(--space-4);
    }
    .datensatz-formular label {
      display: grid;
      gap: var(--space-1);
    }
    .nur-lesen {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
    .feld {
      display: grid;
      gap: var(--space-1);
    }
    :global(.zell-fehler),
    .formular-fehler {
      color: var(--danger, inherit);
    }
  }
</style>
