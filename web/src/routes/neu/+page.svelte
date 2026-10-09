<!--
  Neue Seite: a title, where it goes, and optionally a template to copy.

  A plain POST form, so it works with no script at all; every control is a native one, so it is
  keyboard-usable as it stands. The picker lists only templates the caller may read.
-->
<script lang="ts">
  let { data, form } = $props();
  const unter = $derived(form?.unter ?? data.unter);
</script>

<svelte:head>
  <title>Neue Seite – great-wiki</title>
</svelte:head>

<div class="page">
  <h1>Neue Seite</h1>

  {#if form?.fehler}
    <p class="fehler" id="neu-fehler" role="alert">{form.fehler}</p>
  {/if}

  <form method="POST" action="?/anlegen">
    <p>
      <label for="neu-titel">Titel</label><br />
      <input id="neu-titel" name="titel" type="text" required maxlength="200" value={form?.titel ?? ''} />
    </p>
    <p>
      <label for="neu-unter">Unterhalb von (Adresse, leer für die oberste Ebene)</label><br />
      <input id="neu-unter" name="unter" type="text" placeholder="/bereich" value={unter} />
    </p>
    <p>
      <label for="neu-vorlage">Vorlage</label><br />
      <select id="neu-vorlage" name="vorlage">
        <option value="">Leere Seite</option>
        {#each data.vorlagen as vorlage (vorlage.path)}
          <option value={vorlage.path} selected={form?.vorlage === vorlage.path}>
            {vorlage.title}
          </option>
        {/each}
      </select>
    </p>
    <p><button type="submit">Seite anlegen</button></p>
  </form>
</div>
