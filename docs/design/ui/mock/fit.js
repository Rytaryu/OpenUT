// Usage: node docs/design/ui/mock/fit.js   (prints the tables in m1.md, 2.8 and 5.5). Needs Playwright.
// Measures, in the font of the mockups, whether the words fit the main window at its minimum size
// and how many lines the VR notice overlay needs for the longest labels (wrap.js, 5.4).
// The longest tails are 32 characters (design 3.2, 4.4): the two made-up serials of window.html?s=j,
// and, as a bound, 32 times "W" (the widest letter of the font).
const { chromium } = require("playwright");
const path = require("path");

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto("file://" + path.resolve(__dirname, "vr.html"));
  await page.evaluate(() => document.fonts.ready);
  const out = await page.evaluate(() => {
    const fam = getComputedStyle(document.documentElement).getPropertyValue("--font").trim();
    const { wrap, wrapFromEnd, measure } = window.OpenUTWrap;
    const [SA, SB] = window.OpenUTLong32, SW = "W".repeat(32);
    const X = window.OpenUTText, W = X.words;
    const m = (w, px, s) => measure(`${w} ${px}px ${fam}`, s);
    // Minimum window 1000: cards of 153 (inner 129). Default 1120: 176 (inner 152). 2.4.
    const MIN_IN = 129, DEF_IN = 152;
    const rows = [];
    const add = (what, need, have) => rows.push({ what, need: Math.ceil(need), have });
    const widest = (px, list) => Math.max(...list.map((t) => m(700, px, t)));
    const cardWords = ["未接続", "同期中", "接続済み", "USB接続", "異常"];
    add("カードの名前（16 Bold。「トラッカー16」）", m(700, 16, "トラッカー16"), MIN_IN);
    add("★の付いた名前（星 16 ＋ 4 ＋ 16 Bold「トラッカー16」）", 16 + 4 + m(700, 16, "トラッカー16"), MIN_IN);
    add("カードの語（印 14 ＋ 6 ＋ 16 Bold。最も広い語）", 14 + 6 + widest(16, cardWords), MIN_IN);
    add("電池の行（電池 26 ＋ 6 ＋ 13 Bold「100%」＋ 8 ＋ USB 16）", 26 + 6 + m(700, 13, "100%") + 8 + 16, MIN_IN);
    add("シリアル番号（13。13 文字、末尾 5 文字が Bold）", m(400, 13, "00XX0000") + m(700, 13, "A0001"), MIN_IN);
    add("シリアル番号（13 Bold。13 文字がすべて末尾）", m(700, 13, "00XX0000A0001"), MIN_IN);
    add("ファームウェアのバージョンとメニューのボタン（13「10.10.9999.9999」＋ 4 ＋ 24）", m(400, 13, "10.10.9999.9999") + 4 + 24, MIN_IN);
    add("ドングルの語（16 Bold「接続済み」。幅 120、内側 96）", m(700, 16, "接続済み"), 96);
    add("全体（印 28 ＋ 12 ＋ 28 Bold「待機中」＋ 左 20。幅 184）", 28 + 12 + m(700, 28, "待機中") + 20, 184);
    add("メニュー（14 の最も広い項目 ＋ 内側 12 × 2 ＋ 4 × 2。幅 148）", Math.max(m(400, 14, W.menu.powerOff), m(400, 14, W.menu.unpair)) + 24 + 8, 148);
    const opW = (t) => 16 + 20 + 10 + m(700, 14, t) + 16;
    const foot = opW(W.ops.pair) + 8 + opW(W.ops.map) + 40 + m(400, 12, W.notify) + 4 + 8 +
      (16 + 8 + m(400, 14, W.notifyVr) + 12) + 8 + (16 + 8 + m(400, 14, W.notifyDesktop) + 12) + 8 + 25 + 8 + 36;
    add("下の行（2 つの操作 ＋ 間 40 以上 ＋「通知」と 2 つのチェックボックス ＋ 区切り ＋ 設定）", foot, 1000 - 40);
    // Serial on a card (2.4): the longest tail that still fits on one line after "…", and the lines of a 32-character tail.
    const fitsOne = (w) => { let n = 1; while (m(400, 13, "…") + m(700, 13, "W".repeat(n + 1)) <= w) n++; return n; };
    const lines = [];
    const L = (what, s, font, w) => lines.push({ what, n: wrap(s, font, w).length, text: wrap(s, font, w).join(" / ") });
    const E = (what, s, font, w) => lines.push({ what, n: wrapFromEnd(s, font, w).length, text: wrapFromEnd(s, font, w).join(" / ") });
    lines.push({ what: "カードの 1 行に収まる末尾（13 Bold。最も広い字 W で。最小 129 / 最初 152）", n: `${fitsOne(MIN_IN)} / ${fitsOne(DEF_IN)} 文字`, text: "" });
    E("カードの末尾 32 文字（13 Bold、129。見本 j）", SA, `700 13px ${fam}`, MIN_IN);
    E("カードの末尾 32 文字（13 Bold、152。見本 j）", SA, `700 13px ${fam}`, DEF_IN);
    E("カードの末尾 32 文字（13 Bold、129。W だけ）", SW, `700 13px ${fam}`, MIN_IN);
    E("カードの末尾 32 文字（13 Bold、152。W だけ）", SW, `700 13px ${fam}`, DEF_IN);
    // Lines of the status bar at the minimum width (1000 - 40 - 184 - 40 = 736; 556 with "フォルダーを選ぶ").
    L("ステータスバーの 1 行 VIVE Hub（18、556）", X.hubLine, `400 18px ${fam}`, 556);
    for (const k of Object.keys(X.health)) L(`ステータスバーの 1 行 ${k}（18、736）`, X.health[k], `400 18px ${fam}`, 736);
    const R1 = X.notice("R1", "Raised", "", "トラッカー5（…00XX0000A0001）");
    L("ステータスバーの見出し R1（18 Bold、736。末尾 13 文字の呼び名）", R1.headline, `700 18px ${fam}`, 736);
    L("ステータスバーのすること R1（16、736。末尾 13 文字の呼び名）", R1.action, `400 16px ${fam}`, 736);
    L("ステータスバーのすること R1（16、736。★マークのトラッカー）", X.notice("R1", "Raised", "", X.label({ host: true })).action, `400 16px ${fam}`, 736);
    for (const [k, S] of [["見本 j", SA], ["W だけ", SW]]) {
      const r1 = X.notice("R1", "Raised", "", X.label({ hub: 99, tail: S }));
      L(`ステータスバーのすること R1（16、736。末尾 32 文字、${k}）`, r1.action, `400 16px ${fam}`, 736);
      const r4 = X.notice("R4", "Raised", X.label({ tail: S }), "");
      L(`案内の一覧の見出し R4（14 Bold、714。番号のない台、末尾 32 文字、${k}）`, r4.headline, `700 14px ${fam}`, 736 - 22);
    }
    // VR notice overlay (5.5, provisional): the widest labels. Only the notices that are pushed (design 7.1): R4 Resolved is not.
    // Set 1: tails of 4, 5 and 13 characters (the whole serial). Set 2: tails of 32 characters.
    const sets = [
      ["トラッカー5（…0001）", "トラッカー5（…A0001）", "トラッカー（…A0001）", "トラッカー5（…00XX0000A0001）"],
      [X.label({ hub: 1, tail: SA }), X.label({ tail: SB }), X.label({ hub: 99, tail: SW }), X.label({ tail: SW })],
    ];
    const mores = [0, 99];
    const waits = [60, 90, 3599, 5400, 86399];
    const rules = [["R1", "Raised"], ["R1", "Escalated"], ["R1", "Resolved"], ["R2", "Raised"], ["R3", "Raised"], ["R4", "Raised"], ["R5", "Raised"]];
    const HEAD = `700 64px ${fam}`, ACT = `400 54px ${fam}`, PW = 1044;
    const height = (h, a) => 52 + 80 * h + (a ? 12 + 73 * a : 0) + 56;
    const vr = sets.map((labels) => rules.map(([r, st]) => {
      const hosts = [...labels, X.label({ host: true })];
      let worst = null;
      for (const T of labels) for (const H of hosts) for (const more of mores) for (const wait of (r === "R5" ? waits : [60])) {
        const n = X.notice(r, st, T, H, { more, wait });
        const hl = wrap(n.headline, HEAD, PW), al = n.action ? wrap(n.action, ACT, PW) : [];
        // Every line within the width (no character is drawn outside the panel).
        const over = [...hl.map((l) => measure(HEAD, l)), ...al.map((l) => measure(ACT, l))].some((w) => w > PW);
        const c = { rule: `${r} ${st}`, h: hl.length, a: al.length, px: height(hl.length, al.length), over, ex: [...hl, "/", ...al].join(" ") };
        if (!worst || c.px > worst.px || c.over) worst = c;
      }
      return worst;
    }));
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
  for (const [i, set] of out.vr.entries()) {
    console.log("");
    console.log(i === 0 ? "末尾が 4〜13 文字" : "末尾が 32 文字");
    console.log("| 規則と段階 | 見出しの行数 | することの行数 | オーバーレイの高さ | 最も高くなる例（「/」の前が見出し） |");
    console.log("|---|---|---|---|---|");
    for (const r of set) console.log(`| ${r.rule} | ${r.h} | ${r.a || "—"} | ${r.px} | ${r.ex} |`);
  }
  // Where the top edge of the tallest overlay is (5.2): 0.64 m wide, centre 0.28 m down and 1.5 m ahead, tilted 10.6 degrees.
  const tallest = Math.max(...out.vr.flat().map((r) => r.px));
  const hm = (0.64 * tallest) / 1280, t = (10.6 * Math.PI) / 180;
  const deg = (Math.atan((0.28 - (hm / 2) * Math.cos(t)) / (1.5 + (hm / 2) * Math.sin(t))) * 180) / Math.PI;
  console.log(`\n最も高いオーバーレイ: 1280 × ${tallest}（0.64 m × ${hm.toFixed(2)} m）。上の端は視線の ${deg.toFixed(1)}° 下。RGBA ${(1280 * tallest * 4 / 1e6).toFixed(1)} MB`);
  const bad = out.rows.filter((r) => r.need > r.have).map((r) => r.what)
    .concat(out.vr.flat().filter((r) => r.over).map((r) => "VR " + r.rule));
  console.error(bad.length ? "NG: " + bad.join(", ") : "all fit");
  await browser.close();
})();
