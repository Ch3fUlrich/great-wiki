<!--
  The audit trail: one row per administrative action, newest first.

  Entries are kept indefinitely (D-M2-13). The log is a row per change, not per page view,
  so it does not grow the way a request log does, and the question it answers — "when did
  this change, and who changed it?" — is usually asked months later.

  Timestamps are UTC and the column says so. Formatting them through `Intl` would be
  prettier and would also differ between the server render and the browser that hydrates
  it, producing a mismatch on a value nobody would think to suspect.
-->
<script lang="ts">
  import SelectField from './SelectField.svelte';
  import Notice from './Notice.svelte';
  import type { Option } from './ComboField.svelte';
  import {
    MAX_AUDIT_LIMIT,
    formatInstant,
    type AdminPrincipal,
    type AuditPage
  } from '$lib/adminApi';

  interface Props {
    page: AuditPage | null;
    error: string | null;
    principals: AdminPrincipal[];
    limit: number;
    onLimitChange: (limit: number) => void;
  }

  let { page, error, principals, limit, onLimitChange }: Props = $props();

  /**
   * German for the actions the API is known to record. Anything unrecognised is shown
   * verbatim rather than as "Unbekannt" — a raw `acl.something` is still information,
   * and hiding it would make a new action type invisible in the one place that exists to
   * make actions visible.
   */
  const ACTION_LABEL: Record<string, string> = {
    'acl.grant': 'Zugriff gewährt',
    'acl.revoke': 'Zugriff entzogen',
    'document.visibility': 'Sichtbarkeit geändert',
    'team.create': 'Team angelegt',
    'team.member.add': 'Mitglied hinzugefügt',
    'team.member.remove': 'Mitglied entfernt',
    'principal.create': 'Konto angelegt',
    'principal.activate': 'Konto aktiviert',
    'principal.deactivate': 'Konto deaktiviert',
    'session.start': 'Anmeldung',
    'session.end': 'Abmeldung'
  };

  /**
   * The log stores a principal id and nothing else, so names are resolved here.
   *
   * An id that resolves to nobody is shown raw rather than as "Unbekannt": the account
   * having been deleted is precisely the kind of thing an audit log exists to preserve.
   */
  function who(principalId: string | null): string {
    if (!principalId) return 'System';
    const found = principals.find((person) => person.id === principalId);
    return found ? `${found.display_name} (${found.username})` : principalId;
  }

  // 500 is the API's ceiling, and it clamps rather than refuses — so offering 1000 would
  // caption the table with a number that is not the number of rows in it.
  const limitOptions: Option[] = [
    { value: '50', label: '50 Einträge' },
    { value: '200', label: '200 Einträge' },
    { value: String(MAX_AUDIT_LIMIT), label: `${MAX_AUDIT_LIMIT} Einträge` }
  ];

  // Sorted here as well as on the server. Defensive rather than distrustful: "newest
  // first" is a property of this screen, and an ISO-8601 or SQLite timestamp sorts
  // correctly as a string. `sort` is stable, so equal timestamps keep the server's order.
  const entries = $derived(
    page ? [...page.entries].sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0)) : []
  );
</script>

<div class="gw-adm-section">
  <div class="gw-adm-section-head">
    <h2 class="gw-adm-h2">Protokoll</h2>
    <SelectField
      label="Umfang"
      options={limitOptions}
      value={String(limit)}
      onChange={(value) => onLimitChange(Number(value))}
    />
  </div>

  {#if error}
    <Notice tone="fail" title="Protokoll nicht geladen." text={error} />
  {:else if !page}
    <Notice text="Es liegt kein Protokoll vor." />
  {:else if entries.length === 0}
    <Notice text="Es ist noch nichts protokolliert." />
  {:else}
    {#if page.truncated}
      <Notice
        text={`Es werden die neuesten ${limit} Einträge gezeigt. Ältere sind vorhanden und werden dauerhaft aufbewahrt.`}
      />
    {/if}
    <div class="gw-adm-scroll">
      <table class="gw-adm-table">
        <caption>Verwaltungsvorgänge, neueste zuerst</caption>
        <thead>
          <tr>
            <th scope="col">Zeitpunkt (UTC)</th>
            <th scope="col">Wer</th>
            <th scope="col">Was</th>
            <th scope="col">Pfad</th>
          </tr>
        </thead>
        <tbody>
          {#each entries as entry (entry.id)}
            <tr>
              <th scope="row" class="gw-adm-mono">{formatInstant(entry.at)}</th>
              <td>{who(entry.principal_id)}</td>
              <td>
                {ACTION_LABEL[entry.action] ?? entry.action}
                <!-- `target` is what the action names — a team slug, a username, a
                     subject. It sits with the verb rather than in the path column,
                     because it is usually not a path. -->
                {#if entry.target}
                  <span class="gw-adm-muted gw-adm-mono">{entry.target}</span>
                {/if}
              </td>
              <!-- The subtree the entry concerns. Empty means instance-wide, which is a
                   fact worth saying rather than leaving as a blank cell. -->
              <td class="gw-adm-mono">{entry.path ?? 'instanzweit'}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
