import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { ANONYMOUS } from '$lib/api';
import Page from './+page.svelte';
import type { SidebarMode, TopicSummary } from '$lib/topics';

/**
 * The topic index, rendered exactly as the server renders it.
 *
 * **This page makes no request of its own, and that is the design rather than an omission.**
 * The owner put browsing by topic here *and* in the shell's sidebar, and the cost of two
 * placements is that they must agree. They agree by being one query: `+layout.server.ts` asks
 * `GET /api/topics` once per render, and this page renders the answer the sidebar is already
 * rendering. So there is no `+page.server.ts` beside this file — a loader here would be the
 * second answer to "which topics exist", and because a topic's own NAME is a disclosure
 * (ADR 0011), a second answer is also a second chance to leak one.
 *
 * What is asserted here is only what this page adds around that list, plus the one thing an
 * aggregate view can add that its filtering cannot take back: a number about what was hidden.
 * There is none.
 */
const themen: TopicSummary[] = [
  { path: '/format', name: 'Format', display_path: 'Format', documents: 1 },
  { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang', documents: 3 },
  { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen', documents: 1 }
];

function html(
  {
    topics = themen,
    themenFehler = null,
    seitenleiste = 'seiten'
  }: { topics?: TopicSummary[]; themenFehler?: string | null; seitenleiste?: SidebarMode } = {}
): string {
  return render(Page, {
    props: {
      data: {
        // The shell's own data, merged in from the root layout — including the one topic
        // query this page renders.
        me: ANONYMOUS,
        tree: [],
        tabHrefs: [],
        hier: '/themen',
        seitenleiste,
        themen: topics,
        themenFehler
      }
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the index', () => {
  it('is a page you can arrive at and understand', () => {
    const out = html();
    expect(out).toMatch(/<h1[^>]*>Themen<\/h1>/);
    // The same sentence /projekte and /aufgaben carry, and it is what licenses the counts
    // below: the reader is told the list is theirs, so a number beside a topic needs no
    // qualifier and no total to be honest.
    expect(out).toContain('Es erscheint nur, was Sie auch lesen dürfen.');
  });

  it('renders every topic it was handed, nested as they are named', () => {
    const out = html();
    expect(out).toContain('href="/themen/format"');
    expect(out).toContain('href="/themen/rundgang"');
    expect(out).toContain('href="/themen/rundgang/tabellen"');
    expect(out).toMatch(/<li\b[^>]*>[\s\S]*Rundgang[\s\S]*<ul[\s\S]*Tabellen[\s\S]*<\/ul>[\s\S]*<\/li>/);
  });

  it('says how many pages are under a topic, and never how many are not', () => {
    const out = html();
    expect(out).toContain('3 Seiten');
    expect(out).not.toMatch(/weitere|insgesamt|ausgeblendet|verborgen|von \d/i);
  });

  it('keeps the sidebar‘s own choice when a topic is followed from here', () => {
    expect(html({ seitenleiste: 'themen' })).toContain('href="/themen/format?seitenleiste=themen"');
  });
});

describe('when there is nothing to show', () => {
  it('says »Keine Themen« rather than nothing at all', () => {
    expect(html({ topics: [] })).toContain('Keine Themen');
  });

  it('never reports a failed request as a wiki with no topics', () => {
    const out = html({
      topics: [],
      themenFehler: 'Die Themen konnten nicht geladen werden (Fehler 500).'
    });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Keine Themen');
  });
});
