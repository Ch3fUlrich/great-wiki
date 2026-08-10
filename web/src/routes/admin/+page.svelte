<!--
  The administration console.

  Access-first: it opens on the question that is actually asked — "who can reach this
  page?" — with people and teams reachable from there, rather than on a list of accounts.
  Four tabs, one screen each, no long scroll.

  Every interactive part is Ark UI (D-M2-14): tabs, tree view, dialog, combobox, select,
  switch, menu, tooltip and field. Ark ships no CSS at all, so all appearance comes from
  `admin.css` and every value in it resolves through a token — a component a theme cannot
  repaint would break the plugin contract ADR 0005 exists to protect.
-->
<script lang="ts">
  import { Tabs } from '@ark-ui/svelte/tabs';
  import { goto, invalidateAll } from '$app/navigation';
  import { page } from '$app/state';

  import '$lib/components/admin/admin.css';
  import AccessPanel from '$lib/components/admin/AccessPanel.svelte';
  import AuditPanel from '$lib/components/admin/AuditPanel.svelte';
  import DocumentTree from '$lib/components/admin/DocumentTree.svelte';
  import Notice from '$lib/components/admin/Notice.svelte';
  import PeoplePanel from '$lib/components/admin/PeoplePanel.svelte';
  import TeamsPanel from '$lib/components/admin/TeamsPanel.svelte';
  import {
    addGrant,
    addTeamMember,
    createPrincipal,
    createTeam,
    removeGrant,
    removeTeamMember,
    setPrincipalActive,
    subjectLabel,
    type NewPrincipal,
    type Outcome,
    type Permission,
    type Subject,
    type AdminPrincipal
  } from '$lib/adminApi';
  import type { TreeNode } from '$lib/api';

  let { data } = $props();

  const people = $derived(data.people.data ?? []);
  const teams = $derived(data.teams.data ?? []);

  /** Which tab is showing. Not in the URL: switching tabs needs a script anyway. */
  let tab = $state('zugriff');

  let busy = $state(false);
  let notice = $state<{ tone: 'ok' | 'fail'; text: string } | null>(null);

  /**
   * One path for every mutation: run it, say what happened, reload if it changed anything.
   *
   * Reloading through `invalidateAll` rather than patching local state on purpose. After
   * a grant the effective rights of the selected path may have changed in ways this
   * screen cannot derive — a first grant on a path replaces everything inherited from
   * above — so the only honest thing to show is what the server now says.
   */
  async function run(action: () => Promise<Outcome<unknown>>, success: string): Promise<boolean> {
    if (busy) return false;
    busy = true;
    notice = null;
    try {
      const outcome = await action();
      if (outcome.ok) {
        notice = { tone: 'ok', text: success };
        await invalidateAll();
        return true;
      }
      notice = { tone: 'fail', text: outcome.message };
      return false;
    } finally {
      busy = false;
    }
  }

  function withParam(key: string, value: string) {
    const target = new URL(page.url);
    target.searchParams.set(key, value);
    // `keepFocus` so picking a page in the tree does not throw focus back to the top of
    // the document, which would make the tree unusable from the keyboard.
    return goto(target, { keepFocus: true, noScroll: true });
  }

  function selectPath(path: string) {
    void withParam('pfad', path);
  }

  const currentPath = $derived(data.selectedPath);

  /**
   * The document's visibility, taken from the tree.
   *
   * `/api/admin/acl` does not carry it — grants and visibility are two independent gates
   * and the API keeps them apart — but the access panel is meaningless without it: a page
   * with no grants at all is still readable by everyone if it is `public`.
   */
  function findNode(nodes: TreeNode[], path: string): TreeNode | null {
    for (const node of nodes) {
      if (node.path === path) return node;
      const inside = findNode(node.children, path);
      if (inside) return inside;
    }
    return null;
  }

  const currentVisibility = $derived(
    currentPath && data.tree.data ? (findNode(data.tree.data, currentPath)?.visibility ?? null) : null
  );

  async function grant(subject: Subject, permission: Permission) {
    if (!currentPath) return false;
    return run(
      () => addGrant(currentPath, subject, permission),
      `Zugriff auf ${currentPath} für ${subjectLabel(subject, people, teams)} eingetragen.`
    );
  }

  async function revoke(subject: Subject, permission: Permission) {
    if (!currentPath) return false;
    return run(
      () => removeGrant(currentPath, subject, permission),
      `Zugriff auf ${currentPath} für ${subjectLabel(subject, people, teams)} entzogen.`
    );
  }

  async function create(input: NewPrincipal) {
    return run(() => createPrincipal(input), `»${input.username}« wurde angelegt.`);
  }

  async function setActive(person: AdminPrincipal, active: boolean) {
    return run(
      () => setPrincipalActive(person.id, person.display_name, active),
      active
        ? `»${person.display_name}« ist wieder aktiv.`
        : `»${person.display_name}« ist deaktiviert; bestehende Sitzungen sind beendet.`
    );
  }

  async function newTeam(slug: string, name: string) {
    return run(() => createTeam(slug, name), `Team »${name}« angelegt.`);
  }

  async function addMember(slug: string, principalId: string) {
    const who = people.find((person) => person.id === principalId);
    return run(
      () => addTeamMember(slug, principalId),
      `${who?.display_name ?? principalId} gehört jetzt zu »${slug}«.`
    );
  }

  async function removeMember(slug: string, principalId: string) {
    const who = people.find((person) => person.id === principalId);
    return run(
      () => removeTeamMember(slug, principalId),
      `${who?.display_name ?? principalId} gehört nicht mehr zu »${slug}«.`
    );
  }
