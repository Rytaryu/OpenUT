// Scenes of the main window, written in the shape of the display model (design 7.4, DisplayModel).
// No guide (PRD W-10, D-46): `guide` is not used, and the line of the status panel is a notice,
// a problem on the side of the app, or "VIVE Hubを起動してください" (m1.md 2.3).
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

  const t1 = dev({ serial: S(1), tail: "0001", hub: 2, battery: 82, state: "ok", host: true });
  const t2 = dev({ serial: S(2), tail: "0002", hub: 1, battery: 64, state: "wait" });
  const t3 = dev({ serial: S(3), tail: "0003", link: "none" });

  const scenes = {};

  scenes.a = {
    title: "待機中（同期中の台がある）",
    overall: "wait", line: null, faults: [], dongle: "on",
    devices: [t1, t2, t3],
  };

  {
    const h = dev({ serial: S(1), tail: "0001", hub: 2, battery: 82, state: "fault", host: true });
    const n = X.notice("R1", "Raised", "", lab(h));
    scenes.b = {
      title: "異常（トラッキングが始まらない。★の台の電源を入れ直す）",
      overall: "fault", line: { fault: n }, faults: [n], dongle: "on",
      devices: [h, dev({ serial: S(2), tail: "0002", hub: 1, battery: 64, state: "wait" }),
                dev({ serial: S(3), tail: "0003", hub: 3, battery: 71, state: "wait" })],
    };
  }

  scenes.c = {
    title: "正常（5 台。1 台は USB で充電中）",
    overall: "ok", line: null, faults: [], dongle: "on",
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
    overall: "wait", line: { hub: true }, faults: [], dongle: "unknown",
    devices: [1, 2, 3].map((n) => dev({ serial: S(n), tail: `000${n}`, link: "none" })),
  };

  scenes.e = {
    title: "VIVE Hub のログが見つからない（フォルダーを選ぶ）",
    overall: "wait", line: { hub: true }, faults: [], dongle: "unknown",
    devices: [], pickFolder: true,
  };

  scenes.f = {
    title: "灰色（最後に読めた値。VIVE Hub を起動し直す）",
    overall: "wait", line: { health: "Stalled" }, faults: [], dongle: "on",
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
      title: "異常が 2 つと、つながった台（案内の一覧）",
      overall: "fault", line: { fault: r4 }, faults: [ok, r2, r4], dongle: "on",
      devices: [a, b, c, d],
    };
  }

  scenes.h = {
    title: "行のメニュー（仮。中身は #76）",
    overall: "ok", line: null, faults: [], dongle: "on",
    devices: scenes.c.devices, menuOpen: 2,
  };

  {
    // Minimum size, six trackers: five connected (tails of 5 characters, because two serials share "0001")
    // and one that is not. The sixth card is reached by scrolling.
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
      overall: "fault", line: { fault: n }, faults: [n], dongle: "on",
      devices: [a, b, c, d, e, f], size: [1000, 600],
    };
  }

  {
    // Minimum size, the longest tails (design 3.2, 4.4): two made-up serials of 32 characters that share their
    // last 31, so every tail is 32 characters (the tail has one length for every tracker; a shorter serial shows whole).
    // One is the host with R1, the other is not connected (no VIVE Hub number) with R4.
    const R = "7KWM3X9QH2VT8ND4ZR6BY0PC5FJ0001";
    const a = dev({ serial: "00XX0000A0003", tail: "00XX0000A0003", hub: 2, battery: 71, state: "ok" });
    const b = dev({ serial: "A" + R, tail: "A" + R, hub: 1, battery: 58, state: "fault", host: true });
    const c = dev({ serial: "B" + R, tail: "B" + R, link: "none", state: "fault" });
    const r1 = X.notice("R1", "Raised", "", lab(b));
    const r4 = X.notice("R4", "Raised", lab(c), "");
    scenes.j = {
      title: "最小の大きさ、末尾が 32 文字の 2 台",
      overall: "fault", line: { fault: r1 }, faults: [r4, r1], dongle: "on",
      devices: [a, b, c], size: [1000, 600],
    };
  }

  {
    // Minimum size, a serial of 32 characters with a short tail (4) and a firmware version of 32 characters
    // (digits and periods; design 4.4 takes up to 32). Both made up.
    const a = dev({ serial: "00XX0000A0001", tail: "0001", hub: 1, battery: 90, state: "ok", host: true });
    const b = dev({ serial: "Q2HN8XW4TB6RM0ZK3JD7VC5PL9FY1234", tail: "1234", hub: 3, battery: 64, state: "wait",
                    fw: "1234567.1234567.1234567.12345678" });
    const c = dev({ serial: "00XX0000A0002", tail: "0002", hub: 2, battery: 77, state: "ok" });
    scenes.k = {
      title: "最小の大きさ、32 文字のシリアル番号（末尾 4 文字）とバージョン",
      overall: "wait", line: null, faults: [], dongle: "on",
      devices: [a, c, b], size: [1000, 600],
    };
  }

  window.OpenUTScenes = scenes;
  // The two made-up serials of scene j, for vr.html and fit.js.
  window.OpenUTLong32 = ["A7KWM3X9QH2VT8ND4ZR6BY0PC5FJ0001", "B7KWM3X9QH2VT8ND4ZR6BY0PC5FJ0001"];
})();
