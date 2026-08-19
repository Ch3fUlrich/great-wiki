// Deterministic browser behaviour checks: assert and exit, no screenshots and no human
// in the loop. Mirrors shots.mjs — same base-URL variable, same "point at the local dev
// server" posture, same container invocation (see `just behaviour`).
import { chromium } from 'playwright';

const BASE = process.env.SHOT_BASE ?? 'http://127.0.0.1:5173';

/** Thrown by a check to fail it with a short, specific reason. */
class CheckFailure extends Error {}

function assert(condition, message) {
  if (!condition) throw new CheckFailure(message);
}

/** @type {import('playwright').Browser} */
let browser;

const results = [];

/**
 * Runs one named check in its own browser context (so no check can leak state — focus,
 * cookies, an open dialog — into the next one), records ok/FAIL, and never lets one
 * check's throw stop the run.
 */
async function check(name, fn) {
  const ctx = await browser.newContext();
  try {
    const page = await ctx.newPage();
    await fn(page);
    results.push({ name, ok: true });
    console.log(`ok   ${name}`);
  } catch (err) {
    const reason = err instanceof Error ? err.message : String(err);
    results.push({ name, ok: false, reason });
    console.log(`FAIL ${name} — ${reason}`);
  } finally {
    await ctx.close();
  }
}

/**
 * Navigates to the isolated Dialog mount (see routes/_behaviour) and, unless told
 * otherwise, opens it — the shared setup for every Group B check below. Returns the
 * locators each check needs so it doesn't have to re-derive them.
 */
async function loadDialogPage(page, { open } = { open: true }) {
  // networkidle, not domcontentloaded: the trigger's click handler is wired up by
  // Svelte's client-side hydration, which is a dynamic import that resolves after
  // domcontentloaded fires. Clicking too early lands on a button that is visible but
  // not yet interactive — the click does nothing and every check below times out
  // waiting for a dialog that was never asked to open. shots.mjs hits the same
  // requirement for the same reason.
  await page.goto(BASE + '/_behaviour', { waitUntil: 'networkidle' });
  const trigger = page.getByRole('button', { name: 'Dialog öffnen' });
  await trigger.waitFor({ state: 'visible' });
  const dialog = page.locator('.gw-dialog');
  const backdrop = page.locator('.gw-dialog-backdrop');
  if (open) {
    await trigger.click();
    await dialog.waitFor({ state: 'visible', timeout: 5_000 });
    // Focus entering the dialog (what B3 asserts) is driven by a separate effect from
    // the one that flips .gw-dialog visible, and so is closeOnEscape's own listener
    // registration. Pressing Escape (B5-B8) right after the visibility wait sometimes
    // beat that registration and the keypress was silently dropped — confirmed by
    // tracing attribute state over 5s: the dialog just stayed open with no further
    // transition. Waiting for focus to land is a real readiness signal (same one B3
    // checks), not a fixed sleep, and it reliably lands after both effects have run.
    await page
      .waitForFunction(() => document.activeElement?.closest('.gw-dialog') != null, { timeout: 5_000 })
      .catch(() => {});
  }
  return { trigger, dialog, backdrop };
}

browser = await chromium.launch();

// Reachability gate. Every check below assumes BASE answers; if it doesn't, every one of
// them would fail for the same uninformative reason ("net::ERR_CONNECTION_REFUSED" on
// each of thirteen lines). Fail loudly once, here, and stop — this script must never
// exit 0 because it had nothing to check.
try {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  const response = await page.goto(BASE + '/', { waitUntil: 'domcontentloaded', timeout: 10_000 });
  if (!response || !response.ok()) {
    throw new Error(`unexpected status ${response ? response.status() : '(no response)'}`);
  }
  await ctx.close();
} catch (err) {
  const reason = err instanceof Error ? err.message : String(err);
  console.error(`FAIL: ${BASE} is unreachable — ${reason}`);
  console.error('Cannot run any check against an unreachable site. Is the dev server running?');
  await browser.close();
  process.exit(1);
}

// ---------------------------------------------------------------------------------------
// Group A — the reader (paths that exist today)
// ---------------------------------------------------------------------------------------

await check('A1 home page has a document tree with a link', async (page) => {
  const response = await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });
  assert(response !== null && response.status() === 200, `expected 200, got ${response?.status()}`);

  const tree = page.locator('nav[aria-label="Seitenbaum"]');
  await tree.waitFor({ state: 'visible' });
  const links = tree.locator('a');
  await links.first().waitFor({ state: 'visible' });
  const count = await links.count();
  assert(count >= 1, `expected at least one tree link, found ${count}`);
});

await check('A2 tree links read as links, by hue or underline', async (page) => {
  await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });

  const link = page.locator('nav[aria-label="Seitenbaum"] a').first();
  await link.waitFor({ state: 'visible' });

  // "Different from body text" is NOT the requirement, and asserting it made this check
  // useless: the actual regression was tree links set to `--ink-muted`, a grey that is
  // perfectly distinct from body ink while reading as de-emphasised label text rather
  // than as something clickable. Reverting the fix therefore passed this check.
  //
  // The requirement is that a navigation link is IDENTIFIABLE as a link, which this
  // design does by hue. An underline is the other legitimate way, so either satisfies it
  // — but muted grey with no underline satisfies neither, which is the case that must
  // fail.
  const { linkColor, accent, underlined } = await link.evaluate((el) => {
    const cs = getComputedStyle(el);
    // Resolve the token through a probe element, so the comparison is against whatever
    // --accent currently is rather than a hex literal copied out of tokens.css.
    const probe = document.createElement('span');
    probe.style.color = 'var(--accent)';
    el.appendChild(probe);
    const accent = getComputedStyle(probe).color;
    probe.remove();
    return {
      linkColor: cs.color,
      accent,
      underlined: cs.textDecorationLine.includes('underline')
    };
  });

  assert(
    linkColor === accent || underlined,
    `a tree link is neither accent-coloured nor underlined, so it does not read as a link ` +
      `(color ${linkColor}, accent ${accent}, underline ${underlined})`
  );
});

await check('A3 a document page renders a real table with scoped headers', async (page) => {
  const response = await page.goto(BASE + '/rundgang/tabellen-was-heute-passiert', {
    waitUntil: 'domcontentloaded'
  });
  assert(response !== null && response.status() === 200, `expected 200, got ${response?.status()}`);

  const table = page.locator('table');
  await table.first().waitFor({ state: 'visible' });
  assert((await table.count()) >= 1, 'expected at least one <table> element');

  const scopedHeader = table.first().locator('th[scope="col"]');
  await scopedHeader.first().waitFor({ state: 'attached' });
  assert((await scopedHeader.count()) >= 1, 'expected at least one <th scope="col">');
});

