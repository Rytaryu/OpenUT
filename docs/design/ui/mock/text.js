// Wording of the 5th version (docs/design/ui/m1.md, chapter 3), for the mockups and for mock/fit.js.
// "|" marks a place where a line may break, "\n" a new line after "。" (section 5.4, wrap.js).
// The app keeps the same strings in openut_core::text.

(function () {
  "use strict";

  // Tracker label (design 3.2), written without the word "ホスト" (PM, 2026-11-28): "トラッカー{N}（…{tail}）",
  // "トラッカー（…{tail}）", and, when neither the number nor the tail is known, "★マークのトラッカー" for the host
  // (VIVE Hub marks the host with a star) or "トラッカー".
  function label(d) {
    if (!d.tail) return d.host ? "★マークのトラッカー" : "トラッカー";
    return (d.hub ? `トラッカー${d.hub}` : "トラッカー") + `（…${d.tail}）`;
  }

  // R5: the wait in seconds, written without rounding (3.3).
  function span(s) {
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
    return (h ? `${h}時間` : "") + (m ? `${m}分` : "") + (x || !s ? `${x}秒` : "");
  }

  // Notices (design 7.1). T, H: labels. more: n of "（ほか{n}件）" (0 = none).
  function notice(rule, stage, T, H, opts) {
    const o = opts || {};
    const more = o.more ? `|（ほか${o.more}件）` : "";
    // A label may break before its "（" (5.4), so a long tail does not break inside the brackets.
    T = (T || "").replace("（", "|（");
    H = (H || "").replace("（", "|（");
    const R1A = `${H}の|電源だけを|入れ直してください。\nほかの|トラッカーと|ドングルは、|そのままに|してください`;
    const t = {
      "R1 Raised": [`トラッキングが|始まりません${more}`, R1A],
      "R1 Escalated": [`まだ|トラッキングが|始まりません${more}`, R1A],
      "R1 Resolved": [`トラッキングが|始まらない|問題が|直りました${more}`, null],
      "R2 Raised": [`${T}の|準備に|時間が|かかっています${more}`, `${H}の|近くに|置いて、|待ってください`],
      "R3 Raised": [`${T}の|トラッキングが|始まりません${more}`, `手に持ち、|カメラに|壁や家具が|映るように、|ゆっくり|左右に|向きを|変えてください`],
      "R4 Raised": [`${T}の|接続が|切れました${more}`, `電源が|入っているか|確かめてください`],
      // Only in the list of the main window's status bar, for 30 seconds: not pushed (design 7.1, m1.md 2.3).
      "R4 Resolved": [`${T}が|つながりました${more}`, null],
      "R5 Raised": [`${T}の|トラッキングが|${span(o.wait || 60)}以上|止まっています${more}`, `カメラの前を|あけて、|明るい方へ|向けてください`],
    }[`${rule} ${stage}`];
    return { headline: t[0], action: t[1] };
  }

  // The one line of the status bar when VIVE Hub is not running or its logs are not found (design 7.4, G1).
  const hubLine = "VIVE Hubを|起動してください";

  // Problems on the side of the app (design 7.4): the line only.
  const health = {
    Unreadable: "OpenUTを|起動し直してください",
    Stalled: "VIVE Hubを|起動し直してください",
    FormatMismatch: "OpenUTを|新しい|バージョンに|してください",
  };

  // The word on a tracker card (2.4): one word for the connection and the state together, in the words of
  // VIVE Hub where known (provisional, to be checked by the owner: m1.md 8.1).
  function cardWord(d) {
    if (d.link === "none") return "未接続";
    if (d.state === "fault") return "異常";
    if (d.state === "wait") return "同期中";
    return d.link === "usb" ? "USB接続" : "接続済み";
  }

  const words = {
    overall: { ok: "正常", wait: "待機中", fault: "異常" },
    dongle: { on: "接続済み", off: "未接続", unknown: "—" },
    unknown: "—",
    tracker: "トラッカー",
    dongleName: "ドングル",
    serial: "シリアル番号",
    firmware: "ファームウェア",
    ops: { pair: "ペアリング", map: "セットアップ" },
    menu: { powerOff: "電源を切る", unpair: "ペアリングを解除" },
    menuLabel: "トラッカーの操作",
    settings: "設定",
    pickFolder: "フォルダーを選ぶ",
    notify: "通知",
    notifyVr: "VR内",
    notifyDesktop: "デスクトップ",
    trayOpen: "OpenUTを開く",
    trayQuit: "終了",
  };

  const plain = (s) => (s || "").replace(/\|/g, "").replace(/\n/g, "");

  window.OpenUTText = { label, span, notice, hubLine, health, cardWord, words, plain };
})();
