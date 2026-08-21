<!--
  Mounts AccessPanel.svelte in isolation for behaviour.mjs Group H. Static props, no
  API: what these checks are about is what the panel SAYS about the reach of a grant,
  and that has to be true of the component, not of the fixture's database.
-->
<script lang="ts">
  import '$lib/components/admin/admin.css';
  import AccessPanel from '$lib/components/admin/AccessPanel.svelte';
  import type { AclView, AdminPrincipal, Team } from '$lib/adminApi';
  import type { PageData } from './$types';

  let { data }: { data: PageData } = $props();

  const principals: AdminPrincipal[] = [
    {
      id: 'p1',
      kind: 'oidc',
      username: 'sergej',
      display_name: 'Sergej Maul',
      email: null,
      groups: ['admins'],
      teams: ['redaktion'],
      active: true
    }
  ];

  const teams: Team[] = [{ slug: 'redaktion', name: 'Redaktion', members: [] }];

  const geerbt: AclView = {
    path: '/handbuch/onboarding',
    effective: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }],
    inherited_from: '/handbuch',
    defined_here: [],
    ancestor_source: '/handbuch',
    ancestor_grants: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }]
  };

  // `inherited_from` is the path itself whenever that path carries any grant of its own
  // — `Store::effective_grants` returns the first ancestor WITH rows, and a path is its
  // own first ancestor. The panel must not call that "inherited".
  const eigen: AclView = {
    path: '/handbuch',
    effective: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }],
    inherited_from: '/handbuch',
    defined_here: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }],
    // `/handbuch` is a top-level page, so nothing above it carries anything: revoking the
    // one entry here brings nothing back, and the dialog must not claim otherwise.
    ancestor_source: null,
    ancestor_grants: []
  };

  /**
   * The LAST entry on a path that has an ancestor carrying some.
   *
   * Removing it makes `/oberhalb`'s entries apply here again, and on every page below
   * that carries nothing of its own — `grants_for_path` returns the rows of the nearest
   * ancestor that has any. The revoke dialog said the opposite by omission until now.
   */
  const letzter: AclView = {
    path: '/oberhalb/unterseite',
    effective: [{ subject: { kind: 'principal', id: 'p1' }, permission: 'admin' }],
    inherited_from: '/oberhalb/unterseite',
    defined_here: [{ subject: { kind: 'principal', id: 'p1' }, permission: 'admin' }],
    ancestor_source: '/oberhalb',
    ancestor_grants: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }]
  };

  /** A page with a public share link on it, and no entry above. */
  const freigabe: AclView = {
    path: '/geteilt',
    effective: [{ subject: { kind: 'anyone' }, permission: 'read' }],
    inherited_from: '/geteilt',
    defined_here: [{ subject: { kind: 'anyone' }, permission: 'read' }],
    ancestor_source: null,
    ancestor_grants: []
  };

  const faelle: Record<string, AclView> = { geerbt, eigen, letzter, freigabe };
  const acl = $derived(faelle[data.fall] ?? geerbt);
</script>

<!--
  Real links, so `behaviour.mjs` can drive an actual client-side navigation between two
  mounted states of the SAME AccessPanel instance — exactly what `routes/admin/+page.svelte`
  does via `selectPath`'s `goto()`, and exactly the transition I1 needs a browser to prove:
  a same-origin `?fall=` link is intercepted by SvelteKit's router, which re-runs this
  route's load and updates `data.fall` in place, never remounting the component below.
  `page.goto()` in Playwright would instead be a hard navigation and could not expose the
  bug even if the fix were reverted, so behaviour.mjs must click one of these rather than
  navigate the address bar directly.
-->
<nav aria-label="Testfälle">
  {#each Object.keys(faelle) as fall (fall)}
    <a href="?fall={fall}">{fall}</a>
  {/each}
</nav>

<AccessPanel
  path={acl.path}
  {acl}
  visibility="restricted"
  error={null}
  {principals}
  {teams}
  adminGroups={['admins']}
  internalGroups={['users']}
  onGrant={async () => true}
  onRevoke={async () => true}
  onSetVisibility={async () => true}
  onSelectPath={() => {}}
/>
