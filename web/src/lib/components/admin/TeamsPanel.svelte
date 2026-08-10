<!--
  Teams: great-wiki's own grouping, distinct from the Authelia groups on the Personen tab.

  A team is the unit a grant should normally name. Granting to a person means revisiting
  the grant when that person leaves; granting to a team means editing one membership list
  and every path that names the team follows. That is worth saying on screen, because the
  Zugriff tab offers both and the difference only shows up months later.
-->
<script lang="ts">
  import Dialog from '$lib/components/Dialog.svelte';
  import { Dialog as ArkDialog } from '@ark-ui/svelte/dialog';
  import { Field } from '@ark-ui/svelte/field';
  import ComboField, { type Option } from './ComboField.svelte';
  import Notice from './Notice.svelte';
  import type { AdminPrincipal, Team } from '$lib/adminApi';

  interface Props {
    teams: Team[] | null;
    /** Needed to turn membership ids into names — the API stores ids and nothing else. */
    principals: AdminPrincipal[];
    error: string | null;
    busy?: boolean;
    onCreateTeam: (slug: string, name: string) => Promise<boolean>;
    onAddMember: (slug: string, principalId: string) => Promise<boolean>;
    onRemoveMember: (slug: string, principalId: string) => Promise<boolean>;
  }

  let {
    teams,
    principals,
    error,
    busy = false,
    onCreateTeam,
    onAddMember,
    onRemoveMember
  }: Props = $props();

  const uid = $props.id();
  const formId = `gw-adm-new-team-${uid}`;

  let slug = $state('');
  let name = $state('');
  let touched = $state({ slug: false, name: false });

  // A slug ends up in URLs and in ACL rows, so it is constrained here rather than left
  // for the server to reject after the dialog has already closed.
  const SLUG_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;
  const slugOk = $derived(SLUG_PATTERN.test(slug.trim()));
  const slugBad = $derived(touched.slug && !slugOk);
  const nameBad = $derived(touched.name && name.trim().length === 0);
  const complete = $derived(slugOk && name.trim().length > 0);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!complete) return;
    if (await onCreateTeam(slug.trim(), name.trim())) {
      slug = '';
      name = '';
      touched = { slug: false, name: false };
    }
  }

  /** One pending choice per team, so two open dialogs cannot overwrite each other. */
  let picked = $state<Record<string, string | null>>({});

  function candidates(team: Team): Option[] {
    const already = new Set(team.members);
    return principals
      .filter((person) => !already.has(person.id))
      .map((person) => ({
        value: person.id,
        label: `${person.display_name} (${person.username})`,
        hint: person.active ? undefined : 'deaktiviert'
      }));
  }

  /**
   * A membership id turned into a person.
   *
   * An id nobody matches is shown as itself rather than hidden — a membership pointing
   * at a principal that no longer exists is exactly the row somebody came here to remove.
   */
  function member(id: string): { id: string; name: string; username: string } {
    const found = principals.find((person) => person.id === id);
    return found
      ? { id, name: found.display_name, username: found.username }
      : { id, name: id, username: 'unbekannt' };
  }

  async function add(team: Team) {
    const id = picked[team.slug];
    if (!id) return;
    if (await onAddMember(team.slug, id)) picked[team.slug] = null;
  }
</script>

