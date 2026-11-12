// Chromium checks of the History tab, the theme colours of the status dots and the lanes of
// the tree diagram. Each test starts its own browser_server fixture.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const {fixture, root, testType} = require('./board_fixture.cjs');

const domains = {domain: {title: 'Domains', role: 'group', status:'children', root: true, parents: [], badge: 'D'}};

test('ea0ca1c0 history-tab: History pages 25 rows newest first with Load more, dates, Open only and Oldest first', async () => {
  const f = await fixture(domains);
  try {
    // Every entry before the 30 new saves is processed: the first one is entry 1.
    for (let open = await f.mcp('joppa_history', {open: true}); open.items.length; open = await f.mcp('joppa_history', {open: true}))
      await f.mcp('joppa_history_mark', {position: open.items[0].position, reason: 'Fixture setup'});
    const setup = (await f.mcp('joppa_history', {})).total;
    assert.ok(setup >= 1 && setup <= 20, 'setup entries: ' + setup);
    for (let n = 1; n <= 30; n++) await f.save('domain:d' + n, 'Domain ' + n);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const tab = f.page.getByRole('tab', {name: 'History'});
    await tab.click();
    const panel = f.page.locator('.history-collection');
    const rows = panel.locator('.history-entry');
    const count = n => f.page.waitForFunction(n => document.querySelectorAll('.history-collection .history-entry').length === n, n);
    const positions = () => rows.evaluateAll(list => list.map(row => Number(row.dataset.position)));
    const processed = () => rows.locator('.note-state:not(.history-at)').filter({hasText: 'Processed'}).count();
    await count(25);
    assert.deepEqual(await f.page.locator('.collection-tabs [role=tab]').allTextContents(), ['Domains', 'History']);
    // The 25 newest entries come first, and all of them are open.
    const first = await positions();
    assert.deepEqual(first, [...first].sort((a, b) => b - a));
    assert.equal(await processed(), 0);
    const dates = await rows.locator('.history-at').allTextContents();
    assert.equal(dates.length, 25);
    for (const date of dates) assert.match(date, /^\d{1,2} [A-Z][a-z]{2} \d{4}, \d{2}:\d{2}$/);
    const more = panel.getByRole('button', {name: 'Load more'});
    await more.click();
    await count(setup + 30);
    assert.equal(await more.count(), 0);
    const all = await positions();
    assert.deepEqual(all, [...all].sort((a, b) => b - a));
    assert.equal(new Set(all).size, setup + 30);
    assert.equal(all.at(-1), 1);
    assert.equal(await rows.last().locator('.note-state:not(.history-at)').innerText(), 'Processed');
    // Open only starts the list again with the 25 newest open entries.
    await panel.getByLabel('Open only').check();
    await f.page.waitForFunction(() => document.querySelectorAll('.history-collection .history-entry').length === 25
      && document.querySelector('.history-collection .collection-items > button'));
    assert.equal(await processed(), 0);
    // Oldest first starts it again from the oldest open entry.
    await panel.getByLabel('Oldest first').check();
    await f.page.waitForFunction(first => document.querySelector('.history-collection .history-entry')?.dataset.position === String(first), setup + 1);
    assert.equal(await processed(), 0);
  } finally { await f.close(); }
});

test('b3ecb4e3 dot-theme: each dot takes the light and the dark colour of its tone', async () => {
  const types = {
    requirement: {title: 'Requirements', role: 'requirement', status:'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status:'children', parents: ['requirement']},
    test: testType(['ac']),
  };
  const statuses = [{tone: 'green', light: '#22a559', dark: '#2fbf6a'}, {tone: 'blue', light: '#4a7fb5', dark: '#9cc3ea'}];
  const f = await fixture(types, statuses);
  try {
    await f.save('requirement:r', 'Requirement');
    await f.save('ac:g', 'Green criterion', 'requirement:r');
    await f.save('ac:n', 'Blue criterion', 'requirement:r');
    // Both criteria have a test node; only the test of ac:g has a result, so ac:n stays blue.
    await f.addTest('ac:g', 'tests/x.rs > x');
    await f.addTest('ac:n', 'tests/x.rs > n');
    await f.mcp('joppa_record_results', {commit: 'c0ffee3', results: [{test: 'tests/x.rs > x', status: 'pass'}]});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const r = f.page.locator('#tree .common-object[data-object-address="requirement:r"]');
    await r.locator(':scope > summary').click();
    const dot = address => r.locator('.common-object[data-object-address="' + address + '"] > summary .status-circle').first();
    await dot('ac:n').waitFor();
    // A dot takes its colour when the board data has set the tone colours.
    await f.page.waitForFunction(() => [...document.querySelectorAll('#tree .status-circle[data-tone]')].every(node => getComputedStyle(node).backgroundColor !== ''));
    const colours = async () => ({green: await dot('ac:g').evaluate(node => [node.dataset.tone, getComputedStyle(node).backgroundColor]),
      blue: await dot('ac:n').evaluate(node => [node.dataset.tone, getComputedStyle(node).backgroundColor])});
    assert.deepEqual(await colours(), {green: ['green', 'rgb(34, 165, 89)'], blue: ['blue', 'rgb(74, 127, 181)']});
    await f.page.locator('#theme').click();
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.deepEqual(await colours(), {green: ['green', 'rgb(47, 191, 106)'], blue: ['blue', 'rgb(156, 195, 234)']});
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
    const options = await select.locator('option').evaluateAll(list => list.map(option => ({value: option.value, label: option.textContent.trim(),
      dot: option.querySelector('.status-circle') && getComputedStyle(option.querySelector('.status-circle')).backgroundColor})));
    // The saved order comes first; the tones that the list omits follow in the default order.
    // The purple tone of archived nodes is not listed: the board lists only present nodes.
    assert.deepEqual(options.map(option => option.value), ['', 'orange', 'green', 'red', 'yellow', 'blue']);
    // The server gives the label of each tone (status_colors::LABELS).
    assert.deepEqual(options.slice(1).map(option => option.label), ['No tests', 'Checks passed', 'Checks failed', 'Input awaits processing', 'Awaits checks']);
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
