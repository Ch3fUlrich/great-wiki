<!--
  Einladungen — how a second person gets in.

  This panel is the missing half of a flow that was otherwise finished. `POST/GET/DELETE
  /api/admin/invites` and the acceptance page at `/auth/invite/{token}` shipped with 42
  tests behind them and **no caller**: the console had four tabs and none of them could
  create an invitation, so the only route the interface offered for bringing somebody in
  was »Person anlegen« — which means the owner chooses that person's password and passes
  it over a chat app. D-M2-3 exists precisely to stop that, and without this tab the
  decision was documented, implemented, tested and unreachable.

  Three things this panel must get right, all of them about the link:

  * **It exists exactly once.** The plaintext token is never stored — the table holds only
    its SHA-256 — so the answer to the POST is the only copy that will ever exist. If it is
    lost the invitation is dead and a new one has to be made. That is said on screen,
    before the person navigates away, not in a tooltip.
  * **It is itself a credential.** Whoever holds it can create the account and walk into
    whatever the invitation carries. »Hier ist ein Link« does not convey that.
  * **A listing never carries one.** `InviteSummary` has no token field and no digest
    field, deliberately, and nothing here invents one.

  What is offered mirrors what the API will accept (D-M2-2), so the interface never asks
  for something that is going to be refused: a path grant is bounded by its path and needs
  only path administration, but a team reaches wherever it has been granted, instance-wide,
  so the team field appears only for an instance administrator.

  THE E-MAIL FIELD IS REQUIRED, AND IT IS NOT A NOTE (ADR 0021)

  It is the merge key. When somebody later signs in through Authelia whose address that
  provider asserts as *verified* matches this one, they are the same principal — same
  grants, same history, one credential each way and no second password to lose. An
  invitation with no address can never become that person, and nothing fixes it afterwards
  but withdrawing the link and making another; so this asks for one, refuses to submit
  without it, and says on screen what it is for rather than leaving »warum ist das grau?«
  to be worked out.

  The owner sets it, never the invitee: the acceptance page asks for a display name and a
  password and nothing else. An invitee who could nominate their own merge key could
  nominate somebody else's.
