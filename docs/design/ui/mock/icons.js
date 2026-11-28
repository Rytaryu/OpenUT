// Icons of the OpenUT main window, VR panels and toasts (docs/design/ui/m1.md, section 1.4).
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

  // Tracker pictogram, 60 x 48 (not a picture of the product: a rounded body and one lens).
  // ring: colour of the lens ring (the state colour). body: "solid" when connected, "outline" when not.
  function tracker(ring, body) {
    const solid = body !== "outline";
    return svg(60, 48,
      `<rect x="5" y="4" width="50" height="40" rx="12" fill="${solid ? "var(--panel-2)" : "none"}" ` +
        `stroke="${solid ? "#4A525D" : "var(--idle)"}" stroke-width="2"/>` +
      `<circle cx="30" cy="24" r="11" fill="${solid ? "#15181C" : "none"}"/>` +
      `<circle cx="30" cy="24" r="11" fill="none" stroke="${ring}" stroke-width="4"/>` +
      (solid ? `<circle cx="34" cy="20" r="2.3" fill="#5A6370"/>` : ""), "pict");
  }

  // Dongle pictogram, 60 x 48: a USB stick with its plug. look: "on", "off", "unknown" (dashed outline).
  function dongle(look) {
    const on = look === "on";
    const stroke = on ? "#4A525D" : "var(--idle)";
    const dash = look === "unknown" ? ` stroke-dasharray="4 3"` : "";
    return svg(60, 48,
      `<rect x="5" y="13" width="36" height="22" rx="6" fill="${on ? "var(--panel-2)" : "none"}" stroke="${stroke}" stroke-width="2"${dash}/>` +
      `<rect x="41" y="17" width="14" height="14" rx="2" fill="none" stroke="${stroke}" stroke-width="2"${dash}/>` +
      (on ? `<rect x="46" y="21" width="4" height="2" fill="#5A6370"/><rect x="46" y="25" width="4" height="2" fill="#5A6370"/>` +
            `<circle cx="15" cy="24" r="3" fill="var(--ink-2)"/>` : ""), "pict");
  }

  // Connection icons, 16 x 16. "dongle" = radio waves, "usb" = plug, "none" = waves with a slash.
  function link(kind, color) {
    const c = color || "var(--ink-2)";
    if (kind === "usb")
      return svg(16, 16,
        `<rect x="4.5" y="6" width="7" height="6" rx="1.5" fill="none" stroke="${c}" stroke-width="1.6"/>` +
        `<path d="M6.5 2.5V6M9.5 2.5V6M8 12V15" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/>`, "ic");
    const waves =
      `<circle cx="8" cy="12.5" r="1.7" fill="${c}"/>` +
      `<path d="${arc(8, 12.5, 4.8, 225, 315)}" fill="none" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/>` +
      `<path d="${arc(8, 12.5, 8.2, 225, 315)}" fill="none" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/>`;
    if (kind === "none") return svg(16, 16, waves + `<path d="M2.5 2.5L13.5 13.5" stroke="${c}" stroke-width="1.6" stroke-linecap="round"/>`, "ic");
    return svg(16, 16, waves, "ic");
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

  // Operation icons, 20 x 20: pairing (two linked rings), map (folded map), calibration (crosshair).
  function op(kind, color) {
    const c = color || "currentColor";
    const st = `fill="none" stroke="${c}" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"`;
    if (kind === "pair") return svg(20, 20, `<circle cx="7.5" cy="10" r="4.6" ${st}/><circle cx="12.5" cy="10" r="4.6" ${st}/>`, "ic");
    if (kind === "map")
      return svg(20, 20,
        `<polygon points="2.5,5 7,3 13,5 17.5,3 17.5,15 13,17 7,15 2.5,17" ${st}/><path d="M7 3V15M13 5V17" ${st}/>`, "ic");
    return svg(20, 20,
      `<circle cx="10" cy="10" r="5.5" ${st}/><path d="M10 1.8V5M10 15V18.2M1.8 10H5M15 10H18.2" ${st}/>` +
      `<circle cx="10" cy="10" r="1.4" fill="${c}"/>`, "ic");
  }

  // Check mark for a finished step, in a box of s.
  function check(s, color) {
    return svg(s, s, `<path d="M${s * 0.28} ${s * 0.52}L${s * 0.44} ${s * 0.67}L${s * 0.73} ${s * 0.36}" fill="none" stroke="${color}" stroke-width="${s * 0.11}" stroke-linecap="round" stroke-linejoin="round"/>`, "g");
  }

  window.OpenUTIcons = { state, tracker, dongle, link, battery, op, check, arc };
})();
