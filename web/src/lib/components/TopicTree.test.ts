import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import TopicTree from './TopicTree.svelte';
import type { TopicSummary } from '$lib/topics';

/**
 * The one rendering both topic placements use.
 *
 * The owner put topic browsing at `/themen` **and** in the shell's sidebar, and named the
 * same cost D-12 named for the board: two places that must agree. They agree by being one
 * query (`GET /api/topics`, asked once in `+layout.server.ts`) and one component — this one.
 * So every rule about how a topic reads belongs here and is asserted here once, rather than
 * twice in two page tests that could drift apart.
 *
 * Rendered with `svelte/server`, which is the first response: there is no DOM environment in
 * this project, so a hierarchy that only appeared after hydration could not pass any of this.
 */
const flat: TopicSummary[] = [
  { path: '/format', name: 'Format', display_path: 'Format', documents: 1 },
  { path: '/rundgang', name: 'Rundgang', display_path: 'Rundgang', documents: 3 },
  {
    path: '/rundgang/tabellen',
    name: 'Tabellen',
    display_path: 'Rundgang/Tabellen',
    documents: 1
  }
];

function html(
  {
    topics = flat,
    titel = 'Alle Themen',
    ebene = 2,
    fehler = null,
    current = undefined
  }: {
    topics?: TopicSummary[];
    titel?: string;
    ebene?: 2 | 3;
    fehler?: string | null;
    current?: string;
  } = {}
): string {
  return render(TopicTree, { props: { topics, titel, ebene, fehler, current } }).body.replace(
    /<!--.*?-->/g,
    ''
  );
}

describe('the hierarchy', () => {
  it('puts a nested topic inside a list inside its parent‘s item', () => {
    // The requirement is that nesting is in the MARKUP, not only in the indentation: a
    // reader who cannot see the indent still has to be told that Tabellen is inside
    // Rundgang, and only a nested list says so.
    const out = html();
    expect(out).toMatch(/<li\b[^>]*>[\s\S]*Rundgang[\s\S]*<ul[\s\S]*Tabellen[\s\S]*<\/ul>[\s\S]*<\/li>/);
  });

  it('names each topic by its leaf, because its ancestry is the list it sits in', () => {
    const out = html();
    expect(out).toContain('>Tabellen<');
    // Not "Rundgang/Tabellen" — the path is spelled out on a chip, where there is no list
    // around it to say where the topic sits.
    expect(out).not.toContain('Rundgang/Tabellen');
  });

  it('links every topic to its own page', () => {
    const out = html();
    expect(out).toContain('href="/themen/format"');
    expect(out).toContain('href="/themen/rundgang"');
    expect(out).toContain('href="/themen/rundgang/tabellen"');
  });

  it('marks the topic being looked at, and marks only that one', () => {
    const out = html({ current: '/rundgang/tabellen' });
    expect(out.match(/aria-current="page"/g)).toHaveLength(1);
    expect(out).toMatch(/href="\/themen\/rundgang\/tabellen"[^>]*aria-current="page"/);
  });

  it('sends every link through hrefFor when the shell hands one over', () => {
    const out = render(TopicTree, {
      props: {
        topics: flat,
        titel: 'Themen',
        ebene: 2,
        fehler: null,
        hrefFor: (href: string) => `${href}?reiter=%2F`
      }
    }).body;
    expect(out).toContain('href="/themen/format?reiter=%2F"');
  });
});

describe('the counts', () => {
  it('says how many pages are under a topic, in words', () => {
    const out = html();
    expect(out).toContain('3 Seiten');
    expect(out).toContain('1 Seite');
  });

  it('never says how many were left out', () => {
    // The one thing an aggregate view can add that the filtering cannot take back. ADR 0011
    // licenses the count of what IS shown and forbids everything else; this asserts the
    // absence structurally rather than trusting a comment.
    const out = html();
    expect(out).not.toMatch(/weitere|insgesamt|von \d|ausgeblendet|verborgen/i);
  });
});

describe('when there is nothing to show', () => {
  it('says »Keine Themen« and says how one comes about', () => {
    const out = html({ topics: [] });
    expect(out).toContain('Keine Themen');
    expect(out).not.toContain('<ul');
  });

  it('conflates »nobody has filed anything« with »none of it is yours«', () => {
    // The same conflation every empty state in this interface makes, and here it is not a
    // style choice: saying which of the two it is would say that something is being
    // withheld, which is the whole of what ADR 0011 is keeping back.
    const out = html({ topics: [] });
    expect(out).not.toMatch(/dürfen|Recht|Berechtigung|sichtbar für/i);
  });

  it('never conflates a failed request with an empty wiki', () => {
    const out = html({ topics: [], fehler: 'Die Themen konnten nicht geladen werden (Fehler 500).' });
    expect(out).toContain('Fehler 500');
    expect(out).not.toContain('Keine Themen');
    expect(out).toMatch(/role="alert"/);
  });
});

describe('how it sits in the page', () => {
  it('is a named landmark, so it can be reached without reading down to it', () => {
    expect(html({ titel: 'Themen' })).toMatch(/<nav[^>]*aria-label="Themen"/);
  });

  it('takes the heading level from the placement, because the two sit at different depths', () => {
    expect(html({ ebene: 2 })).toMatch(/<h2[^>]*>Alle Themen<\/h2>/);
    expect(html({ ebene: 3 })).toMatch(/<h3[^>]*>Alle Themen<\/h3>/);
  });
});
