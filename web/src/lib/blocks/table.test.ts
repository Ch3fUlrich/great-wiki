import { describe, expect, it } from 'vitest';
import type { Block } from './render';
import {
  MIN_INTERACTIVE_ROWS,
  columnIsNumeric,
  columns,
  fold,
  isInteractive,
  matches,
  rowCountLabel,
  rowMatches,
  sectionRows,
  sortAnnouncement,
  sortOrder,
  sortStateFor,
  textGrid,
  visibleOrder
} from './table';

/// A cell as the converter produces one: block content, so the text sits in a paragraph.
/// (crates/gw-core/src/markdown.rs opens an implicit paragraph for any inline text whose
/// innermost block cannot hold it, and a table cell cannot.)
function cell(kind: 'tableHeader' | 'tableCell', text: string, align?: string): Block {
  return {
    kind,
    attrs: align ? { align } : undefined,
    content: [{ kind: 'paragraph', content: [{ kind: 'text', text }] }]
  };
}

function row(kind: 'tableHeader' | 'tableCell', ...texts: string[]): Block {
  return { kind: 'tableRow', content: texts.map((t) => cell(kind, t)) };
}

/// The order a column sorts into, as text, so a failure reads as the table a person sees.
function sortedTexts(column: string[], direction: 'ascending' | 'descending' = 'ascending') {
  const grid = column.map((value) => [value]);
  const order = sortOrder(
    grid,
    grid.map((_, i) => i),
    0,
    direction
  );
  return order.map((i) => column[i]);
}

describe('sectionRows', () => {
  it('splits the leading run of header rows off the body', () => {
    const rows = [row('tableHeader', 'A', 'B'), row('tableCell', '1', '2')];
    expect(sectionRows(rows)).toEqual({ head: [rows[0]], body: [rows[1]] });
  });

  it('leaves a header row further down where the author put it', () => {
    // Moving it would reorder the document, which a renderer may not do.
    const rows = [row('tableHeader', 'A'), row('tableCell', '1'), row('tableHeader', 'A again')];
    expect(sectionRows(rows).body).toHaveLength(2);
  });
});

describe('columns', () => {
  it('reads label and alignment off the header cells', () => {
    const head = {
      kind: 'tableRow',
      content: [cell('tableHeader', 'Feld'), cell('tableHeader', 'Wert', 'right')]
    } as Block;
    expect(columns(head)).toEqual([
      { index: 0, label: 'Feld', align: undefined },
      { index: 1, label: 'Wert', align: 'right' }
    ]);
  });

  it('names an unlabelled column rather than leaving a control with no name', () => {
    // A comparison table often leaves the corner cell empty. A sort button whose only
    // content is that cell would be announced as just "button".
    const head = { kind: 'tableRow', content: [cell('tableHeader', '  ')] } as Block;
    expect(columns(head)[0].label).toBe('Spalte 1');
  });
});

describe('isInteractive', () => {
  it(`stays out of the way below ${MIN_INTERACTIVE_ROWS} body rows`, () => {
    const head = [row('tableHeader', 'A')];
    const body = Array.from({ length: MIN_INTERACTIVE_ROWS - 1 }, (_, i) =>
      row('tableCell', String(i))
    );
    expect(isInteractive({ head, body })).toBe(false);
  });

  it(`offers controls from ${MIN_INTERACTIVE_ROWS} body rows up`, () => {
    const head = [row('tableHeader', 'A')];
    const body = Array.from({ length: MIN_INTERACTIVE_ROWS }, (_, i) =>
      row('tableCell', String(i))
    );
    expect(isInteractive({ head, body })).toBe(true);
  });

  it('refuses a table with no header row, because every control would be unnamed', () => {
    const body = Array.from({ length: 20 }, (_, i) => row('tableCell', String(i)));
    expect(isInteractive({ head: [], body })).toBe(false);
  });

  it('refuses a table with two header rows rather than guess which one to decorate', () => {
    const head = [row('tableHeader', 'A'), row('tableHeader', 'B')];
    const body = Array.from({ length: 20 }, (_, i) => row('tableCell', String(i)));
    expect(isInteractive({ head, body })).toBe(false);
  });
});

describe('textGrid', () => {
  it('flattens each row to one string per cell, joined with a space', () => {
    // Concatenating without the space is a real bug this repository already made once:
    // `Länge | Meter` became `LängeMeter`, and a `contains` assertion did not notice.
    const rows = [row('tableCell', 'Länge', '42')];
    expect(textGrid(rows)).toEqual([['Länge', '42']]);
  });
});

