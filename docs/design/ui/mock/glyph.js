// OpenUT glyph renderer for the mockups.
// It follows the procedure in docs/design/ui/m1.md (section 2.2) pixel by pixel,
// so the implementation in Rust can reproduce the same RGBA image without image files.
// The tray, the window icon and the toast logo use it (4th version: one look per overall state).

(function () {
  "use strict";

  // Geometry, as fractions of the icon size S.
  const G = {
    ringRadius: 0.34, // centre line of the ring
    ringHalf: 0.08, // half of the ring width (width 0.16 S)
    dotRadius: 0.13,
    badgeX: 0.76,
    badgeY: 0.24,
    badgeRadius: 0.22,
    badgeGap: 0.07, // transparent gap around the badge
    cutDeg: 90, // "wait": one gap of 90 degrees in the ring, centred on 315 degrees (upper right; clockwise from the right)
  };

  // Coverage of a pixel from a signed distance in pixels (negative = inside).
  function cover(d) {
    return Math.min(1, Math.max(0, 0.5 - d));
  }

  function hex(c) {
    const n = parseInt(c.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  }

  // variant (2.2): "ok" (ring + dot), "wait" (ring with one gap + dot), "fault" (ring + dot + badge).
  // color: glyph colour. badge: badge colour (fault only; the same colour as the glyph when omitted).
  function render(size, variant, color, badge) {
    const S = size;
    const c = hex(color);
    const b = badge ? hex(badge) : c;
    const out = new Uint8ClampedArray(S * S * 4);
    for (let y = 0; y < S; y++) {
      for (let x = 0; x < S; x++) {
        const px = x + 0.5, py = y + 0.5;
        const r = Math.hypot(px - 0.5 * S, py - 0.5 * S);
        let a = cover(Math.abs(r - G.ringRadius * S) - G.ringHalf * S);
        if (variant === "wait") {
          // Distance (in px, along the ring's centre line) from the pixel's angle to the nearest gap edge.
          const ang = ((Math.atan2(py - 0.5 * S, px - 0.5 * S) * 180) / Math.PI + 360) % 360;
          let off = 180;
          for (const g of [315]) off = Math.min(off, Math.abs(((ang - g + 540) % 360) - 180));
          const edge = ((off - G.cutDeg / 2) * Math.PI / 180) * G.ringRadius * S;
          a *= cover(-edge);
        }
        a = Math.max(a, cover(r - G.dotRadius * S));
        let rgb = c, alpha = a;
        if (variant === "fault") {
          const db = Math.hypot(px - G.badgeX * S, py - G.badgeY * S);
          alpha = a * (1 - cover(db - (G.badgeRadius + G.badgeGap) * S));
          const ab = cover(db - G.badgeRadius * S);
          const outA = ab + alpha * (1 - ab);
          if (outA > 0) {
            rgb = [0, 1, 2].map((i) => (b[i] * ab + c[i] * alpha * (1 - ab)) / outA);
          }
          alpha = outA;
        }
        const i = (y * S + x) * 4;
        out[i] = rgb[0];
        out[i + 1] = rgb[1];
        out[i + 2] = rgb[2];
        out[i + 3] = Math.round(alpha * 255);
      }
    }
    return out;
  }

  // Draws into every <canvas data-glyph="variant" data-size="N" data-color="#..." data-badge="#...">.
  // data-scale lets a 16 px icon be shown enlarged with hard pixels (for inspection).
  function paintAll(root) {
    (root || document).querySelectorAll("canvas[data-glyph]").forEach((cv) => {
      const size = parseInt(cv.dataset.size, 10);
      const scale = parseInt(cv.dataset.scale || "1", 10);
      const css = getComputedStyle(cv);
      const pick = (v) => (v && v.startsWith("--") ? css.getPropertyValue(v).trim() : v);
      const color = pick(cv.dataset.color);
      const badge = pick(cv.dataset.badge);
      cv.width = size;
      cv.height = size;
      cv.style.width = size * scale / (parseFloat(cv.dataset.dpr || "1")) + "px";
      cv.style.height = cv.style.width;
      if (scale > 1) cv.style.imageRendering = "pixelated";
      const ctx = cv.getContext("2d");
      ctx.putImageData(new ImageData(render(size, cv.dataset.glyph, color, badge), size, size), 0, 0);
    });
  }

  window.OpenUTGlyph = { render, paintAll, G };
  document.addEventListener("DOMContentLoaded", () => paintAll());
})();
