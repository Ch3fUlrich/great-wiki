// Screenshot the reader so a design can be looked at rather than imagined.
// Points at the local dev server, bypassing the proxy, so no login is involved.
import { chromium } from 'playwright';
import { mkdirSync } from 'node:fs';

const BASE = process.env.SHOT_BASE ?? 'http://127.0.0.1:5173';
const OUT = process.env.SHOT_OUT ?? '/tmp/shots';
mkdirSync(OUT, { recursive: true });

const PAGES = [
  ['home', '/'],
  ['doc', '/rundgang/was-schon-geht'],
  ['table', '/rundgang/tabellen-was-heute-passiert'],
];
const VIEWPORTS = [
  ['desktop', { width: 1440, height: 1000 }],
  ['phone', { width: 390, height: 844 }],
];

const browser = await chromium.launch();
for (const [vpName, viewport] of VIEWPORTS) {
  for (const scheme of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport, colorScheme: scheme });
    const page = await ctx.newPage();
    for (const [name, path] of PAGES) {
      await page.goto(BASE + path, { waitUntil: 'networkidle' });
      await page.screenshot({
        path: `${OUT}/${name}-${vpName}-${scheme}.png`,
        fullPage: vpName === 'phone',
      });
    }
    await ctx.close();
  }
}
await browser.close();
console.log('screenshots written to', OUT);
