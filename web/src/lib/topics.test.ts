import { describe, expect, it } from 'vitest';
import {
  activeTopicPath,
  ancestryOf,
  countText,
  describeSetTopics,
  describeTopic,
  describeTopics,
  documentTopicsApiPath,
  sidebarModeOf,
  topicApiPath,
  topicHref,
  topicPathFromRoute,
  treeOf,
  withSidebar,
  type TopicSummary
} from '$lib/topics';

function summary(path: string, name: string, display_path: string, documents = 1): TopicSummary {
  return { path, name, display_path, documents };
}

/**
 * Everything about topics that is a RULE rather than markup.
 *
 * The same split `$lib/board` makes and for the same reason: this module is imported from a
 * `+page.server.ts`, from the shell, and from three components, so every rule that could
 * otherwise drift between those placements is a function here with a test beside it.
 *
 * **What is NOT tested here is who may see a topic.** `GET /api/topics` answers only the
 * topics the caller may see at all — ADR 0011 — and that filtering belongs to
 * `Store::topics_for`, where it is mutation-tested. What these tests pin is the half that is
 * this file's business: that the shapes are turned into a hierarchy without losing an entry,
 * and that no function here invents a number.
 */
describe('turning the flat index into the tree it describes', () => {
  const flat = [
    summary('/format', 'Format', 'Format'),
    summary('/rundgang', 'Rundgang', 'Rundgang', 3),
    summary('/rundgang/tabellen', 'Tabellen', 'Rundgang/Tabellen'),
    summary('/rundgang/umlaute', 'Umlaute', 'Rundgang/Umlaute')
  ];

  it('puts a topic inside the topic it is named after', () => {
    const tree = treeOf(flat);
    expect(tree.map((node) => node.topic.path)).toEqual(['/format', '/rundgang']);
    expect(tree[1].children.map((node) => node.topic.path)).toEqual([
      '/rundgang/tabellen',
      '/rundgang/umlaute'
    ]);
  });

  it('keeps the order the API answered in, at every level', () => {
    // `Store::topics_for` walks a BTreeMap keyed by canonical path, so parents already
    // precede their children and siblings are already sorted. Re-sorting here would be a
    // second opinion about an order that has one.
    const tree = treeOf([...flat].reverse());
    expect(tree.map((node) => node.topic.path)).toEqual(['/rundgang', '/format']);
    expect(tree[0].children.map((node) => node.topic.path)).toEqual([
      '/rundgang/umlaute',
      '/rundgang/tabellen'
    ]);
  });

  it('loses nothing when a parent is not in the list', () => {
    // It cannot happen through the API — a topic is visible only when a document under it
    // is, and that document is under its ancestors too — but a topic dropped because its
    // parent was missing would be a topic the reader may see and cannot reach, which is
    // exactly the dead end a topic page exists to prevent. It surfaces at the top instead.
    const tree = treeOf([summary('/rundgang/tabellen', 'Tabellen', 'Rundgang/Tabellen')]);
    expect(tree.map((node) => node.topic.path)).toEqual(['/rundgang/tabellen']);
  });

  it('is empty for an empty index, and invents no node', () => {
    expect(treeOf([])).toEqual([]);
  });
});

describe('the trail up from a nested topic', () => {
  it('names every topic above this one, outermost first', () => {
    expect(
      ancestryOf({ path: '/medizin/darm/labor', name: 'Labor', display_path: 'Medizin/Darm/Labor' })
    ).toEqual([
      { path: '/medizin', name: 'Medizin' },
      { path: '/medizin/darm', name: 'Darm' }
    ]);
  });

  it('is empty for a topic that sits at the top', () => {
    expect(ancestryOf({ path: '/format', name: 'Format', display_path: 'Format' })).toEqual([]);
  });

  it('gives up rather than guessing when the two spellings disagree', () => {
    // `display_path` is assembled by the store from the same ancestry as `path`, so the two
    // always have the same number of segments. If they ever did not, a trail assembled
    // anyway would put a slug in front of somebody as a name they had typed.
    expect(ancestryOf({ path: '/a/b/c', name: 'C', display_path: 'A/C' })).toEqual([]);
  });
});

