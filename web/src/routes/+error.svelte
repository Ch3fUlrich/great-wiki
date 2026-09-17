<!--
  The page every refusal and every failure lands on.

  In German, and addressed as »Sie«, like the sign-in page, the invitation page and the
  administration console. It was in English — "Something went wrong", "Back to the start
  page" — until the invitation flow was walked by a second person, and it is the screen an
  invited relative is most likely to meet without anybody having meant to send them there:
  a link from a chat to a page they were not granted, a bookmark to a page since deleted.

  The message itself comes from whichever loader refused, so it is that loader's job to be
  in German too. What is here is the fallback for a failure that carried no message at all
  — SvelteKit's own, mostly — and it must not be a translation of "Something went wrong"
  that says nothing: a person who cannot tell "I am not allowed" from "it is broken" does
  not know whether to ask for access or to report a fault, so the status is shown plainly
  beside it.
-->
<script lang="ts">
  import { page } from '$app/state';

  /**
   * A sentence for a failure that arrived without one.
   *
   * Only the statuses a reader can actually reach are named. Anything else falls through
   * to the general sentence rather than to an English default.
   */
  const fallback = $derived(
    page.status === 403
      ? 'Diese Seite ist nicht für Sie freigegeben.'
      : page.status === 404
        ? 'Diese Seite gibt es nicht.'
        : page.status === 401
          ? 'Dafür müssen Sie angemeldet sein.'
          : 'Da ist etwas schiefgegangen. Bitte versuchen Sie es noch einmal.'
  );
</script>

<svelte:head><title>{page.status} — great-wiki</title></svelte:head>

<main class="wrap">
  <h1>{page.status}</h1>
  <p>{page.error?.message ?? fallback}</p>
  <p><a href="/">Zurück zur Startseite</a></p>
</main>
