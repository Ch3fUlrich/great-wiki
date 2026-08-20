<!--
  "Who can reach this page?" — the question the console opens on.

  The one thing this panel exists to make impossible: believing you revoked access that
  you did not revoke. Grants inherit down the tree from the nearest ancestor that has
  any, so most of what applies to a page is not stored on that page and cannot be removed
  from it. The panel therefore says where each grant lives, and offers a revoke control
  ONLY for the grants stored here. An inherited row gets the ancestor's path as text and
  no button at all — a disabled button would still read as "this is the control for this
  row", and a button that quietly does nothing is worse than no button.

  The second belief, added later because it turned out to be the more dangerous one:
  that marking a page `restricted` keeps it away from somebody who holds a grant above
  it. It does not. `permits()` in `crates/gw-store/src/acl.rs` asks `can()` with the
  document presented at `Visibility::Restricted` BEFORE it looks at the real visibility,
  so a matching grant has already decided — "a grant decides on its own, at any
  visibility", as that file puts it. A grant on `/handbuch` therefore reaches every page
  under it whatever those pages say about themselves, and the only thing that stops it is
  a grant row on a page further down, which replaces the inherited set rather than adding
  to it. Both halves of that are now sentences on this screen: `reach.ts` holds the
  wording, the warning sits inside the grant dialog where the decision is made, and the
  explanation sits above the table where the consequence is already in force.
-->
<script lang="ts">
  import Dialog from '$lib/components/Dialog.svelte';
  import { Dialog as ArkDialog } from '@ark-ui/svelte/dialog';
  import { Menu } from '@ark-ui/svelte/menu';
  import { Portal } from '@ark-ui/svelte/portal';
  import ComboField, { type Option } from './ComboField.svelte';
  import SelectField from './SelectField.svelte';
  import Notice from './Notice.svelte';
  import {
    grantConsequence,
    grantReachText,
    grantReachTitle,
    inheritedReachText,
    inheritedReachTitle
  } from './reach.js';
  import {
    PERMISSION_LABEL,
    SUBJECT_KIND_LABEL,
    VISIBILITY_LABEL,
    parseSubjectKey,
    subjectKey,
    subjectLabel,
    type AclView,
    type AdminPrincipal,
    type Grant,
    type Permission,
    type Subject,
    type Team
  } from '$lib/adminApi';

  interface Props {
    /** The selected path, or `null` when nothing has been picked yet. */
    path: string | null;
    acl: AclView | null;
    /**
     * `public` | `internal` | `restricted`, from the document itself.
     *
     * Not from the ACL endpoint, which does not carry it — grants and visibility are two
     * independent gates and the API keeps them apart. It belongs on this screen anyway:
     * a table of grants means something quite different under `public`, where everyone
     * can read the page regardless of what any row below says.
     */
    visibility?: string | null;
    error: string | null;
    principals: AdminPrincipal[];
    teams: Team[];
    busy?: boolean;
    /** Resolves true when the grant landed. The picker is cleared only then. */
    onGrant: (subject: Subject, permission: Permission) => Promise<boolean>;
    onRevoke: (subject: Subject, permission: Permission) => Promise<boolean>;
    /** Move the panel to another path — the ancestor a grant is inherited from, usually. */
    onSelectPath: (path: string) => void;
  }

  let {
    path,
    acl,
    visibility = null,
    error,
    principals,
    teams,
    busy = false,
    onGrant,
    onRevoke,
    onSelectPath
  }: Props = $props();

  /** `/a/b/c` → `/a/b`, and `/a` → null. */
  function parentOf(value: string): string | null {
    const cut = value.lastIndexOf('/');
    return cut > 0 ? value.slice(0, cut) : null;
  }

  /** A grant is revocable here exactly when it is stored here. */
  function definedHere(view: AclView, grant: Grant): boolean {
    return view.defined_here.some(
      (own) =>
        subjectKey(own.subject) === subjectKey(grant.subject) && own.permission === grant.permission
    );
  }

  /**
   * The ancestor these grants are written on — `null` when that ancestor is this page.
   *
   * `Store::effective_grants` walks ancestors NEAREST FIRST and a path is its own first
   * ancestor, so `inherited_from` names the path itself whenever the path carries any
   * grant of its own. Rendering that as "Geerbt von /handbuch" while standing on
   * /handbuch sends somebody up the tree after a row that is already in front of them.
   */
  const inheritedFrom = $derived(
    acl && acl.inherited_from && acl.inherited_from !== acl.path ? acl.inherited_from : null
  );

  const rows = $derived(
    acl
      ? acl.effective.map((grant) => ({
          grant,
          key: `${subjectKey(grant.subject)}|${grant.permission}`,
          own: definedHere(acl, grant)
        }))
      : []
  );

  /** Every subject a grant could name: people, teams, verified OIDC groups, and the two
      that name no one in particular. */
  const subjectOptions = $derived.by<Option[]>(() => {
    const groups = [...new Set(principals.flatMap((person) => person.groups))].sort();
    return [
      ...principals.map((person) => ({
        value: subjectKey({ kind: 'principal', id: person.id }),
        label: `${person.display_name} (${person.username})`,
        hint: SUBJECT_KIND_LABEL.principal
      })),
      ...teams.map((team) => ({
        value: subjectKey({ kind: 'team', id: team.slug }),
        label: `${team.name} (${team.slug})`,
        hint: SUBJECT_KIND_LABEL.team
      })),
      ...groups.map((group) => ({
        value: subjectKey({ kind: 'group', id: group }),
        label: group,
        hint: SUBJECT_KIND_LABEL.group
      })),
      { value: 'authenticated', label: 'Alle angemeldeten Personen', hint: 'Sonderfall' },
      { value: 'anyone', label: 'Alle, auch nicht angemeldete', hint: 'Sonderfall' }
    ];
  });

  const permissionOptions: Option[] = (
    ['read', 'comment', 'write', 'admin'] as Permission[]
  ).map((permission) => ({ value: permission, label: PERMISSION_LABEL[permission] }));

  let newSubject = $state<string | null>(null);
  let newPermission = $state<Permission>('read');

  async function grant() {
    if (!newSubject) return;
    const granted = await onGrant(parseSubjectKey(newSubject), newPermission);
    // Kept on failure, so a retry does not mean picking the same person again.
    if (granted) newSubject = null;
  }
