<script lang="ts">
  import { page } from '$app/state';
</script>

<main class="wrap">
  <h1>{page.status}</h1>
  <p>{page.error?.message ?? 'Something went wrong.'}</p>
  <p><a href="/">Back to the start page</a></p>
</main>
