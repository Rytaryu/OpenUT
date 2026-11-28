// Usage: node docs/design/ui/mock/contrast.js   (prints the table in m1.md, section 10)
// Contrast ratios (WCAG 2) for the token pairs used by the M1 design.
const hex = (h) => { const n = parseInt(h.slice(1), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
const lin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const L = (rgb) => 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
const over = (fg, a, bg) => fg.map((c, i) => Math.round(c * a + bg[i] * (1 - a)));
const ratio = (a, b) => { const x = L(a), y = L(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
const light = { bg: '#F4F6F8', surface: '#FFFFFF', ink: '#1B2230', ink2: '#4D5768', track: '#C4CDD8', stage: '#2F5D9E', ok: '#1E7A5A', signal: '#B4610A', onStage: '#FFFFFF' };
const dark = { bg: '#172130', surface: '#1F2B3D', ink: '#E7ECF2', ink2: '#A3AFBF', track: '#3A4C64', stage: '#7FA6E6', ok: '#4CC39A', signal: '#F2A33A', onStage: '#10233F' };
const rows = [];
const add = (what, kind, fg, bg, need) => { const r = ratio(hex(fg), Array.isArray(bg) ? bg : hex(bg)); rows.push([what, kind, fg, Array.isArray(bg) ? '#' + bg.map(c => c.toString(16).padStart(2, '0')).join('').toUpperCase() : bg, r.toFixed(1), need, r >= need ? 'OK' : 'NG']); };
for (const [name, t] of [['明るい', light], ['暗い', dark]]) {
  add(`${name}: 文字 / 地`, '文字', t.ink, t.bg, 4.5);
  add(`${name}: 文字 / 案内の欄`, '文字', t.ink, t.surface, 4.5);
  add(`${name}: 補助の文字 / 地`, '文字', t.ink2, t.bg, 4.5);
  add(`${name}: 補助の文字 / 案内の欄`, '文字', t.ink2, t.surface, 4.5);
  add(`${name}: ホストの印 / 地`, '文字', t.stage, t.bg, 4.5);
  add(`${name}: ボタンの文字 / ボタン`, '文字', t.onStage, t.stage, 4.5);
  add(`${name}: 段（進んだ）/ 地`, '図形', t.stage, t.bg, 3);
  add(`${name}: 段（追跡中）/ 地`, '図形', t.ok, t.bg, 3);
  add(`${name}: 案内の帯 / 案内の欄`, '図形', t.signal, t.surface, 3);
  add(`${name}: 直った帯 / 案内の欄`, '図形', t.ok, t.surface, 3);
}
// VR panel: rgba(23,35,52,.94) composited over the worst cases (white and black scenes)
for (const [scene, bg] of [['白い景色', [255, 255, 255]], ['黒い景色', [0, 0, 0]]]) {
  const panel = over(hex('#172334'), 0.94, bg);
  add(`VR（${scene}）: 見出し / 板`, '文字', '#F3F6FA', panel, 4.5);
  add(`VR（${scene}）: すること / 板`, '文字', '#C3CEDC', panel, 4.5);
  add(`VR（${scene}）: 印（案内）/ 板`, '図形', '#F2A33A', panel, 3);
  add(`VR（${scene}）: 印（直った）/ 板`, '図形', '#4CC39A', panel, 3);
}
for (const [bar, bg] of [['明るいタスクバー', '#F3F3F3'], ['暗いタスクバー', '#1C1C1C'], ['明るい窓の地', '#F4F6F8'], ['暗い窓の地', '#172130']]) {
  add(`${bar}: 印（ふつう）`, '図形', '#3A6FB8', bg, 3);
  add(`${bar}: 印（案内ありの点）`, '図形', '#C8650F', bg, 3);
  add(`${bar}: 印（見えていない）`, '図形', '#7D8794', bg, 3);
}
for (const [bar, bg] of [['トースト（明るい）', '#F9F9F9'], ['トースト（暗い）', '#2C2C2C']]) {
  add(`${bar}: 印（案内）`, '図形', '#C8650F', bg, 3);
  add(`${bar}: 印（直った）`, '図形', '#2E9E77', bg, 3);
}
console.log('| 組み合わせ | 種類 | 前 | 後ろ | 比 | 基準 | 結果 |');
console.log('|---|---|---|---|---|---|---|');
for (const r of rows) console.log('| ' + r.join(' | ') + ' |');
const bad = rows.filter(r => r[6] === 'NG');
console.error(bad.length ? 'NG: ' + bad.map(r => r[0]).join(', ') : 'all OK');
