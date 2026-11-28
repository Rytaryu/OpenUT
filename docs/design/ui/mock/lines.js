// Usage: node docs/design/ui/mock/lines.js   (prints the tables in m1.md, sections 3.3 and 4.5). Needs Playwright.
// It breaks the notice texts with wrap.js (section 4.4) in the font of the mockups, for the widest names
// allowed by the display-name rule (6.4), and checks the widths of the status window at its minimum width.
const { chromium } = require("playwright");
const path = require("path");

(async () => {
  const browser = await chromium.launch();
  const page = await browser.newPage();
  await page.goto("file://" + path.resolve(__dirname, "vr.html"));
  await page.evaluate(() => document.fonts.ready);
  const out = await page.evaluate(() => {
    const family = getComputedStyle(document.documentElement).getPropertyValue("--font").trim();
    const { wrap, measure } = window.OpenUTWrap;
    const HEAD = `700 64px ${family}`, ACT = `400 54px ${family}`, W = 1044;
    // Widest names after the display-name rule: 8 characters, or 7 + "…". "ホストのトラッカー" is the fixed word for an unknown host.
    const names = ["左足のトラッカー", "WWWWWWWW", "ＷＷＷＷＷＷＷ…", "腰のトラッカー…"];
    const hosts = [...names, "ホストのトラッカー"];
    const more = ["", "|（ほか99件）"];
    const waits = ["1分", "1分30秒", "59分59秒", "1時間30分", "23時間59分59秒"];
    const R1A = "{H}の|電源だけを|入れ直す。\nほかの|トラッカーと|ドングルは|そのまま";
    const rules = [
      ["R1 Raised", "トラッカーが|始まりません{M}", R1A],
      ["R1 Escalated", "トラッカーが|まだ|始まりません{M}", R1A],
      ["R1 Resolved", "始まらない|状態が|直りました{M}", null],
      ["R2 Raised", "{T}が|地図を|受け取れていません{M}", "{H}の|近くに|置いて|待つ"],
      ["R3 Raised", "{T}の|位置合わせが|終わりません{M}", "手に持って、|部屋の中を|ゆっくり|見せる"],
      ["R4 Raised", "{T}の|接続が|切れました{M}", "電源が|入っているか|確かめる"],
      ["R5 Raised", "{T}が|{W}以上、|位置を|失っています{M}", "カメラの前を|あけ、|明るい方へ|向ける"],
    ];
    const height = (h, a) => 52 + 80 * h + (a ? 12 + 73 * a : 0) + 56;
    const vr = [];
    for (const [rule, h, a] of rules) {
      let worst = null;
      for (const T of names) for (const H of hosts) for (const M of more) for (const Wt of (h.includes("{W}") ? waits : [""])) {
        const fill = (s) => s.replaceAll("{T}", T).replaceAll("{H}", H).replaceAll("{M}", M).replaceAll("{W}", Wt);
        const hl = wrap(fill(h), HEAD, W);
        const al = a ? wrap(fill(a), ACT, W) : [];
        const c = { rule, h: hl.length, a: al.length, px: height(hl.length, al.length), example: [...hl, "/", ...al].join(" ") };
        if (!worst || c.px > worst.px) worst = c;
      }
      vr.push(worst);
    }
    // Status window at the minimum width 360: content 328, text column 328 - 57 - 12 = 259.
    const N14B = (s) => measure(`700 14px ${family}`, s), N14 = (s) => measure(`400 14px ${family}`, s);
    const tag = measure(`700 13px ${family}`, "ホスト") + 2 * 5 + 2;
    const widest = Math.max(...names.map(N14B)), widestRegular = Math.max(...names.map(N14));
    const win = [
      ["名前の行（名前 ＋ 8 ＋ ホストの印）", widest + 8 + tag + 12, 259],
      ["名前の行（入力欄 136 ＋ 8 ＋ ホストの印）", 136 + 8 + tag, 259],
      ["入力欄の中（8 文字 ＋ 内側 12 ＋ 枠 2）", widestRegular + 12 + 2, 136],
      ["状態の行（位置取り直し中 ＋ 12 ＋ 99時間59分）", N14("位置取り直し中") + 12 + N14("99時間59分"), 259],
      ["今日の名前の列", widestRegular, 328 - 112 - 64 - 24],
    ].map(([what, need, have]) => ({ what, need: Math.ceil(need), have }));
    return { vr, win };
  });
  console.log("| 規則と段階 | 見出しの行数 | することの行数 | 板の高さ | 最も高くなる例 |");
  console.log("|---|---|---|---|---|");
  for (const r of out.vr) console.log(`| ${r.rule} | ${r.h} | ${r.a || "—"} | ${r.px} | ${r.example} |`);
  console.log("");
  console.log("| 最小幅 360 で確かめたこと | 要る幅 | ある幅 | 結果 |");
  console.log("|---|---|---|---|");
  for (const w of out.win) console.log(`| ${w.what} | ${w.need} | ${w.have} | ${w.need <= w.have ? "OK" : "NG"} |`);
  await browser.close();
})();
