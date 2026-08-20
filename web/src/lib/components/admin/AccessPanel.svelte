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

  THE THIRD BELIEF, and the reason the table below is no longer captioned "Wer /x
  erreicht": that a table of entries answers the question. It does not, and it never
  could. `permits()` decides in four steps and only the first is an entry — `Anyone`
  entries are answered before the caller is even known to be signed in, a `public` page is
  readable by the whole internet, an `internal` page by every group with internal reach,
  and a `restricted` page by everybody holding the `admin` baseline, with no entry written
  anywhere. None of those has a row here and none ever will. So the panel answers the
  question in `.gw-adm-reach` above the table, and the table is captioned with what it
  actually lists. The worst thing this screen used to say — "Auf /x ist kein Zugriff
  eingetragen. Es gilt allein die Sichtbarkeit der Seite" — is gone with it: true about
  entries, and read as "nobody else gets in".

  And the badge that said `Sichtbarkeit: Eingeschränkt` now has a control beside it, which
  it did not before: `/api/admin/visibility` is the only write path to that field, gated
  by the same `path_admin` as a grant and audited like one.
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
    inheritedReachTitle,
    reachRoutes,
    revokeResumeText,
    revokeResumeTitle,
    visibilityChangeText,
    visibilityChangeTitle,
    visibilityConsequence
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
    /**
     * The OIDC groups mapped to the `admin` and `internal` baselines (D-M2-1).
     *
     * `null` means this console could not read the mapping — `/api/admin/roles` is
     * instance-wide, so a space admin is refused it. Kept apart from an empty array on
     * purpose: "no group has that reach" is a claim about the instance, and a view that
     * was not allowed to look must not make it.
     */
    adminGroups?: string[] | null;
    internalGroups?: string[] | null;
    busy?: boolean;
    /** Resolves true when the grant landed. The picker is cleared only then. */
    onGrant: (subject: Subject, permission: Permission) => Promise<boolean>;
    onRevoke: (subject: Subject, permission: Permission) => Promise<boolean>;
    /**
     * Change how open this page is. Absent when this console cannot — and then no control
     * is offered at all, rather than one that quietly fails.
     */
    onSetVisibility?: (visibility: string) => Promise<boolean>;
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
    adminGroups = null,
    internalGroups = null,
    busy = false,
    onGrant,
    onRevoke,
    onSetVisibility,
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
          own: definedHere(acl, grant),
          /** The one subject `can()` answers before it knows who the caller is. */
          share: grant.subject.kind === 'anyone'
        }))
      : []
  );

  /**
   * Every way into this page, not only the entries.
   *
   * The panel used to caption a table of grants "Wer /x erreicht" and stop there, which is
   * silent about the two routes that need no entry at all: the `admin` baseline reads
   * every `restricted` page in the corpus, and an `internal` page is open to every group
   * with internal reach. `reach.ts` holds the wording and the reasoning; this hands it the
   * facts.
   */
  const routes = $derived(
    acl
      ? reachRoutes({
          path: acl.path,
          visibility,
          shareLink: [
            ...new Set(
              acl.effective
                .filter((grant) => grant.subject.kind === 'anyone')
                .map((grant) => PERMISSION_LABEL[grant.permission])
            )
          ],
          hasGrants: acl.effective.length > 0,
          adminGroups,
          internalGroups
        })
      : []
  );

  /**
   * What a revoke would hand this page back to, when it would empty the page.
   *
   * `null` unless this path carries exactly one entry AND something above it carries any:
   * removing the last row here makes the nearest ancestor's set apply again, here and on
   * every page below that carries nothing of its own. With more than one entry left, or
   * with nothing above, the revoke really is local.
   */
  /** Whether this path carries exactly one entry, so a revoke would empty it. */
  const lastHere = $derived(acl?.defined_here.length === 1);

  const resuming = $derived.by(() => {
    if (!acl || !lastHere || !acl.ancestor_source) return null;
    return {
      source: acl.ancestor_source,
      labels: acl.ancestor_grants.map(
        (grant) =>
          `${subjectLabel(grant.subject, principals, teams)}: ${PERMISSION_LABEL[grant.permission]}`
      )
    };
  });

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

  /* --- Visibility -------------------------------------------------------------------
   *
   * Least open first, so `Öffentlich` is the one value somebody has to travel to. It
   * publishes the page to the internet, and a list that puts it under the cursor is a
   * list that will eventually be clicked by accident.
   */
  const visibilityOptions: Option[] = ['restricted', 'internal', 'public'].map((value) => ({
    value,
    label: VISIBILITY_LABEL[value] ?? value
  }));

  /** `null` until somebody picks; the page's current value stands in until then. */
  let pickedVisibility = $state<string | null>(null);
  const chosenVisibility = $derived(pickedVisibility ?? visibility ?? 'restricted');

  async function changeVisibility() {
    if (!onSetVisibility || chosenVisibility === visibility) return;
    const changed = await onSetVisibility(chosenVisibility);
    // Cleared only on success, so the choice survives a failed attempt — and so the
    // control falls back to whatever the reloaded page now says.
    if (changed) pickedVisibility = null;
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

      {#if visibility && onSetVisibility}
        <!--
          The control the badge above always implied and never had. Until now nothing in
          this system wrote `documents.visibility` at all: the value arrived from
          frontmatter at import and `seed --update` refuses to change it, so the badge
          named a fact with no door beside it.

          A dialog rather than an inline select, because this is not a preference. It is
          the decision that can put a page on the open internet, and it deserves a
          sentence, a deliberate second click, and an audit row with a name on it.
        -->
        <span class="gw-adm-trigger">
          <Dialog
            title="Sichtbarkeit ändern"
            description={`Wie offen ${acl.path} ist, wenn niemand einen Zugriffseintrag hat.`}
          >
            {#snippet trigger()}
              <span>Sichtbarkeit ändern</span>
            {/snippet}
            {#snippet children()}
              <div class="gw-adm-form">
                <!--
                  The grant dialog's warning, turned around for this decision rather than
                  copied into it. There the surprise is that an entry reaches DOWN the
                  tree; here it is that visibility does not — and, more dangerously, that
                  lowering it closes nothing an entry has opened, because `permits()` asks
                  `can()` about the grants before it ever reads this field.
                -->
                <Notice
                  tone="warn"
                  title={visibilityChangeTitle(acl.path)}
                  text={visibilityChangeText(acl.path)}
                />
                <SelectField
                  label="Sichtbarkeit"
                  options={visibilityOptions}
                  value={chosenVisibility}
                  onChange={(value) => (pickedVisibility = value)}
                />
                <p class="gw-adm-muted">{visibilityConsequence(acl.path, chosenVisibility)}</p>
              </div>
            {/snippet}
            {#snippet footer()}
              <ArkDialog.CloseTrigger class="gw-adm-btn">Abbrechen</ArkDialog.CloseTrigger>
              <!-- Disabled while the choice is the value the page already has: a
                   CloseTrigger closes whatever it was clicked for, so an unchanged click
                   would dismiss the dialog having done nothing at all. -->
              <ArkDialog.CloseTrigger
                class="gw-adm-btn gw-adm-btn--primary"
                disabled={busy || chosenVisibility === visibility}
                onclick={changeVisibility}
              >
                Sichtbarkeit setzen
              </ArkDialog.CloseTrigger>
            {/snippet}
          </Dialog>
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

    <!--
      The panel's actual answer to its own question, and the reason the table below is no
      longer captioned as if it were the answer.

      What this replaces was the worst sentence on the screen: "Auf /x ist kein Zugriff
      eingetragen. Es gilt allein die Sichtbarkeit der Seite." True about entries, and
      read as "nobody else gets in" — while `permits()` hands every `restricted` page in
      the corpus to anybody holding the `admin` baseline, with no entry anywhere, and
      hands an `internal` page to every group with internal reach. Neither has ever had a
      row in this table and neither ever will: they are not entries.

      A plain list, not a stack of `Notice`s: four `role="status"` elements in a row is
      four announcements for one paragraph, and none of these elements is rendered by Ark,
      so the `hidden` trap in Dialog.svelte is not in play here.
    -->
    <section class="gw-adm-reach" aria-labelledby="gw-adm-reach-heading">
      <h3 class="gw-adm-reach-heading" id="gw-adm-reach-heading">Wer {acl.path} erreicht</h3>
      <ul class="gw-adm-reach-list">
        {#each routes as route (route.key)}
          <li class="gw-adm-reach-item gw-adm-reach-item--{route.tone}">
            <span class="gw-adm-reach-title">{route.title}</span>
            {route.text}
          </li>
        {/each}
      </ul>
    </section>

    {#if rows.length > 0}
      <div class="gw-adm-scroll">
        <table class="gw-adm-table">
          <!-- What this table lists, and nothing more. It listed entries all along; the
               old caption promised the whole answer and delivered a quarter of it. -->
          <caption>Zugriffseinträge, die auf {acl.path} gelten</caption>
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
                <th scope="row">
                  {subjectLabel(row.grant.subject, principals, teams)}
                  {#if row.share}
                    <!-- `can()` answers an `Anyone` grant BEFORE it looks at whether the
                         caller is signed in — that is what makes this a share link and
                         not merely a very large group. Unmarked it reads like a team. -->
                    <span class="gw-adm-badge gw-adm-badge--off">Offenes Internet</span>
                  {/if}
                </th>
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
                          <div class="gw-adm-form">
                            {#if resuming}
                              <!--
                                Revoking the LAST entry on a path is not a local change,
                                and the old sentence here said the opposite by omission:
                                "Rechte, die von weiter oben geerbt werden, bleiben davon
                                unberührt" is true, and reads as "they stay where they
                                are". They resume applying HERE — `grants_for_path` returns
                                the rows of the nearest ancestor that has any, so a path
                                with nothing of its own falls through to the one above, and
                                so does every page under it that carries nothing.
                              -->
                              <Notice
                                tone="warn"
                                title={revokeResumeTitle(resuming.source)}
                                text={revokeResumeText(
                                  acl.path,
                                  resuming.source,
                                  resuming.labels
                                )}
                              />
                            {/if}
                            <p class="gw-adm-muted">
                              Der Eintrag wird auf {acl.path} gelöscht.
                              {#if !lastHere}
                                Die übrigen Einträge auf {acl.path} bleiben bestehen — und weil
                                der nächstgelegene Eintrag vollständig und allein gilt, ändert
                                sich an geerbten Rechten hier nichts.
                              {:else if !resuming}
                                Danach trägt weder {acl.path} noch eine übergeordnete Seite
                                einen Eintrag. Wer die Seite dann noch erreicht, steht oben
                                unter »Wer {acl.path} erreicht«.
                              {/if}
                            </p>
                          </div>
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
                system does not have. It must still never point at the visibility control
                two lines up: that control now exists, which makes pointing at it worse
                rather than better, because it would send somebody to a real button that
                does not do what they were told it does.
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