describe('the addresses', () => {
  it('asks the API for one topic under its own prefix', () => {
    expect(topicApiPath('/rundgang/tabellen')).toBe('/api/topics/tagged/rundgang/tabellen');
  });

  it('asks the API for a page‘s topics under the other one', () => {
    expect(documentTopicsApiPath('/rundgang/tabellen')).toBe(
      '/api/topics/document/rundgang/tabellen'
    );
  });

  it('links a topic to its own page in this interface', () => {
    expect(topicHref({ path: '/rundgang/tabellen' })).toBe('/themen/rundgang/tabellen');
  });

  it('reads a topic path back off the route that carried it', () => {
    expect(topicPathFromRoute('rundgang/tabellen')).toBe('/rundgang/tabellen');
    expect(topicPathFromRoute('/rundgang')).toBe('/rundgang');
  });

  it('encodes each segment on its own, so a separator can never come from a name', () => {
    // A slug is ASCII today (`gw_core::slugify`), so this is a no-op in practice. It is here
    // because the day it is not, an unencoded segment would be a topic path with an extra
    // `/` in it — a different topic, silently.
    expect(topicApiPath('/a b/c%d')).toBe('/api/topics/tagged/a%20b/c%25d');
  });
});

describe('which topic an address is showing', () => {
  it('reads it off a topic page, and answers nothing for anything else', () => {
    expect(activeTopicPath('/themen/rundgang/tabellen')).toBe('/rundgang/tabellen');
    expect(activeTopicPath('/themen')).toBeNull();
    expect(activeTopicPath('/themen/')).toBeNull();
    expect(activeTopicPath('/rundgang')).toBeNull();
    expect(activeTopicPath('/themenabend')).toBeNull();
  });

  it('undoes the encoding the address carries, so it matches a canonical path', () => {
    expect(activeTopicPath('/themen/a%20b')).toBe('/a b');
  });
});

describe('the count beside a topic', () => {
  it('counts in words, singular and plural', () => {
    expect(countText(1)).toBe('1 Seite');
    expect(countText(3)).toBe('3 Seiten');
  });

  it('says nothing about what was left out — there is no second number to say it with', () => {
    // The whole disclosure rule in one assertion: the only number this interface renders is
    // the length of the list the reader is being handed. A signature that took a total would
    // be the place a "und 3 weitere" could later be written.
    expect(countText.length).toBe(1);
  });
});

describe('why something is not there', () => {
  it('tells a wiki with no topics apart from an API that did not answer', () => {
    expect(describeTopics(0)).toContain('antwortet nicht');
    expect(describeTopics(500)).toContain('500');
  });

  it('says the same thing about a topic nobody typed and one you may see nothing of', () => {
    // ADR 0011: the refusal and the absence must be the same answer, or the difference is
    // the oracle. The API answers 404 to both; this sentence must not undo that by hinting.
    const missing = describeTopic(404);
    expect(missing).not.toMatch(/dürfen|Recht|Berechtigung|gesperrt/i);
    expect(missing).toContain('Thema');
    expect(describeTopic(500)).toContain('500');
  });

  it('names what a refused change did not do, in every branch', () => {
    for (const status of [0, 400, 401, 403, 404, 500]) {
      expect(describeSetTopics(status, null)).toMatch(/nicht geändert|nicht gespeichert/);
    }
  });

  it('passes on what the API said about a topic it would not take', () => {
    // A 400 from `set_document_topics` names the string it rejected and why. Dropping it
    // turns a typo into "Fehler 400", which is a refusal nobody can act on.
    expect(describeSetTopics(400, '`a//b` ist kein Thema')).toContain('`a//b` ist kein Thema');
  });

  it('says how to get back in when the session is the problem', () => {
    expect(describeSetTopics(401, null)).toMatch(/anmelden/i);
    expect(describeSetTopics(403, null)).toMatch(/Schreibrecht/);
  });
});

describe('which half of the sidebar is showing', () => {
  it('reads the pages by default, and the topics only when asked', () => {
    expect(sidebarModeOf(null)).toBe('seiten');
    expect(sidebarModeOf('themen')).toBe('themen');
    expect(sidebarModeOf('seiten')).toBe('seiten');
  });

  it('reads anything else as the pages, rather than as nothing at all', () => {
    // The value comes from the address bar. A sidebar that rendered neither tree because
    // somebody typed `?seitenleiste=x` would be a blank column with no way back.
    expect(sidebarModeOf('themen ')).toBe('seiten');
    expect(sidebarModeOf('')).toBe('seiten');
  });

  it('carries the choice onto the next address, and drops it again when it is the default', () => {
    expect(withSidebar('/rundgang', 'themen')).toBe('/rundgang?seitenleiste=themen');
    expect(withSidebar('/rundgang?seitenleiste=themen', 'seiten')).toBe('/rundgang');
  });

  it('leaves whatever else the address said alone, fragment included', () => {
    expect(withSidebar('/aufgaben?projekt=p1#hinweis', 'themen')).toBe(
      '/aufgaben?projekt=p1&seitenleiste=themen#hinweis'
    );
    expect(withSidebar('/aufgaben?projekt=p1#hinweis', 'seiten')).toBe(
      '/aufgaben?projekt=p1#hinweis'
    );
  });
});