-->
<script lang="ts">
  import Dialog from '$lib/components/Dialog.svelte';
  import { Dialog as ArkDialog } from '@ark-ui/svelte/dialog';
  import { Field } from '@ark-ui/svelte/field';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import Notice from './Notice.svelte';
  import SelectField from './SelectField.svelte';
  import {
    formatInstant,
    INVITE_EMAIL_ERROR,
    INVITE_EMAIL_HELP,
    INVITE_STATE_LABEL,
    isMatchableEmail,
    PERMISSION_LABEL,
    type CreatedInvite,
    type Invite,
    type NewInvite,
    type Permission,
    type Team
  } from '$lib/adminApi';
  import type { TreeNode } from '$lib/api';

  interface Props {
    invites: Invite[] | null;
    /** The page tree, which is where the path field's choices come from. */
    tree: TreeNode[] | null;
    teams: Team[] | null;
    error: string | null;
    /**
     * The invitation just created, carrying the one and only copy of its link.
     *
     * Held by the PAGE and passed down, not kept here: creating one reloads the console
     * through `invalidateAll`, and state that lived in this component would be at the
     * mercy of whether Svelte chose to keep the instance. Losing it loses the link.
     */
    created: CreatedInvite | null;
    /** False for a space admin: a team-carrying invitation is instance admins only. */
    canInviteTeam: boolean;
    busy?: boolean;
    /** Resolves to the created invitation, or null when it was refused. */
    onCreate: (input: NewInvite) => Promise<CreatedInvite | null>;
    onRevoke: (invite: Invite) => Promise<boolean>;
    onDismissCreated: () => void;
  }

  let {
    invites,
    tree,
    teams,
    error,
    created,
    canInviteTeam,
    busy = false,
    onCreate,
    onRevoke,
    onDismissCreated
  }: Props = $props();

  const uid = $props.id();
  const formId = `gw-adm-new-invite-${uid}`;

  /** Every page, flattened, so the path field can offer them as one list. */
  function flatten(nodes: TreeNode[], depth = 0): { value: string; label: string }[] {
    return nodes.flatMap((node) => [
      { value: node.path, label: `${'  '.repeat(depth)}${node.title} (${node.path})` },
      ...flatten(node.children, depth + 1)
    ]);
  }

  const NO_PATH = '';
  const NO_TEAM = '';

  const pathOptions = $derived([
    { value: NO_PATH, label: 'Keine Seite' },
    ...flatten(tree ?? [])
  ]);
  const teamOptions = $derived([
    { value: NO_TEAM, label: 'Kein Team' },
    ...(teams ?? []).map((team) => ({ value: team.slug, label: `${team.name} (${team.slug})` }))
  ]);
  const permissionOptions: { value: Permission; label: string }[] = [
    { value: 'read', label: PERMISSION_LABEL.read },
    { value: 'comment', label: PERMISSION_LABEL.comment },
    { value: 'write', label: PERMISSION_LABEL.write },
    { value: 'admin', label: PERMISSION_LABEL.admin }
  ];

  let username = $state('');
  let email = $state('');
  let path = $state(NO_PATH);
  let permission = $state<Permission>('read');
  let team = $state(NO_TEAM);
  let touched = $state(false);

  const usernameBad = $derived(touched && username.trim().length === 0);

  const emailBad = $derived(touched && !isMatchableEmail(email));
  /**
   * D-M2-20: an invitation carrying neither a page nor a team creates an account that can
   * sign in and see nothing. The API refuses it; the button is disabled rather than
   * letting somebody submit into that refusal.
   */
  const carriesNothing = $derived(path === NO_PATH && team === NO_TEAM);
  const complete = $derived(
    username.trim().length > 0 && isMatchableEmail(email) && !carriesNothing
  );

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    if (!complete) return;
    const made = await onCreate({
      username: username.trim(),
      email: email.trim(),
      path: path === NO_PATH ? undefined : path,
      permission: path === NO_PATH ? undefined : permission,
      team: team === NO_TEAM ? undefined : team
    });
    if (made) {
      username = '';
      email = '';
      path = NO_PATH;
      permission = 'read';
      team = NO_TEAM;
      touched = false;
    }
  }

  /**
   * The absolute link, built in the BROWSER from the origin it is already on.
   *
   * The API deliberately returns an origin-relative path: the only origin it could derive
   * an absolute URL from is the `Host` header, which any client can set, so a link built
   * server-side could be made to point at somebody else's host and collect the token when
   * the recipient clicks it. During server rendering there is no origin to prepend, so
   * the relative form is shown and hydration replaces it — which is honest either way,
   * because the relative form is still the right path on this site.
   */
  const absoluteLink = $derived(
    created
      ? typeof window === 'undefined'
        ? created.delivery.url
        : new URL(created.delivery.url, window.location.origin).toString()
      : ''
  );

  let copied = $state(false);

  async function copyLink() {
    try {
      await navigator.clipboard.writeText(absoluteLink);
      copied = true;
    } catch {
      // Clipboard access can be refused outright, and a button that silently does nothing
      // is worse than one that says it did not work — the link is on screen to select by
      // hand either way.
      copied = false;
    }
  }

  let confirming = $state<Invite | null>(null);

  async function confirmRevoke() {
    const invite = confirming;
    confirming = null;
    if (invite) await onRevoke(invite);
  }

  /** What one invitation hands over, in a sentence rather than three columns of nulls. */
  function carries(invite: Invite): string {
    const parts: string[] = [];
    if (invite.path && invite.permission) {
      parts.push(`${PERMISSION_LABEL[invite.permission]} auf ${invite.path}`);
    }
    if (invite.team) parts.push(`Mitglied im Team »${invite.team}«`);
    return parts.length ? parts.join(', ') : '—';
  }
</script>

