<!--
  Everyone great-wiki knows about, where they came from, and whether they can still sign in.

  great-wiki never writes Authelia's user database (ADR 0002). A row with the source
  "Authelia" is a mirror of a homelab account — created here on that person's first
  sign-in and refreshed from the verified `groups` claim on every later one. Deactivating
  such a row stops them reaching great-wiki and does nothing to their homelab account.
  That is said on screen, not only here, because it is the single most likely thing for
  somebody to get wrong on this tab.
-->
<script lang="ts">
  import { tick } from 'svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import { Dialog as ArkDialog } from '@ark-ui/svelte/dialog';
  import { Field } from '@ark-ui/svelte/field';
  import { Switch } from '@ark-ui/svelte/switch';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import Notice from './Notice.svelte';
  import { SOURCE_LABEL, type AdminPrincipal, type NewPrincipal } from '$lib/adminApi';

  interface Props {
    principals: AdminPrincipal[] | null;
    error: string | null;
    busy?: boolean;
    /** Resolves true when the account was created. Used to clear the form, and only then. */
    onCreate: (input: NewPrincipal) => Promise<boolean>;
    onSetActive: (person: AdminPrincipal, active: boolean) => Promise<boolean>;
  }

  let { principals, error, busy = false, onCreate, onSetActive }: Props = $props();

  // `$props.id()` has to be its own declaration — Svelte rejects it inside an
  // expression, including a template literal.
  const uid = $props.id();
  const formId = `gw-adm-new-person-${uid}`;

  let username = $state('');
  let displayName = $state('');
  let email = $state('');
  let password = $state('');
  /** A field complains only once it has been typed in. Nobody wants three red lines on
      a form they have not started filling out. */
  let touched = $state({ username: false, displayName: false, password: false });

  /** D-M2-16: length only, twelve characters. No composition rules. */
  const MIN_PASSWORD = 12;
  const usernameBad = $derived(touched.username && username.trim().length === 0);
  const displayNameBad = $derived(touched.displayName && displayName.trim().length === 0);
  const passwordBad = $derived(touched.password && password.length < MIN_PASSWORD);
  const complete = $derived(
    username.trim().length > 0 && displayName.trim().length > 0 && password.length >= MIN_PASSWORD
  );

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!complete) return;
    const created = await onCreate({
      username: username.trim(),
      display_name: displayName.trim(),
      email: email.trim() || undefined,
      password
    });
    // Kept on failure — a rejected username is worth retyping once, not the whole form.
    if (created) {
      username = '';
      displayName = '';
      email = '';
      password = '';
      touched = { username: false, displayName: false, password: false };
    }
  }

  // --- The switch, and the fight it puts up -------------------------------
  //
  // Ark's Switch owns its checked state even when a value is passed in: it flips on
  // click and reports afterwards, and there is no hook that can refuse the change. For
  // activation that is fine. Deactivation has to be confirmed first, and a switch sitting
  // at "off" while a dialog is still asking is claiming something that has not happened.
  //
  // The only lever Ark leaves is to build the control again: `{#key}` on a token that
  // changes whenever an attempt ends without the data behind it changing — a confirmation
  // raised, a confirmation cancelled, a request refused. A rebuilt switch reads the prop
  // again, which is the truth.
  //
  // Rebuilding costs the focus, and that cost is not optional to pay back. Ark's dialog
  // restores focus on close to the element that had it on open — which this rebuild has
  // just destroyed, so focus lands on <body> and a keyboard user is dumped at the top of
  // the document. `focusSwitch` puts it back on the new control.
  let confirming = $state<AdminPrincipal | null>(null);
  let switchToken = $state(0);

  function focusSwitch(personId: string) {
    if (typeof document === 'undefined') return;
    // After Ark's own restore attempt, not before it, or it would immediately undo this.
    setTimeout(() => {
      document
        .querySelector<HTMLElement>(`[data-gw-switch="${CSS.escape(personId)}"] input`)
        ?.focus();
    }, 0);
  }

  async function rebuildSwitch(personId: string) {
    // The first `tick` is not padding. This runs from inside Ark's own change handler,
    // and destroying the component that is still executing it makes Svelte warn
    // `derived_inert` — the machine reads its own deriveds after the teardown. Yielding
    // first lets Ark finish before the control is replaced under it.
    await tick();
    switchToken += 1;
    await tick();
    focusSwitch(personId);
  }

  async function requested(person: AdminPrincipal, next: boolean) {
    if (!next) {
      confirming = person;
      // Ark has already flipped its control. Put it back before the dialog is answered.
      await rebuildSwitch(person.id);
      return;
    }
    const ok = await onSetActive(person, true);
    if (!ok) await rebuildSwitch(person.id);
  }

  async function confirmDeactivate() {
    const person = confirming;
    confirming = null;
    if (!person) return;
    // The control already shows the truth — it was rebuilt when the dialog opened — so
    // there is nothing to undo here either way. On success the reloaded data flips the
    // prop and the switch follows it; on failure the message above says why it did not.
    focusSwitch(person.id);
    await onSetActive(person, false);
  }
