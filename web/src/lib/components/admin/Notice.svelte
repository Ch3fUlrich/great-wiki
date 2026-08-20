<!--
  The one way this console says something went wrong, or that there is nothing to show.

  It exists so that "the request failed" and "the list is empty" can never render as the
  same blank rectangle. A panel with no data and no explanation is the failure mode the
  console is specifically required not to have: the administration API ships separately
  from this interface, so "the endpoint is not there yet" is a state a person will meet,
  and it has to read as a sentence rather than as an empty table.

  `role="status"` with `aria-live="polite"` on the failing tone, so a screen-reader user
  learns that a request failed without having to go looking for the message.
-->
<script lang="ts">
  interface Props {
    /**
     * `fail` is announced; the others are not, being either expected or benign.
     *
     * `warn` is not a failure — nothing went wrong — but it carries a consequence
     * somebody is about to cause and would otherwise not expect, so it is coloured to be
     * read rather than skimmed past. It stays `role="status"`: interrupting a screen
     * reader for a statement of fact is what `assertive` is for, and this is not that.
     */
    tone?: 'fail' | 'warn' | 'ok' | 'info';
    title?: string;
    text: string;
  }

  let { tone = 'info', title, text }: Props = $props();
</script>

<p
  class="gw-adm-notice gw-adm-notice--{tone}"
  role={tone === 'fail' ? 'alert' : 'status'}
  aria-live={tone === 'fail' ? 'assertive' : 'polite'}
>
  {#if title}<span class="gw-adm-notice-title">{title}</span>{' '}{/if}{text}
</p>
