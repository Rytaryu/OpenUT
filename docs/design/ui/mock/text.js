// Wording of the 4th version (docs/design/ui/m1.md, section 6), for the mockups and for mock/fit.js.
// "|" marks a place where a line may break, "\n" a new line after "。" (section 4.4, wrap.js).
// The app keeps the same strings in openut_core::text.

(function () {
  "use strict";

  // Tracker label (design 3.2): "トラッカー{N}（…{tail}）", "トラッカー（…{tail}）", "ホストのトラッカー", "トラッカー".
  function label(d) {
    if (!d.tail) return d.host ? "ホストのトラッカー" : "トラッカー";
    return (d.hub ? `トラッカー${d.hub}` : "トラッカー") + `（…${d.tail}）`;
  }

  // R5: the wait in seconds, written without rounding (6.3).
  function span(s) {
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
    return (h ? `${h}時間` : "") + (m ? `${m}分` : "") + (x || !s ? `${x}秒` : "");
  }

  // Notices (design 7.1). T, H: labels. more: n of "（ほか{n}件）" (0 = none).
  function notice(rule, stage, T, H, opts) {
    const o = opts || {};
    const more = o.more ? `|（ほか${o.more}件）` : "";
    // A label may break before its "（" (4.4), so a long tail does not break inside the brackets.
    T = (T || "").replace("（", "|（");
    H = (H || "").replace("（", "|（");
    const R1A = `${H}の|電源だけを|入れ直してください。\nほかの|トラッカーと|ドングルは、|そのままに|してください`;
    const t = {
      "R1 Raised": [`トラッカーの|追跡が|始まりません${more}`, R1A],
      "R1 Escalated": [`まだ|追跡が|始まりません${more}`, R1A],
      "R1 Resolved": [`追跡が|始まらない|問題が|直りました${more}`, null],
      "R2 Raised": [`${T}の|準備に|時間が|かかっています${more}`, `${H}の|近くに|置いて、|待ってください`],
      "R3 Raised": [`${T}の|追跡が|始まりません${more}`, `手に持ち、|カメラに|壁や家具が|映るように、|ゆっくり|左右に|向きを|変えてください`],
      "R4 Raised": [`${T}の|接続が|切れました${more}`, `電源が|入っているか|確かめてください`],
      // Only in the list of the main window's status band, for 30 seconds: not pushed (design 7.1, m1.md 3.3).
      "R4 Resolved": [`${T}が|つながりました${more}`, null],
      "R5 Raised": [`${T}の|追跡が|${span(o.wait || 60)}以上|止まっています${more}`, `カメラの前を|あけて、|明るい方へ|向けてください`],
    }[`${rule} ${stage}`];
    return { headline: t[0], action: t[1] };
  }

  // Guide steps (design 7.4): the step name (guide frame) and the line of the status band.
  const guide = {
    G1: { step: "VIVE Hubを|起動する", line: "VIVE Hubを|起動してください" },
    G2: { step: "ドングルを|つなぐ", line: "ドングルを|PCに|つないでください" },
    G3: { step: "電源を|入れる", line: "トラッカーの|電源を|入れてください" },
    G4: {
      step: "追跡を|待つ",
      line: "追跡が|始まるまで|待ってください。\n始まらないときは、|トラッカーを|手に持ち、|カメラに|壁や家具が|映るように、|ゆっくり|左右に|向きを|変えてください",
    },
    G5: { step: "身につける", line: "トラッカーを|身につけてください" },
  };

  // Problems on the side of the app (design 7.4): the line only.
  const health = {
    Unreadable: "OpenUTを|起動し直してください",
    Stalled: "VIVE Hubを|起動し直してください",
    FormatMismatch: "OpenUTを|新しい|バージョンに|してください",
  };

  const words = {
    overall: { ok: "正常", wait: "待機中", fault: "異常" },
    state: { ok: "正常", wait: "待機中", fault: "異常", none: "未接続" },
    link: { dongle: "接続済み", usb: "USB接続", none: "未接続" },
    dongle: { on: "接続済み", off: "未接続", unknown: "—" },
    unknown: "—",
    tracker: "トラッカー",
    dongleName: "ドングル",
    serial: "シリアル番号",
    firmware: "ファームウェア",
    prepare: "準備",
    ops: { pair: "ペアリング", map: "セットアップ", cal: "キャリブレーション" },
    stop: "やめる",
    pickFolder: "フォルダーを選ぶ",
    notify: "通知",
    notifyVr: "VRの中",
    notifyDesktop: "デスクトップ",
    trayOpen: "状態を開く",
    trayQuit: "終了",
  };

  const plain = (s) => (s || "").replace(/\|/g, "").replace(/\n/g, "");

  window.OpenUTText = { label, span, notice, guide, health, words, plain };
})();
