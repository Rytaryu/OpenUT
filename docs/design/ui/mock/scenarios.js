// Scenes of the main window, written in the shape of the display model (design 7.4, DisplayModel).
// The serial numbers are made-up examples in the shape of the design's examples (00XX0000A0001):
// no real serial number is in this repository.

(function () {
  "use strict";
  const X = window.OpenUTText;

  // A device row: { serial, tail, hub, link: "dongle"|"usb"|"none", battery: % | null, charging,
  //                 state: "ok"|"wait"|"fault"|null, fw, host }
  const dev = (o) => Object.assign({ link: "dongle", battery: null, charging: false, state: null, host: false, fw: "1.0.999.178" }, o);
  const S = (n) => `00XX0000A000${n}`;
  const lab = (d) => X.label(d);

  // Guide steps: done flags and the current one (design 7.4, "いまの段").
  const steps = (current, progress) =>
    ["G1", "G2", "G3", "G4", "G5"].map((id, i) => ({
      id, done: i < ["G1", "G2", "G3", "G4", "G5"].indexOf(current), current: id === current,
      progress: id === current ? progress || null : null,
    }));

  const t1 = dev({ serial: S(1), tail: "0001", hub: 2, battery: 82, state: "ok", host: true });
  const t2 = dev({ serial: S(2), tail: "0002", hub: 1, battery: 64, state: "wait" });
  const t3 = dev({ serial: S(3), tail: "0003", link: "none" });

  const scenes = {};

  scenes.a = {
    title: "待機中（追跡を待っている台がある）",
    overall: "wait", line: { guide: "G4" }, faults: [], guide: steps("G4", [1, 2]), dongle: "on",
    devices: [t1, t2, t3],
  };

  {
    const h = dev({ serial: S(1), tail: "0001", hub: 2, battery: 82, state: "fault", host: true });
    const n = X.notice("R1", "Raised", "", lab(h));
    scenes.b = {
      title: "異常（追跡が始まらない。ホストの電源を入れ直す）",
      overall: "fault", line: { fault: n }, faults: [n], guide: steps("G4", [0, 3]), dongle: "on",
      devices: [h, dev({ serial: S(2), tail: "0002", hub: 1, battery: 64, state: "wait" }),
                dev({ serial: S(3), tail: "0003", hub: 3, battery: 71, state: "wait" })],
    };
  }

  scenes.c = {
    title: "正常（5 台。1 台は USB で充電中）",
    overall: "ok", line: { guide: "G5" }, faults: [], guide: steps("G5"), dongle: "on",
    devices: [
      dev({ serial: S(1), tail: "0001", hub: 3, battery: 82, state: "ok" }),
      dev({ serial: S(2), tail: "0002", hub: 1, battery: 64, state: "ok", host: true }),
      dev({ serial: S(3), tail: "0003", hub: 2, battery: 100, state: "ok", link: "usb", charging: true }),
      dev({ serial: S(4), tail: "0004", hub: 5, battery: 47, state: "ok" }),
      dev({ serial: S(5), tail: "0005", hub: 4, battery: 9, state: "ok" }),
    ],
  };

  scenes.d = {
    title: "VIVE Hub が止まっている",
    overall: "wait", line: { guide: "G1" }, faults: [], guide: steps("G1"), dongle: "unknown",
    devices: [1, 2, 3].map((n) => dev({ serial: S(n), tail: `000${n}`, link: "none" })),
  };

  scenes.e = {
    title: "VIVE Hub のログが見つからない（フォルダーを選ぶ）",
    overall: "wait", line: { guide: "G1" }, faults: [], guide: steps("G1"), dongle: "unknown",
    devices: [], pickFolder: true,
  };

  scenes.f = {
    title: "灰色（最後に読めた値。VIVE Hub を起動し直す）",
    overall: "wait", line: { health: "Stalled" }, faults: [], guide: steps("G4", [1, 2]), dongle: "on",
    devices: [t1, t2, t3], greyed: true,
  };

  {
    const a = dev({ serial: S(1), tail: "0001", hub: 2, battery: 82, state: "ok", host: true });
    const b = dev({ serial: S(2), tail: "0002", hub: 1, battery: 64, state: "fault" });
    const c = dev({ serial: S(3), tail: "0003", link: "none", state: "fault" });
    const d = dev({ serial: S(4), tail: "0004", hub: 3, battery: 55, state: "ok" });
    const r4 = X.notice("R4", "Raised", lab(c), "");
    const r2 = X.notice("R2", "Raised", lab(b), lab(a));
    const ok = Object.assign(X.notice("R4", "Resolved", lab(d), ""), { resolved: true });
    scenes.g = {
      title: "異常が 2 つと、つながった台（欄に並ぶ）",
      overall: "fault", line: { fault: r4 }, faults: [ok, r2, r4], guide: steps("G4", [2, 3]), dongle: "on",
      devices: [a, b, c, d],
    };
  }

  scenes.h = {
    title: "操作の手引き（キャリブレーションの例。手順の中身は仮）",
    overall: "ok", line: { guide: "G5" }, faults: [], guide: steps("G5"), dongle: "on",
    devices: scenes.c.devices,
    op: {
      kind: "cal",
      steps: ["トラッカーと|コントローラーを|重ねて持つ", "8の字を描いて|手首を回す", "終わり"],
      current: 1, progress: 0.6,
    },
  };

  {
    // Minimum size, six trackers: five connected (tails of 5 characters, because two serials share "0001")
    // and one that is not. The sixth tile is reached by scrolling.
    const T = (s, tail, o) => dev(Object.assign({ serial: s, tail }, o));
    const a = T("00XX0000A0001", "A0001", { hub: 4, battery: 100, state: "ok", link: "usb", charging: true });
    const b = T("00XX0000A0002", "A0002", { hub: 2, battery: 58, state: "fault", host: true });
    const c = T("00XX0000A0003", "A0003", { hub: 1, battery: null, state: "wait" });
    const d = T("00XX0000B0001", "B0001", { hub: 5, battery: 33, state: "wait" });
    const e = T("00XX0000B0002", "B0002", { hub: 3, battery: 76, state: "wait" });
    const f = T("00XX0000B0003", "B0003", { link: "none", fw: null });
    const n = X.notice("R1", "Escalated", "", lab(b));
    scenes.i = {
      title: "最小の大きさ（1000 × 600）、6 台",
      overall: "fault", line: { fault: n }, faults: [n], guide: steps("G4", [1, 5]), dongle: "on",
      devices: [a, b, c, d, e, f], size: [1000, 600],
    };
  }

  window.OpenUTScenes = scenes;
})();
