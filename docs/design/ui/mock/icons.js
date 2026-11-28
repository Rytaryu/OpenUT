// Icons of the OpenUT main window, VR overlays and toast notifications (docs/design/ui/m1.md, section 1.4).
// Every icon is made of circles, rounded rectangles, line segments, arcs and polygons only,
// so the app can draw the same shapes with egui's Painter (and fontdue / tiny loops for VR images)
// without image files. Coordinates are in a box of the given size; numbers are fractions of that box
// unless noted. Colours are CSS variables of tokens.css.

(function () {
  "use strict";

  const svg = (w, h, body, cls) =>
    `<svg class="${cls || ""}" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" aria-hidden="true">${body}</svg>`;

  // Arc path (degrees, 0 = right, clockwise in screen coordinates).
  function arc(cx, cy, r, a0, a1) {
    const p = (a) => [cx + r * Math.cos((a * Math.PI) / 180), cy + r * Math.sin((a * Math.PI) / 180)];
    const [x0, y0] = p(a0), [x1, y1] = p(a1);
    const large = Math.abs(a1 - a0) > 180 ? 1 : 0;
    return `M${x0.toFixed(2)} ${y0.toFixed(2)} A${r} ${r} 0 ${large} 1 ${x1.toFixed(2)} ${y1.toFixed(2)}`;
  }

  // State mark (1.4): ok = filled disc, wait = ring with the left half filled, fault = triangle with "!",
  // none = empty ring (not connected). s = box size in px. Shapes differ, so colour is not the only cue.
  function state(kind, s, color) {
    const c = s / 2, r = s * 0.42, sw = Math.max(1.5, s * 0.12);
    if (kind === "ok") return svg(s, s, `<circle cx="${c}" cy="${c}" r="${r}" fill="${color}"/>`, "g");
    if (kind === "wait")
      return svg(s, s,
        `<circle cx="${c}" cy="${c}" r="${r - sw / 2}" fill="none" stroke="${color}" stroke-width="${sw}"/>` +
        `<path d="M${c} ${c - r} A${r} ${r} 0 0 0 ${c} ${c + r} Z" fill="${color}"/>`, "g");
    if (kind === "fault") {
      // Triangle with rounded corners approximated by a stroke join; the "!" is cut in the on-state colour.
      const t = s * 0.06, top = s * 0.1, bot = s * 0.9, l = s * 0.06, rr = s * 0.94;
      return svg(s, s,
        `<polygon points="${c},${top} ${rr},${bot} ${l},${bot}" fill="${color}" stroke="${color}" stroke-width="${t * 2}" stroke-linejoin="round"/>` +
        `<rect x="${c - s * 0.055}" y="${s * 0.36}" width="${s * 0.11}" height="${s * 0.28}" rx="${s * 0.05}" fill="var(--on-state)"/>` +
        `<circle cx="${c}" cy="${s * 0.75}" r="${s * 0.065}" fill="var(--on-state)"/>`, "g");
    }
    return svg(s, s, `<circle cx="${c}" cy="${c}" r="${r - sw / 2}" fill="none" stroke="${color}" stroke-width="${sw}"/>`, "g");
  }

  // Tracker pictogram (1.4), drawn on a 60 x 48 grid and shown at k times that size (k = 0.75 in a row: 45 x 36).
  // Not a picture of the product and without its logo: the outline seen from the front, a wide body with very round
  // corners, a smaller frame inside it, and one small dot at the middle of each side.
  // look: "on" (connected: a light body, dark details), "grey" (connected, last values: a dimmed body),
  //       "off" (not connected: a body close to the panel, details in the idle grey).
  // The state is not drawn on the pictogram; it is the mark and the word of the state column.
  function tracker(look, k) {
    const s = k || 1;
    const on = look !== "off";
    const body = look === "on" ? "var(--ink)" : look === "grey" ? "var(--muted)" : "var(--dark-body)";
    const detail = on ? "var(--panel)" : "var(--idle)";
    return `<svg class="pict" width="${60 * s}" height="${48 * s}" viewBox="0 0 60 48" aria-hidden="true">` +
      `<rect x="6" y="4" width="48" height="40" rx="15" fill="${body}"` + (on ? "" : ` stroke="var(--idle)" stroke-width="1.5"`) + `/>` +
      `<rect x="15" y="12.5" width="30" height="23" rx="8" fill="none" stroke="${detail}" stroke-width="2"/>` +
      `<circle cx="10.5" cy="24" r="1.8" fill="${detail}"/><circle cx="49.5" cy="24" r="1.8" fill="${detail}"/>` +
      `</svg>`;
  }

  // Host mark, a five-pointed star in a box of s (the host tracker, like VIVE Hub; no word "ホスト").
  function star(s, color) {
    const c = s / 2, R = s * 0.48, r = R * 0.47, pts = [];
    for (let i = 0; i < 10; i++) {
      const a = (-90 + i * 36) * Math.PI / 180, q = i % 2 ? r : R;
      pts.push(`${(c + q * Math.cos(a)).toFixed(2)},${(c + 0.04 * s + q * Math.sin(a)).toFixed(2)}`);
    }
    return svg(s, s, `<polygon points="${pts.join(" ")}" fill="${color}" stroke="${color}" stroke-width="${(s * 0.06).toFixed(2)}" stroke-linejoin="round"/>`, "g");
  }

  // Dongle pictogram (1.4) on the same 60 x 48 grid, shown at k times: a USB stick, its body and its plug.
  // Lit like the tracker: look "on" (light body), "grey", "off" (body close to the panel), "unknown" (dashed outline).
  function dongle(look, k) {
    const s = k || 1;
    const lit = look === "on" || look === "grey";
    const body = look === "on" ? "var(--ink)" : look === "grey" ? "var(--muted)" : look === "off" ? "var(--dark-body)" : "none";
    const edge = lit ? "" : ` stroke="var(--idle)" stroke-width="1.5"` + (look === "unknown" ? ` stroke-dasharray="4 3"` : "");
    const plug = lit ? body : "var(--idle)";
    return `<svg class="pict" width="${60 * s}" height="${48 * s}" viewBox="0 0 60 48" aria-hidden="true">` +
      `<rect x="4" y="12" width="38" height="24" rx="8" fill="${body}"${edge}/>` +
      `<rect x="42" y="17" width="14" height="14" rx="2" fill="none" stroke="${plug}" stroke-width="2"` +
        (look === "unknown" ? ` stroke-dasharray="4 3"` : "") + `/>` +
      (lit ? `<circle cx="14" cy="24" r="3" fill="var(--panel)"/>` : "") + `</svg>`;
  }

  // USB plug, 16 x 16: next to the battery of a tracker connected by USB whose state word is not "USB接続" (2.4).
  function usb(color) {
    const c = color || "var(--ink-2)";
    return svg(16, 16,
      `<rect x="4.5" y="6" width="7" height="6" rx="1.5" fill="none" stroke="${c}" stroke-width="1.6"/>` +
      `<path d="M6.5 2.5V6M9.5 2.5V6M8 12V15" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/>`, "ic");
  }

  // Battery, 26 x 14. pct: 0..100 or null (unknown: outline only). charging: a bolt on top, cut out of the panel.
  function battery(pct, charging, color) {
    const c = color || "var(--ink)";
    const w = pct == null ? 0 : Math.max(1.5, (17 * pct) / 100);
    const bolt = charging
      ? `<polygon points="12.5,1.5 7.5,8 11,8 9.5,12.5 15,6 11.5,6" fill="${c}" stroke="var(--panel)" stroke-width="1.6" stroke-linejoin="round"/>`
      : "";
    return svg(26, 14,
      `<rect x="1" y="1.5" width="21" height="11" rx="2.5" fill="none" stroke="${c}" stroke-width="1.5"/>` +
      `<rect x="22.5" y="4.5" width="2.5" height="5" rx="1" fill="${c}"/>` +
      (w ? `<rect x="3" y="3.5" width="${w.toFixed(1)}" height="7" rx="1.2" fill="${c}"/>` : "") + bolt, "bat");
  }

  // Settings, 20 x 20: a gear of eight teeth (one polygon) around a ring. For each tooth at angle t
  // (0, 45, ... 315 degrees): inner radius 6.3 at t - 17 and t + 17, outer radius 8.8 at t - 10 and t + 10.
  function gear(color) {
    const c = color || "currentColor", pts = [];
    const p = (r, d) => `${(10 + r * Math.cos((d * Math.PI) / 180)).toFixed(2)},${(10 + r * Math.sin((d * Math.PI) / 180)).toFixed(2)}`;
    for (let i = 0; i < 8; i++) { const t = i * 45; pts.push(p(6.3, t - 17), p(8.8, t - 10), p(8.8, t + 10), p(6.3, t + 17)); }
    return svg(20, 20,
      `<polygon points="${pts.join(" ")}" fill="none" stroke="${c}" stroke-width="1.6" stroke-linejoin="round"/>` +
      `<circle cx="10" cy="10" r="2.8" fill="none" stroke="${c}" stroke-width="1.6"/>`, "ic");
  }

  // Power, 20 x 20: an arc of radius 6.5 around (10, 11), open by 60 degrees at the top, and a bar from y 3 to y 10.
  function power(color) {
    const c = color || "currentColor";
    return svg(20, 20,
      `<path d="${arc(10, 11, 6.5, -60, 240)}" fill="none" stroke="${c}" stroke-width="1.7" stroke-linecap="round"/>` +
      `<path d="M10 3V10" stroke="${c}" stroke-width="1.7" stroke-linecap="round"/>`, "ic");
  }

  // More (the row's menu), 20 x 20: three dots in a column (x = 10; y = 4, 10, 16; radius 1.8).
  function more(color) {
    const c = color || "currentColor";
    return svg(20, 20, [4, 10, 16].map((y) => `<circle cx="10" cy="${y}" r="1.8" fill="${c}"/>`).join(""), "ic");
  }

  window.OpenUTIcons = { state, tracker, star, dongle, usb, battery, gear, power, more, arc };
})();
