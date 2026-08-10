<script module lang="ts">
  export interface Option {
    value: string;
    label: string;
    /** A short qualifier shown beside the label — "Team", "Gruppe", a username. */
    hint?: string;
  }
</script>

<!--
  A searchable single-choice field, on Ark's Combobox.

  It exists because the two places this console has to pick something — who a grant is
  about, and who joins a team — are both lists that get long and are both answered by
  typing a name. A `<select>` would work at ten people and be unusable at two hundred.

  Filtering is done here rather than through Ark's `useListCollection`, deliberately.
  That helper snapshots its `initialItems` inside `untrack`, so a collection built from it
  never notices when the props it came from change — and in this console the option list
  arrives from a server load and changes on every mutation. Deriving the collection from
  the props directly is both shorter and correct; the helper's editing operations (insert,
  move, reorder) are for a list the user owns, which this is not.
-->
<script lang="ts">
  import { Combobox, createListCollection } from '@ark-ui/svelte/combobox';
  import { Portal } from '@ark-ui/svelte/portal';

  interface Props {
    label: string;
    options: Option[];
    value: string | null;
    onChange: (value: string | null) => void;
    placeholder?: string;
    /** What to say when the search matches nothing. */
    emptyText?: string;
    help?: string;
    disabled?: boolean;
  }

  let {
    label,
    options,
    value,
    onChange,
    placeholder = 'Tippen, um zu suchen',
    emptyText = 'Keine Treffer',
    help,
    disabled = false
  }: Props = $props();

  let query = $state('');

  const matches = $derived.by(() => {
    const needle = query.trim().toLocaleLowerCase('de');
    if (!needle) return options;
    return options.filter(
      (option) =>
        option.label.toLocaleLowerCase('de').includes(needle) ||
        (option.hint ?? '').toLocaleLowerCase('de').includes(needle)
    );
  });

  const collection = $derived(
    createListCollection<Option>({
      items: matches,
      itemToValue: (item) => item.value,
      itemToString: (item) => item.label
    })
  );
</script>

<Combobox.Root
  {collection}
  {disabled}
  class="gw-adm-combo"
  value={value ? [value] : []}
  onValueChange={(details) => onChange(details.value[0] ?? null)}
  onInputValueChange={(details) => (query = details.inputValue)}
>
  <Combobox.Label class="gw-adm-field-label">{label}</Combobox.Label>
  <Combobox.Control class="gw-adm-combo-control">
    <Combobox.Input class="gw-adm-input" {placeholder} />
    <!-- The trigger is a second control on the same field, so it needs its own name.
         Without one a screen reader announces two unlabelled buttons here. -->
    <Combobox.Trigger class="gw-adm-combo-trigger" aria-label="Liste öffnen">▾</Combobox.Trigger>
  </Combobox.Control>
  {#if help}<span class="gw-adm-help">{help}</span>{/if}

  <!-- Portalled, so the list escapes the dialog's `overflow-y: auto` instead of being
       clipped by it. Nothing inside this Portal is rendered on the server. -->
  <Portal>
    <Combobox.Positioner class="gw-adm-popper">
      <Combobox.Content class="gw-adm-listbox">
        {#each collection.items as item (item.value)}
          <Combobox.Item {item} class="gw-adm-option">
            <Combobox.ItemText>{item.label}</Combobox.ItemText>
            {#if item.hint}<span class="gw-adm-badge">{item.hint}</span>{/if}
          </Combobox.Item>
        {:else}
          <Combobox.Empty class="gw-adm-empty">{emptyText}</Combobox.Empty>
        {/each}
      </Combobox.Content>
    </Combobox.Positioner>
  </Portal>
</Combobox.Root>
