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

  // State glyph (1.4): ok = filled disc, wait = ring with the left half filled, fault = triangle with "!",
  // none = empty ring (not connected). s = box size in px.
  function state(kind, s, color) {
    const c = s / 2, r = s * 0.42, sw = Math.max(1.5, s * 0.12);
    if (kind === "ok") return svg(s, s, `<circle cx="${c}" cy="${c}" r="${r}" fill="${color}"/>`, "g");
    if (kind === "wait")
      return svg(s, s,
        `<circle cx="${c}" cy="${c}" r="${r - sw / 2}" fill="none" stroke="${color}" stroke-width="${sw}"/>` +
        `<path d="M${c} ${c - r} A${r} ${r} 0 0 0 ${c} ${c + r} Z" fill="${color}"/>`, "g");
    if (kind === "fault") {
      // Triangle with rounded corners approximated by a stroke join; the "!" is cut in the background colour.
      const t = s * 0.06, top = s * 0.1, bot = s * 0.9, l = s * 0.06, rr = s * 0.94;
      return svg(s, s,
        `<polygon points="${c},${top} ${rr},${bot} ${l},${bot}" fill="${color}" stroke="${color}" stroke-width="${t * 2}" stroke-linejoin="round"/>` +
        `<rect x="${c - s * 0.055}" y="${s * 0.36}" width="${s * 0.11}" height="${s * 0.28}" rx="${s * 0.05}" fill="var(--on-state)"/>` +
        `<circle cx="${c}" cy="${s * 0.75}" r="${s * 0.065}" fill="var(--on-state)"/>`, "g");
    }
    return svg(s, s, `<circle cx="${c}" cy="${c}" r="${r - sw / 2}" fill="none" stroke="${color}" stroke-width="${sw}"/>`, "g");
  }

  // Tracker pictogram, drawn on a 60 x 48 grid and shown at k times that size (k = 1.5 on a card: 90 x 72).
  // Not a picture of the product: a rounded body seen from the front with its two cameras near the top edge.
  // frame: colour of the body's outline (the state colour, or idle).
  // body: "solid" when connected, "outline" when not (no fill, thin outline, cameras as rings).
  function tracker(frame, body, k) {
    const s = k || 1, solid = body !== "outline";
    const lens = (cx) => solid
      ? `<circle cx="${cx}" cy="17" r="5" fill="#15181C" stroke="#5A6370" stroke-width="1.5"/>`
      : `<circle cx="${cx}" cy="17" r="4.5" fill="none" stroke="${frame}" stroke-width="1.5"/>`;
    return `<svg class="pict" width="${60 * s}" height="${48 * s}" viewBox="0 0 60 48" aria-hidden="true">` +
      `<rect x="7.5" y="5.5" width="45" height="37" rx="14" fill="${solid ? "var(--panel-2)" : "none"}" ` +
        `stroke="${frame}" stroke-width="${solid ? 3 : 2}"/>` +
      lens(20) + lens(40) + `</svg>`;
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

  // Dongle pictogram on the same 60 x 48 grid, shown at k times: a USB stick with its plug.
  // look: "on", "off", "unknown" (dashed outline).
  function dongle(look, k) {
    const s = k || 1, on = look === "on";
    const stroke = on ? "#4A525D" : "var(--idle)";
    const dash = look === "unknown" ? ` stroke-dasharray="4 3"` : "";
    return `<svg class="pict" width="${60 * s}" height="${48 * s}" viewBox="0 0 60 48" aria-hidden="true">` +
      `<rect x="5" y="13" width="36" height="22" rx="6" fill="${on ? "var(--panel-2)" : "none"}" stroke="${stroke}" stroke-width="2"${dash}/>` +
      `<rect x="41" y="17" width="14" height="14" rx="2" fill="none" stroke="${stroke}" stroke-width="2"${dash}/>` +
      (on ? `<rect x="46" y="21" width="4" height="2" fill="#5A6370"/><rect x="46" y="25" width="4" height="2" fill="#5A6370"/>` +
            `<circle cx="15" cy="24" r="3" fill="var(--ink-2)"/>` : "") + `</svg>`;
  }

  // USB plug, 16 x 16: shown next to the battery of a tracker that is connected by USB (2.4).
  // The radio-wave icon of the 4th version is gone (owner: a Wi-Fi mark with "ドングル" says little).
  function usb(color) {
    const c = color || "var(--ink-2)";
    return svg(16, 16,
      `<rect x="4.5" y="6" width="7" height="6" rx="1.5" fill="none" stroke="${c}" stroke-width="1.6"/>` +
      `<path d="M6.5 2.5V6M9.5 2.5V6M8 12V15" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/>`, "ic");
  }

  // Battery, 26 x 14. pct: 0..100 or null (unknown: outline only). charging: bolt on top.
  function battery(pct, charging, color) {
    const c = color || "var(--ink)";
    const w = pct == null ? 0 : Math.max(1.5, (17 * pct) / 100);
    const bolt = charging
      ? `<polygon points="12.5,1.5 7.5,8 11,8 9.5,12.5 15,6 11.5,6" fill="var(--accent)" stroke="var(--panel)" stroke-width="1.2" stroke-linejoin="round"/>`
      : "";
    return svg(26, 14,
      `<rect x="1" y="1.5" width="21" height="11" rx="2.5" fill="none" stroke="${c}" stroke-width="1.5"/>` +
      `<rect x="22.5" y="4.5" width="2.5" height="5" rx="1" fill="${c}"/>` +
      (w ? `<rect x="3" y="3.5" width="${w.toFixed(1)}" height="7" rx="1.2" fill="${c}"/>` : "") + bolt, "bat");
  }

  // Operation icons, 20 x 20: pairing (two linked rings), setup (folded map).
  function op(kind, color) {
    const c = color || "currentColor";
    const st = `fill="none" stroke="${c}" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"`;
    if (kind === "pair") return svg(20, 20, `<circle cx="7.5" cy="10" r="4.6" ${st}/><circle cx="12.5" cy="10" r="4.6" ${st}/>`, "ic");
    return svg(20, 20,
      `<polygon points="2.5,5 7,3 13,5 17.5,3 17.5,15 13,17 7,15 2.5,17" ${st}/><path d="M7 3V15M13 5V17" ${st}/>`, "ic");
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

  // More (the card's menu), 20 x 20: three dots in a row.
  function more(color) {
    const c = color || "currentColor";
    return svg(20, 20, [4, 10, 16].map((x) => `<circle cx="${x}" cy="10" r="1.8" fill="${c}"/>`).join(""), "ic");
  }

  // Check mark of a checkbox, in a box of s.
  function check(s, color) {
    return svg(s, s, `<path d="M${s * 0.28} ${s * 0.52}L${s * 0.44} ${s * 0.67}L${s * 0.73} ${s * 0.36}" fill="none" stroke="${color}" stroke-width="${s * 0.11}" stroke-linecap="round" stroke-linejoin="round"/>`, "g");
  }

  window.OpenUTIcons = { state, tracker, star, dongle, usb, battery, op, gear, more, check, arc };
})();