await check('A4 no horizontal scroll at 390px width', async (page) => {
  await page.setViewportSize({ width: 390, height: 844 });
  // The table page is the sharpest test of this: a wide table is exactly what pushed the
  // page sideways before BlockView.svelte gave it its own scroll box.
  await page.goto(BASE + '/rundgang/tabellen-was-heute-passiert', { waitUntil: 'networkidle' });

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth
  }));
  assert(
    scrollWidth <= innerWidth + 1,
    `document.documentElement.scrollWidth (${scrollWidth}) exceeds window.innerWidth + 1 (${innerWidth + 1})`
  );
});

await check('A5 skip link is the first focusable element', async (page) => {
  await page.goto(BASE + '/', { waitUntil: 'domcontentloaded' });

  // Start from a known position (nothing focused) before the single Tab.
  await page.evaluate(() => document.body.focus());
  await page.keyboard.press('Tab');

  const focused = await page.evaluate(() => ({
    tag: document.activeElement?.tagName ?? null,
    href: document.activeElement instanceof HTMLAnchorElement ? document.activeElement.href : null,
    className: document.activeElement?.className ?? null
  }));
  assert(focused.tag === 'A', `expected an <a> to be focused first, got <${focused.tag}>`);
  // The real markup (web/src/routes/+layout.svelte) is `<a class="skip" href="#content">`,
  // not literally "#main" — the spec allows "or similar", and #content is what this app uses.
  assert(
    focused.href !== null && /#(content|main)$/.test(focused.href),
    `expected the skip link's href to end with #content or #main, got ${focused.href}`
  );
  assert(
    typeof focused.className === 'string' && focused.className.includes('skip'),
    `expected the focused link to carry the "skip" class, got "${focused.className}"`
  );
});

// ---------------------------------------------------------------------------------------
// Group B — the dialog component (web/src/lib/components/Dialog.svelte, Ark UI)
// Mounted in isolation at /_behaviour (see routes/_behaviour/+page.svelte and its
// +page.server.ts guard) because no real route uses Dialog yet.
// ---------------------------------------------------------------------------------------

await check('B1 dialog is not visible before opening', async (page) => {
  const { dialog } = await loadDialogPage(page, { open: false });
  assert(!(await dialog.isVisible()), 'expected .gw-dialog to be hidden on first load');
});

await check('B2 clicking the trigger makes the dialog visible', async (page) => {
  const { dialog } = await loadDialogPage(page, { open: true });
  assert(await dialog.isVisible(), 'expected .gw-dialog to be visible after clicking the trigger');
});

await check('B3 opening the dialog moves focus into it', async (page) => {
  await loadDialogPage(page, { open: true });
  const focusedInside = await page.evaluate(() => document.activeElement?.closest('.gw-dialog') != null);
  assert(focusedInside, 'expected document.activeElement to be inside .gw-dialog after opening');
});

await check('B4 Tab twelve times keeps focus inside the dialog', async (page) => {
  await loadDialogPage(page, { open: true });
  for (let i = 0; i < 12; i += 1) {
    await page.keyboard.press('Tab');
  }
  const focusedInside = await page.evaluate(() => document.activeElement?.closest('.gw-dialog') != null);
  assert(focusedInside, 'focus escaped .gw-dialog after twelve Tab presses (the focus trap did not hold)');
});

await check('B5 Escape makes the dialog not visible', async (page) => {
  // This is the check that caught the real bug: Ark closes the dialog by setting the
  // `hidden` attribute and data-state="closed", relying on the user-agent rule
  // `[hidden] { display: none }`. An author `display` rule with no `:not([hidden])` guard
  // silently outranks that UA rule, so the dialog stayed visible and interactive with
  // data-state="closed" sitting right there in the DOM — nothing threw, nothing logged.
  // `.isVisible()` reads actual rendering (computed display/visibility plus a non-empty
  // bounding box), not the `hidden` attribute or `data-state`, so it cannot be fooled by
  // the same bug that fooled the CSS.
  const { dialog } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  assert(!(await dialog.isVisible()), '.gw-dialog is still visible after Escape');
});

await check('B6 after Escape, the backdrop is also not visible', async (page) => {
  const { backdrop } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  // Wait on the backdrop's own hidden state, not the content's: they are separate Ark
  // Presence regions and do not necessarily flip on the exact same tick. Waiting on
  // .gw-dialog here and then asserting on .gw-dialog-backdrop was a real race in an
  // earlier version of this script — it failed about 1 run in 8 under load even though
  // the component has no bug, because the backdrop occasionally hides one tick later.
  await backdrop.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  assert(!(await backdrop.isVisible()), '.gw-dialog-backdrop is still visible after Escape');
});

await check('B7 after Escape, focus returns to the trigger', async (page) => {
  const { trigger, dialog } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  // Poll for the actual condition (focus-trap restoreFocus completing) rather than
  // trusting that it already happened by the time .gw-dialog went hidden — same class of
  // race as B6, just for a different pair of independently-timed effects.
  const triggerHandle = await trigger.elementHandle();
  await page
    .waitForFunction((el) => el === document.activeElement, triggerHandle, { timeout: 5_000 })
    .catch(() => {});
  const returnedToTrigger = await trigger.evaluate((el) => el === document.activeElement);
  assert(returnedToTrigger, 'focus did not return to the trigger after Escape');
});

await check('B8 the dialog can be reopened after being closed', async (page) => {
  const { trigger, dialog } = await loadDialogPage(page, { open: true });
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden', timeout: 5_000 }).catch(() => {});
  await trigger.click();
  await dialog.waitFor({ state: 'visible', timeout: 5_000 });
  assert(await dialog.isVisible(), '.gw-dialog did not reopen after being closed once');
});

// ---------------------------------------------------------------------------------------
// Group C — sorting and filtering (web/src/lib/components/TableView.svelte)
//
// Against the tour page, which carries two tables on purpose: a two-row one that must stay
// plain, and an eight-row one whose cells were chosen to exercise every rule the comparator
// has (German umlauts, a thousands point, a decimal comma, a comparator prefix, a range,
// ticks and crosses, and one empty cell in each of two columns).
//
// Every assertion below is an EXACT expected value or a structural fact. None of them is of
// the "differs from the previous value" shape that made A2 a false pass for months: a
// regression that reorders rows differently would satisfy "the order changed" just as well
// as the fix does.
// ---------------------------------------------------------------------------------------

