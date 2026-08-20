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
    defined_here: []
  };

  // `inherited_from` is the path itself whenever that path carries any grant of its own
  // — `Store::effective_grants` returns the first ancestor WITH rows, and a path is its
  // own first ancestor. The panel must not call that "inherited".
  const eigen: AclView = {
    path: '/handbuch',
    effective: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }],
    inherited_from: '/handbuch',
    defined_here: [{ subject: { kind: 'team', id: 'redaktion' }, permission: 'write' }]
  };

  const acl = $derived(data.fall === 'eigen' ? eigen : geerbt);
</script>

<AccessPanel
  path={acl.path}
  {acl}
  visibility="restricted"
  error={null}
  {principals}
  {teams}
  onGrant={async () => true}
  onRevoke={async () => true}
  onSelectPath={() => {}}
/>
