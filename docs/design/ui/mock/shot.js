// Usage: node docs/design/ui/mock/shot.js <html> <outdir> <deviceScaleFactor> <name>[=<query>]... Needs Playwright.
// Each name is an element id to capture ("page" = the whole page); "=<query>" opens the page with that query first,
// e.g. "window-a=s=a" captures #win of window.html?s=a into window-a.png.
const { chromium } = require('playwright');
const path = require('path');
(async () => {
  const [html, outdir, dsf, ...items] = process.argv.slice(2);
  const browser = await chromium.launch();
  const page = await browser.newPage({ deviceScaleFactor: Number(dsf), viewport: { width: 1800, height: 1400 } });
  for (const item of items) {
    const [name, query] = item.split(/=(.*)/s);
    await page.goto('file://' + path.resolve(html) + (query ? '?' + query : ''));
    await page.evaluate(() => document.fonts.ready);
    await page.waitForTimeout(300);
    const id = query ? (await page.evaluate(() => (document.querySelector('[data-shot]') || {}).id)) || 'win' : name;
    const out = path.join(outdir, name + '.png');
    if (id === 'page') await page.screenshot({ path: out, fullPage: true });
    else await page.locator('#' + id).screenshot({ path: out });
    console.log('wrote', out);
  }
  await browser.close();
})();
