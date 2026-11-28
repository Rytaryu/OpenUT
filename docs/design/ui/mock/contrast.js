// Usage: node docs/design/ui/mock/contrast.js   (prints the table in m1.md, section 10)
// Contrast ratios (WCAG 2) for the colour pairs used by the M1 design.
const hex = (h) => { const n = parseInt(h.slice(1), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
const lin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const L = (rgb) => 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
const over = (fg, a, bg) => fg.map((c, i) => Math.round(c * a + bg[i] * (1 - a)));
const ratio = (a, b) => { const x = L(a), y = L(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
const t = {
  bg: '#141A22', tile: '#1C2430', ink: '#E8EDF3', ink2: '#9AA6B5', track: '#66748A', reach: '#4D79B8',
  stage: '#8DB6F5', onStage: '#0E1A2C', okReach: '#2E8F6C', ok: '#5FD3A6', onOk: '#0B2A1F', idle: '#475263',
  signal: '#F2A33A', hover: '#263142',
};
const rows = [];
const show = (c) => (Array.isArray(c) ? '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('').toUpperCase() : c);
const add = (what, kind, fg, bg, need) => {
  const r = ratio(hex(fg), Array.isArray(bg) ? bg : hex(bg));
  rows.push([what, kind, fg, show(bg), r.toFixed(1), need, r >= need ? 'OK' : 'NG']);
};
// Status window (one dark theme)
add('文字 / 地', '文字', t.ink, t.bg, 4.5);
add('文字 / タイルと案内', '文字', t.ink, t.tile, 4.5);
add('文字 / 名前に指を置いたとき', '文字', t.ink, t.hover, 4.5);
add('補助の文字 / 地', '文字', t.ink2, t.bg, 4.5);
add('補助の文字 / タイル', '文字', t.ink2, t.tile, 4.5);
add('ホストの印 / タイル', '文字', t.stage, t.tile, 4.5);
add('いまの段の経過時間 / いまの段', '文字', t.onStage, t.stage, 4.5);
add('いまの段の経過時間（追跡中）/ いまの段', '文字', t.onOk, t.ok, 4.5);
add('未接続の経過時間 / 未接続の段', '文字', t.ink, t.idle, 4.5);
add('ボタンの文字 / ボタン', '文字', t.onStage, t.stage, 4.5);
add('段（まだ。輪郭）/ タイル', '図形', t.track, t.tile, 3);
add('段（通った）/ タイル', '図形', t.reach, t.tile, 3);
add('段（いま）/ タイル', '図形', t.stage, t.tile, 3);
add('段（追跡中で通った）/ タイル', '図形', t.okReach, t.tile, 3);
add('段（追跡中）/ タイル', '図形', t.ok, t.tile, 3);
add('案内の帯 / 案内', '図形', t.signal, t.tile, 3);
add('つながった案内の帯 / 案内', '図形', t.ok, t.tile, 3);
add('マーク（ふつう）/ 地', '図形', t.stage, t.bg, 3);
add('マーク（案内ありの点）/ 地', '図形', t.signal, t.bg, 3);
add('マーク（見えていない）/ 地', '図形', t.ink2, t.bg, 3);
add('チェックボックス（入）/ 地', '図形', t.stage, t.bg, 3);
add('チェックボックス（切）の枠 / 地', '図形', t.ink2, t.bg, 3);
add('入力欄の枠 / 地', '図形', t.stage, t.bg, 3);
// VR panel: rgba(20,26,34,.94) composited over the worst cases (white and black scenes)
for (const [scene, bg] of [['白い景色', [255, 255, 255]], ['黒い景色', [0, 0, 0]]]) {
  const panel = over(hex('#141A22'), 0.94, bg);
  add(`VR（${scene}）: 見出し / 板`, '文字', '#F3F6FA', panel, 4.5);
  add(`VR（${scene}）: すること / 板`, '文字', '#C3CEDC', panel, 4.5);
  add(`VR（${scene}）: マーク（案内）/ 板`, '図形', '#F2A33A', panel, 3);
  add(`VR（${scene}）: マーク（直った）/ 板`, '図形', '#5FD3A6', panel, 3);
}
// Windows surfaces (taskbar, toast): one colour for light and dark
for (const [bar, bg] of [['明るいタスクバー', '#F3F3F3'], ['暗いタスクバー', '#1C1C1C']]) {
  add(`${bar}: マーク（ふつう）`, '図形', '#3A6FB8', bg, 3);
  add(`${bar}: マーク（案内ありの点）`, '図形', '#C8650F', bg, 3);
  add(`${bar}: マーク（見えていない）`, '図形', '#7D8794', bg, 3);
}
for (const [bar, bg] of [['トースト（明るい）', '#F9F9F9'], ['トースト（暗い）', '#2C2C2C']]) {
  add(`${bar}: マーク（案内）`, '図形', '#C8650F', bg, 3);
  add(`${bar}: マーク（直った）`, '図形', '#2E9E77', bg, 3);
}
console.log('| 組み合わせ | 種類 | 前 | 後ろ | 比 | 基準 | 結果 |');
console.log('|---|---|---|---|---|---|---|');
for (const r of rows) console.log('| ' + r.join(' | ') + ' |');
const bad = rows.filter((r) => r[6] === 'NG');
console.error(bad.length ? 'NG: ' + bad.map((r) => r[0]).join(', ') : 'all OK');
