// Usage: node docs/design/ui/mock/export.js   (writes docs/design/ui/icons/*.png). Needs Playwright.
// Renders the OpenUT mark (glyph.js, 2.2) for the tray (16, 20, 24, 32), and the toast logo (96).
const { chromium } = require("playwright");
const fs = require("fs");
const path = require("path");
const COLORS = { ok: "#2F9466", wait: "#B07A12", fault: "#D64A3C" };
(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto("file://" + path.resolve(__dirname, "tray.html"));
  const jobs = [];
  for (const v of ["ok", "wait", "fault"]) for (const n of [16, 20, 24, 32]) jobs.push([`tray-${v}-${n}.png`, v, n]);
  jobs.push(["toast-fault-96.png", "fault", 96], ["toast-ok-96.png", "ok", 96]);
  const dir = path.resolve(__dirname, "../icons");
  fs.mkdirSync(dir, { recursive: true });
  for (const [name, v, n] of jobs) {
    const url = await page.evaluate(([v, n, c]) => {
      const cv = document.createElement("canvas");
      cv.width = n; cv.height = n;
      cv.getContext("2d").putImageData(new ImageData(window.OpenUTGlyph.render(n, v, c), n, n), 0, 0);
      return cv.toDataURL("image/png");
    }, [v, n, COLORS[v]]);
    fs.writeFileSync(path.join(dir, name), Buffer.from(url.split(",")[1], "base64"));
    console.log("wrote", name);
  }
  await browser.close();
})();
