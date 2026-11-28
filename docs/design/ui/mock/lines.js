// Usage: node docs/design/ui/mock/lines.js   (prints the tables in m1.md, sections 3.3 and 4.5). Needs Playwright.
// It breaks the notice texts with wrap.js (section 4.4) in the font of the mockups, for the widest names
// allowed by the display-name rule (6.4), and checks the widths in the status window at its minimum size.
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
    // Display names (6.4) of the examples in m1.md: each is at most NAME_MAX_W (112 px at 14 px Bold) wide.
    // "ホストのトラッカー" is the fixed word for an unknown host (not a display name).
    const sets = [
      [{ n: 1, name: "腰のトラッカー（ベルトの後ろ）" }, { n: 2, name: "腰のトラッカー（ベルトの前）" }, { n: 3, name: "トラッカー1" },
       { n: 4, name: "\u2E3A".repeat(8) }, { n: 5 }],
      [{ n: 1, name: "足" }, { n: 2, name: "足" }, { n: 3, name: "足（2）" }],
      [{ n: 1, name: "左足のトラッカー" }, { n: 2, name: "WWWWWWWW" }, { n: 3, name: "ＷＷＷＷＷＷＷＷＷ" }, { n: 12, name: "トラッカー2" }, { n: 2 }],
    ];
    const names = [...new Set(sets.flatMap((s) => window.OpenUTNames.displayNames(s)))];
    const widest = Math.max(...names.map(window.OpenUTNames.widthOf));
    const hosts = [...names, "ホストのトラッカー"];
    const more = ["", "|（ほか99件）"];
    const waits = ["1分", "1分30秒", "59分59秒", "1時間30分", "23時間59分59秒"];
    // Phrases as in m1.md 6.1 ("|" = a place where a line may break, "\n" = a new line after "。").
    const R1A = "{H}の|電源だけを|入れ直してください。\nほかの|トラッカーと|ドングルは|そのままにしてください。";
    const rules = [
      ["R1 Raised", "トラッカーの|追跡が|始まりません{M}", R1A],
      ["R1 Escalated", "トラッカーの|追跡が|まだ|始まりません{M}", R1A],
      ["R1 Resolved", "追跡が|始まらない|問題が|直りました{M}", null],
      ["R2 Raised", "{T}が|地図を|受け取れていません{M}", "{H}の|近くに|置いて待ってください"],
      ["R3 Raised", "{T}の|位置合わせが|終わりません{M}", "手に持って、|部屋の中を|見回すように|ゆっくり動かしてください"],
      ["R4 Raised", "{T}の|接続が|切れました{M}", "電源が|入っているか|確かめてください"],
      ["R5 Raised", "{T}の|追跡が|{W}以上|途切れています{M}", "カメラの前の|物を|どけて、|明るい方へ向けてください"],
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
    // Status window at the minimum size 920: content 920 - 2 * 24 = 872, the label column 112 + 16,
    // five tiles with gaps of 12: tile = (872 - 128 - 48) / 5 = 139.2, inner = tile - 2 * 12.
    const tile = (872 - 128 - 48) / 5, inner = tile - 24;
    const B14 = (s) => measure(`700 14px ${family}`, s), R14 = (s) => measure(`400 14px ${family}`, s);
    const B13 = (s) => measure(`700 13px ${family}`, s), R13 = (s) => measure(`400 13px ${family}`, s);
    const B16 = (s) => measure(`700 16px ${family}`, s);
    const labels = ["追跡中", "位置合わせ中", "地図を同期中", "Wi-Fi待ち", "接続済み", "未接続", "追跡できた割合", "追跡が途切れた"];
    const win = [
      ["タイルの名前（14 Bold。6.4 の例で最も広いもの）", widest, inner],
      ["名前の入力欄の中（表示名と同じ幅の字が 14 Regular で入るか。広い名前は欄の中で横に送る）", Math.max(...names.map(R14)), tile - 12 - 12],
      ["ホストのタグ（13 Bold ＋ 内側 6 × 2 ＋ 枠 2）", B13("ホスト") + 14, inner],
      ["段階の枠の経過時間（13 Bold ＋ 内側 8 × 2）", Math.max(...["99時間59分", "59分", "59秒"].map(B13)) + 16, inner],
      ["今日の値（14 Regular ＋ 内側 8）", Math.max(...["100%", "999回"].map(R14)) + 8, inner],
      ["左の列の段階と集計の名前（13 Regular）", Math.max(...labels.map(R13)), 112],
      ["上の行（16 Bold の最も長い状態 ＋ マーク 30 ＋ 24 ＋ ホストのWi-Fi情報）",
        30 + B16("このフォルダーにはログがありません") + 24 + R13("ホストのWi-Fi情報") + 8 + B14("なし"), 872],
      ["下の行（不具合の数 2 つ ＋ 通知の切り替え）",
        R13("追跡が始まらない") + 8 + B14("999回") + 24 + R13("予期しない切断") + 8 + B14("999回") + 24 +
        R13("通知") + 20 + (16 + 8 + R14("VR")) + 20 + (16 + 8 + R14("デスクトップ")), 872],
    ].map(([what, need, have]) => ({ what, need: Math.ceil(need), have: Math.floor(have) }));
    return { vr, win, names: names.map((n) => `${n} ${window.OpenUTNames.widthOf(n).toFixed(1)}`) };
  });
  console.log("| 規則と段階 | 見出しの行数 | することの行数 | 板の高さ | 最も高くなる例 |");
  console.log("|---|---|---|---|---|");
  for (const r of out.vr) console.log(`| ${r.rule} | ${r.h} | ${r.a || "—"} | ${r.px} | ${r.example} |`);
  console.log("");
  console.log("| 最小の大きさ 920 で確かめたこと | 要る幅 | ある幅 | 結果 |");
  console.log("|---|---|---|---|");
  for (const w of out.win) console.log(`| ${w.what} | ${w.need} | ${w.have} | ${w.need <= w.have ? "OK" : "NG"} |`);
  console.log("");
  console.log("表示名の例と幅（14 px Bold）: " + out.names.join("、"));
  await browser.close();
})();
