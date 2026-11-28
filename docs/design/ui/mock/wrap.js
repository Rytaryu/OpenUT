// Line breaking of docs/design/ui/m1.md (section 5.4), for the mockups and for mock/fit.js.
// The text is given as lines separated by "\n" (one line per sentence, after "。"),
// and each line as phrases (文節) separated by "|". The result is the list of drawn lines.
// The app does the same in Rust with the advance widths of the loaded font.

(function () {
  "use strict";

  // Characters that must not start a line (rule 3).
  const NO_START = new Set(Array.from("、。，．）」』ーぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮ…"));
  // Characters of a serial number (design 4.4: ASCII letters, digits and "-"). A run of them is kept on one line
  // when it fits on one line (rule 3).
  const RUN = /[0-9A-Za-z-]/;

  let ctx = null;
  function measure(font, s) {
    if (!ctx) ctx = document.createElement("canvas").getContext("2d");
    ctx.font = font;
    return ctx.measureText(s).width;
  }

  // Rule 3: a phrase wider than the line goes on after the current line, character by character,
  // and is broken where the line is full.
  function breakPhrase(font, width, phrase, out, cur) {
    const chars = Array.from(phrase);
    for (let i = 0; i < chars.length; i++) {
      const c = chars[i];
      if (cur === "" || measure(font, cur + c) <= width) { cur += c; continue; }
      const head = Array.from(cur);
      let carry = "";
      // A run of serial characters cut in two moves down whole, if the whole run fits on one line.
      // A longer run (a tail of up to 32 characters, design 3.2) is cut where the line is full.
      if (RUN.test(c) && RUN.test(head[head.length - 1])) {
        let k = head.length;
        while (k > 0 && RUN.test(head[k - 1])) k--;
        let j = i;
        while (j < chars.length && RUN.test(chars[j])) j++;
        const run = head.slice(k).join("") + chars.slice(i, j).join("");
        if (k > 0 && measure(font, run) <= width) carry = head.splice(k).join("");
      }
      // No line starts with a NO_START character: move characters down until it does not.
      while (head.length > 1 && NO_START.has(Array.from(carry + c)[0])) carry = head.pop() + carry;
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
        if (measure(font, phrase) <= width) {                                    // rule 2
          if (cur !== "") out.push(cur);
          cur = phrase;
          continue;
        }
        cur = breakPhrase(font, width, phrase, out, cur);                        // rule 3
      }
      out.push(cur);
    }
    return out;
  }

  // The serial number on a card when its tail does not fit on one line (m1.md 2.4, case C): lines are cut
  // from the end, each holding as many characters as fit, and what is left goes on the first line.
  // The end of the serial stays on one line, and the first line, where two long tails differ, stands apart.
  function wrapFromEnd(text, font, width) {
    const chars = Array.from(text);
    const out = [];
    let end = chars.length;
    while (end > 0) {
      let start = end - 1;
      while (start > 0 && measure(font, chars.slice(start - 1, end).join("")) <= width) start--;
      out.unshift(chars.slice(start, end).join(""));
      end = start;
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

  window.OpenUTWrap = { wrap, wrapFromEnd, measure, apply };
  document.addEventListener("DOMContentLoaded", () => document.fonts.ready.then(() => apply()));
})();
