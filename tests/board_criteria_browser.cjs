// Chromium checks of the History tab, the theme colours of the status dots and the lanes of
// the tree diagram. Each test starts its own browser_server fixture.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const {fixture, root} = require('./board_fixture.cjs');

const domains = {domain: {title: 'Domains', role: 'group', status:'children', root: true, parents: [], badge: 'D'}};

test('ea0ca1c0 history-tab: History pages 25 rows with Load more, dates and Show processed too', async () => {
  const f = await fixture(domains);
  try {
    // Every entry before the 30 new saves is processed: the first one is entry 1.
    for (let open = await f.mcp('joppa_history', {}); open.items.length; open = await f.mcp('joppa_history', {}))
      await f.mcp('joppa_history_mark', {position: open.items[0].position, reason: 'Fixture setup'});
    for (let n = 1; n <= 30; n++) await f.save('domain:d' + n, 'Domain ' + n);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const tab = f.page.getByRole('tab', {name: 'History'});
    await tab.click();
    const panel = f.page.locator('.history-collection');
    const rows = panel.locator('.history-entry');
    await f.page.waitForFunction(() => document.querySelectorAll('.history-collection .history-entry').length === 25);
    assert.deepEqual(await f.page.locator('.collection-tabs [role=tab]').allTextContents(), ['Domains', 'History']);
    assert.equal(await rows.locator('.note-state:not(.history-at)').filter({hasText: 'Processed'}).count(), 0);
    const dates = await rows.locator('.history-at').allTextContents();
    assert.equal(dates.length, 25);
    for (const date of dates) assert.match(date, /^\d{1,2} [A-Z][a-z]{2} \d{4}, \d{2}:\d{2}$/);
    const more = panel.getByRole('button', {name: 'Load more'});
    await more.click();
    await f.page.waitForFunction(() => document.querySelectorAll('.history-collection .history-entry').length === 30);
    assert.equal(await more.count(), 0);
    const positions = await rows.evaluateAll(list => list.map(row => Number(row.dataset.position)));
    assert.deepEqual(positions, [...positions].sort((a, b) => a - b));
    assert.equal(new Set(positions).size, 30);
    await panel.getByLabel('Show processed too').check();
    await f.page.waitForFunction(() => document.querySelector('.history-collection .history-entry')?.dataset.position === '1');
    assert.equal(await rows.first().locator('.note-state:not(.history-at)').innerText(), 'Processed');
    assert.equal(await rows.count(), 25);
  } finally { await f.close(); }
});

