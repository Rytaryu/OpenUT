// Line breaking of docs/design/ui/m1.md (section 4.4), for the mockups and for mock/lines.js.
// The text is given as lines separated by "\n" (one line per sentence, after "。"),
// and each line as phrases (文節) separated by "|". The result is the list of drawn lines.
// The app does the same in Rust with the advance widths of the loaded font.

(function () {
  "use strict";

  // Characters that must not start a line (rule 3).
  const NO_START = new Set(Array.from("、。，．）」』ーぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮ…"));
  const ALNUM = /[0-9A-Za-z]/;

  let ctx = null;
  function measure(font, s) {
    if (!ctx) ctx = document.createElement("canvas").getContext("2d");
    ctx.font = font;
    return ctx.measureText(s).width;
  }

  // Rule 3: a phrase wider than the line is broken by characters.
  function breakPhrase(font, width, phrase, out, cur) {
    for (const c of Array.from(phrase)) {
      if (cur === "" || measure(font, cur + c) <= width) { cur += c; continue; }
      // The next line starts with carry + c. Move characters down while that start is not allowed:
      // a NO_START character first, or a run of digits and letters cut in two.
      const head = Array.from(cur);
      let carry = "";
      const first = () => Array.from(carry + c)[0];
      while (head.length > 1 && (NO_START.has(first()) || (ALNUM.test(head[head.length - 1]) && ALNUM.test(first())))) {
        carry = head.pop() + carry;
      }
      out.push(head.join(""));
      cur = carry + c;
    }
    return cur;
  }

  function wrap(text, font, width) {
    const out = [];
    for (const line of text.split("\n")) {
      let cur = "";
      for (const phrase of line.split("|")) {
        if (measure(font, cur + phrase) <= width) { cur += phrase; continue; }   // rule 1
        if (cur !== "") { out.push(cur); cur = ""; }                            // rule 2
        if (measure(font, phrase) <= width) { cur = phrase; continue; }
        cur = breakPhrase(font, width, phrase, out, cur);                        // rule 3
      }
      out.push(cur);
    }
    return out;
  }

  function fontOf(el) {
    const s = getComputedStyle(el);
    return `${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
  }

  // Lay out every element with data-wrap: the text in data-wrap, the width of the element's content box
  // (or data-wrap-width, in CSS px before any zoom), the font of the element (or data-wrap-font).
  function apply(root) {
    (root || document).querySelectorAll("[data-wrap]").forEach((el) => {
      const s = getComputedStyle(el);
      const width = el.dataset.wrapWidth ? Number(el.dataset.wrapWidth)
        : el.clientWidth - parseFloat(s.paddingLeft) - parseFloat(s.paddingRight);
      const font = el.dataset.wrapFont || fontOf(el);
      const lines = wrap(el.dataset.wrap.replace(/\\n/g, "\n"), font, width);
      el.replaceChildren(...lines.flatMap((l, i) => (i ? [document.createElement("br"), l] : [l])));
      el.dataset.lines = String(lines.length);
    });
  }

  window.OpenUTWrap = { wrap, measure, apply };
  document.addEventListener("DOMContentLoaded", () => document.fonts.ready.then(() => apply()));
})();
