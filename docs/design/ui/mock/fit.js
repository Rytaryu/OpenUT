// Usage: node docs/design/ui/mock/fit.js   (prints the tables in m1.md, 2.7 and 5.5). Needs Playwright.
// Measures, in the font of the mockups, whether the words fit the main window at its minimum size
// and how many lines the VR notice overlay needs for the longest labels (wrap.js, 5.4).
// The longest tails and firmware versions are 32 characters (design 3.2, 4.4): the two made-up serials of window.html?s=j,
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
    // Main window (2.7). At the minimum width 1000 the content is 960 wide; the table's inner width is 928.
    // Columns: PCトラッカー (what is left), 状態 128, バッテリー 120, ファームウェア 168, アクション 96 (2.5).
    const MIN_W = 1000, DEF_W = 1120;
    const whoText = (w) => w - 40 - 32 - 128 - 120 - 168 - 96 - 45 - 14;
    const lineW = (w, pick) => w - 40 - 168 - 184 - 2 - 40 - (pick ? 128 + 20 : 0);
    const rows = [];
    const add = (what, need, have) => rows.push({ what, need: Math.ceil(need), have });
    const widest = (w, px, list) => Math.max(...list.map((t) => m(w, px, t)));
    const cardWords = ["未接続", "同期中", "接続済み", "USB接続", "異常"];
    add("名前（★ 14 ＋ 4 ＋ 16 Bold「トラッカー16」）", 14 + 4 + m(700, 16, "トラッカー16"), whoText(MIN_W));
    add("シリアル番号（13。13 文字、末尾 5 文字が Bold）", m(400, 13, "00XX0000") + m(700, 13, "A0001"), whoText(MIN_W));
    add("状態（印 12 ＋ 8 ＋ 14 Bold。最も広い語。列 128）", 12 + 8 + widest(700, 14, cardWords), 128 - 16);
    add("バッテリー（電池 26 ＋ 6 ＋ 14「100%」＋ 8 ＋ USB 16。列 120）", 26 + 6 + m(400, 14, "100%") + 8 + 16, 120 - 16);
    add("ファームウェア（14「10.10.9999.9999」。列 168）", m(400, 14, "10.10.9999.9999"), 168 - 16);
    add("アクション（ボタン 32 ＋ 4 ＋ 32。列 96）", 32 + 4 + 32, 96);
    for (const [k, w] of [["状態", 128], ["バッテリー", 120], ["ファームウェア", 168], ["アクション", 96]])
      add(`列の見出し「${W.cols[k === "状態" ? "state" : k === "バッテリー" ? "battery" : k === "ファームウェア" ? "firmware" : "actions"]}」（13）`,
        m(400, 13, k === "状態" ? W.cols.state : k === "バッテリー" ? W.cols.battery : k === "ファームウェア" ? W.cols.firmware : W.cols.actions), w - 16);
    add("全体（左 20 ＋ 印 22 ＋ 10 ＋ 22 Bold「待機中」。列 168）", 20 + 22 + 10 + m(700, 22, "待機中"), 168 - 12);
    add("ドングル（20 ＋ 絵 45 ＋ 12 ＋ 印 10 ＋ 6 ＋ 14 Bold「接続済み」＋ 20。列 184）", 20 + 45 + 12 + 10 + 6 + m(700, 14, "接続済み") + 20, 184);
    const opW = (k) => 20 + m(400, 14, W.ops[k].name) + 24 + (16 + m(700, 14, W.ops[k].button) + 16) + 9;
    add("ペアリングの行（20 ＋ 名前 14 ＋ 24 以上 ＋ ボタン ＋ 9。幅は (960 − 52 − 24) ÷ 2）", Math.max(opW("pair"), opW("setup")), Math.floor((MIN_W - 40 - 52 - 24) / 2));
    add("台数とすべてオフにする（14 ＋ 40 以上 ＋ ボタン 13 Bold）", m(400, 14, X.tracking(16, 16)) + 40 + 14 + m(700, 13, W.allOff) + 14, MIN_W - 40);
    add("メニュー（14 の項目 ＋ 内側 12 × 2 ＋ 4 × 2。幅 160）", m(400, 14, W.menu.unpair) + 24 + 8, 160);
    // Lines of the serial number and the firmware version in a row (2.5), and of the status panel's line (2.3).
    const lines = [];
    const L = (what, s, font, w) => lines.push({ what, n: wrap(s, font, w).length, text: wrap(s, font, w).join(" / ") });
    const E = (what, s, font, w) => lines.push({ what, n: wrapFromEnd(s, font, w).length, text: wrapFromEnd(s, font, w).join(" / ") });
    E("シリアル番号 32 文字（13 Bold。すべて末尾。見本 j）", SA, `700 13px ${fam}`, whoText(MIN_W));
    E("シリアル番号 32 文字（13 Bold。すべて末尾。W だけ。最小の大きさ）", SW, `700 13px ${fam}`, whoText(MIN_W));
    E("シリアル番号 32 文字（13 Bold。すべて末尾。W だけ。最初の大きさ）", SW, `700 13px ${fam}`, whoText(DEF_W));
    const fwLines = (v, w) => { const F = `400 14px ${fam}`, out = []; let cur = "";
      for (const tok of v.split(/(?<=\.)/)) { if (measure(F, cur + tok) <= w) { cur += tok; continue; } if (cur) { out.push(cur); cur = ""; }
        for (const ch of tok) { if (cur && measure(F, cur + ch) > w) { out.push(cur); cur = ""; } cur += ch; } }
      out.push(cur); return out; };
    for (const v of ["1234567.1234567.1234567.12345678", "9".repeat(32)]) {
      const f = fwLines(v, 168 - 16);
      lines.push({ what: `ファームウェア 32 文字（14。列 168。${v[7] === "." ? "数字とピリオド" : "数字だけ"}）`, n: f.length, text: f.join(" / ") });
    }
    L("ステータスパネルの 1 行 VIVE Hub（16。「フォルダーを選ぶ」あり）", X.hubLine, `400 16px ${fam}`, lineW(MIN_W, true));
    for (const k of Object.keys(X.health)) L(`ステータスパネルの 1 行 ${k}（16）`, X.health[k], `400 16px ${fam}`, lineW(MIN_W));
    const R1 = X.notice("R1", "Raised", "", "トラッカー5（…00XX0000A0001）");
    L("ステータスパネルの見出し R1（16 Bold。末尾 13 文字の呼び名）", R1.headline, `700 16px ${fam}`, lineW(MIN_W));
    L("ステータスパネルのすること R1（14。末尾 13 文字の呼び名）", R1.action, `400 14px ${fam}`, lineW(MIN_W));
    L("ステータスパネルのすること R1（14。★マークのトラッカー）", X.notice("R1", "Raised", "", X.label({ host: true })).action, `400 14px ${fam}`, lineW(MIN_W));
    for (const [r, T] of [["R2", "トラッカー5（…00XX0000A0001）"], ["R3", "トラッカー5（…00XX0000A0001）"], ["R4", "トラッカー（…00XX0000A0001）"], ["R5", "トラッカー5（…00XX0000A0001）"]]) {
      const n = X.notice(r, "Raised", T, "トラッカー1（…00XX0000A0002）", { more: 99, wait: 86399 });
      lines.push({ what: `ステータスパネル ${r}（見出し 16 Bold ＋ すること 14。末尾 13 文字、ほか 99 件）`,
        n: wrap(n.headline, `700 16px ${fam}`, lineW(MIN_W)).length + wrap(n.action, `400 14px ${fam}`, lineW(MIN_W)).length, text: "" });
    }
    for (const [k, S] of [["見本 j", SA], ["W だけ", SW]]) {
      const r1 = X.notice("R1", "Raised", "", X.label({ hub: 99, tail: S }));
      L(`ステータスパネルのすること R1（14。末尾 32 文字、${k}）`, r1.action, `400 14px ${fam}`, lineW(MIN_W));
      const r4 = X.notice("R4", "Raised", X.label({ tail: S }), "");
      L(`案内の一覧の見出し R4（14 Bold、${MIN_W - 40 - 32 - 16 - 12}。番号のない台、末尾 32 文字、${k}）`, r4.headline, `700 14px ${fam}`, MIN_W - 40 - 32 - 16 - 12);
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
