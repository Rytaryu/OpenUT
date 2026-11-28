// Usage: node docs/design/ui/mock/fit.js   (prints the tables in m1.md, 3.6 and 4.5). Needs Playwright.
// Measures, in the font of the mockups, whether the words fit the main window at its minimum size
// and how many lines the VR notice panel needs for the longest labels (wrap.js, 4.4).
const { chromium } = require("playwright");
const path = require("path");

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto("file://" + path.resolve(__dirname, "vr.html"));
  await page.evaluate(() => document.fonts.ready);
  const out = await page.evaluate(() => {
    const fam = getComputedStyle(document.documentElement).getPropertyValue("--font").trim();
    const { wrap, measure } = window.OpenUTWrap;
    const X = window.OpenUTText, W = X.words;
    const m = (w, px, s) => measure(`${w} ${px}px ${fam}`, s);
    // Minimum window 1000: tiles of 153 (inner 129). Default 1120: 176 (inner 152). 3.3.
    const MIN_IN = 129, DEF_IN = 152;
    const rows = [];
    const add = (what, need, have) => rows.push({ what, need: Math.ceil(need), have });
    add("タイルの名前（16 Bold。「トラッカー16」）", m(700, 16, "トラッカー16"), MIN_IN);
    add("シリアル番号（13。13 文字、末尾 5 文字が Bold）", m(400, 13, "00XX0000") + m(700, 13, "A0001"), MIN_IN);
    add("ファームウェアの版（13。「10.10.9999.9999」）", m(400, 13, "10.10.9999.9999"), MIN_IN);
    add("状態（印 14 ＋ 6 ＋ 16 Bold「待機中」）", 14 + 6 + m(700, 16, "待機中"), MIN_IN);
    add("ホストの印（12 Bold ＋ 内側 8 × 2）", m(700, 12, W.host) + 16, MIN_IN);
    add("接続（印 16 ＋ 6 ＋ 13「ドングル」）", 16 + 6 + m(400, 13, "ドングル"), MIN_IN);
    add("電池（印 26 ＋ 6 ＋ 13 Bold「100%」）", 26 + 6 + m(700, 13, "100%"), MIN_IN);
    add("接続と電池を 1 行に（既定の幅。間 8）", 16 + 6 + m(400, 13, "ドングル") + 8 + 26 + 6 + m(700, 13, "100%"), DEF_IN);
    add("ドングルの状態（16 Bold「接続済み」。幅 120、内側 96）", m(700, 16, "接続済み"), 96);
    add("全体（印 28 ＋ 12 ＋ 28 Bold「待機中」＋ 左 20。幅 184）", 28 + 12 + m(700, 28, "待機中") + 20, 184);
    add("操作のボタン（印 20 ＋ 10 ＋ 14 Bold「キャリブレーション」＋ 内側 16 × 2。幅 232）", 20 + 10 + m(700, 14, W.ops.cal) + 32, 232);
    add("操作の手引きの見出し（印 20 ＋ 8 ＋ 16 Bold「キャリブレーション」。幅 176）", 20 + 8 + m(700, 16, W.ops.cal), 176);
    add("下の行（12「通知」＋ 20 ＋ 2 つのチェックボックス）", m(400, 12, W.notify) + 20 + (16 + 8 + m(400, 14, W.notifyVr)) + 20 + (16 + 8 + m(400, 14, W.notifyDesktop)), 1000 - 40);
    // Lines: band line at the minimum width (1000 - 40 - 184 - 40 = 736), step names (126 per step).
    const lines = [];
    const L = (what, s, font, w) => lines.push({ what, n: wrap(s, font, w).length, text: wrap(s, font, w).join(" / ") });
    for (const g of ["G1", "G2", "G3", "G4", "G5"]) L(`帯の 1 行 ${g}（18、736）`, X.guide[g].line, `400 18px ${fam}`, 736);
    for (const k of Object.keys(X.health)) L(`帯の 1 行 ${k}（18、736）`, X.health[k], `400 18px ${fam}`, 736);
    const R1 = X.notice("R1", "Raised", "", "トラッカー5（…00XX0000A0001）");
    L("帯の見出し R1（18 Bold、736。末尾 13 文字の呼び名）", R1.headline, `700 18px ${fam}`, 736);
    L("帯のすること R1（16、736。末尾 13 文字の呼び名）", R1.action, `400 16px ${fam}`, 736);
    for (const g of ["G1", "G2", "G3", "G4", "G5"]) L(`手引きの段 ${g}（14 Bold、126）`, X.guide[g].step, `700 14px ${fam}`, 126);
    // VR notice panel (4.5): the widest labels. Tails of 4, 5 and 13 characters (the whole serial).
    const labels = ["トラッカー5（…0001）", "トラッカー5（…A0001）", "トラッカー（…A0001）", "トラッカー5（…00XX0000A0001）"];
    const hosts = [...labels, "ホストのトラッカー"];
    const mores = [0, 99];
    const waits = [60, 90, 3599, 5400, 86399];
    const rules = [["R1", "Raised"], ["R1", "Escalated"], ["R1", "Resolved"], ["R2", "Raised"], ["R3", "Raised"], ["R4", "Raised"], ["R4", "Resolved"], ["R5", "Raised"]];
    const HEAD = `700 64px ${fam}`, ACT = `400 54px ${fam}`, PW = 1044;
    const height = (h, a) => 52 + 80 * h + (a ? 12 + 73 * a : 0) + 56;
    const vr = [];
    for (const [r, st] of rules) {
      let worst = null;
      for (const T of labels) for (const H of hosts) for (const more of mores) for (const wait of (r === "R5" ? waits : [60])) {
        const n = X.notice(r, st, T, H, { more, wait });
        const hl = wrap(n.headline, HEAD, PW), al = n.action ? wrap(n.action, ACT, PW) : [];
        const c = { rule: `${r} ${st}`, h: hl.length, a: al.length, px: height(hl.length, al.length), ex: [...hl, "/", ...al].join(" ") };
        if (!worst || c.px > worst.px) worst = c;
      }
      vr.push(worst);
    }
    return { rows, lines, vr };
  });
  console.log("| 確かめたこと | 要る幅 | ある幅 | 結果 |");
  console.log("|---|---|---|---|");
  for (const r of out.rows) console.log(`| ${r.what} | ${r.need} | ${r.have} | ${r.need <= r.have ? "OK" : "NG"} |`);
  console.log("");
  console.log("| 文言 | 行数 | 折り返した形 |");
  console.log("|---|---|---|");
  for (const l of out.lines) console.log(`| ${l.what} | ${l.n} | ${l.text} |`);
  console.log("");
  console.log("| 規則と段階 | 見出しの行数 | することの行数 | 板の高さ | 最も高くなる例（「/」の前が見出し） |");
  console.log("|---|---|---|---|---|");
  for (const r of out.vr) console.log(`| ${r.rule} | ${r.h} | ${r.a || "—"} | ${r.px} | ${r.ex} |`);
  const bad = out.rows.filter((r) => r.need > r.have);
  console.error(bad.length ? "NG: " + bad.map((r) => r.what).join(", ") : "all fit");
  await browser.close();
})();