test('b3ecb4e3 dot-theme: each dot takes the light and the dark colour of its tone', async () => {
  const types = {
    requirement: {title: 'Requirements', role: 'requirement', status:'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status:'tests', parents: ['requirement']},
  };
  const statuses = [{tone: 'green', light: '#22a559', dark: '#2fbf6a'}, {tone: 'grey', light: '#c8c3b8', dark: '#8a8f96'}];
  const f = await fixture(types, statuses);
  try {
    await f.save('requirement:r', 'Requirement');
    await f.save('ac:g', 'Green criterion', 'requirement:r');
    await f.save('ac:n', 'Grey criterion', 'requirement:r');
    await f.command({op: 'set_ac_tests', target: 'ac:g', tests: ['tests/x.rs > x'], reason: 'Bind'});
    await f.mcp('joppa_record_results', {commit: 'c0ffee3', results: [{test: 'tests/x.rs > x', status: 'pass'}]});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const r = f.page.locator('#tree .common-object[data-object-address="requirement:r"]');
    await r.locator(':scope > summary').click();
    const dot = address => r.locator('.common-object[data-object-address="' + address + '"] > summary .status-circle').first();
    await dot('ac:n').waitFor();
    const colours = async () => ({green: await dot('ac:g').evaluate(node => [node.dataset.tone, getComputedStyle(node).backgroundColor]),
      grey: await dot('ac:n').evaluate(node => [node.dataset.tone, getComputedStyle(node).backgroundColor])});
    assert.deepEqual(await colours(), {green: ['green', 'rgb(34, 165, 89)'], grey: ['grey', 'rgb(200, 195, 184)']});
    await f.page.locator('#theme').click();
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.deepEqual(await colours(), {green: ['green', 'rgb(47, 191, 106)'], grey: ['grey', 'rgb(138, 143, 150)']});
  } finally { await f.close(); }
});

test('5c9f319a tree-lanes: two edges never share one vertical segment', async () => {
  const types = {
    alpha: {title: 'Alpha', role: 'group', status:'children', root: true, parents: []},
    beta: {title: 'Beta', role: 'group', status:'children', root: true, parents: []},
    child: {title: 'Child', role: 'document', status:'none', parents: ['alpha', 'beta']},
  };
  const f = await fixture(types);
  try {
    await f.page.goto(f.live.root + '/tree?workspace=' + encodeURIComponent(f.live.ws));
    for (const source of ['alpha', 'beta'])
      await f.page.locator('.schema-edge[data-source="' + source + '"][data-target="child"]').waitFor({state: 'attached'});
    // Each vertical part of a path: consecutive points with the same x and different y.
    const segments = await f.page.locator('.schema-edge').evaluateAll(paths => paths.flatMap((path, index) => {
      const numbers = path.getAttribute('d').match(/-?\d+(\.\d+)?/g).map(Number), points = [];
      for (let i = 0; i + 1 < numbers.length; i += 2) points.push([numbers[i], numbers[i + 1]]);
      const parts = [];
      for (let i = 1; i < points.length; i++) {
        const [a, b] = [points[i - 1], points[i]];
        if (Math.abs(a[0] - b[0]) < 0.01 && Math.abs(a[1] - b[1]) > 1)
          parts.push({index, edge: path.dataset.source + '>' + path.dataset.target, x: a[0], low: Math.min(a[1], b[1]), high: Math.max(a[1], b[1])});
      }
      return parts;
    }));
    const lanes = segments.filter(s => s.edge === 'alpha>child' || s.edge === 'beta>child');
    assert.ok(lanes.some(s => s.edge === 'alpha>child') && lanes.some(s => s.edge === 'beta>child'), JSON.stringify(segments));
    for (const a of segments) for (const b of segments) {
      if (a.index >= b.index) continue;
      const overlap = Math.min(a.high, b.high) - Math.max(a.low, b.low);
      assert.ok(Math.abs(a.x - b.x) >= 0.5 || overlap <= 0.5, 'merged lines: ' + JSON.stringify([a, b]));
    }
  } finally { await f.close(); }
});

test('5e504068 status-filter: the Object status filter lists the schema tones in order, each with its colour dot', async () => {
  const statuses = [{tone: 'orange', light: '#d0661a', dark: '#f08a3c'}, {tone: 'green', light: '#22a559', dark: '#2fbf6a'},
    {tone: 'red', light: '#c0392b', dark: '#ef6b5e'}];
  const f = await fixture(domains, statuses);
  try {
    await f.save('domain:d1', 'Domain 1');
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const select = f.page.getByLabel('Object status');
    await select.waitFor();
    const options = await select.locator('option').evaluateAll(list => list.map(option => ({value: option.value,
      dot: option.querySelector('.status-circle') && getComputedStyle(option.querySelector('.status-circle')).backgroundColor})));
    // The saved order comes first; the tones that the list omits follow in the default order.
    assert.deepEqual(options.map(option => option.value), ['', 'orange', 'green', 'red', 'purple', 'yellow', 'grey']);
    assert.deepEqual(options.slice(1, 4).map(option => option.dot), ['rgb(208, 102, 26)', 'rgb(34, 165, 89)', 'rgb(192, 57, 43)']);
    assert.ok(options.slice(4).every(option => option.dot && option.dot !== 'rgba(0, 0, 0, 0)'), JSON.stringify(options));
    await select.click();
    const listed = f.page.locator('.status-option[data-tone="orange"] .status-circle');
    await listed.waitFor({state: 'visible'});
    const box = await listed.boundingBox();
    assert.ok(box && box.width >= 6 && box.height >= 6, 'the open list shows the dot: ' + JSON.stringify(box));
    await f.page.locator('.status-option[data-tone="green"]').click();
    assert.equal(await select.inputValue(), 'green');
    const closed = await select.evaluate(node => ({tone: node.dataset.tone, image: getComputedStyle(node).backgroundImage}));
    assert.equal(closed.tone, 'green');
    assert.match(closed.image, /radial-gradient\(.*rgb\(34, 165, 89\)/);
  } finally { await f.close(); }
});

test('board-filters-and-theme-ac2 theme-reload: the dark theme shows its palette at once and after a reload', async () => {
  // The configured palettes: --bg and --text of the light and the dark theme in board.css.
  const css = readFileSync(path.join(root, 'src/board.css'), 'utf8');
  const palette = selector => {
    const block = css.slice(css.indexOf(selector + '{')).split('}')[0];
    const rgb = name => {
      const hex = block.match(new RegExp(name + ':#([0-9a-f]{3,6});'))[1].replace(/^(.)(.)(.)$/, '$1$1$2$2$3$3');
      return 'rgb(' + [0, 2, 4].map(i => parseInt(hex.slice(i, i + 2), 16)).join(', ') + ')';
    };
    return {background: rgb('--bg'), foreground: rgb('--text')};
  };
  const light = palette(':root'), dark = palette(':root[data-theme=dark]');
  assert.notDeepEqual(light, dark);
  const f = await fixture(domains);
  try {
    const rendered = () => f.page.evaluate(() => ({background: getComputedStyle(document.body).backgroundColor,
      foreground: getComputedStyle(document.documentElement).color}));
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.locator('#theme').waitFor();
    assert.notEqual(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.deepEqual(await rendered(), light);
    await f.page.locator('#theme').click();
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.deepEqual(await rendered(), dark, 'the switch shows the dark palette at once');
    await f.page.reload();
    await f.page.locator('#theme').waitFor();
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.deepEqual(await rendered(), dark, 'the reload keeps the dark palette');
  } finally { await f.close(); }
});
