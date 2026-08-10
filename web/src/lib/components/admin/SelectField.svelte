<!--
  A fixed choice from a short list, on Ark's Select.

  Distinct from ComboField on purpose: four permissions do not need a search box, and a
  text input that accepts anything invites a value the API will reject. The Select cannot
  express a value that is not in the collection.
-->
<script lang="ts">
  import { Select, createListCollection } from '@ark-ui/svelte/select';
  import { Portal } from '@ark-ui/svelte/portal';
  import type { Option } from './ComboField.svelte';

  interface Props {
    label: string;
    options: Option[];
    value: string;
    onChange: (value: string) => void;
    disabled?: boolean;
  }

  let { label, options, value, onChange, disabled = false }: Props = $props();

  const collection = $derived(
    createListCollection<Option>({
      items: options,
      itemToValue: (item) => item.value,
      itemToString: (item) => item.label
    })
  );
</script>

<Select.Root
  {collection}
  {disabled}
  class="gw-adm-select"
  value={[value]}
  onValueChange={(details) => {
    // Ark reports an empty array when a selected item is chosen again. Keeping the
    // current value is right: this field has no "nothing" state to fall back to.
    const [next] = details.value;
    if (next) onChange(next);
  }}
>
  <Select.Label class="gw-adm-field-label">{label}</Select.Label>
  <Select.Control>
    <Select.Trigger class="gw-adm-select-trigger">
      <Select.ValueText placeholder="Bitte wählen" />
      <Select.Indicator>▾</Select.Indicator>
    </Select.Trigger>
  </Select.Control>

  <Portal>
    <Select.Positioner class="gw-adm-popper">
      <Select.Content class="gw-adm-listbox">
        {#each collection.items as item (item.value)}
          <Select.Item {item} class="gw-adm-option">
            <Select.ItemText>{item.label}</Select.ItemText>
            <!-- A tick, not colour, marks the current choice. -->
            <Select.ItemIndicator>✓</Select.ItemIndicator>
          </Select.Item>
        {/each}
      </Select.Content>
    </Select.Positioner>
  </Portal>
</Select.Root>
