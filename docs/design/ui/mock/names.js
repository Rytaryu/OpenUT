// Display names of docs/design/ui/m1.md (section 6.4), for the mockups and for mock/lines.js.
// The app does the same in Rust (openut_core), with a width function for Yu Gothic UI Bold 14 px
// given by the app. Here the width comes from a canvas, in the font of the mockups.

(function () {
  "use strict";

  const MAX_W = 112;         // NAME_MAX_W: the name line of a tile at the minimum window size (3.3)
  const ELLIPSIS = "…";

  let ctx = null;
  function widthOf(s) {
    if (!ctx) {
      ctx = document.createElement("canvas").getContext("2d");
      const family = getComputedStyle(document.documentElement).getPropertyValue("--font");
      ctx.font = `700 14px ${family}`;
    }
    return ctx.measureText(s).width;
  }

  // Step 1: the name from the settings, or "トラッカー{N}".
  function base(n, configured) {
    const s = (configured || "").replace(/[\u0000-\u001F\u007F-\u009F]/g, " ").trim();
    return s === "" ? `トラッカー${n}` : s;
  }

  // Step 2: the longest head of s (cut between characters) that fits w with "…" after it.
  function fit(s, w, width) {
    if (width(s) <= w) return s;
    const chars = Array.from(s);
    let lo = 0, hi = chars.length;          // the answer keeps lo characters
    while (lo < hi) {
      const mid = Math.ceil((lo + hi) / 2);
      if (width(chars.slice(0, mid).join("") + ELLIPSIS) <= w) lo = mid; else hi = mid - 1;
    }
    return chars.slice(0, lo).join("") + ELLIPSIS;
  }

  // trackers: [{ n, name }] in the order of the numbers. Returns the display names, in the same order.
  function displayNames(trackers, width = widthOf, maxW = MAX_W) {
    const bases = trackers.map((t) => base(t.n, t.name));
    const out = bases.map((b) => fit(b, maxW, width));
    const numbered = new Set();
    const number = (i) => {
      const tag = `（${trackers[i].n}）`;
      out[i] = fit(bases[i], maxW - width(tag), width) + tag;
      numbered.add(i);
    };
    // Step 3: names that are the same as another one get the number.
    const clash = (i) => out.some((o, j) => j !== i && o === out[i]);
    out.map((_, i) => clash(i)).forEach((c, i) => { if (c) number(i); });
    // Step 4: repeat while a name without the number is the same as another name.
    for (let changed = true; changed; ) {
      changed = false;
      for (let i = 0; i < out.length; i++) if (!numbered.has(i) && clash(i)) { number(i); changed = true; }
    }
    return out;
  }

  window.OpenUTNames = { displayNames, fit, base, widthOf, MAX_W };
})();