</script>

<div class="gw-adm-section">
  {#if !path}
    <Notice text="Wähle links eine Seite aus, um zu sehen, wer sie erreicht." />
  {:else if error}
    <!-- The failure, in words. Never an empty panel: this endpoint may not exist yet. -->
    <Notice tone="fail" title="Zugriffsrechte nicht geladen." text={error} />
  {:else if !acl}
    <Notice text="Für {path} liegen noch keine Angaben vor." />
  {:else}
    <div class="gw-adm-pathbar">
      <span class="gw-adm-path">{acl.path}</span>
      {#if visibility}
        <span class="gw-adm-badge gw-adm-badge--accent">
          Sichtbarkeit: {VISIBILITY_LABEL[visibility] ?? visibility}
        </span>
      {/if}

      {#if inheritedFrom}
        <span class="gw-adm-badge">Geerbt von {inheritedFrom}</span>
      {/if}

      <!-- A real link, not a menu entry, so it can be opened in a new tab the usual way. -->
      <a class="gw-adm-btn" href={acl.path}>Seite ansehen</a>

      <!--
        Two commands that move the panel rather than change anything. The second is the
        one that matters: from an inherited row, the place the grant can actually be
        revoked is the ancestor, and without this the only way there is to find that path
        again in the tree by hand.
      -->
      <Menu.Root>
        <Menu.Trigger class="gw-adm-btn">Springen ▾</Menu.Trigger>
        <Portal>
          <Menu.Positioner class="gw-adm-popper">
            <Menu.Content class="gw-adm-menu">
              {#if inheritedFrom}
                <Menu.Item
                  value="quelle"
                  class="gw-adm-menu-item"
                  onSelect={() => onSelectPath(inheritedFrom)}
                >
                  Zu {inheritedFrom} – dort lassen sich diese Rechte entziehen
                </Menu.Item>
              {/if}
              {#if parentOf(acl.path)}
                <Menu.Item
                  value="eltern"
                  class="gw-adm-menu-item"
                  onSelect={() => onSelectPath(parentOf(acl.path) as string)}
                >
                  Zur übergeordneten Seite {parentOf(acl.path)}
                </Menu.Item>
              {/if}
              {#if !parentOf(acl.path) && !inheritedFrom}
                <Menu.Item value="keine" class="gw-adm-menu-item" disabled>
                  Von hier führt nichts weiter nach oben
                </Menu.Item>
              {/if}
            </Menu.Content>
          </Menu.Positioner>
        </Portal>
      </Menu.Root>
    </div>

    {#if inheritedFrom}
      <!--
        The sentence the whole panel is built around, and it was a tooltip until it was
        measured: everything Ark renders through a `Portal` is absent from the server
        render, invisible to a touch reader, and only ever seen by somebody who already
        suspected there was something to hover. It changes what every control below
        means, so it is body text.

        What it says is the thing `crates/gw-store/src/acl.rs` puts as "a grant decides
        on its own, at any visibility": `permits()` consults the grants BEFORE it looks
        at the document's visibility, so this page is reached through the entry on the
        ancestor and the `restricted` badge two lines up holds nobody back.
      -->
      <Notice
        tone={visibility === 'public' ? 'info' : 'warn'}
        title={inheritedReachTitle(inheritedFrom, visibility)}
        text={inheritedReachText(acl.path, inheritedFrom, visibility)}
      />
    {/if}

    {#if rows.length === 0}
      <Notice
        text={`Auf ${acl.path} ist kein Zugriff eingetragen. Es gilt allein die Sichtbarkeit der Seite${
          visibility ? ` (${VISIBILITY_LABEL[visibility] ?? visibility})` : ''
        }.`}
      />
    {:else}
      <div class="gw-adm-scroll">
        <table class="gw-adm-table">
          <caption>Wer {acl.path} erreicht</caption>
          <thead>
            <tr>
              <th scope="col">Subjekt</th>
              <th scope="col">Art</th>
              <th scope="col">Berechtigung</th>
              <th scope="col">Eingetragen auf</th>
              <th scope="col">Aktion</th>
            </tr>
          </thead>
          <tbody>
            {#each rows as row (row.key)}
              <tr>
                <th scope="row">{subjectLabel(row.grant.subject, principals, teams)}</th>
                <td>{SUBJECT_KIND_LABEL[row.grant.subject.kind]}</td>
                <td>{PERMISSION_LABEL[row.grant.permission]}</td>
                <td class="gw-adm-mono">
                  {row.own ? acl.path : (inheritedFrom ?? acl.path)}
                </td>
                <td>
                  {#if row.own}
                    <span class="gw-adm-trigger gw-adm-trigger--danger">
                      <Dialog
                        title="Zugriff entziehen?"
                        description={`${subjectLabel(row.grant.subject, principals, teams)} verliert »${PERMISSION_LABEL[row.grant.permission]}« auf ${acl.path} und allen darunter liegenden Seiten, die nichts Eigenes eingetragen haben.`}
                      >
                        {#snippet trigger()}
                          <!-- Not a <button>: Ark's Dialog.Trigger already renders one,
                               and a button inside a button is repaired by the browser. -->
                          <span>Entziehen</span>
                        {/snippet}
                        {#snippet children()}
                          <p class="gw-adm-muted">
                            Der Eintrag wird auf {acl.path} gelöscht. Rechte, die von weiter oben
                            geerbt werden, bleiben davon unberührt.
                          </p>
                        {/snippet}
                        {#snippet footer()}
                          <ArkDialog.CloseTrigger class="gw-adm-btn">
                            Abbrechen
                          </ArkDialog.CloseTrigger>
                          <ArkDialog.CloseTrigger
                            class="gw-adm-btn gw-adm-btn--danger"
                            disabled={busy}
                            onclick={() => onRevoke(row.grant.subject, row.grant.permission)}
                          >
                            Entziehen
                          </ArkDialog.CloseTrigger>
                        {/snippet}
                      </Dialog>
                    </span>
                  {:else}
                    <!-- Deliberately no control. Revoking here would change nothing, and
                         the API would refuse it. -->
                    <span class="gw-adm-muted">Geerbt von {inheritedFrom ?? acl.path}</span>
                  {/if}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}

    <div>
      <span class="gw-adm-trigger gw-adm-trigger--primary">
        <Dialog
          title="Zugriff gewähren"
          description={`Ein neuer Eintrag auf ${acl.path}. Sobald hier etwas eingetragen ist, ersetzt es die geerbten Rechte vollständig.`}
        >
          {#snippet trigger()}
            <span>Zugriff gewähren</span>
          {/snippet}
          {#snippet children()}
            <div class="gw-adm-form">
              <!--
                Where the decision is actually made, which is why it is here and not a
                standing paragraph on the panel: a grant is written on a path and applies
                to the WHOLE subtree under it, and no page down there can opt out by being
                marked `restricted`. `permits()` asks `can()` at `Visibility::Restricted`
                before it ever consults the document's own visibility, so a matching grant
                has already returned true by then.

                The only thing that stops it is a grant row on the descendant itself — the
                nearest ancestor with any rows wins outright and grants are never unioned
                — and the text says so rather than gesturing at a narrowing control this
                system does not have. It must never suggest changing a page's visibility
                either: nothing here writes that field at all.
              -->
              <Notice
                tone="warn"
                title={grantReachTitle(acl.path)}
                text={grantReachText(acl.path)}
              />
              <ComboField
                label="Wer"
                options={subjectOptions}
                value={newSubject}
                onChange={(value) => (newSubject = value)}
                emptyText="Keine Person, kein Team und keine Gruppe passt dazu"
                help="Personen, Teams und verifizierte Authelia-Gruppen."
              />
              <SelectField
                label="Berechtigung"
                options={permissionOptions}
                value={newPermission}
                onChange={(value) => (newPermission = value as Permission)}
              />
              {#if newSubject}
                <!-- The rule above, in the terms of this particular grant. -->
                <p class="gw-adm-muted">
                  {grantConsequence(
                    acl.path,
                    subjectLabel(parseSubjectKey(newSubject), principals, teams),
                    PERMISSION_LABEL[newPermission]
                  )}
                </p>
              {/if}
            </div>
          {/snippet}
          {#snippet footer()}
            <ArkDialog.CloseTrigger class="gw-adm-btn">Abbrechen</ArkDialog.CloseTrigger>
            <!-- Disabled until somebody is chosen. A CloseTrigger closes whatever it was
                 clicked for, so an empty click would dismiss the dialog having granted
                 nothing at all — the exact silent no-op this console must not have. -->
            <ArkDialog.CloseTrigger
              class="gw-adm-btn gw-adm-btn--primary"
              disabled={busy || !newSubject}
              onclick={grant}
            >
              Gewähren
            </ArkDialog.CloseTrigger>
          {/snippet}
        </Dialog>
      </span>
    </div>
  {/if}
</div>