</script>

<div class="gw-adm-section">
  <div class="gw-adm-section-head">
    <h2 class="gw-adm-h2">Personen</h2>
    <span class="gw-adm-trigger gw-adm-trigger--primary">
      <Dialog
        title="Lokales Konto anlegen"
        description="Ein great-wiki-Konto für jemanden ohne Homelab-Zugang. Es entsteht mit keinerlei Rechten; Zugriff wird anschließend unter »Zugriff« vergeben."
      >
        {#snippet trigger()}
          <span>Person anlegen</span>
        {/snippet}
        {#snippet children()}
          <!--
            `novalidate` deliberately. `Field.Root required` puts `required` on the input,
            and without this the browser's own constraint validation intercepts the submit
            before `onsubmit` ever runs — so the German error text below never appeared and
            the person got a bubble in whatever language their browser is in instead. The
            fields keep `required` for the accessibility tree; the checking is ours.
          -->
          <form id={formId} class="gw-adm-form" novalidate onsubmit={submit}>
            <Field.Root class="gw-adm-field" invalid={usernameBad} required>
              <Field.Label class="gw-adm-field-label">
                Benutzername <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                bind:value={username}
                autocomplete="off"
                oninput={() => (touched.username = true)}
              />
              <Field.ErrorText class="gw-adm-error">
                Ein Benutzername wird gebraucht.
              </Field.ErrorText>
            </Field.Root>

            <Field.Root class="gw-adm-field" invalid={displayNameBad} required>
              <Field.Label class="gw-adm-field-label">
                Anzeigename <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                bind:value={displayName}
                autocomplete="off"
                oninput={() => (touched.displayName = true)}
              />
              <Field.ErrorText class="gw-adm-error">Ein Anzeigename wird gebraucht.</Field.ErrorText>
            </Field.Root>

            <Field.Root class="gw-adm-field">
              <Field.Label class="gw-adm-field-label">E‑Mail (optional)</Field.Label>
              <Field.Input class="gw-adm-input" type="email" bind:value={email} autocomplete="off" />
              <Field.HelperText class="gw-adm-help">
                Wird nur zur Wiedererkennung gespeichert. great-wiki verschickt keine Post.
              </Field.HelperText>
            </Field.Root>

            <Field.Root class="gw-adm-field" invalid={passwordBad} required>
              <Field.Label class="gw-adm-field-label">
                Passwort <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                type="password"
                bind:value={password}
                autocomplete="new-password"
                oninput={() => (touched.password = true)}
              />
              <Field.HelperText class="gw-adm-help">
                Mindestens {MIN_PASSWORD} Zeichen. Keine Vorgaben zu Zeichenarten — eine lange
                Passphrase ist besser als ein kurzes Passwort mit Sonderzeichen.
              </Field.HelperText>
              <Field.ErrorText class="gw-adm-error">
                Mindestens {MIN_PASSWORD} Zeichen.
              </Field.ErrorText>
            </Field.Root>
          </form>
        {/snippet}
        {#snippet footer()}
          <ArkDialog.CloseTrigger class="gw-adm-btn">Abbrechen</ArkDialog.CloseTrigger>
          <!--
            Three things are load-bearing about this one button.

            `form=` rather than nesting: the shared dialog puts children and footer in two
            different boxes, so the submit button cannot sit inside the `<form>`.
            Attaching it by id keeps Enter-to-submit working from every field.

            It is Ark's `CloseTrigger`, because `Dialog.svelte` renders an uncontrolled
            `Dialog.Root` and there is no other way to close it from here. Closing is what
            makes the outcome visible: the success or failure sentence is rendered behind
            the dialog, where a modal backdrop would hide it.

            And it is disabled until the form is complete — because a CloseTrigger closes
            whatever the click was for, so a click on an incomplete form would dismiss the
            dialog and silently do nothing. The field-level messages above say what is
            missing while it is disabled.
          -->
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

  <Notice
    text="Homelab-Konten werden in der Konten-App verwaltet, nicht hier. great-wiki spiegelt sie nur und schreibt niemals in die Benutzerdatenbank von Authelia."
  />

  {#if error}
    <Notice tone="fail" title="Personen nicht geladen." text={error} />
  {:else if !principals}
    <Notice text="Es liegt keine Personenliste vor." />
  {:else if principals.length === 0}
    <Notice text="Es ist noch niemand angelegt. Auch keine Anmeldung über Authelia hat bisher stattgefunden." />
  {:else}
    <div class="gw-adm-scroll">
      <table class="gw-adm-table">
        <caption>Alle Personen, die great-wiki kennt</caption>
        <thead>
          <tr>
            <th scope="col">Name</th>
            <th scope="col">Quelle</th>
            <th scope="col">Gruppen</th>
            <th scope="col">Teams</th>
            <th scope="col">Status</th>
          </tr>
        </thead>
        <tbody>
          {#each principals as person (person.id)}
            <tr>
              <th scope="row">
                {person.display_name}
                <span class="gw-adm-muted gw-adm-mono">{person.username}</span>
              </th>
              <td>
                <span class="gw-adm-badge">{SOURCE_LABEL[person.kind]}</span>
              </td>
              <td>{person.groups.length ? person.groups.join(', ') : '—'}</td>
              <td>{person.teams.length ? person.teams.join(', ') : '—'}</td>
              <td data-gw-switch={person.id}>
                {#key `${person.id}:${switchToken}`}
                  <Switch.Root
                    class="gw-adm-switch"
                    checked={person.active}
                    disabled={busy}
                    onCheckedChange={(details) => requested(person, details.checked)}
                  >
                    <Switch.Control class="gw-adm-switch-control">
                      <Switch.Thumb class="gw-adm-switch-thumb" />
                    </Switch.Control>
                    <!-- The state is written out, not only shown as a position. A switch
                         whose meaning depends on which end the knob is at is unreadable
                         for anyone who cannot see it. -->
                    <Switch.Label>{person.active ? 'Aktiv' : 'Deaktiviert'}</Switch.Label>
                    <Switch.HiddenInput />
                  </Switch.Root>
                {/key}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>

<ConfirmDialog
  open={confirming !== null}
  title="Konto deaktivieren?"
  body={confirming
    ? `»${confirming.display_name}« (${confirming.username}) kann sich danach nicht mehr anmelden, und alle bestehenden Sitzungen enden sofort. Erteilte Rechte bleiben eingetragen und gelten wieder, sobald das Konto reaktiviert wird.`
    : ''}
  confirmLabel="Deaktivieren"
  {busy}
  onConfirm={confirmDeactivate}
  onCancel={() => {
    const person = confirming;
    confirming = null;
    // Nothing to undo — the control was rebuilt when the dialog opened — but the focus
    // Ark tried to restore went to an element that no longer exists.
    if (person) focusSwitch(person.id);
  }}
/>
