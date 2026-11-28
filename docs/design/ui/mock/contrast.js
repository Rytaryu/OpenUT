// Usage: node docs/design/ui/mock/contrast.js   (prints the table in m1.md, chapter 9)
// Contrast ratios (WCAG 2) for the colour pairs of the 6th version. Text needs 4.5, marks and outlines 3.
const hex = (h) => { const n = parseInt(h.slice(1), 16); return [(n >> 16) & 255, (n >> 8) & 255, n & 255]; };
const lin = (c) => { c /= 255; return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
const L = (rgb) => 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
const over = (fg, a, bg) => fg.map((c, i) => Math.round(c * a + bg[i] * (1 - a)));
const ratio = (a, b) => { const x = L(a), y = L(b); return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
const t = {
  bg: '#141821', panel: '#1C212B', panel2: '#262C38', line: '#2F3644', ink: '#ECEFF4', ink2: '#A2ABBA', muted: '#8B94A4',
  idle: '#6B7588', darkBody: '#252B36', onInk: '#10131A', ok: '#45C486', wait: '#4BA3FF', fault: '#FF7266', onState: '#10131A',
};
const rows = [];
const show = (c) => (Array.isArray(c) ? '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('').toUpperCase() : c);
const add = (what, kind, fg, bg, need) => {
  const r = ratio(Array.isArray(fg) ? fg : hex(fg), Array.isArray(bg) ? bg : hex(bg));
  rows.push([what, kind, show(fg), show(bg), r.toFixed(1), need, r >= need ? 'OK' : 'NG']);
};
// Main window
add('文字 / 背景', '文字', t.ink, t.bg, 4.5);
add('文字 / パネル・表', '文字', t.ink, t.panel, 4.5);
add('文字 / メニュー・指を置いたボタン', '文字', t.ink, t.panel2, 4.5);
add('見出しの列・シリアル番号の前の部分・灰色の値 / 表', '文字', t.ink2, t.panel, 4.5);
add('台数の文字 / 背景', '文字', t.ink2, t.bg, 4.5);
add('灰色の状態の語 / 表', '文字', t.muted, t.panel, 4.5);
add('接続済み・正常の文字 / パネル・表', '文字', t.ok, t.panel, 4.5);
add('同期中・待機中の文字 / パネル・表', '文字', t.wait, t.panel, 4.5);
add('異常の文字 / パネル・表', '文字', t.fault, t.panel, 4.5);
add('「フォルダーを選ぶ」の文字 / ボタン', '文字', t.onInk, t.ink, 4.5);
add('メニューの項目（指を置いたとき）/ その背景', '文字', t.ink, t.line, 4.5);
add('ボタンの枠 / パネル', '図形', t.idle, t.panel, 3);
add('ボタンの枠 / 背景（すべてオフにする）', '図形', t.idle, t.bg, 3);
add('異常の印の「!」/ 異常の色', '図形', t.onState, t.fault, 3);
add('正常・待機中・異常の印 / 表', '図形', t.ok, t.panel, 3);
add('　（待機中）', '図形', t.wait, t.panel, 3);
add('　（異常）', '図形', t.fault, t.panel, 3);
add('灰色の印 / 表', '図形', t.muted, t.panel, 3);
add('未接続の印・トラッカーの輪郭 / 表', '図形', t.idle, t.panel, 3);
add('つながったトラッカー・ドングルの胴 / 表', '図形', t.ink, t.panel, 3);
add('つながったトラッカーの中の枠と点 / 胴', '図形', t.panel, t.ink, 3);
add('灰色のトラッカーの胴 / 表', '図形', t.muted, t.panel, 3);
add('灰色のトラッカーの中の枠と点 / 胴', '図形', t.panel, t.muted, 3);
add('未接続のトラッカーの中の枠と点 / 胴', '図形', t.idle, t.darkBody, 3);
add('★ / 表', '図形', t.ink, t.panel, 3);
add('電源・メニュー・歯車のボタン、USB の印 / 表・パネル', '図形', t.ink2, t.panel, 3);
add('メニューのボタン（開いているとき）/ その背景', '図形', t.ink, t.panel2, 3);
// VR notice overlay: rgba(30,34,40,.94) over the worst cases (white and black scenes)
for (const [scene, bg] of [['白い景色', [255, 255, 255]], ['黒い景色', [0, 0, 0]]]) {
  const panel = over(hex('#1E2228'), 0.94, bg);
  add(`VR（${scene}）: 見出し / オーバーレイ`, '文字', '#F3F6FA', panel, 4.5);
  add(`VR（${scene}）: すること / オーバーレイ`, '文字', '#C3CEDC', panel, 4.5);
  add(`VR（${scene}）: 異常の印 / オーバーレイ`, '図形', t.fault, panel, 3);
  add(`VR（${scene}）: 正常の印 / オーバーレイ`, '図形', t.ok, panel, 3);
}
// Windows surfaces (taskbar, toast notification): one colour for light and dark
for (const [bar, bg] of [['明るいタスクバー', '#F3F3F3'], ['暗いタスクバー', '#1C1C1C'], ['トースト通知（明るい）', '#F9F9F9'], ['トースト通知（暗い）', '#2C2C2C']]) {
  add(`${bar}: マーク（正常）`, '図形', '#2F9466', bg, 3);
  add(`${bar}: マーク（待機中）`, '図形', '#3A7BD5', bg, 3);
  add(`${bar}: マーク（異常）`, '図形', '#D64A3C', bg, 3);
}
console.log('| 組み合わせ | 種類 | 前 | 後ろ | 比 | 基準 | 結果 |');
console.log('|---|---|---|---|---|---|---|');
for (const r of rows) console.log('| ' + r.join(' | ') + ' |');
const bad = rows.filter((r) => r[6] === 'NG');
console.error(bad.length ? 'NG: ' + bad.map((r) => r[0]).join(', ') : 'all OK');
