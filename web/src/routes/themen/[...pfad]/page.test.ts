import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import { ANONYMOUS } from '$lib/api';
import Page from './+page.svelte';
import type { SidebarMode, TopicPageResponse } from '$lib/topics';

/**
 * One topic's page, rendered exactly as the server renders it.
 *
 * D-4 made this the ONLY way a topic is reachable, so two things have to be true of it and
 * both are asserted here: everything the API listed is on the page and reachable, and nothing
 * that was not listed is hinted at. The second is the disclosure rule — a number about what
 * was left out would be a fact about pages the reader may not read, and it is the one thing
 * the filtering cannot take back.
 *
 * The third thing is nesting, which the store settled and this page must not undo: opening a
 * topic shows everything inside it. The page says so in words, because a reader who does not
 * know that would read a list of forty as a list of two plus a mistake.
 */
const nested: TopicPageResponse = {
  topic: { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen' },
  documents: [{ path: '/rundgang/tabellen', title: 'Tabellen — was heute passiert' }],
  children: []
};

const top: TopicPageResponse = {
  topic: { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang' },
  documents: [
    { path: '/rundgang', title: 'Rundgang' },
    { path: '/rundgang/tabellen', title: 'Tabellen — was heute passiert' }
  ],
  children: [
    { path: '/rundgang/tabellen', name: 'Tabellen', display_path: 'Rundgang/Tabellen', documents: 1 }
  ]
};

function html(thema: TopicPageResponse = top, seitenleiste: SidebarMode = 'seiten'): string {
  return render(Page, {
    props: {
      data: {
        me: ANONYMOUS,
        tree: [],
        tabHrefs: [],
        hier: `/themen${thema.topic.path}`,
        seitenleiste,
        themen: [],
        themenFehler: null,
        thema
      }
    }
  }).body.replace(/<!--.*?-->/g, '');
}

describe('the topic itself', () => {
  it('is titled by its own name, not by its whole path', () => {
    expect(html(nested)).toMatch(/<h1[^>]*>Tabellen<\/h1>/);
  });

  it('offers a way back up to the topic it sits inside', () => {
    // Nesting is real, so a nested topic has somewhere above it — and without this link the
    // only route to `Rundgang` from here is the index, which is a dead end in the making.
    const out = html(nested);
    expect(out).toContain('href="/themen/rundgang"');
    expect(out).toContain('Rundgang');
  });

  it('offers no trail at all for a topic that sits at the top', () => {
    expect(html(top)).not.toMatch(/aria-label="Übergeordnete Themen"/);
  });
});

describe('what is filed under it', () => {
  it('lists every page the API answered, and links each of them', () => {
    const out = html(top);
    expect(out).toContain('href="/rundgang"');
    expect(out).toContain('href="/rundgang/tabellen"');
    expect(out).toContain('Tabellen — was heute passiert');
  });

  it('says out loud that everything inside this topic is included', () => {
    // The store settled this and recorded why; an interface that showed the list without
    // saying so would leave the reader to guess whether a page from `Rundgang/Tabellen`
    // belongs here.
    expect(html(top)).toMatch(/darunter|darin/);
  });

  it('never says how many pages were left out', () => {
    const out = html(top);
    expect(out).not.toMatch(/weitere|insgesamt|ausgeblendet|verborgen|von \d/i);
  });
});

describe('the topics inside it', () => {
  it('renders them with the same component the index uses', () => {
    const out = html(top);
    expect(out).toMatch(/<nav[^>]*aria-label="Themen darin"/);
    expect(out).toContain('href="/themen/rundgang/tabellen"');
    expect(out).toContain('1 Seite');
  });

  it('shows nothing about subtopics when there are none', () => {
    expect(html(nested)).not.toMatch(/aria-label="Themen darin"/);
  });

  it('keeps the sidebar‘s own choice when one is followed', () => {
    expect(html(top, 'themen')).toContain(
      'href="/themen/rundgang/tabellen?seitenleiste=themen"'
    );
  });
});