</script>

<svelte:head><title>Verwaltung — great-wiki</title></svelte:head>

<div class="gw-adm" id="content">
  <header class="gw-adm-head">
    <div>
      <h1 class="gw-adm-title">Verwaltung</h1>
      <p class="gw-adm-lede">
        Wer erreicht welche Seite, wer ist angelegt, welche Teams gibt es — und was wurde
        wann geändert.
      </p>
    </div>
  </header>

  {#if notice}
    <Notice tone={notice.tone} text={notice.text} />
  {/if}

  <Tabs.Root value={tab} onValueChange={(details) => (tab = details.value ?? 'zugriff')}>
    <Tabs.List class="gw-adm-tablist">
      <Tabs.Trigger value="zugriff" class="gw-adm-tab">Zugriff</Tabs.Trigger>
      <Tabs.Trigger value="personen" class="gw-adm-tab">Personen</Tabs.Trigger>
      <Tabs.Trigger value="teams" class="gw-adm-tab">Teams</Tabs.Trigger>
      <Tabs.Trigger value="protokoll" class="gw-adm-tab">Protokoll</Tabs.Trigger>
      <Tabs.Indicator class="gw-adm-tabindicator" />
    </Tabs.List>

    <Tabs.Content value="zugriff" class="gw-adm-tabpanel">
      <div class="gw-adm-access">
        <div>
          {#if data.tree.error}
            <Notice tone="fail" title="Seitenbaum nicht geladen." text={data.tree.error} />
          {:else if !data.tree.data || data.tree.data.length === 0}
            <Notice text="Es sind keine Seiten vorhanden." />
          {:else}
            <DocumentTree
              nodes={data.tree.data}
              selected={currentPath}
              onSelect={(path) => selectPath(path)}
            />
          {/if}
        </div>

        <AccessPanel
          path={currentPath}
          acl={data.acl.data}
          visibility={currentVisibility}
          error={data.acl.error}
          principals={people}
          {teams}
          {busy}
          onGrant={grant}
          onRevoke={revoke}
          onSelectPath={selectPath}
        />
      </div>
    </Tabs.Content>

    <Tabs.Content value="personen" class="gw-adm-tabpanel">
      <PeoplePanel
        principals={data.people.data}
        error={data.people.error}
        {busy}
        onCreate={create}
        onSetActive={setActive}
      />
    </Tabs.Content>

    <Tabs.Content value="teams" class="gw-adm-tabpanel">
      <TeamsPanel
        teams={data.teams.data}
        principals={people}
        error={data.teams.error}
        {busy}
        onCreateTeam={newTeam}
        onAddMember={addMember}
        onRemoveMember={removeMember}
      />
    </Tabs.Content>

    <Tabs.Content value="protokoll" class="gw-adm-tabpanel">
      <AuditPanel
        page={data.audit.data}
        error={data.audit.error}
        principals={people}
        limit={data.limit}
        onLimitChange={(limit) => void withParam('anzahl', String(limit))}
      />
    </Tabs.Content>
  </Tabs.Root>
</div>
