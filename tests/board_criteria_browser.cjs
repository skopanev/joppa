// Chromium checks of the History tab, the theme colours of the status dots and the lanes of
// the tree diagram. Each test starts its own browser_server fixture.
const {test, before, after} = require('node:test');
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {once} = require('node:events');
const path = require('node:path');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

const root = path.join(__dirname, '..');
let browser, helper;
before(async () => {
  assert.ok(process.env.JOPPA_TEST_DATABASE_URL, 'isolated test database required');
  const build = spawnSync('bun', ['run', 'test:rust', '--test', 'browser_server', '--no-run', '--message-format=json'],
    {cwd: root, encoding: 'utf8'});
  assert.equal(build.status, 0, build.stderr);
  const artifact = build.stdout.split('\n').filter(Boolean).map(line => JSON.parse(line))
    .find(value => value.reason === 'compiler-artifact' && value.target.name === 'browser_server' && value.executable);
  assert.ok(artifact, 'browser fixture executable required');
  helper = artifact.executable;
  browser = await chromium.launch({headless: true});
});
after(async () => { await browser?.close(); });

/// Start the fixture server, configure `types` (and `statuses`) and open a signed-in page.
async function fixture(types, statuses) {
  const child = spawn(helper, ['--ignored', '--exact', 'browser_server', '--nocapture'],
    {cwd: root, env: {...process.env, JOPPA_BROWSER_HELPER: '1'}, stdio: ['pipe', 'pipe', 'pipe']});
  let output = '', errors = '';
  child.stderr.on('data', chunk => { errors += chunk; });
  const live = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.stdin.end(); reject(Error('browser server timed out')); }, 45000);
    child.on('exit', code => { clearTimeout(timer); reject(Error('browser fixture exited ' + code + ': ' + errors)); });
    child.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/JOPPA_BROWSER_READY (\{[^\n]+\})\n/);
      if (match) { clearTimeout(timer); resolve(JSON.parse(match[1])); }
    });
  });
  const api = async (suffix, body, token = live.agent) => {
    const response = await fetch(live.root + '/api/workspaces/' + live.ws + suffix, {method: body === undefined ? 'GET' : 'POST',
      headers: {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}, body: body && JSON.stringify(body)});
    const value = await response.json();
    assert.equal(response.status, 200, JSON.stringify(value));
    return value;
  };
  const command = (value, token) => api('/commands', {request_id: crypto.randomUUID(), command: value}, token);
  const mcp = async (name, args) => {
    const response = await fetch(live.root + '/mcp', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + live.agent, 'Content-Type': 'application/json'},
      body: JSON.stringify({jsonrpc: '2.0', id: 1, method: 'tools/call', params: {name, arguments: {workspace: live.ws, ...args}}})});
    const reply = await response.json();
    assert.notEqual(reply.result?.isError, true, JSON.stringify(reply));
    return JSON.parse(reply.result.content[0].text);
  };
  const save = (address, title, parent = null) => {
    const [kind, id] = address.split(':');
    return command({op: 'save_object', kind, id, revision: 0, parent, data: {title, owner: 'owner', body: 'Fixture text'}, reason: 'Define the browser fixture'});
  };
  const context = await browser.newContext({reducedMotion: 'reduce', colorScheme: 'light', viewport: {width: 1280, height: 900}});
  await context.addCookies([{name: 'joppa_session', value: live.cookie, url: live.root}]);
  const page = await context.newPage(), pageErrors = [];
  page.on('pageerror', error => pageErrors.push(error.message));
  const close = async () => {
    try { await context.close(); const exit = once(child, 'exit'); child.stdin.end(); assert.equal((await exit)[0], 0, errors); }
    finally { assert.deepEqual(pageErrors, []); }
  };
  try {
    await command({op: 'member', id: 'bot', role: 'agent'}, live.owner);
    await command({op: 'member', id: 'alex', role: 'human'}, live.owner);
    const tree = await api('/tree');
    await command({op: 'set_tree_schema', revision: tree.current_revision, types, ...(statuses ? {statuses} : {}), reason: 'Configure the browser tree'});
  } catch (error) { await close(); throw error; }
  return {live, page, api, command, mcp, save, close};
}

const domains = {domain: {title: 'Domains', role: 'group', root: true, parents: [], badge: 'D'}};

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
    requirement: {title: 'Requirements', role: 'requirement', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', parents: ['requirement']},
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
    alpha: {title: 'Alpha', role: 'group', root: true, parents: []},
    beta: {title: 'Beta', role: 'group', root: true, parents: []},
    child: {title: 'Child', role: 'document', parents: ['alpha', 'beta']},
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
