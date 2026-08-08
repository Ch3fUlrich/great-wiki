<script lang="ts">
  import type { Me } from '$lib/api';

  let { me }: { me: Me } = $props();

  /** The reach a baseline confers (D-M2-1), said plainly rather than as a keyword. */
  const REACH: Record<Me['baseline'], string> = {
    public: 'Öffentliche Seiten',
    internal: 'Öffentlich und intern',
    admin: 'Alle Seiten'
  };
</script>

<!--
  Plain links and a plain form. Signing in is a navigation and signing out is a POST that
  deletes a row, so both work with no JavaScript at all — which matters here more than
  elsewhere, because a login that needs a script is a login that can fail silently.

  The whole thing is one landmark labelled "Konto", so a screen-reader user can jump to it
  instead of hunting through the header.
-->
<nav class="account no-print" aria-label="Konto">
  {#if me.authenticated}
    <span class="who">
      <span class="name">{me.display_name ?? me.username}</span>
      <span class="reach">{REACH[me.baseline]}</span>
    </span>

    {#if me.groups.length}
      <!-- Why they can see what they can see. `title` alone would be invisible to a
           keyboard user, so the list is real text, merely small. -->
      <span class="groups">
        <span class="sr-only">Gruppen:</span>
        {me.groups.join(', ')}
      </span>
    {/if}

    <form method="post" action="/auth/logout">
      <button type="submit">Abmelden</button>
    </form>
  {:else if me.login_available}
    <a class="signin" href="/auth/login" rel="nofollow">Anmelden</a>
  {:else}
    <span class="reach">Nicht angemeldet</span>
  {/if}
</nav>

<style>
  .account {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    font-size: var(--text-sm);
  }

  .who {
    display: flex;
    flex-direction: column;
    line-height: 1.2;
  }

  .name {
    color: var(--ink);
    font-weight: 650;
  }

  .reach,
  .groups {
    color: var(--ink-faint);
    font-size: var(--text-xs);
  }

  /* One shared shape for the two controls, so "Anmelden" and "Abmelden" do not sit at
     different weights in the same row. */
  .signin,
  button {
    display: inline-flex;
    align-items: center;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--ink);
    font: inherit;
    font-size: var(--text-sm);
    text-decoration: none;
    cursor: pointer;
  }

  .signin:hover,
  button:hover {
    border-color: var(--border-strong);
    background: var(--bg-sunken);
  }

  form {
    margin: 0;
  }

  /* Below the narrow breakpoint the header has no room for the explanatory line. It is
     removed from the accessibility tree too, not merely hidden, because a screen reader
     announcing text nobody can see is worse than not having it. */
  @media (max-width: 40rem) {
    .reach,
    .groups {
      display: none;
    }
  }

  /* Visible to a screen reader, invisible on screen. `display: none` would hide it from
     both, which defeats the purpose of the label. */
  .sr-only {
    position: absolute;
    inline-size: 1px;
    block-size: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
