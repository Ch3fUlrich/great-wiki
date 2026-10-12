<script lang="ts">
  /**
   * A dataset page's saved CALENDAR view (M8d D3b): the same rows the table shows, laid over a
   * month, each on the day it falls in — and a row spanning days appears on every one of them.
   *
   * A failed read, a missing page or a config the dataset cannot place rows by is STATED in a
   * role=alert, never drawn as an empty month. Entries reach the page as TEXT. The month is an
   * address (`?view=ID&month=YYYY-MM`), so prev and next are plain links and the view works
   * before any script arrives, like the table's sorting.
   *
   * `path` is carried for the shape of props the page hands every view; a calendar writes
   * nothing, so it has no use for it.
   */
  import {
    bucketRows,
    monthGrid,
    monthHref,
    monthLabel,
    parseMonth,
    rowTitle,
    shiftMonth,
    type CalendarBuckets
  } from './calendar';
  import type { Field, RowPage } from './table';
  import { calendarConfig, type DatasetView } from './views';

  let {
    path,
    fields,
    page,
    view,
    month,
    mayWrite = false,
    error = null
  }: {
    path: string;
    fields: Field[];
    page: RowPage | null;
    view: DatasetView | null;
    month?: string | null;
    mayWrite?: boolean;
    error?: string | null;
  } = $props();

  const WOCHENTAGE = ['Mo', 'Di', 'Mi', 'Do', 'Fr', 'Sa', 'So'];

  /** What there is to draw when there is nothing to draw. Never shown: `fehler` covers it. */
  const KEIN_KALENDER: CalendarBuckets = { days: new Map(), undated: [] };

  /**
   * The reader's own zone, because "which day is this appointment on" is their question, not the
   * server's. It is why an instant written just before midnight can land on a different day than
   * the server printed, and why a date worth putting in a calendar is best recorded as a plain
   * day.
   */
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;

  const gezeigt = $derived(parseMonth(month ?? null, new Date()));

  /**
   * One read of the rows, decided once: either the reason no month is drawn, or what to draw.
   * The three projections below are what the markup reads, so no block has to remember which
   * half of the union it is in.
   */
  const gebaut = $derived.by(
    (): { message: string } | { buckets: CalendarBuckets; titleKey: string | null | undefined } => {
      if (error) return { message: error };
      if (!page) return { message: 'Die Zeilen des Kalenders konnten nicht geladen werden.' };
      const config = calendarConfig(view);
      if (!config) return { message: 'Dieser Kalender hat kein Feld, nach dem er die Zeilen einordnen kann.' };
      try {
        return { buckets: bucketRows(page.rows, fields, config, zone), titleKey: config.title };
      } catch (e) {
        return { message: e instanceof Error ? e.message : 'Der Kalender konnte nicht aufgebaut werden.' };
      }
    }
  );

  const fehler = $derived('message' in gebaut ? gebaut.message : null);
  const kalender = $derived('message' in gebaut ? KEIN_KALENDER : gebaut.buckets);
  const titelFeld = $derived('message' in gebaut ? null : gebaut.titleKey);

  const raster = $derived(
    fehler === null ? monthGrid(Number(gezeigt.slice(0, 4)), Number(gezeigt.slice(5, 7))) : []
  );
</script>