<div class="gw-adm-section">
  <div class="gw-adm-section-head">
    <h2 class="gw-adm-h2">Einladungen</h2>
    <span class="gw-adm-trigger gw-adm-trigger--primary">
      <Dialog
        title="Jemanden einladen"
        description="Die eingeladene Person legt ihr Passwort selbst fest. Sie erhalten einen Link zum Weitergeben — great-wiki verschickt keine Post."
      >
        {#snippet trigger()}
          <span>Einladung erstellen</span>
        {/snippet}
        {#snippet children()}
          <!-- `novalidate` for the same reason PeoplePanel gives: the browser's own
               constraint bubbles arrive in the browser's language, not in German. -->
          <form id={formId} class="gw-adm-form" novalidate onsubmit={submit}>
            <Field.Root class="gw-adm-field" invalid={usernameBad} required>
              <Field.Label class="gw-adm-field-label">
                Benutzername <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                bind:value={username}
                autocomplete="off"
                oninput={() => (touched = true)}
              />
              <Field.HelperText class="gw-adm-help">
                Steht fest, sobald die Einladung erstellt ist. Die eingeladene Person wählt
                nur noch Anzeigename und Passwort.
              </Field.HelperText>
              <Field.ErrorText class="gw-adm-error">Ein Benutzername wird gebraucht.</Field.ErrorText>
            </Field.Root>

            <Field.Root class="gw-adm-field" invalid={emailBad} required>
              <Field.Label class="gw-adm-field-label">
                E‑Mail <Field.RequiredIndicator>*</Field.RequiredIndicator>
              </Field.Label>
              <Field.Input
                class="gw-adm-input"
                type="email"
                bind:value={email}
                autocomplete="off"
                oninput={() => (touched = true)}
              />
              <Field.HelperText class="gw-adm-help">{INVITE_EMAIL_HELP}</Field.HelperText>
              <Field.ErrorText class="gw-adm-error">{INVITE_EMAIL_ERROR}</Field.ErrorText>
            </Field.Root>

            <div class="gw-adm-field">
              <SelectField
                label="Seite"
                options={pathOptions}
                value={path}
                disabled={busy}
                onChange={(next) => (path = next)}
              />
            </div>

            {#if path !== NO_PATH}
              <div class="gw-adm-field">
                <SelectField
                  label="Recht auf dieser Seite"
                  options={permissionOptions}
                  value={permission}
                  disabled={busy}
                  onChange={(next) => (permission = next as Permission)}
                />
                <p class="gw-adm-help">
                  Gilt für diese Seite und alles darunter, solange dort nichts Engeres
                  eingetragen ist.
                </p>
              </div>
            {/if}

            {#if canInviteTeam}
              <div class="gw-adm-field">
                <SelectField
                  label="Team (optional)"
                  options={teamOptions}
                  value={team}
                  disabled={busy}
                  onChange={(next) => (team = next)}
                />
              </div>
            {/if}

            {#if carriesNothing}
              <!-- Said while the button is disabled, so »warum ist das grau?« has an
                   answer on screen. -->
              <p class="gw-adm-error">
                Eine Einladung muss eine Seite oder ein Team mitbringen. Sonst entsteht ein
                Konto, das sich anmelden kann und nichts sieht.
              </p>
            {/if}
          </form>
        {/snippet}
        {#snippet footer()}
          <ArkDialog.CloseTrigger class="gw-adm-btn">Abbrechen</ArkDialog.CloseTrigger>
          <ArkDialog.CloseTrigger
            type="submit"
            form={formId}
            class="gw-adm-btn gw-adm-btn--primary"
            disabled={busy || !complete}
          >
            Einladung erstellen
          </ArkDialog.CloseTrigger>
        {/snippet}
      </Dialog>
    </span>
  </div>

  {#if created}
    <!--
      The one and only copy. Rendered as a region with `role="alert"` so it is announced
      rather than silently appearing above the fold, and kept until it is dismissed on
      purpose — a notice that faded would take the link with it.
    -->
    <div class="gw-adm-invite-link" role="alert">
      <h3 class="gw-adm-h3">Einladung für »{created.username}« ist fertig</h3>
      <p>
        Geben Sie diesen Link weiter. Er wird <strong>nur dieses eine Mal</strong> angezeigt
        — great-wiki speichert ihn nicht und kann ihn nicht noch einmal zeigen. Geht er
        verloren, ziehen Sie die Einladung zurück und erstellen eine neue.
      </p>
      <p class="gw-adm-warn">
        <strong>Wer den Link hat, kann das Konto anlegen.</strong> Er ist selbst das
        Passwort, bis er benutzt wurde. Schicken Sie ihn auf einem Weg, dem Sie trauen.
      </p>
      <code class="gw-adm-mono gw-adm-invite-url">{absoluteLink}</code>
      <div class="gw-adm-invite-actions">
        <button type="button" class="gw-adm-btn gw-adm-btn--primary" onclick={copyLink}>
          Kopieren
        </button>
        <button type="button" class="gw-adm-btn" onclick={onDismissCreated}>
          Ich habe ihn weitergegeben
        </button>
        {#if copied}<span class="gw-adm-muted">In die Zwischenablage kopiert.</span>{/if}
      </div>
      <p class="gw-adm-help">
        Gültig bis {formatInstant(created.expires_at)} (UTC). Danach ist der Link wertlos.
      </p>
    </div>
  {/if}

  <Notice
    text="Eingeladene Personen legen ihr Passwort selbst fest. So geht kein Passwort durch einen Chat — und Sie kennen es nie."
  />

  {#if error}
    <Notice tone="fail" title="Einladungen nicht geladen." text={error} />
  {:else if !invites}
    <Notice text="Es liegt keine Liste der Einladungen vor." />
  {:else if invites.length === 0}
    <Notice
      text="Es ist derzeit niemand eingeladen. »Einladung erstellen« legt einen Link an, den Sie weitergeben."
    />
  {:else}
    <div class="gw-adm-scroll">
      <table class="gw-adm-table">
        <caption>Einladungen, die Sie sehen dürfen</caption>
        <thead>
          <tr>
            <th scope="col">Für</th>
            <th scope="col">Bringt mit</th>
            <th scope="col">Eingeladen von</th>
            <th scope="col">Gültig bis</th>
            <th scope="col">Stand</th>
            <th scope="col">Zurückziehen</th>
          </tr>
        </thead>
        <tbody>
          {#each invites as invite (invite.id)}
            <tr>
              <th scope="row">
                <span class="gw-adm-mono">{invite.username}</span>
                {#if invite.email}<span class="gw-adm-muted">{invite.email}</span>{/if}
              </th>
              <td>{carries(invite)}</td>
              <!-- An invitation outlives whoever made it; naming nobody beats inventing one. -->
              <td>{invite.invited_by_name ?? 'jemandem, der nicht mehr da ist'}</td>
              <td>{formatInstant(invite.expires_at)}</td>
              <td><span class="gw-adm-badge">{INVITE_STATE_LABEL[invite.state]}</span></td>
              <td>
                {#if invite.state === 'pending'}
                  <button
                    type="button"
                    class="gw-adm-btn"
                    disabled={busy}
                    aria-label="Einladung für {invite.username} zurückziehen"
                    onclick={() => (confirming = invite)}
                  >
                    Zurückziehen
                  </button>
                {:else if invite.state === 'accepted'}
                  <!-- Withdrawing does nothing to an account that already exists, and the
                       API answers 404 rather than pretending. Say where to go instead. -->
                  <span class="gw-adm-muted">Konto besteht — unter »Personen« deaktivieren</span>
                {:else}
                  <span class="gw-adm-muted">—</span>
                {/if}
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
  title="Einladung zurückziehen?"
  body={confirming
    ? `Der Link für »${confirming.username}« wird sofort wertlos. Wer ihn schon erhalten hat, kann damit kein Konto mehr anlegen. Rückgängig machen lässt sich das nicht — erstellen Sie nötigenfalls eine neue Einladung.`
    : ''}
  confirmLabel="Zurückziehen"
  {busy}
  onConfirm={confirmRevoke}
  onCancel={() => (confirming = null)}
/>