const TABLE_PAGE = '/rundgang/tabellen-was-heute-passiert';

/**
 * Polls for a condition instead of sleeping, and reports what it last actually saw — a
 * fixed sleep either flakes or wastes time, and "timed out" with no observed value is the
 * least useful failure a check can produce.
 */
async function until(read, message, timeout = 5_000) {
  const deadline = Date.now() + timeout;
  let last;
  for (;;) {
    last = await read();
    if (last.ok) return last;
    if (Date.now() > deadline) {
      throw new CheckFailure(`${message} — last saw ${JSON.stringify(last.saw)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}

/**
 * Loads the tour page and returns both tables. `networkidle` for the reason loadDialogPage
 * gives: the controls are added by client-side hydration, which is a dynamic import that
 * resolves after domcontentloaded — and here that is the whole point, because the SSR HTML
 * deliberately has no controls at all.
 */
async function loadTablePage(page) {
  await page.goto(BASE + TABLE_PAGE, { waitUntil: 'networkidle' });
  const plain = page.locator('.gw-tbl').first();
  const table = page.locator('.gw-tbl').nth(1);
  await table.locator('.gw-tbl-count').waitFor({ state: 'visible', timeout: 5_000 });
  return { plain, table };
}

/** One column of the body, top to bottom, as trimmed text. */
const column = (table, index) =>
  table
    .locator('tbody tr')
    .evaluateAll((rows, i) => rows.map((row) => row.cells[i]?.textContent.trim() ?? ''), index);

const ariaSorts = (table) =>
  table.locator('thead th').evaluateAll((ths) => ths.map((th) => th.getAttribute('aria-sort')));

/** Clicks a column's sort button and waits until `aria-sort` actually says so. */
async function sortBy(table, index, expected) {
  await table.locator('.gw-tbl-sort').nth(index).click();
  await until(
    async () => {
      const saw = await ariaSorts(table);
      return { ok: saw[index] === expected, saw };
    },
    `column ${index} never reported aria-sort="${expected}"`
  );
}

await check('C1 a short table gets no controls at all', async (page) => {
  // Two rows do not need a toolbar; a filter box in front of a table you can already see
  // whole is noise, and this one must stay exactly as it was.
  const { plain } = await loadTablePage(page);
  assert((await plain.locator('.gw-tbl-bar').count()) === 0, 'the two-row table grew a toolbar');
  assert((await plain.locator('.gw-tbl-sort').count()) === 0, 'the two-row table grew sort buttons');
  assert(
    (await plain.locator('th[aria-sort]').count()) === 0,
    'the two-row table claims a sort state it cannot change'
  );
});

await check('C2 a long table gets named controls and a row count', async (page) => {
  const { table } = await loadTablePage(page);

  assert(
    (await table.locator('.gw-tbl-count').textContent()) === '8 von 8 Zeilen',
    'the unfiltered row count must still state the total, not just the visible rows'
  );

  // Real buttons in the header, not click handlers on the `th`: only a button is reachable
  // by Tab, operable with both Enter and Space, and announced as something to press.
  const names = await table
    .locator('.gw-tbl-sort')
    .evaluateAll((els) => els.map((el) => el.textContent.replace(/\s+/g, ' ').trim()));
  assert(names.length === 4, `expected four sort buttons, found ${names.length}`);
  assert(
    names[0] === 'Probe, aufsteigend sortieren ⇅',
    `a sort button must say which column AND what pressing it does, got "${names[0]}"`
  );

  // Every column filter is a real <label for=…> naming its own column. Eight boxes all
  // called "Filter" are eight boxes a screen-reader user has to count along.
  const labelled = await table.locator('.gw-tbl-filter').evaluateAll((fields) =>
    fields.map((field) => {
      const label = field.querySelector('label');
      const input = field.querySelector('input');
      return label && input && label.htmlFor === input.id ? label.textContent.trim() : null;
    })
  );
  assert(
    JSON.stringify(labelled) ===
      JSON.stringify(['Probe filtern', 'Menge filtern', 'Geprüft filtern', 'Anteil filtern']),
    `each column filter must be labelled with its column, got ${JSON.stringify(labelled)}`
  );

  const live = await table.locator('.gw-tbl-count').getAttribute('aria-live');
  assert(live === 'polite', `the row count must be a polite live region, got ${live}`);
});

await check('C3 aria-sort cycles ascending, descending, none — one column at a time', async (page) => {
  const { table } = await loadTablePage(page);
  assert(
    JSON.stringify(await ariaSorts(table)) === JSON.stringify(['none', 'none', 'none', 'none']),
    'every sortable column must state a sort of "none" before anything is sorted'
  );

  await sortBy(table, 0, 'ascending');
  assert(
    JSON.stringify(await ariaSorts(table)) ===
      JSON.stringify(['ascending', 'none', 'none', 'none']),
    'exactly one column may claim to be sorted'
  );

  await sortBy(table, 0, 'descending');
  await sortBy(table, 0, 'none');

  // Off means the order the author wrote, which in a document is itself information.
  const back = await column(table, 0);
  assert(
    JSON.stringify(back) ===
      JSON.stringify(['Öl', 'Apfel', 'Ähre', 'Zucker', 'Möhre', 'Äpfel', 'Bohne', 'Nuss']),
    `a third press must restore document order, got ${JSON.stringify(back)}`
  );
});

await check('C4 a numeric column sorts by value, with empties last both ways', async (page) => {
  const { table } = await loadTablePage(page);

  await sortBy(table, 1, 'ascending');
  const up = await column(table, 1);
  assert(
    JSON.stringify(up) ===
      JSON.stringify(['<0,5 g', '1,5 g', '3-5 g', '42 g', '80 g', '900 g', '1.200 g', '']),
    `units, comparator prefixes, a range and a thousands point must all be read, got ${JSON.stringify(up)}`
  );

  await sortBy(table, 1, 'descending');
  const down = await column(table, 1);
  assert(
    JSON.stringify(down) ===
      JSON.stringify(['1.200 g', '900 g', '80 g', '42 g', '3-5 g', '1,5 g', '<0,5 g', '']),
    `descending must reverse the values, got ${JSON.stringify(down)}`
  );
  // The one that a negated comparator gets wrong: the gap must NOT float to the top, where
  // it reads as "these are the matches" and pushes the rows being hunted for off the bottom.
  assert(down[down.length - 1] === '', 'an empty cell must stay last when sorting descending');
});

await check('C5 German text sorts as a German reader expects', async (page) => {
  const { table } = await loadTablePage(page);
  await sortBy(table, 0, 'ascending');
  const names = await column(table, 0);
  assert(
    JSON.stringify(names) ===
      JSON.stringify(['Ähre', 'Apfel', 'Äpfel', 'Bohne', 'Möhre', 'Nuss', 'Öl', 'Zucker']),
    `codepoint order would put every umlaut after Z; got ${JSON.stringify(names)}`
  );
});

await check('C6 a second sort keeps the first one inside its ties', async (page) => {
  const { table } = await loadTablePage(page);
  await sortBy(table, 0, 'ascending'); // by name
  await sortBy(table, 2, 'ascending'); // then by the tick column, which has three ties

  const ticks = await column(table, 2);
  const names = await column(table, 0);
  const combined = ticks.map((tick, i) => `${tick}:${names[i]}`);
  assert(
    JSON.stringify(combined) ===
      JSON.stringify([
        '❌:Apfel',
        '❌:Nuss',
        '❌:Zucker',
        '✅:Ähre',
        '✅:Bohne',
        '✅:Möhre',
        '✅:Öl',
        '—:Äpfel'
      ]),
    `inside each group the previous sort must survive, and a lone dash counts as empty; got ${JSON.stringify(combined)}`
  );
  assert(combined.length === 8, 'no row may be lost or duplicated by sorting');
});

await check('C7 filtering changes the count and never hides the total', async (page) => {
  const { table } = await loadTablePage(page);
  const search = table.locator('.gw-tbl-bar input');

  await search.fill('öl');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '1 von 8 Zeilen', saw };
    },
    'the row count must state both the visible rows and the total'
  );
  assert((await table.locator('tbody tr').count()) === 1, 'exactly one row matches "öl"');

  // Case and umlaut marks are folded, so a filter typed in a hurry still finds the row.
  await search.fill('OL');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '1 von 8 Zeilen', saw };
    },
    '"OL" must find "Öl" — a filter that only matches perfect spelling is a filter nobody uses'
  );

  // A column filter confines itself to its column.
  await search.fill('');
  await table.locator('.gw-tbl-filter input').nth(2).fill('✅');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '4 von 8 Zeilen', saw };
    },
    'the tick column filter must keep exactly the four ticked rows'
  );

  // Reset puts everything back and then has nothing left to do.
  await table.locator('.gw-tbl-reset').click();
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '8 von 8 Zeilen', saw };
    },
    'resetting the filters must restore every row'
  );
  assert(
    await table.locator('.gw-tbl-reset').isDisabled(),
    'with nothing filtered the reset button must not offer to do anything'
  );
});

await check('C8 a filter that matches nothing says so', async (page) => {
  // An empty tbody looks like a broken table. Saying "0 von 8" and naming the reason is the
  // difference between a filtered table and a lost one.
  const { table } = await loadTablePage(page);
  await table.locator('.gw-tbl-bar input').fill('zzzz');
  await until(
    async () => {
      const saw = await table.locator('.gw-tbl-count').textContent();
      return { ok: saw === '0 von 8 Zeilen', saw };
    },
    'the count must report zero against the total'
  );
  const empty = table.locator('.gw-tbl-empty');
  await empty.waitFor({ state: 'visible', timeout: 5_000 });
  assert(
    (await empty.getAttribute('colspan')) === '4',
    'the message must span the table rather than sitting under the first column'
  );
});

await check('C9 a sort button works from the keyboard, with Enter and with Space', async (page) => {
  const { table } = await loadTablePage(page);
  const button = table.locator('.gw-tbl-sort').nth(1);
  await button.focus();

  await page.keyboard.press('Enter');
  await until(
    async () => {
      const saw = await ariaSorts(table);
      return { ok: saw[1] === 'ascending', saw };
    },
    'Enter did not sort the column'
  );

  // Space too. A div with a click handler answers Enter through no mechanism at all and
  // Space never — which is exactly the failure a real <button> exists to prevent.
  await page.keyboard.press(' ');
  await until(
    async () => {
      const saw = await ariaSorts(table);
      return { ok: saw[1] === 'descending', saw };
    },
    'Space did not sort the column'
  );

  assert(
    await button.evaluate((el) => el === document.activeElement),
    'focus must stay on the button that was pressed'
  );
});

await check('C10 the header stays put while the table scrolls under it', async (page) => {
  // Sized so eight rows cannot fit in the box's 70vh, which is what makes the box a real
  // scrollport — a sticky header in a box that never scrolls looks correct and does nothing.
  await page.setViewportSize({ width: 1280, height: 320 });
  const { table } = await loadTablePage(page);
  const box = table.locator('.gw-tbl-scroll');

  const scrollable = await box.evaluate((el) => el.scrollHeight > el.clientHeight + 1);
  assert(scrollable, 'the scroll box must actually scroll vertically at this height');

  const geometry = await box.evaluate(async (el) => {
    el.scrollTop = 200;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const th = el.querySelector('thead th');
    const firstRow = el.querySelector('tbody tr');
    return {
      offset: th.getBoundingClientRect().top - el.getBoundingClientRect().top,
      firstRowTop: firstRow.getBoundingClientRect().top,
      headerTop: th.getBoundingClientRect().top,
      background: getComputedStyle(th).backgroundColor
    };
  });
  assert(
    Math.abs(geometry.offset) < 2,
    `the header must sit at the top of the scrolled box, it sat ${geometry.offset}px away`
  );
  assert(
    geometry.firstRowTop < geometry.headerTop,
    'the first row must have scrolled up behind the header, not stayed below it'
  );
  // Without an opaque background the rows scroll THROUGH the header and both become
  // unreadable — which is a rendering bug that a geometry assertion alone cannot see.
  assert(
    geometry.background !== 'rgba(0, 0, 0, 0)' && geometry.background !== 'transparent',
    `the sticky header must be opaque, computed background was ${geometry.background}`
  );
});

// ---------------------------------------------------------------------------------------
// Group D — the page metadata (Breadcrumb.svelte, PageMeta.svelte, Subpages.svelte)
//
// Against the tour corpus, which has what this needs: a container page with six children,
// a branch three levels deep, and pages that are actually `public` — so every check below
// works for an anonymous visitor. Nothing here touches `/rundgang/nur-intern`, which is
// `restricted` on purpose and would 403 for a developer whose `.env` sets no dev identity,
// failing for a reason that has nothing to do with this code.
//
// The colour assertions resolve the token through a probe element rather than comparing
// against a hex copied out of tokens.css, and they assert EQUALITY with the resolved
// value. "Differs from something else" is the shape that made A2 a false pass for months.
// ---------------------------------------------------------------------------------------

const CONTAINER_PAGE = '/rundgang'; // public, six children
const NESTED_PAGE = '/rundgang/import-export'; // public, one child, two levels down
const DEEP_PAGE = '/rundgang/import-export/heikel'; // public, the deepest page there is

/** The computed value of a custom property, as this page currently resolves it. */
const resolveToken = (locator, token) =>
  locator.evaluate((el, name) => {
    const probe = document.createElement('span');
    probe.style.color = `var(${name})`;
    el.appendChild(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  }, token);

await check('D1 the whole panel is in the HTML the server sends, before any script runs', async (page) => {
  // `page.request` is a plain HTTP fetch through the browser's networking stack: nothing
  // is rendered, nothing hydrates, no module is imported. What comes back is exactly what
  // a reader with JavaScript switched off receives — which is the requirement, and which
  // no amount of poking at the live DOM can demonstrate, because by then the app has run.
  const response = await page.request.get(BASE + CONTAINER_PAGE);
  assert(response.ok(), `expected 200 from ${CONTAINER_PAGE}, got ${response.status()}`);
  const html = await response.text();

  assert(/<nav[^>]*aria-label="Pfad"/.test(html), 'no breadcrumb in the server-rendered HTML');
  assert(
    /aria-label="Angaben zu dieser Seite"/.test(html),
    'no metadata panel in the server-rendered HTML'
  );
  assert(
    html.includes('Öffentlich im Internet'),
    'the server-rendered HTML does not state the visibility'
  );

  // The subpage links, checked INSIDE the subpage list. Every one of these paths also
  // appears in the sidebar tree on the same page, so searching the whole document would
  // pass with the list missing entirely.
  const section = html.match(/<nav[^>]*aria-labelledby="gw-subpages"[\s\S]*?<\/nav>/)?.[0];
  assert(section !== undefined, 'no subpage list in the server-rendered HTML');
  assert(section.includes('Unterseiten'), 'the subpage list is not headed');
  assert(
    section.includes(`href="${NESTED_PAGE}"`),
    'the subpage list does not link the child it is supposed to list'
  );
});

await check('D2 breadcrumb links read as chrome, and still read as links', async (page) => {
  // Two failures in one check, because the fix for either alone reintroduces the other.
  //
  // The colour is muted ink DELIBERATELY — accent blue above every heading competes with
  // the heading. But muted with no underline is exactly the regression A2 documents: it
  // reads as a row of labels rather than as something you can click. So: muted AND
  // underlined, both asserted.
  //
  // Equality with the resolved `--ink-muted` is also the only thing that would notice a
  // cascade-layer regression here. `@layer base` sets `a { color: var(--accent) }`; the
  // breadcrumb's own rule lives in `@layer components` and wins only because `components`
  // comes later in the order app.css declares. Load the page stylesheet before the
  // layout's `@layer` statement and the order silently inverts — every breadcrumb turns
  // accent blue and nothing else in this suite notices.
  await page.goto(BASE + NESTED_PAGE, { waitUntil: 'domcontentloaded' });
  const link = page.locator('nav[aria-label="Pfad"] a').first();
  await link.waitFor({ state: 'visible' });

  const muted = await resolveToken(link, '--ink-muted');
  const { color, underlined } = await link.evaluate((el) => {
    const cs = getComputedStyle(el);
    return { color: cs.color, underlined: cs.textDecorationLine.includes('underline') };
  });

  assert(color === muted, `a breadcrumb link is ${color}, expected --ink-muted (${muted})`);
  assert(underlined, 'a breadcrumb link is muted AND unmarked, so it does not read as a link');
});

await check('D3 the breadcrumb names every level and marks only the current one', async (page) => {
  await page.goto(BASE + DEEP_PAGE, { waitUntil: 'domcontentloaded' });
  const crumbs = page.locator('nav[aria-label="Pfad"] a');
  await crumbs.first().waitFor({ state: 'visible' });

  const hrefs = await crumbs.evaluateAll((els) => els.map((el) => el.getAttribute('href')));
  assert(
    JSON.stringify(hrefs) ===
      JSON.stringify(['/', '/rundgang', NESTED_PAGE, DEEP_PAGE]),
    `the breadcrumb must run root-first through every ancestor, got ${JSON.stringify(hrefs)}`
  );

  const marked = await page
    .locator('nav[aria-label="Pfad"] a[aria-current="page"]')
    .evaluateAll((els) => els.map((el) => el.getAttribute('href')));
  assert(
    JSON.stringify(marked) === JSON.stringify([DEEP_PAGE]),
    `exactly one crumb — the last — may claim to be the current page, got ${JSON.stringify(marked)}`
  );
});

await check('D4 a breadcrumb ancestor and a subpage link both actually navigate', async (page) => {
  // The two directions a reader moves from this page. Asserted by navigating, not by
  // reading the href: an anchor with a correct href inside a container that swallows the
  // click is a link that looks right in the markup and does nothing.
  await page.goto(BASE + DEEP_PAGE, { waitUntil: 'domcontentloaded' });
  await page.locator(`nav[aria-label="Pfad"] a[href="${NESTED_PAGE}"]`).click();
  await page.waitForURL(`**${NESTED_PAGE}`, { timeout: 5_000 });

  const down = page.locator(`nav[aria-labelledby="gw-subpages"] a[href="${DEEP_PAGE}"]`);
  await down.waitFor({ state: 'visible' });
  await down.click();
  await page.waitForURL(`**${DEEP_PAGE}`, { timeout: 5_000 });

  const heading = (await page.locator('h1').first().textContent()) ?? '';
  assert(
    heading.trim().startsWith('Heikler Text'),
    `the subpage link did not land on the subpage, the heading says "${heading.trim()}"`
  );
});

await check('D5 a container page lists its children, a leaf offers no empty section', async (page) => {
  await page.goto(BASE + CONTAINER_PAGE, { waitUntil: 'domcontentloaded' });
  const list = page.locator('nav[aria-labelledby="gw-subpages"] a');
  await list.first().waitFor({ state: 'visible' });
  const count = await list.count();
  assert(count === 6, `the tour page has six children, the list shows ${count}`);

  // The one child that has a child of its own says so, and says it in German.
  const note = await page
    .locator(`nav[aria-labelledby="gw-subpages"] li:has(a[href="${NESTED_PAGE}"]) .count`)
    .textContent();
  assert(note?.trim() === '1 Unterseite', `expected "1 Unterseite", got "${note?.trim()}"`);

  // A page with no children gets no heading, no rule and no empty list — a permanent cost
  // for every leaf otherwise, and an empty section reads as a section that failed to load.
  await page.goto(BASE + DEEP_PAGE, { waitUntil: 'domcontentloaded' });
  const sections = await page.locator('nav[aria-labelledby="gw-subpages"]').count();
  assert(sections === 0, 'a page with no children still rendered a subpage section');
});

await check('D6 a world-readable page says so, in the colour that means "notice this"', async (page) => {
  // Not a value judgement about being public — it is the state whose consequences are
  // irreversible if unintended, on a wiki whose every imported page is `restricted`. The
  // words carry the meaning on their own; the tint only makes it visible from across the
  // room, and is mixed from `--warn` so a theme repaints it along with everything else.
  await page.goto(BASE + CONTAINER_PAGE, { waitUntil: 'domcontentloaded' });
  const chip = page.locator('[aria-label="Angaben zu dieser Seite"] .chip');
  await chip.waitFor({ state: 'visible' });

  const label = (await chip.textContent())?.trim();
  assert(
    label === 'Öffentlich im Internet',
    `"Öffentlich" alone reads as "everyone in the organisation" in an intranet; got "${label}"`
  );
  assert(
    (await chip.getAttribute('data-visibility')) === 'public',
    'the chip does not state which of the three levels it is showing'
  );

  const detail = await page
    .locator('[aria-label="Angaben zu dieser Seite"] .detail')
    .textContent();
  assert(
    detail?.trim() === 'Jede Person kann diese Seite ohne Anmeldung lesen.',
    `the sentence that makes the label unmistakable is missing, got "${detail?.trim()}"`
  );

  const warn = await resolveToken(chip, '--warn');
  const border = await chip.evaluate((el) => getComputedStyle(el).borderTopColor);
  assert(border === warn, `the public chip is bordered ${border}, expected --warn (${warn})`);
});

await check('D7 no horizontal scroll at 390px with the panel and the subpage grid', async (page) => {
  // A4 checks this for the widest thing a document can contain. This checks it for the
  // two new grids: the panel's `max-content` label column, which a long term can push
  // wide, and the subpage list's `minmax(14rem, 1fr)` columns.
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(BASE + CONTAINER_PAGE, { waitUntil: 'networkidle' });

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth
  }));
  assert(
    scrollWidth <= innerWidth + 1,
    `document.documentElement.scrollWidth (${scrollWidth}) exceeds window.innerWidth + 1 (${innerWidth + 1})`
  );
});

// ---------------------------------------------------------------------------------------
// Group E — the editor (web/src/lib/editor/**)
//
// Everything here works for BOTH answers to "may this developer write this page", because
// the answer depends on a grant in whatever database the dev server is pointed at, and a
// check that only passes on a seeded-and-granted instance is a check that gets deleted.
//
// The refusal is the case worth the most: a fresh instance has NO write grants at all —
// `seed` creates none and no migration inserts any — so this is what nearly every developer
// and every reader actually meets. What must be true either way is that the reading page
// never regresses, that no editable surface exists before the server has agreed to one, and
// that whatever the editor says about the work is not a lie.
// ---------------------------------------------------------------------------------------

const EDIT_PAGE = '/rundgang';

/** Waits for the session to settle into a state that has a headline. */
async function loadEditor(page) {
  await page.goto(BASE + EDIT_PAGE + '?edit=1', { waitUntil: 'networkidle' });
  const region = page.locator('section[aria-label="Seite bearbeiten"]');
  await region.waitFor({ state: 'visible', timeout: 10_000 });
  const head = region.locator('.gw-ed-status-head');
  await head.waitFor({ state: 'visible', timeout: 10_000 });
  // Not "connecting": that is the state before an answer, and asserting against it would
  // pass whatever the answer turned out to be.
  const settled = await until(
    async () => {
      const saw = (await head.textContent())?.trim() ?? '';
      return { ok: saw.length > 0 && !saw.includes('wird geöffnet'), saw };
    },
    'the editing session never settled into an answer',
    10_000
  );
  return { region, headline: settled.saw };
}

await check('E1 asking to edit still serves the whole document in the first response', async (page) => {
  // The requirement that outranks the feature. `page.request` is a plain fetch: nothing
  // hydrates, no module is imported, so this is exactly what a reader with JavaScript off
  // receives — and it must be the page, not a mount point for one.
  const response = await page.request.get(BASE + EDIT_PAGE + '?edit=1');
  assert(response.ok(), `expected 200, got ${response.status()}`);
  const html = await response.text();

  assert(/<article[^>]*class="prose/.test(html), 'the document is not in the server-rendered HTML');
  assert(/<nav[^>]*aria-label="Pfad"/.test(html), 'the breadcrumb went missing while editing');
  // And nothing editable, because the server has not been asked yet whether this caller may.
  assert(!html.includes('contenteditable'), 'the SSR HTML contains an editable surface');
  assert(!html.includes('role="textbox"'), 'the SSR HTML claims an editing surface exists');
});

await check('E2 the editor never both refuses and offers a place to type', async (page) => {
  // The one thing that must never happen, whichever way the permission goes: an editor that
  // appears and silently throws keystrokes away. Either the session is live and there is a
  // surface, or it is not and there is none — never a surface without a live session.
  const { region, headline } = await loadEditor(page);
  const live = headline.includes('Verbunden');
  const surfaces = await region.locator('[contenteditable="true"]').count();

  if (live) {
    assert(surfaces === 1, `a live session must have exactly one surface, found ${surfaces}`);
  } else {
    assert(
      surfaces === 0,
      `the session says "${headline}" and yet offers ${surfaces} place(s) to type`
    );
    // A refusal has to say what to do about it, and must not read as a network fault.
    const detail = await region.locator('.gw-ed-status-detail').textContent();
    assert(
      /Berechtigung|Verbindung|Server/.test(detail ?? ''),
      `the refusal explains nothing: "${detail}"`
    );
  }
});

await check('E3 the reading page is intact underneath, whatever the session decided', async (page) => {
  // A refused session must leave a readable page behind, not a blank frame where the
  // document was. This is the failure a reader would notice first and forgive least.
  const { region, headline } = await loadEditor(page);
  if (headline.includes('Verbunden')) {
    const text = await region.locator('[contenteditable="true"]').textContent();
    assert((text ?? '').trim().length > 0, 'the live editor mounted with no content in it');
  } else {
    const article = page.locator('article.prose');
    await article.waitFor({ state: 'visible', timeout: 5_000 });
    const text = await article.textContent();
    assert((text ?? '').trim().length > 0, 'a refused session left an empty page behind');
  }
});

await check('E4 the history warning is on screen the whole time somebody is editing', async (page) => {
  // D-M2-9 and D-M3-5 require this AT THE POINT OF EDITING rather than in documentation:
  // anyone who may read the page may read every revision, so removing a sentence is an edit
  // and not a redaction. Somebody pasting a password needs to learn that now, not later.
  const { region } = await loadEditor(page);
  const warning = region.locator('.gw-ed-history');
  await warning.waitFor({ state: 'visible', timeout: 5_000 });
  const text = (await warning.textContent()) ?? '';
  assert(/Versionsgeschichte/.test(text), `the warning does not mention the history: "${text}"`);
  assert(/lösch/i.test(text), `the warning does not mention deleting: "${text}"`);
  // Permanent, not a toast: nothing may dismiss it.
  assert(
    (await region.locator('.gw-ed-history button').count()) === 0,
    'the history warning can be dismissed, which makes it a notification rather than a fact'
  );
});

await check('E5 the toolbar offers exactly what a revision can actually store', async (page) => {
  // `gw_core::Block` grew a `marks` field (Task 5) and `gw-collab` now writes and reads them,
  // so the five marks below joined the block controls that were always here. The check keeps
  // its original shape — an exact list, not a superset check — because that shape is what
  // catches EITHER direction of drift: a control added for something the server cannot store,
  // or one silently missing for something it can.
  const { region } = await loadEditor(page);
  const toolbar = region.locator('[role="toolbar"]');
  await toolbar.waitFor({ state: 'visible', timeout: 5_000 });

  const labels = await toolbar
    .locator('button')
    .evaluateAll((els) => els.map((el) => el.getAttribute('aria-label')));
  assert(
    JSON.stringify(labels) ===
      JSON.stringify([
        'Überschrift 2',
        'Überschrift 3',
        'Überschrift 4',
        'Aufzählung',
        'Nummerierte Liste',
        'Zitat',
        'Codeblock',
        'Fett',
        'Kursiv',
        'Code',
        'Durchgestrichen',
        'Link'
      ]),
    `the toolbar offers something the server cannot store, or lost something it can: ${JSON.stringify(labels)}`
  );
  // `MarkKind` has no `underline` — that control must still never appear, for the same
  // reason a bold button could not before this task: publishing would throw its effect away.
  assert(!labels.includes('Unterstrichen'), 'the toolbar offers "Unterstrichen", which publishing drops');
});

await check('E6 a control that cannot reach the document says so by being disabled', async (page) => {
  // A refused session's toolbar must not look operable. An enabled button that does nothing
  // is the same lie as an editor that discards keystrokes, in a smaller box.
  const { region, headline } = await loadEditor(page);
  const buttons = region.locator('[role="toolbar"] button');
  await buttons.first().waitFor({ state: 'attached', timeout: 5_000 });
  const disabled = await buttons.evaluateAll((els) => els.map((el) => el.disabled));

  if (headline.includes('Verbunden')) {
    assert(
      disabled.every((d) => d === false),
      'a live session left its toolbar disabled'
    );
  } else {
    assert(
      disabled.every((d) => d === true),
      `the session says "${headline}" and yet its toolbar is operable`
    );
  }
});

await check('E7 toggling Fett on a live selection marks it up, and the toolbar agrees', async (page) => {
  // The live counterpart to `extensions.test.ts`'s CRDT-level proof. That test calls the
  // exact conversion `@tiptap/extension-collaboration` uses to sync into a Y.Doc directly, so
  // it already pins the wire keys; this exercises the part it cannot reach — a real browser,
  // a real ContentEditable, the real ProseMirror keymaps — to confirm the renamed `Strong`
  // extension still behaves like a normal mark all the way through typing and toggling it.
  const { region, headline } = await loadEditor(page);
  if (!headline.includes('Verbunden')) return; // nothing to type into without a live session

  const surface = region.locator('[contenteditable="true"]');
  await surface.click();
  await page.keyboard.type('fett');
  await page.keyboard.press('Shift+Home');

  const fettButton = region.locator('[role="toolbar"] button[aria-label="Fett"]');
  await fettButton.click();
  await until(
    async () => {
      const saw = await fettButton.getAttribute('data-state');
      return { ok: saw === 'on', saw };
    },
    'the Fett button never reported itself pressed after being clicked'
  );

  const html = await surface.innerHTML();
  assert(/<strong[^>]*>fett<\/strong>/.test(html), `expected a <strong>fett</strong> run, got: ${html}`);
});

// ---------------------------------------------------------------------------------------
// Group F — the graph (web/src/routes/graph/+page.svelte, web/src/lib/graph/layout.ts)
//
// The one screen in this plan that is a PICTURE rather than text or a control, so it is the
// one none of groups A-E can stand in for: a wrong arrowhead direction, an unreachable node
// or a broken focus style are all things `page.test.ts`'s server-rendered-string assertions
// cannot see, because they are about what the browser actually does with the markup, not
// about which tags are in it.
//
// Against `/verweisbeispiel` and its child `/verweisbeispiel/verweist-zurueck`
// (content-example), added alongside this task for exactly this purpose: `content-example`
// otherwise has no links at all between its pages (every existing tour page is prose with no
// `[text](/…)` in it), so the graph route had nothing real to draw in a fresh clone. The
// child is seeded AFTER its parent — collect_markdown's shallowest-first order guarantees
// that regardless of alphabetical position among root files — so the one link in it
// (`[Verweisbeispiel](/verweisbeispiel)`) always resolves to a document that already exists,
// giving a graph of exactly two nodes and one edge, deterministically, from `content-example`
// alone. `content-darm` is NOT used here: every page in it is `restricted`, which would need
// a dev identity these checks do not assume, and this task is explicit that `content-darm/`
// is not to be touched.
// ---------------------------------------------------------------------------------------

const GRAPH_SOURCE = '/verweisbeispiel/verweist-zurueck'; // the edge starts here
const GRAPH_TARGET = '/verweisbeispiel'; // and points here

await check('F1 an arrowhead marker is present and the edge points from source to target', async (page) => {
  await page.goto(BASE + '/graph', { waitUntil: 'domcontentloaded' });
  const svg = page.locator('svg');
  await svg.waitFor({ state: 'visible' });

  // The marker itself: a triangle, referenced by the line as `marker-end`. Not merely
  // "some <marker> exists" — the exact id the line points at, and a path inside it, or a
  // marker with nothing to draw would satisfy "a marker is present" just as well.
  const markerPath = svg.locator('marker#gw-graph-pfeil path');
  assert((await markerPath.count()) === 1, 'expected exactly one <path> inside marker#gw-graph-pfeil');

  const line = svg.locator('.edges line');
  assert((await line.count()) === 1, `expected exactly one edge, found ${await line.count()}`);
  assert(
    (await line.getAttribute('marker-end')) === 'url(#gw-graph-pfeil)',
    'the edge line does not reference the arrowhead marker'
  );

  // Direction: `edgeLine` starts the line at the SOURCE node's rim and ends it at the
  // TARGET node's rim (see web/src/lib/graph/layout.ts), so (x1,y1) must sit closer to the
  // source's centre than to the target's, and (x2,y2) the other way round. A marker on the
  // wrong end of a correctly-drawn line would pass every check above and still point the
  // wrong way, which is why this is checked separately from "a marker exists".
  const centre = async (href) => {
    const circle = svg.locator(`a[href="${href}"] circle`);
    const [cx, cy] = await Promise.all([circle.getAttribute('cx'), circle.getAttribute('cy')]);
    return { x: Number(cx), y: Number(cy) };
  };
  const source = await centre(GRAPH_SOURCE);
  const target = await centre(GRAPH_TARGET);
  const [x1, y1, x2, y2] = await Promise.all([
    line.getAttribute('x1'),
    line.getAttribute('y1'),
    line.getAttribute('x2'),
    line.getAttribute('y2')
  ]).then((vals) => vals.map(Number));

  const dist = (ax, ay, b) => Math.hypot(ax - b.x, ay - b.y);
  assert(
    dist(x1, y1, source) < dist(x1, y1, target),
    `the line's start (${x1},${y1}) is not closer to the source node ${JSON.stringify(source)} than to the target ${JSON.stringify(target)}`
  );
  assert(
    dist(x2, y2, target) < dist(x2, y2, source),
    `the line's end (${x2},${y2}), where the arrowhead sits, is not closer to the target node ${JSON.stringify(target)} than to the source ${JSON.stringify(source)}`
  );
});

await check('F2 a node is a real link and navigating it reaches that page', async (page) => {
  await page.goto(BASE + '/graph', { waitUntil: 'domcontentloaded' });
  const node = page.locator(`svg a[href="${GRAPH_TARGET}"]`);
  await node.waitFor({ state: 'visible' });
  await node.click();
  await page.waitForURL(`**${GRAPH_TARGET}`, { timeout: 5_000 });

  const heading = (await page.locator('h1').first().textContent())?.trim() ?? '';
  assert(
    heading === 'Verweisbeispiel',
    `clicking the node did not land on its page: heading reads "${heading}"`
  );
});

await check('F3 keyboard focus reaches a node, with a focus style that is visibly distinct', async (page) => {
  await page.goto(BASE + '/graph', { waitUntil: 'domcontentloaded' });

  // Start from the last thing before the diagram in DOM order — the filter button — and
  // Tab once. The <line>s and the marker's <path> carry no tabindex, so the very next stop
  // after the button is the first node's own <a>.
  await page.locator('.filter button[type="submit"]').focus();
  await page.keyboard.press('Tab');

  const focused = page.locator('svg .nodes a:focus');
  assert(
    (await focused.count()) === 1,
    'Tab from the filter button did not land on a node link inside svg .nodes'
  );

  // The design's own rule (+page.svelte): `.nodes a:focus-visible circle { fill: var(--ink) }`
  // against a default of `var(--accent)`. Resolved through a probe element and compared for
  // EQUALITY with --ink specifically — not merely "different from before" — because a focus
  // style that changed to some OTHER wrong colour would pass an inequality check too.
  const circle = focused.locator('circle');
  const ink = await resolveToken(circle, '--ink');
  const fill = await circle.evaluate((el) => getComputedStyle(el).fill);
  assert(fill === ink, `a focused node's circle is filled ${fill}, expected --ink (${ink})`);
});

await check('F4 no horizontal scroll at 390px width', async (page) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(BASE + '/graph', { waitUntil: 'networkidle' });

  const { scrollWidth, innerWidth } = await page.evaluate(() => ({
    scrollWidth: document.documentElement.scrollWidth,
    innerWidth: window.innerWidth
  }));
  assert(
    scrollWidth <= innerWidth + 1,
    `document.documentElement.scrollWidth (${scrollWidth}) exceeds window.innerWidth + 1 (${innerWidth + 1})`
  );
});

await check('F5 the empty state renders its German sentence and no <svg>', async (page) => {
  // A root naming a subtree that does not exist answers an empty graph, not an error (see
  // `within_root` in crates/gw-store/src/links.rs) — a deterministic, content-independent
  // way to reach the empty state without relying on the whole wiki having no links.
  await page.goto(BASE + '/graph?wurzel=%2Fnichts-hier-xyz-123', { waitUntil: 'domcontentloaded' });

  const empty = page.locator('p.empty');
  await empty.waitFor({ state: 'visible' });
  const text = (await empty.textContent())?.replace(/\s+/g, ' ').trim() ?? '';
  assert(
    text.startsWith('Noch keine Verweise'),
    `expected the empty-state paragraph to start with "Noch keine Verweise", got "${text}"`
  );
  assert((await page.locator('svg').count()) === 0, 'the empty state must render no <svg> at all');
});

await browser.close();

// ---------------------------------------------------------------------------------------
const passed = results.filter((r) => r.ok).length;
const failed = results.length - passed;
console.log(`\n${passed}/${results.length} checks passed${failed ? `, ${failed} FAILED` : ''}`);

process.exit(failed === 0 ? 0 : 1);
