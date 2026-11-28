// Usage: node docs/design/ui/mock/shot.js <html> <outdir> <deviceScaleFactor> <element id>... ("page" = whole page). Needs Playwright.
const { chromium } = require('playwright');
const path = require('path');
(async () => {
  const [html, outdir, dsf, ...ids] = process.argv.slice(2);
  const browser = await chromium.launch();
  const page = await browser.newPage({ deviceScaleFactor: Number(dsf), viewport: { width: 1800, height: 1200 } });
  await page.goto('file://' + path.resolve(html));
  await page.evaluate(() => document.fonts.ready);
  await page.waitForTimeout(300);
  for (const id of ids) {
    const out = path.join(outdir, id + '.png');
    if (id === 'page') await page.screenshot({ path: out, fullPage: true });
    else await page.locator('#' + id).screenshot({ path: out });
    console.log('wrote', out);
  }
  await browser.close();
})();
