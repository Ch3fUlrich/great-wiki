<script lang="ts">
  import { NOTIFICATIONS_PATH } from '$lib/notifications';

  /** Unread count; `null` or 0 renders nothing (a failed count request is not a zero). */
  let { count }: { count: number | null } = $props();
</script>

{#if count && count > 0}
  <a
    class="bell no-print"
    href={NOTIFICATIONS_PATH}
    aria-label={`Benachrichtigungen, ${count} ungelesen`}
  >
    <span aria-hidden="true">&#128276;</span>
    <span class="badge" aria-hidden="true">{count}</span>
  </a>
{/if}

<style>
  .bell {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--ink);
    text-decoration: none;
    font-size: var(--text-sm);
  }
  .badge {
    padding: 0 var(--space-2);
    border-radius: 999px;
    background: var(--ink);
    color: var(--bg-raised);
    font-size: var(--text-xs);
  }
</style>