{#if fehler !== null}
  <p class="kalender-fehler" role="alert">{fehler}</p>
{:else}
  <section class="kalender" data-calendar aria-labelledby="kalender-titel">
    <nav class="kalender-blatt" aria-label="Monat wechseln">
      <a href={monthHref(view, shiftMonth(gezeigt, -1))} rel="prev">{monthLabel(shiftMonth(gezeigt, -1))}</a>
      <h2 id="kalender-titel" class="kalender-titel">{monthLabel(gezeigt)}</h2>
      <a href={monthHref(view, shiftMonth(gezeigt, 1))} rel="next">{monthLabel(shiftMonth(gezeigt, 1))}</a>
    </nav>

    <table class="kalender-tafel">
      <caption class="nur-lesen">Termine im {monthLabel(gezeigt)}</caption>
      <thead>
        <tr>
          {#each WOCHENTAGE as tag (tag)}
            <th scope="col">{tag}</th>
          {/each}
        </tr>
      </thead>
      <tbody>
        {#each raster as woche (woche[0])}
          <tr>
            {#each woche as tag (tag)}
              {@const eintraege = kalender.days.get(tag) ?? []}
              <td data-day={tag} class:fremd={!tag.startsWith(gezeigt)}>
                <p class="kalender-tag">{Number(tag.slice(8, 10))}</p>
                {#if eintraege.length > 0}
                  <ul class="kalender-liste">
                    {#each eintraege as eintrag (eintrag.row.id)}
                      <li data-event={eintrag.row.id} class:fortsetzung={eintrag.continues}>
                        {eintrag.title}{#if eintrag.continues}<span class="kalender-marke">fortgesetzt</span>{/if}
                      </li>
                    {/each}
                  </ul>
                {/if}
              </td>
            {/each}
          </tr>
        {/each}
      </tbody>
    </table>

    <section class="kalender-ohne" data-undated aria-labelledby="ohne-datum-titel">
      <h3 id="ohne-datum-titel">Ohne Datum</h3>
      {#if kalender.undated.length > 0}
        <ul class="kalender-liste">
          {#each kalender.undated as zeile (zeile.id)}
            <li data-ohne-datum={zeile.id}>{rowTitle(zeile, fields, titelFeld)}</li>
          {/each}
        </ul>
      {:else}
        <p class="kalender-leer">Jede Zeile hat ein Datum.</p>
      {/if}
    </section>

    {#if !mayWrite}
      <p class="kalender-hinweis" role="status">Zum Ändern der Termine ist Schreibrecht nötig.</p>
    {/if}
    <p class="kalender-zone">Die Tage sind in der Zeitzone {zone} gezählt.</p>
  </section>
{/if}

<style>
  @layer components {
    .kalender {
      margin-block: var(--space-4);
    }
    .kalender-blatt {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: var(--space-3);
      margin-block-end: var(--space-3);
    }
    .kalender-blatt a {
      color: var(--accent);
      text-decoration: none;
      font-size: var(--text-sm);
    }
    .kalender-blatt a:hover,
    .kalender-blatt a:focus-visible {
      text-decoration: underline;
    }
    .kalender-titel {
      font-size: var(--text-xl);
      line-height: var(--leading-tight);
      text-align: center;
    }
    .kalender-tafel {
      inline-size: 100%;
      border-collapse: collapse;
      table-layout: fixed;
    }
    .kalender-tafel th,
    .kalender-tafel td {
      border: 1px solid var(--border);
      padding: var(--space-1) var(--space-2);
      text-align: start;
      vertical-align: top;
    }
    .kalender-tafel th {
      font-size: var(--text-xs);
      font-weight: 650;
      color: var(--ink-muted);
    }
    .kalender-tafel td {
      min-block-size: 4rem;
    }
    .kalender-tag {
      font-size: var(--text-xs);
      font-weight: 600;
      color: var(--ink-muted);
    }
    .fremd {
      background: var(--bg-raised);
    }
    .fremd .kalender-tag {
      color: var(--ink-faint);
    }
    .kalender-liste {
      list-style: none;
      margin: 0;
      padding: 0;
      font-size: var(--text-sm);
      overflow-wrap: anywhere;
    }
    .kalender-liste li + li {
      margin-block-start: var(--space-1);
    }
    .fortsetzung {
      color: var(--ink-muted);
    }
    .kalender-marke {
      display: inline-block;
      margin-inline-start: var(--space-1);
      padding: 0 var(--space-1);
      border: 1px solid var(--border);
      border-radius: var(--radius-sm);
      font-size: var(--text-xs);
      color: var(--ink-faint);
    }
    .kalender-ohne {
      margin-block-start: var(--space-4);
    }
    .kalender-ohne h3 {
      font-size: var(--text-sm);
      font-weight: 650;
      margin-block-end: var(--space-2);
    }
    .kalender-leer,
    .kalender-hinweis,
    .kalender-zone {
      font-size: var(--text-sm);
      color: var(--ink-muted);
    }
    .kalender-hinweis {
      margin-block-start: var(--space-3);
    }
    .kalender-zone {
      margin-block-start: var(--space-2);
      color: var(--ink-faint);
    }
    .kalender-fehler {
      color: var(--danger, inherit);
      margin-block: var(--space-4);
    }
    .nur-lesen {
      position: absolute;
      inline-size: 1px;
      block-size: 1px;
      overflow: hidden;
      clip-path: inset(50%);
      white-space: nowrap;
    }
  }
</style>