<div class="gw-adm-section">
  <div class="gw-adm-section-head">
    <h2 class="gw-adm-h2">Teams</h2>
    <span class="gw-adm-trigger gw-adm-trigger--primary">
      <Dialog
        title="Team anlegen"
        description="Ein Team bündelt Personen, damit Rechte einmal vergeben und danach über die Mitgliederliste gepflegt werden."
      >
        {#snippet trigger()}
          <span>Team anlegen</span>
        {/snippet}
        {#snippet children()}
          <!-- `novalidate`: see the note in PeoplePanel. Ark's `required` triggers the
               browser's own validation, which would pre-empt the German messages here. -->
          <form id={formId} class="gw-adm-form" novalidate onsubmit={submit}>
            <Field.Root class="gw-adm-field" invalid={slugBad} required>
              <Field.Label class="gw-adm-field-label">
                Kürzel <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                bind:value={slug}
                autocomplete="off"
                oninput={() => (touched.slug = true)}
              />
              <Field.HelperText class="gw-adm-help">
                Kleinbuchstaben, Ziffern und Bindestriche, etwa <span class="gw-adm-mono"
                  >redaktion</span
                >. Das Kürzel steht später in den Zugriffseinträgen.
              </Field.HelperText>
              <Field.ErrorText class="gw-adm-error">
                Nur Kleinbuchstaben, Ziffern und einzelne Bindestriche.
              </Field.ErrorText>
            </Field.Root>

            <Field.Root class="gw-adm-field" invalid={nameBad} required>
              <Field.Label class="gw-adm-field-label">
                Name <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                bind:value={name}
                autocomplete="off"
                oninput={() => (touched.name = true)}
              />
              <Field.ErrorText class="gw-adm-error">Ein Name wird gebraucht.</Field.ErrorText>
            </Field.Root>
          </form>
        {/snippet}
        {#snippet footer()}
          <ArkDialog.CloseTrigger class="gw-adm-btn">Abbrechen</ArkDialog.CloseTrigger>
          <!-- Disabled until complete, for the reason spelled out in PeoplePanel: a
               CloseTrigger closes regardless, so an incomplete click would dismiss the
               dialog having done nothing. -->
          <ArkDialog.CloseTrigger
            type="submit"
            form={formId}
            class="gw-adm-btn gw-adm-btn--primary"
            disabled={busy || !complete}
          >
            Anlegen
          </ArkDialog.CloseTrigger>
        {/snippet}
      </Dialog>
    </span>
  </div>

  {#if error}
    <Notice tone="fail" title="Teams nicht geladen." text={error} />
  {:else if !teams}
    <Notice text="Es liegt keine Teamliste vor." />
  {:else if teams.length === 0}
    <Notice text="Es ist noch kein Team angelegt." />
  {:else}
    {#each teams as team (team.slug)}
      <section class="gw-adm-card">
        <div class="gw-adm-section-head">
          <h3 class="gw-adm-h3">
            {team.name}
            <span class="gw-adm-muted gw-adm-mono">{team.slug}</span>
          </h3>
          <span class="gw-adm-trigger">
            <Dialog
              title="Mitglied hinzufügen"
              description={`Wer »${team.name}« beitritt, erhält sofort alles, was diesem Team irgendwo eingeräumt wurde.`}
            >
              {#snippet trigger()}
                <span>Mitglied hinzufügen</span>
              {/snippet}
              {#snippet children()}
                <div class="gw-adm-form">
                  <ComboField
                    label="Person"
                    options={candidates(team)}
                    value={picked[team.slug] ?? null}
                    onChange={(value) => (picked[team.slug] = value)}
                    emptyText="Alle bekannten Personen sind bereits Mitglied"
                  />
                </div>
              {/snippet}
              {#snippet footer()}
                <ArkDialog.CloseTrigger class="gw-adm-btn">Abbrechen</ArkDialog.CloseTrigger>
                <ArkDialog.CloseTrigger
                  class="gw-adm-btn gw-adm-btn--primary"
                  disabled={busy || !picked[team.slug]}
                  onclick={() => add(team)}
                >
                  Hinzufügen
                </ArkDialog.CloseTrigger>
              {/snippet}
            </Dialog>
          </span>
        </div>

        {#if team.members.length === 0}
          <Notice text={`»${team.name}« hat noch keine Mitglieder.`} />
        {:else}
          <div class="gw-adm-scroll">
            <table class="gw-adm-table">
              <caption>Mitglieder von {team.name}</caption>
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Benutzername</th>
                  <th scope="col">Aktion</th>
                </tr>
              </thead>
              <tbody>
                {#each team.members.map(member) as person (person.id)}
                  <tr>
                    <th scope="row">{person.name}</th>
                    <td class="gw-adm-mono">{person.username}</td>
                    <td>
                      <span class="gw-adm-trigger gw-adm-trigger--danger">
                        <Dialog
                          title="Mitglied entfernen?"
                          description={`${person.name} verliert alles, was »${team.name}« irgendwo eingeräumt wurde — sofort, nicht erst bei der nächsten Anmeldung.`}
                        >
                          {#snippet trigger()}
                            <span>Entfernen</span>
                          {/snippet}
                          {#snippet children()}
                            <p class="gw-adm-muted">
                              Das Konto selbst bleibt bestehen; nur die Mitgliedschaft in
                              »{team.name}« endet.
                            </p>
                          {/snippet}
                          {#snippet footer()}
                            <ArkDialog.CloseTrigger class="gw-adm-btn">
                              Abbrechen
                            </ArkDialog.CloseTrigger>
                            <ArkDialog.CloseTrigger
                              class="gw-adm-btn gw-adm-btn--danger"
                              disabled={busy}
                              onclick={() => onRemoveMember(team.slug, person.id)}
                            >
                              Entfernen
                            </ArkDialog.CloseTrigger>
                          {/snippet}
                        </Dialog>
                      </span>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </section>
    {/each}
  {/if}
</div>
