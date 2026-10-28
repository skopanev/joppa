// Chromium checks of requirement:board-visual-design: the type, the widths and the brand mark,
// the light and dark palettes, and the flat geometry of the board.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const types = {requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []}};
const LIGHT = {'--bg': '#fffcf5', '--text': '#444', '--heading': '#222', '--muted': '#6e685e', '--rule': '#d9d3c7', '--link': '#005cef', '--hover': '#f5efe2', '--selected': '#faf6ec'};
const DARK = {'--bg': '#191c20', '--text': '#d6d8dc', '--heading': '#f1f2f4', '--muted': '#a5a9b0', '--rule': '#444951', '--link': '#8cb5ff', '--hover': '#282d34', '--selected': '#22272d'};
const rgb = hex => {
  const h = hex.length === 4 ? hex.slice(1).split('').map(c => c + c).join('') : hex.slice(1);
  return 'rgb(' + [0, 2, 4].map(i => parseInt(h.slice(i, i + 2), 16)).join(', ') + ')';
};

async function board(f) {
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
  await f.page.locator('.app-header .brand-mark').waitFor();
}

test('board-visual-type: the board and settings use the type, widths and brand mark', async () => {
  const f = await fixture(types);
  try {
    await board(f);
    const look = await f.page.evaluate(() => {
      const s = el => getComputedStyle(el);
      const body = s(document.body), mark = document.querySelector('.app-header .brand-mark'), tagline = document.querySelector('.app-header .tagline');
      return {font: body.fontFamily, size: body.fontSize, line: body.lineHeight, width: body.maxWidth, transform: s(mark).transform,
        bars: [...mark.querySelectorAll('i')].map(i => [s(i).backgroundColor, s(i).height]),
        tagline: [tagline.textContent, s(tagline).fontSize, s(tagline).textTransform]};
    });
    assert.match(look.font, /^Verdana/);
    assert.deepEqual([look.size, look.line, look.width], ['14px', '22.4px', '1100px']);
    const [a, b] = look.transform.match(/matrix\(([^,]+), ([^,]+)/).slice(1).map(Number);
    assert.ok(Math.abs(Math.atan2(b, a) * 180 / Math.PI + 12) < 0.01, look.transform);
    assert.deepEqual(look.bars, [[rgb('#365d41'), '19px'], [rgb('#4b7858'), '28px'], [rgb('#7f9b69'), '14px']]);
    assert.deepEqual(look.tagline, ['AI first product specification', '10.5px', 'uppercase']);
    await f.page.goto(f.live.root + '/settings');
    await f.page.locator('main.settings').waitFor();
    assert.equal(await f.page.evaluate(() => getComputedStyle(document.querySelector('main.settings')).maxWidth), '600px');
  } finally { await f.close(); }
});

test('board-visual-palettes: the light and dark palettes use the listed colour tokens', async () => {
  const f = await fixture(types);
  try {
    for (const [theme, tokens] of [['light', LIGHT], ['dark', DARK]]) {
      await f.page.addInitScript(value => { try { localStorage.setItem('theme', value); } catch {} }, theme);
      await board(f);
      await f.page.evaluate(value => { document.documentElement.dataset.theme = value; }, theme);
      const read = await f.page.evaluate(names => {
        const root = getComputedStyle(document.documentElement), body = getComputedStyle(document.body);
        return {tokens: Object.fromEntries(names.map(n => [n, root.getPropertyValue(n).trim().toLowerCase()])), bg: body.backgroundColor, color: body.color};
      }, Object.keys(tokens));
      assert.deepEqual(read.tokens, tokens, theme);
      assert.equal(read.bg, rgb(tokens['--bg']), theme + ' background');
      assert.equal(read.color, rgb(tokens['--text']), theme + ' text');
    }
  } finally { await f.close(); }
});

test('board-visual-flat: the board is flat with no shadow and square controls', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Paid access');
    await board(f);
    await f.page.locator('#tree .common-object[data-object-address="requirement:r"] > summary').click();
    await f.page.locator('.filter-fields .filter-search').waitFor();
    const look = await f.page.evaluate(() => {
      const s = el => getComputedStyle(el);
      const shadows = [...document.querySelectorAll('*')].filter(el => s(el).boxShadow !== 'none').map(el => el.tagName + '.' + el.className);
      const controls = [...document.querySelectorAll('.filter-fields select, .filter-fields .filter-search')].map(el => [s(el).height, s(el).borderBottomWidth, s(el).borderTopWidth, s(el).borderRadius]);
      const tab = document.querySelector('.collection-tab[aria-selected=true]');
      const badge = document.querySelector('.common-object > summary .type-badge');
      return {shadows, controls, tab: tab && [s(tab).textDecorationStyle, s(tab).textDecorationLine],
        badge: badge && [s(badge).fontSize, s(badge).textTransform]};
    });
    assert.deepEqual(look.shadows, []);
    assert.ok(look.controls.length >= 2, JSON.stringify(look.controls));
    for (const control of look.controls) assert.deepEqual(control, ['44px', '1px', '0px', '0px']);
    if (look.tab) assert.deepEqual(look.tab, ['wavy', 'underline']);
    assert.deepEqual(look.badge, ['10.5px', 'uppercase']);
  } finally { await f.close(); }
});