describe('sorting', () => {
  it('sorts German text the way a German reader expects, not by codepoint', () => {
    // Codepoint order would put every umlaut after Z. DIN 5007-1 sorts ä with a.
    expect(sortedTexts(['Zucker', 'Öl', 'Apfel', 'Übung', 'Ähre'])).toEqual([
      'Ähre',
      'Apfel',
      'Öl',
      'Übung',
      'Zucker'
    ]);
    expect(sortedTexts(['Äpfel', 'Apfel'])).toEqual(['Apfel', 'Äpfel']);
  });

  it('compares numbers as numbers, through their prefixes and units', () => {
    expect(sortedTexts(['>1200 ppm', '<0.5%', '3-5%', '80 mg'])).toEqual([
      '<0.5%',
      '3-5%',
      '80 mg',
      '>1200 ppm'
    ]);
  });

  it('reads a German decimal comma and a thousands point', () => {
    expect(sortedTexts(['1,5 g', '1.200 g', '900 g', '0,75 g'])).toEqual([
      '0,75 g',
      '1,5 g',
      '900 g',
      '1.200 g'
    ]);
  });

  it('sorts a range by its lower bound', () => {
    expect(sortedTexts(['10-20', '2-30', '5-6'])).toEqual(['2-30', '5-6', '10-20']);
  });

  it('puts ❌ before ✅ ascending, so one click groups either answer', () => {
    expect(sortedTexts(['✅', '❌', '✅ (nur Säuglinge)', '❌'])).toEqual([
      '❌',
      '❌',
      '✅',
      '✅ (nur Säuglinge)'
    ]);
  });

  it('keeps empty cells last in BOTH directions', () => {
    // Reversing the comparator would make them first, which reads as "these matched",
    // and silently pushes the rows a reader is looking for off the bottom.
    expect(sortedTexts(['b', '', 'a'], 'ascending')).toEqual(['a', 'b', '']);
    expect(sortedTexts(['b', '', 'a'], 'descending')).toEqual(['b', 'a', '']);
  });

  it('treats a lone dash as empty, because that is what it means in a table', () => {
    expect(sortedTexts(['b', '—', 'a'], 'ascending')).toEqual(['a', 'b', '—']);
  });

  it('does not turn a name that merely contains digits into a number', () => {
    // A column of strain names is a text column even though "BB536" ends in digits and
    // "5-HTP" begins with one. Numeric parsing is a property of the COLUMN, not the cell —
    // one cell cannot tell "5-HTP" from "5 mg", and a column can.
    expect(columnIsNumeric(['5-HTP', 'BB536', 'Lactobacillus'])).toBe(false);
    // One unparseable cell does not cost the column its numbers.
    expect(columnIsNumeric(['5 mg', '12 mg', 'k. A.'])).toBe(true);

    // The consequence, where the two treatments actually differ: a thousands separator is
    // read in a numeric column and not in a textual one, where the collator compares the
    // digit runs it finds ("1" against "900"). So "1.200 Arten" leads a PROSE column and
    // trails a numeric one — the same string, ordered by what its neighbours are.
    expect(sortedTexts(['900 Arten', '1.200 Arten', 'Acetat', 'Butyrat'])).toEqual([
      '1.200 Arten',
      '900 Arten',
      'Acetat',
      'Butyrat'
    ]);
    expect(sortedTexts(['900', '1.200', '80'])).toEqual(['80', '900', '1.200']);
  });

  it('still orders digits inside text sensibly, so a stray label costs nothing', () => {
    // A column that is mostly prose is compared with a numeric-aware collator, so
    // "Phase 10" lands after "Phase 9" rather than after "Phase 1".
    expect(sortedTexts(['Phase 10', 'Phase 9', 'Phase 1'])).toEqual([
      'Phase 1',
      'Phase 9',
      'Phase 10'
    ]);
  });

  it('is stable, so a second sort keeps the first one inside its ties', () => {
    const grid = [
      ['b', '2'],
      ['a', '2'],
      ['b', '1'],
      ['a', '1']
    ];
    const byNumber = sortOrder(grid, [0, 1, 2, 3], 1, 'ascending');
    expect(byNumber.map((i) => grid[i].join(''))).toEqual(['b1', 'a1', 'b2', 'a2']);

    const thenByLetter = sortOrder(grid, byNumber, 0, 'ascending');
    // Within each letter the numeric order from the previous sort survives.
    expect(thenByLetter.map((i) => grid[i].join(''))).toEqual(['a1', 'a2', 'b1', 'b2']);
  });

  it('sorts a column that does not exist in some rows without throwing', () => {
    const grid = [['a'], ['b', 'x'], []];
    expect(() => sortOrder(grid, [0, 1, 2], 1, 'ascending')).not.toThrow();
  });
});

describe('sortStateFor', () => {
  it('cycles ascending, descending, off — so a reader can always get back', () => {
    expect(sortStateFor(null, 2)).toEqual({ column: 2, direction: 'ascending' });
    expect(sortStateFor({ column: 2, direction: 'ascending' }, 2)).toEqual({
      column: 2,
      direction: 'descending'
    });
    expect(sortStateFor({ column: 2, direction: 'descending' }, 2)).toBe(null);
  });

  it('starts a different column from the top rather than inheriting a direction', () => {
    expect(sortStateFor({ column: 2, direction: 'descending' }, 1)).toEqual({
      column: 1,
      direction: 'ascending'
    });
  });
});

describe('fold', () => {
  it('ignores case, umlaut marks and ß, because a filter is a search aid', () => {
    expect(fold('Präparat')).toBe(fold('praparat'));
    expect(fold('MÜSLI')).toBe(fold('muesli'.replace('ue', 'ü')));
    expect(fold('Maß')).toBe(fold('Mass'));
  });
});

describe('matches', () => {
  it('is empty-query-permissive', () => {
    expect(matches('irgendwas', '   ')).toBe(true);
  });

  it('requires every term, in any order and anywhere', () => {
    expect(matches('Lactobacillus rhamnosus GG', 'gg lacto')).toBe(true);
    expect(matches('Lactobacillus rhamnosus GG', 'gg bifido')).toBe(false);
  });
});

describe('rowMatches', () => {
  const grid = [
    ['Lactobacillus rhamnosus', '✅', 'Säuglinge'],
    ['Bifidobacterium longum', '❌', 'Kinder']
  ];

  it('searches the whole row for the table filter', () => {
    expect(rowMatches(grid[0], { query: 'säuglinge', columns: [] })).toBe(true);
    expect(rowMatches(grid[1], { query: 'säuglinge', columns: [] })).toBe(false);
  });

  it('does not let a term span two cells', () => {
    // The row's cells are joined for searching; a term matching across the seam would be
    // a match a reader cannot see.
    expect(rowMatches(grid[0], { query: 'rhamnosus✅', columns: [] })).toBe(false);
  });

  it('confines a column filter to its own column', () => {
    expect(rowMatches(grid[0], { query: '', columns: ['', '', 'kinder'] })).toBe(false);
    expect(rowMatches(grid[1], { query: '', columns: ['', '', 'kinder'] })).toBe(true);
  });

  it('ands the table filter with every column filter', () => {
    expect(rowMatches(grid[0], { query: 'lacto', columns: ['', '❌', ''] })).toBe(false);
  });
});

describe('visibleOrder', () => {
  it('keeps the sorted order while dropping the rows that do not match', () => {
    const grid = [['b'], ['a'], ['c']];
    expect(visibleOrder(grid, [1, 0, 2], { query: '', columns: [] })).toEqual([1, 0, 2]);
    expect(visibleOrder(grid, [1, 0, 2], { query: 'b', columns: [] })).toEqual([0]);
  });
});

describe('the strings a reader is shown', () => {
  it('always states displayed out of total, never just the displayed count', () => {
    // A filtered table that looks complete is the defect this project keeps finding.
    expect(rowCountLabel(26, 26)).toBe('26 von 26 Zeilen');
    expect(rowCountLabel(3, 26)).toBe('3 von 26 Zeilen');
    expect(rowCountLabel(0, 26)).toBe('0 von 26 Zeilen');
  });

  it('says which column is sorted, and how', () => {
    expect(sortAnnouncement(null, [])).toBe('');
    expect(
      sortAnnouncement({ column: 1, direction: 'ascending' }, [
        { index: 0, label: 'Stamm', align: undefined },
        { index: 1, label: 'Dosis', align: undefined }
      ])
    ).toBe('sortiert nach Dosis, aufsteigend');
    expect(
      sortAnnouncement({ column: 0, direction: 'descending' }, [
        { index: 0, label: 'Stamm', align: undefined }
      ])
    ).toBe('sortiert nach Stamm, absteigend');
  });
});
