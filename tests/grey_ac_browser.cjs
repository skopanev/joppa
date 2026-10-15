const {test, describe, before, after} = require('node:test');
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {writeFileSync} = require('node:fs');
const {once} = require('node:events');
const path = require('node:path');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

const root = path.join(__dirname, '..');
const types = {
  domain: {title: 'Domains', role: 'group', root: true, parents: [], badge: 'DOM'},
  capability: {title: 'Capabilities', role: 'group', parents: ['domain'], badge: 'CAP'},
  requirement: {title: 'Requirements', role: 'requirement', parents: ['capability'], badge: 'REQ'},
  ac: {title: 'Criteria', role: 'ac', parents: ['requirement'], badge: 'AC'},
};
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

async function server() {
  const child = spawn(helper, ['--ignored', '--exact', 'browser_server', '--nocapture'],
    {cwd: root, env: {...process.env, JOPPA_BROWSER_HELPER: '1'}, stdio: ['pipe', 'pipe', 'pipe']});
  let output = '', errors = '';
  child.stderr.on('data', chunk => { errors += chunk; });
  const ready = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.stdin.end(); reject(Error('browser server timed out')); }, 15000);
    child.on('error', error => { clearTimeout(timer); reject(error); });
    child.on('exit', code => { clearTimeout(timer); reject(Error('browser fixture exited ' + code + ': ' + errors)); });
    child.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/JOPPA_BROWSER_READY (\{[^\n]+\})\n/);
      if (match) { clearTimeout(timer); resolve(JSON.parse(match[1])); }
    });
  });
  return {...ready, stop: async () => {
    const exit = once(child, 'exit');
    child.stdin.end();
    const [code] = await exit;
    assert.equal(code, 0, 'browser fixture cleanup failed: ' + errors);
  }};
}

async function fixture(records, configured = types, contextOptions = {}) {
  const live = await server();
  const context = await browser.newContext({reducedMotion: 'reduce', ...contextOptions});
  const page = await context.newPage(), requests = [], errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('response', response => {
    if (!response.url().includes('/api/')) return;
    const entry = {url: response.url(), status: response.status()};
    entry.body = response.json().then(value => { entry.value = value; }).catch(() => {});
    requests.push(entry);
  });
  await context.addCookies([{name: 'joppa_session', value: live.cookie, url: live.root}]);
  await context.addInitScript(() => {
    window.copiedIds = [];
    Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => window.copiedIds.push(text)}});
  });
  const api = async (suffix, body, token = live.agent) => {
    const response = await fetch(live.root + '/api/workspaces/' + live.ws + suffix,
      {method: body === undefined ? 'GET' : 'POST', headers: {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'},
        body: body === undefined ? undefined : JSON.stringify(body)});
    const value = await response.json();
    assert.equal(response.status, 200, JSON.stringify(value));
    return value;
  };
  const command = (value, owner = false) => api('/commands', {request_id: crypto.randomUUID(), command: value}, owner ? live.owner : live.agent);
  const mcp = async (name, arguments_, token = live.agent) => {
    const response = await fetch(live.root + '/mcp', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'},
      body: JSON.stringify({jsonrpc: '2.0', id: 1, method: 'tools/call', params: {name, arguments: {...(arguments_.item_id ? {} : {workspace: live.ws}), ...arguments_}}})});
    assert.equal(response.status, 200);
    const reply = await response.json();
    assert.equal(reply.error, undefined, JSON.stringify(reply));
    assert.notEqual(reply.result.isError, true, JSON.stringify(reply));
    return JSON.parse(reply.result.content[0].text);
  };
  const save = (address, record, revision = 0, useMcp = false) => {
    const [kind, id] = address.split(':');
    const value = {op: 'save_object', kind, id, revision, parent: record.parent || null,
      data: {title: record.title, owner: 'owner', body: record.body || 'Current source'}, reason: 'Define the browser fixture'};
    if (configured[kind]?.question) value.question_checked = configured[kind].question;
    return useMcp ? mcp('joppa_command', {request_id: crypto.randomUUID(), command: value}) : command(value);
  };
  const upload = (commit, order, cases, full = true) => {
    const xml = value => value.replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll('<', '&lt;');
    const rows = cases.map(([name, message]) => '<testcase name="' + xml(name) + '">' +
      (message ? '<failure message="' + xml(message) + '"/>' : '') + '</testcase>').join('');
    return api('/reports', {stamp: {commit, order, branch: 'master', default_branch: true, dirty: false, full},
      junit: '<testsuites><testsuite name="checks.cjs">' + rows + '</testsuite></testsuites>'});
  };
  const bind = (address, tests) => command({op: 'set_ac_tests', target: address,
    tests: tests.map(name => 'checks.cjs > ' + name), reason: 'Use the exact uploaded checks'});
  const open = async (address, selectWorkspace = true) => {
    const response = await page.goto(live.root + '/?' + (selectWorkspace ? 'workspace=' + live.ws + '&' : '') + 'object=' + encodeURIComponent(address));
    assert.equal(response.status(), 200);
    const card = page.locator('.object-page > .common-object[data-object-address="' + address + '"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
    await Promise.all(requests.map(entry => entry.body));
    return card;
  };
  const close = async () => {
    try { await context.close(); await live.stop(); } finally { assert.deepEqual(errors, []); }
  };
  try {
    await command({op: 'member', id: 'bot', role: 'agent'}, true);
    await command({op: 'member', id: 'alex', role: 'human'}, true);
    const schema = await api('/tree');
    await command({op: 'set_tree_schema', revision: schema.current_revision, types: configured, reason: 'Configure the browser tree'});
    for (const [address, record] of Object.entries(records)) await save(address, record);
  } catch (error) { await close(); throw error; }
  return {page, requests, open, close, api, command, save, upload, bind, mcp, live};
}

const criterionRecords = {
  'domain:d': {title: 'Domain'},
  'capability:c': {title: 'Capability', parent: 'domain:d'},
  'requirement:r': {title: 'Requirement', parent: 'capability:c'},
  'ac:A': {title: 'Acceptance condition', parent: 'requirement:r'},
};

describe('tests/grey_ac_browser.cjs', () => {
test('suggested-method: the guide reads custom workspace levels and questions', async () => {
  const configured = {
    ledger: {title: 'Ledgers', role: 'document', root: true, parents: [], question: 'Which records belong together?'},
    entry: {title: 'Entries', role: 'document', parents: ['ledger', 'entry'], question: 'Which fact does this entry preserve?'},
    observation: {title: 'Observations', role: 'document', root: true, parents: ['*'], question: 'Which source provided this observation?'},
  };
  const f = await fixture({'ledger:one': {title: 'Product ledger'}, 'entry:one': {title: 'Recorded fact', parent: 'ledger:one'}}, configured);
  try {
    await f.open('ledger:one');
    const link = f.page.locator('#suggested-method-link');
    assert.equal(await link.innerText(), 'How it works ↗');
    const url = new URL(await link.getAttribute('href'), f.live.root);
    assert.equal(url.searchParams.get('workspace'), f.live.ws);
    assert.equal((await f.page.goto(url.href)).status(), 200);
    await f.page.locator('#workspace-levels #level-entry').waitFor();
    assert.equal(await f.page.locator('h1').innerText(), 'How it works');
    assert.equal(await f.page.title(), 'How it works · joppa');
    assert.ok((await f.page.locator('h1 + p').innerText()).startsWith('This is a suggested method.'));
    assert.deepEqual(await f.page.locator('.brand-word').evaluateAll(words => words.map(word => {
      const style = getComputedStyle(word);
      return {text: word.textContent, font: style.fontFamily, weight: style.fontWeight,
        size: Math.round(parseFloat(style.fontSize) / parseFloat(getComputedStyle(word.parentElement).fontSize) * 100) / 100,
        height: parseFloat(style.lineHeight) / parseFloat(style.fontSize), alignment: style.verticalAlign,
        color: style.color, dot: getComputedStyle(word.firstElementChild).color};
    })), Array.from({length: 2}, () => ({text: 'joppa.', font: 'Verdana, sans-serif', weight: '400',
      size: 1.15, height: 1, alignment: 'baseline', color: 'rgb(34, 34, 34)', dot: 'rgb(75, 120, 88)'})));
    assert.equal(await f.page.locator('#default-tree').isVisible(), false);
    assert.deepEqual(await f.page.locator('#workspace-levels h3').allTextContents(), ['Ledgers', 'Entries', 'Observations']);
    for (const [kind, type] of Object.entries(configured)) {
      const level = f.page.locator('#level-' + kind + ' > .level-card');
      assert.ok((await level.innerText()).includes(type.question));
    }
    assert.equal(await f.page.locator('#level-ledger > ol > #level-entry').count(), 1);
    assert.equal(await f.page.locator('#level-entry a[href="#level-entry"]').innerText(), 'Entries');
    assert.ok((await f.page.locator('#level-observation').innerText()).includes('Any level'));
    assert.ok(f.requests.some(request => new URL(request.url).pathname === '/api/workspaces/' + f.live.ws + '/tree'));
    f.requests.length = 0;
    await f.page.goto(f.live.root + '/guide');
    await f.page.waitForLoadState('networkidle');
    assert.equal(await f.page.locator('#default-tree').isVisible(), true);
    assert.equal(await f.page.locator('#workspace-levels').isVisible(), false);
    assert.equal(await f.page.locator('#level-intro').innerText(),
      'Suggested default tree: Domain → Capability → REQ → AC. Each workspace schema sets its own levels and the question each level answers.');
    assert.equal(f.requests.length, 0);
    const response = await f.page.goto(f.live.root + '/guide?workspace=missing');
    assert.equal(response.status(), 200);
    await f.page.waitForFunction(() => document.getElementById('level-intro').textContent === 'Workspace not found');
    assert.equal(await f.page.locator('#default-tree').isVisible(), false);
    assert.equal(await f.page.locator('#workspace-levels .level-card').count(), 0);
  } finally { await f.close(); }
});

test('type-question: the schema question opens from each type badge', async () => {
  const question = 'Which observation does this record preserve?';
  const configured = {record: {title: 'Records', role: 'document', root: true, parents: ['record'], badge: 'REC', question}};
  const f = await fixture({'record:root': {title: 'Source'}, 'record:child': {title: 'Detail', parent: 'record:root'}}, configured);
  try {
    const card = await f.open('record:root');
    const child = card.locator('.common-object[data-object-address="record:child"]');
    await child.waitFor();
    const badge = child.locator(':scope > summary > .type-badge');
    assert.equal(await badge.getAttribute('title'), 'Records\n' + question);
    await badge.hover();
    assert.equal(await f.page.locator('.status-hint').innerText(), 'Records\n' + question);
    await badge.click();
    const panel = f.page.locator('.type-question:popover-open');
    await panel.waitFor();
    assert.equal(await panel.innerText(), question);
    assert.equal(await badge.getAttribute('aria-expanded'), 'true');
    assert.equal(await child.getAttribute('open'), null);
    await f.page.keyboard.press('Escape');
    await f.page.waitForFunction(() => !document.querySelector('.type-question:popover-open'));
    await badge.focus();
    await f.page.keyboard.press('Enter');
    await panel.waitFor();
    await badge.click();
    await f.page.waitForFunction(() => !document.querySelector('.type-question:popover-open'));
    await f.page.setViewportSize({width: 390, height: 844});
    const rootBadge = card.locator(':scope > summary > .type-badge');
    await rootBadge.click();
    await panel.waitFor();
    assert.equal(await panel.innerText(), question);
    const box = await panel.boundingBox();
    assert.ok(box.x >= 0 && box.x + box.width <= 390 && box.y >= 0 && box.y + box.height <= 844);
  } finally { await f.close(); }
});

test('theme: the saved choice overrides or follows the system theme', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}}, types, {colorScheme: 'dark'});
  const background = () => f.page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const toggle = f.page.locator('button#theme');
  try {
    await f.open('domain:d');
    assert.equal(await f.page.locator('select#theme').count(), 0);
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), null);
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await toggle.getAttribute('aria-label'), 'Switch to light theme');
    assert.equal(await toggle.locator('.theme-sun').isVisible(), true);
    assert.equal(await toggle.locator('.theme-moon').isVisible(), false);
    await f.page.emulateMedia({colorScheme: 'light'});
    await f.page.waitForFunction(() => document.getElementById('theme').dataset.dark === 'false');
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await toggle.getAttribute('aria-label'), 'Switch to dark theme');
    assert.equal(await toggle.locator('.theme-moon').isVisible(), true);
    await f.page.emulateMedia({colorScheme: 'dark'});
    await toggle.click();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'light');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await toggle.getAttribute('aria-label'), 'Switch to dark theme');
    await toggle.focus();
    await f.page.keyboard.press('Space');
    await f.page.emulateMedia({colorScheme: 'light'});
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'dark');
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await f.page.setViewportSize({width: 390, height: 844});
    await toggle.click();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.locator('meta[name="theme-color"]').getAttribute('content'), '#fffcf5');
    const box = await toggle.boundingBox();
    assert.ok(box.width >= 44 && box.height >= 44 && box.x >= 0 && box.x + box.width <= 390);
    await f.page.evaluate(() => localStorage.removeItem('theme'));
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    await f.page.emulateMedia({colorScheme: 'dark'});
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), null);
    await f.page.evaluate(() => localStorage.setItem('joppa-theme', 'light'));
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'light');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('joppa-theme')), null);
    await toggle.click();
    await f.page.evaluate(() => localStorage.setItem('joppa-theme', 'light'));
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'dark');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('joppa-theme')), null);
  } finally { await f.close(); }
});

test('account-menu: sign out requires the user menu and ends the real session', async () => {
  const configured = {...types,
    incoming: {title: 'Inbox', role: 'document', root: true, parents: [], inbox: true},
    audience: {title: 'Consumers', role: 'document', root: true, parents: []},
  };
  const f = await fixture({'domain:d': {title: 'Domain'},
    'incoming:one': {title: 'Incoming change'}, 'audience:one': {title: 'Product user'}}, configured);
  try {
    await f.open('domain:d');
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.locator('#identity').focus();
    await f.page.keyboard.press('Enter');
    assert.equal(await f.page.locator('#logout').isVisible(), true);
    assert.deepEqual(await f.page.locator('.account-options > *').allTextContents(), ['Sign out']);
    await f.page.keyboard.press('Escape');
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.locator('#identity').click();
    await f.page.locator('.brand').click();
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    await f.page.locator('#tab-domain').waitFor();
    const closedMenu = async () => {
      assert.equal(await f.page.locator('#account-menu').getAttribute('open'), null);
      assert.equal(await f.page.locator('.account-options').isVisible(), false);
      assert.equal(await f.page.locator('#logout').isVisible(), false);
    };
    await f.page.locator('#identity').click();
    await f.page.locator('#tab-incoming').click();
    await f.page.locator('#collection-incoming .common-object').waitFor();
    await closedMenu();
    await f.page.locator('#identity').click();
    await f.page.locator('#tab-audience').click();
    await f.page.locator('#collection-audience .common-object').waitFor();
    await closedMenu();
    await f.page.locator('#identity').click();
    await f.page.locator('#tab-domain').focus();
    await f.page.keyboard.press('Home');
    await closedMenu();
    await f.page.evaluate(() => history.pushState(null, '', '#menu-history-check'));
    await f.page.locator('#identity').click();
    await f.page.evaluate(() => history.back());
    await f.page.waitForFunction(() => location.hash === '' && !document.getElementById('account-menu').open);
    await closedMenu();
    await f.page.locator('#identity').click();
    await f.open('domain:d');
    await closedMenu();
    await f.page.goBack();
    await f.page.locator('#tab-domain').waitFor();
    await closedMenu();
    await f.page.setViewportSize({width: 390, height: 844});
    await f.page.locator('#identity').click();
    const menu = await f.page.locator('.account-options').boundingBox();
    assert.ok(menu.x >= 0 && menu.x + menu.width <= 390);
    const logout = f.page.waitForResponse(r => new URL(r.url()).pathname === '/auth/logout');
    await f.page.locator('#logout').click();
    assert.equal((await logout).status(), 200);
    await f.page.locator('#login-button').waitFor({state: 'visible'});
    assert.equal(await f.page.locator('.app-header').isVisible(), false);
    assert.ok(!(await f.page.context().cookies()).some(cookie => cookie.name === 'joppa_session'));
  } finally { await f.close(); }
});

test('direct-child-counts: each row uses the shared next-level count', async () => {
  const configured = {
    bundle: {title: 'Bundles', role: 'group', root: true, parents: []},
    branch: {title: 'Branches', role: 'group', parents: ['bundle']},
    sheet: {title: 'Sheets', role: 'requirement', parents: ['branch']},
    proof: {title: 'Proofs', role: 'ac', parents: ['sheet']},
    extra: {title: 'Extra', role: 'document', parents: ['bundle']},
  };
  const records = {
    'bundle:root': {title: 'Root'},
    'branch:good': {title: 'Good', parent: 'bundle:root'},
    'branch:empty': {title: 'Empty', parent: 'bundle:root'},
    'branch:bad': {title: 'Bad', parent: 'bundle:root'},
    'extra:raw': {title: 'Unrelated', parent: 'bundle:root'},
    'sheet:one': {title: 'First sheet', parent: 'branch:good'},
    'sheet:two': {title: 'Second sheet', parent: 'branch:good'},
    'sheet:bad': {title: 'Failed sheet', parent: 'branch:bad'},
    'proof:a': {title: 'First proof', parent: 'sheet:one'},
    'proof:b': {title: 'Second proof', parent: 'sheet:two'},
    'proof:bad': {title: 'Failed proof', parent: 'sheet:bad'},
  };
  const f = await fixture(records, configured);
  try {
    await f.bind('proof:a', ['pass']); await f.bind('proof:b', ['pass']); await f.bind('proof:bad', ['fail']);
    await f.upload('cccccccc', 1, [['pass'], ['fail', 'failed check']]);
    const card = await f.open('bundle:root');
    assert.equal(await card.locator(':scope > summary .verification-chip').innerText(), '1 / 3 Branches');
    assert.equal(await card.locator('.common-object[data-object-address="branch:good"] > summary .verification-chip').innerText(), '2 / 2 Sheets');
    assert.equal(await card.locator('.common-object[data-object-address="branch:empty"] > summary .verification-chip').innerText(), '0 Sheets');
    assert.equal(f.requests.filter(r => new URL(r.url).pathname.endsWith('/tests')).length, 0);
  } finally { await f.close(); }
});

test('child-heading: a repeated parent title shows the configured type and status', async () => {
  const f = await fixture({'domain:d': {title: 'Shared title'},
    'capability:c': {title: 'Shared title', parent: 'domain:d'},
    'capability:other': {title: 'Different title', parent: 'domain:d'}});
  try {
    const card = await f.open('domain:d');
    const titles = card.locator(':scope > .object-body > .object-children > .common-object > summary > .node-title');
    await titles.nth(1).waitFor();
    assert.deepEqual(await titles.allTextContents(), ['Capabilities · Current document', 'Different title']);
  } finally { await f.close(); }
});

test('lazy-children-ac1: a capability reads only its twenty direct requirements', async () => {
  const records = {'domain:d': {title: 'Domain'}, 'capability:X': {title: 'Capability', parent: 'domain:d'}};
  for (let index = 0; index < 20; index++) {
    const id = String(index).padStart(2, '0');
    records['requirement:r' + id] = {title: 'Requirement ' + id, parent: 'capability:X'};
    records['ac:a' + id] = {title: 'Criterion ' + id, parent: 'requirement:r' + id};
  }
  const f = await fixture(records);
  try {
    const card = await f.open('capability:X');
    await card.locator(':scope > .object-body > .object-children > .common-object').nth(19).waitFor();
    await Promise.all(f.requests.map(entry => entry.body));
    const children = f.requests.filter(r => new URL(r.url).searchParams.get('parent') === 'capability:X');
    assert.equal(children.length, 1);
    assert.equal(children[0].value.items.length, 20);
    assert.ok(children[0].value.items.every(item => item.kind === 'requirement'));
    assert.equal(f.requests.filter(r => new URL(r.url).searchParams.get('address')?.startsWith('ac:')).length, 0);
    assert.equal(f.requests.filter(r => new URL(r.url).searchParams.get('address')?.startsWith('requirement:')).length, 0);
  } finally { await f.close(); }
});

test('old-results-history-ac2: the criterion shows both results and the failure message', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const f = await fixture(criterionRecords);
  try {
    await f.bind('ac:A', ['X', 'Y']);
    await f.upload(commit, 1, [['X'], ['Y', 'expected 403, got 200']]);
    const card = await f.open('ac:A'), group = card.locator('.object-tests');
    await group.locator(':scope > summary').click();
    await group.locator('.ac-check').nth(1).waitFor();
    const rows = group.locator('.ac-check');
    assert.equal(await rows.count(), 2);
    assert.match(await rows.nth(0).innerText(), /checks.cjs > X\s+pass · commit ccccccc/);
    assert.match(await rows.nth(1).innerText(), /checks.cjs > Y\s+fail · commit ccccccc/);
    assert.equal(await rows.nth(1).locator('pre').innerText(), 'expected 403, got 200');
  } finally { await f.close(); }
});

test('stable-url-ac1: the original URL keeps the item ID after rename and move', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'},
    'capability:old': {title: 'Old capability', parent: 'domain:d'},
    'capability:new': {title: 'New capability', parent: 'domain:d'},
    'requirement:r': {title: 'Original title', parent: 'capability:old'}});
  try {
    let card = await f.open('requirement:r');
    const originalUrl = f.page.url(), original = await f.api('/objects?address=requirement:r');
    await card.locator(':scope > summary .copy').click();
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), [original.item_id]);
    await f.save('requirement:r', {title: 'Renamed requirement', parent: 'capability:new'}, 1);
    const response = await f.page.goto(originalUrl);
    assert.equal(response.status(), 200);
    card = f.page.locator('.object-page > .common-object[data-object-address="requirement:r"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
    assert.equal(await card.locator(':scope > summary .node-title').innerText(), 'Renamed requirement');
    await card.locator(':scope > summary .copy').click();
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), [original.item_id]);
    const current = await f.api('/objects?address=requirement:r');
    assert.equal(current.document.parent, 'capability:new');
    assert.equal(current.item_id, original.item_id);
  } finally { await f.close(); }
});

test('time-not-stale-ac2: the oldest test commit shows five newer commits', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const f = await fixture(criterionRecords);
  try {
    await f.bind('ac:A', ['X', 'Y']);
    await f.upload(commit, 1, [['X'], ['Y']]);
    for (let order = 2; order <= 6; order++) await f.upload(String(order).repeat(40), order, [['other']], false);
    const card = await f.open('ac:A'), group = card.locator('.object-tests');
    await group.locator(':scope > summary').click();
    await group.locator('.ac-check').nth(1).waitFor();
    assert.match(await group.locator(':scope > .node-meta').innerText(), /commit ccccccc · 5 commits behind/);
  } finally { await f.close(); }
});

test('done-is-not-green-ac3: a done Task under a green criterion shows Checks passed', async () => {
  const configured = {...types, task: {title: 'Tasks', role: 'task', parents: ['ac']}};
  const f = await fixture(criterionRecords, configured);
  try {
    await f.bind('ac:A', ['X']);
    await f.command({op: 'save_object', kind: 'task', id: 'T', revision: 0, parent: 'ac:A',
      data: {title: 'Implement the criterion', owner: 'owner', contract: 'Implement the criterion and record the result', purpose: 'implementation'},
      reason: 'Check completion under a green criterion'});
    await f.command({op: 'claim_task', id: 'task:T', revision: 1});
    await f.command({op: 'complete_task', id: 'task:T', revision: 2, evidence: 'Implemented the criterion'});
    await f.upload('dddddddddddddddddddddddddddddddddddddddd', 1, [['X']]);
    const card = await f.open('ac:A');
    const circle = card.locator('.common-object[data-object-address="task:T"] > summary .status-circle');
    assert.equal(await circle.getAttribute('data-tone'), 'green');
    assert.match(await circle.getAttribute('data-hint'), /^Status: Work complete · Checks passed(\n|$)/);
  } finally { await f.close(); }
});

test('removal-at-once-ac4: a removed member reloads without workspace data', async () => {
  const secret = 'Private source for the browser member';
  const f = await fixture({'domain:d': {title: 'Private domain', body: secret}});
  try {
    const card = await f.open('domain:d', false);
    assert.equal(await card.locator('.object-content').innerText(), secret);
    writeFileSync(f.live.access_file, JSON.stringify({emails: {'alex@example.test': []}}));
    await f.page.reload();
    await f.page.waitForLoadState('networkidle');
    assert.equal(await f.page.locator('.common-object').count(), 0);
    assert.ok(!(await f.page.locator('body').innerText()).includes(secret));
    assert.match(await f.page.locator('#tree').innerText(), /No workspace access/);
    const request = f.page.context().request;
    const response = await request.get(f.live.root + '/api/workspaces/' + f.live.ws);
    const absent = await request.get(f.live.root + '/api/workspaces/absent-' + f.live.ws);
    assert.equal(response.status(), 404);
    assert.equal(absent.status(), 404);
    assert.deepEqual(await response.json(), {error: 'Workspace not found'});
    assert.deepEqual(await response.json(), await absent.json());
  } finally { await f.close(); }
});

test('create-workspace-ac1: alex creates Billing on the settings page and owns it', async () => {
  const f = await fixture({'domain:d': {title: 'Existing domain'}});
  try {
    const request = f.page.context().request;
    const titles = async () => (await (await request.get(f.live.root + '/api/workspaces')).json()).map(w => w.title);
    assert.ok(!(await titles()).includes('Billing'));
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const title = f.page.getByLabel('Workspace title'), create = f.page.getByRole('button', {name: 'Create'});
    // A failed request keeps the form and shows the server error.
    await title.fill('  ');
    await create.click();
    await f.page.getByRole('alert').filter({hasText: 'text must not be empty'}).waitFor();
    assert.equal(await create.isDisabled(), false);
    assert.equal(await title.inputValue(), '  ');
    // Hold the creation to observe the pending state.
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route(f.live.root + '/api/workspaces', async route => { await held; await route.continue(); });
    await title.fill('Billing');
    await create.click();
    assert.equal(await create.isDisabled(), true);
    const board = f.page.waitForResponse(response => /\/api\/workspaces\/[^/]+\/board$/.test(response.url()));
    release();
    const opened = await board;
    assert.equal(opened.status(), 200);
    const id = new URL(f.page.url()).searchParams.get('workspace');
    assert.ok(opened.url().endsWith('/api/workspaces/' + encodeURIComponent(id) + '/board'));
    assert.notEqual(id, f.live.ws);
    assert.equal(await f.page.locator('#workspaces').inputValue(), id);
    // The access rules name only the fixture workspace, so ownership gives alex Billing.
    const list = await (await request.get(f.live.root + '/api/workspaces')).json();
    assert.deepEqual(list.filter(w => w.title === 'Billing'), [{id, title: 'Billing'}]);
    const read = await request.get(f.live.root + '/api/workspaces/' + id);
    assert.equal(read.status(), 200);
    const index = (await read.json()).index;
    assert.equal(index.owner, 'alex');
    assert.deepEqual(Object.keys(index.members), ['alex']);
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + id + '/board')).status(), 200);
    // The fixture owner is not a member of Billing.
    const other = await fetch(f.live.root + '/api/workspaces/' + id, {headers: {Authorization: 'Bearer ' + f.live.owner}});
    assert.equal(other.status, 404);
    assert.deepEqual(await other.json(), {error: 'Workspace not found'});
  } finally { await f.close(); }
});

test('owner-adds-members-ac1: alex adds maria@example.com to an owned workspace in Settings', async () => {
  const f = await fixture({'domain:d': {title: 'Existing domain'}});
  try {
    const request = f.page.context().request;
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Team space'}});
    assert.equal(created.status(), 200);
    const {id} = await created.json();
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const workspace = f.page.getByLabel('Owned workspace'), email = f.page.getByLabel('Google email');
    const add = f.page.getByRole('button', {name: 'Add person'});
    await workspace.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    // Only owned workspaces are offered; the owner of the fixture workspace is not alex.
    const offered = await workspace.locator('option').evaluateAll(options => options.map(option => option.value));
    assert.ok(!offered.includes(f.live.ws));
    await workspace.selectOption(id);
    // A failed request shows the server error.
    await email.fill('not-an-email');
    await add.click();
    await f.page.getByRole('alert').filter({hasText: 'email must be an email address'}).waitFor();
    assert.equal(await add.isDisabled(), false);
    await email.fill('maria@example.com');
    const granted = f.page.waitForResponse(response => response.url().endsWith('/api/email-grants') && response.request().method() === 'POST');
    await add.click();
    assert.equal((await granted).status(), 200);
    const emails = f.page.locator('#grant-emails li');
    await emails.filter({hasText: 'maria@example.com'}).waitFor();
    assert.deepEqual(await emails.allInnerTexts(), ['maria@example.com']);
    assert.equal(await workspace.inputValue(), id);
    const listed = (await (await request.get(f.live.root + '/api/email-grants')).json()).items.filter(item => item.id === id);
    assert.deepEqual(listed, [{id, title: 'Team space', emails: ['maria@example.com']}]);
  } finally { await f.close(); }
});

test('req-has-criteria-ac3: a row without criteria shows zero and a grey dot', async () => {
  const configured = {...types, ac: {...types.ac, title: 'Proofs'}};
  const f = await fixture({'domain:d': {title: 'Domain'},
    'capability:c': {title: 'Capability', parent: 'domain:d'},
    'requirement:empty': {title: 'Empty requirement', parent: 'capability:c'}}, configured);
  try {
    const card = await f.open('capability:c');
    const row = card.locator('.common-object[data-object-address="requirement:empty"] > summary');
    assert.equal(await row.locator('.verification-chip').innerText(), '0 Proofs');
    assert.equal(await row.locator('.status-circle').getAttribute('data-tone'), 'grey');
  } finally { await f.close(); }
});

test('check-list-frozen-ac3: an archived red sibling leaves the requirement grey, not yellow', async () => {
  const f = await fixture({...criterionRecords,
    'ac:B': {title: 'Red sibling', parent: 'requirement:r'},
    'requirement:mixed': {title: 'Mixed requirement', parent: 'capability:c'},
    'ac:M1': {title: 'Passing criterion', parent: 'requirement:mixed'},
    'ac:M2': {title: 'Criterion without tests', parent: 'requirement:mixed'}});
  try {
    await f.bind('ac:A', ['X']); await f.bind('ac:B', ['Y']); await f.bind('ac:M1', ['X']);
    await f.upload('eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', 1, [['X'], ['Y', 'failed check']]);
    await f.command({op: 'archive_object', target: 'ac:B', revision: 1, reason: 'Drop the red criterion'});
    const card = await f.open('capability:c');
    const r = card.locator('.common-object[data-object-address="requirement:r"] > summary');
    assert.equal(await r.locator('.verification-chip').innerText(), '1 Criteria');
    assert.equal(await r.locator('.status-circle').getAttribute('data-tone'), 'grey');
    const mixed = card.locator('.common-object[data-object-address="requirement:mixed"] > summary');
    assert.equal(await mixed.locator('.verification-chip').innerText(), '1 / 2 Criteria');
    assert.equal(await mixed.locator('.status-circle').getAttribute('data-tone'), 'yellow');
  } finally { await f.close(); }
});

test('comments-on-object-ac2: a comment shows its author and server time', async () => {
  const configured = {record: {title: 'Records', role: 'document', root: true, parents: []}};
  const address = 'record:one', text = 'Clarify this object boundary';
  const f = await fixture({[address]: {title: 'A source record'}}, configured,
    {locale: 'en-GB', timezoneId: 'Europe/Berlin'});
  try {
    await f.mcp('joppa_command',
      {request_id: crypto.randomUUID(), command: {op: 'note', target: address, id: 'comment', text}}, f.live.alex);
    const current = await f.api('/objects?address=' + encodeURIComponent(address));
    const comment = current.annotations.comments.comment;
    assert.equal(comment.author, 'alex');
    assert.equal(comment.actor_type, 'person');
    assert.equal(comment.created_at, comment.at);
    const instant = new Date(comment.created_at.replace(' ', 'T').replace(/\+00$/, 'Z'));
    assert.ok(Number.isFinite(instant.getTime()), 'the server timestamp must parse');
    const expected = new Intl.DateTimeFormat('en-GB',
      {dateStyle: 'medium', timeStyle: 'short', timeZone: 'Europe/Berlin'}).format(instant);
    const card = await f.open(address), comments = card.locator('.object-comments');
    assert.equal(await comments.getAttribute('open'), null);
    await comments.locator(':scope > summary').click();
    const line = comments.locator('.note');
    await line.locator('.note-text').waitFor();
    assert.equal(await line.locator('.note-text').innerText(), text);
    assert.equal(await line.locator('.note-meta').innerText(), 'alex · ' + expected);
    assert.equal(await f.page.evaluate(() => Intl.DateTimeFormat().resolvedOptions().timeZone), 'Europe/Berlin');
  } finally { await f.close(); }
});

test('same-revision-everywhere-ac1: web and MCP read the same saved revision', async () => {
  const f = await fixture({'domain:d': {title: 'Shared object', body: 'Body at revision 1'}});
  try {
    await f.open('domain:d');
    for (let revision = 1; revision < 4; revision++) await f.save('domain:d', {title: 'Shared object', body: 'Body at revision ' + (revision + 1)}, revision, true);
    const body = 'Body at revision 5 from MCP';
    const saved = await f.save('domain:d', {title: 'Shared object', body}, 4, true);
    assert.equal(saved.object.revision, 5);
    await f.page.reload();
    const card = f.page.locator('.object-page > .common-object[data-object-address="domain:d"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
    const content = await card.locator('.object-content').innerText();
    const current = await f.api('/objects?address=domain:d');
    const mcp = await f.mcp('joppa_read', {item_id: current.item_id});
    assert.equal(mcp.current_revision, 5);
    const fields = card.locator('.object-details');
    await card.locator(':scope > summary').getByRole('button', {name: 'Properties', exact: true}).click();
    assert.match(await fields.innerText(), /Revision 5/);
    const hash = value => createHash('sha256').update(value).digest('hex');
    assert.equal(hash(content), hash(mcp.document.data.body));
    assert.equal(content, body);
  } finally { await f.close(); }
});

test('blocking-pins-revision-ac1: a blocking link pins the target revision', async () => {
  const f = await fixture({'domain:t': {title: 'Target'}});
  try {
    for (const revision of [1, 2]) await f.save('domain:t', {title: 'Target', body: 'Target at revision ' + (revision + 1)}, revision);
    const link = (id, revision, links) => f.command({op: 'save_object', kind: 'domain', id, revision, parent: null,
      data: {title: 'Source ' + id, owner: 'owner', body: 'Linked source'}, links, reason: 'Link the target'});
    await link('s', 0, [{target: 'domain:t', relation: 'depends_on', revision: 3, blocking: true}]);
    await link('u', 0, [{target: 'domain:t', relation: 'related'}]);
    await link('u', 1, [{target: 'domain:t', relation: 'depends_on', revision: 3}]);
    const read = async suffix => {
      const value = await f.api('/objects?address=' + suffix);
      assert.equal(value.links.length, 1);
      const [item] = value.links;
      assert.deepEqual(item.link, value.document.links[0]);
      assert.deepEqual([item.title, item.kind, item.present, item.metadata_scope], ['Target', 'domain', true, 'current']);
      return value;
    };
    const pinned = await read('domain:s');
    assert.deepEqual([pinned.links[0].blocking, pinned.links[0].revision, pinned.links[0].current_revision], [true, 3, 3]);
    assert.equal(pinned.review_required, false);
    const unpinned = await read('domain:u&revision=1');
    assert.deepEqual([unpinned.historical, unpinned.links[0].blocking, unpinned.links[0].revision], [true, false, null]);
    assert.deepEqual(unpinned.links[0].link, {target: 'domain:t', relation: 'related', revision: null, blocking: false});
    const later = await read('domain:u');
    assert.deepEqual([later.links[0].blocking, later.links[0].revision], [false, 3]);
    await f.save('domain:t', {title: 'Target', body: 'Target at revision 4'}, 3);
    const stale = await read('domain:s');
    assert.deepEqual([stale.links[0].blocking, stale.links[0].revision, stale.links[0].current_revision], [true, 3, 4]);
    assert.deepEqual(stale.document.links, pinned.document.links);
    assert.equal(stale.review_required, true);
  } finally { await f.close(); }
});

test('compact-open-object-ac1: the object row holds the control for its fields', async () => {
  const title = 'Release plan outline';
  assert.equal(title.length, 20);
  const f = await fixture({'domain:X': {title}, 'capability:a': {title: 'First child', parent: 'domain:X'},
    'capability:b': {title: 'Second child', parent: 'domain:X'}});
  const consoleErrors = [];
  f.page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()); });
  try {
    await f.command({op: 'save_object', kind: 'domain', id: 'X', revision: 1, parent: null,
      data: {title, owner: 'owner', body: 'Current source', scope: 'Field scope value'}, reason: 'Add a document field'});
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    const row = X.locator(':scope > summary');
    const control = row.getByRole('button', {name: 'Properties', exact: true});
    const fields = X.locator(':scope > .object-body > .object-secondary > .object-details');
    const children = X.locator(':scope > .object-body > .object-children > .common-object');
    for (const viewport of [null, {width: 390, height: 844}]) {
      if (viewport) await f.page.setViewportSize(viewport);
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      await row.waitFor();
      if (await X.getAttribute('open') === null) await row.click({position: {x: 4, y: 4}});
      await children.nth(1).waitFor();
      assert.equal(await X.getAttribute('open'), '');
      assert.equal(await control.count(), 1);
      assert.equal(await control.evaluate(node => node.tagName + ' ' + node.type), 'BUTTON button');
      // Copy and Revisions stay the last two controls of the row.
      assert.deepEqual((await row.locator('button').evaluateAll(nodes => nodes.map(node => node.className))).slice(-3),
        ['fields-button', 'copy', 'history-button']);
      assert.equal(await X.locator(':scope > .object-body .object-details > summary').count(), 0);
      assert.equal(await fields.evaluate(node => node.tagName), 'DIV');
      assert.deepEqual((await X.locator(':scope > .object-body > .object-secondary > details > summary').allTextContents())
        .filter(text => /details|fields/i.test(text)), []);
      // The control adds no line to the row.
      const shown = (await row.boundingBox()).height;
      // A role query skips a hidden button, so the class selects it while it is hidden.
      const plain = row.locator('.fields-button');
      await plain.evaluate(node => { node.style.display = 'none'; });
      const without = (await row.boundingBox()).height;
      await plain.evaluate(node => { node.style.display = ''; });
      assert.ok(Math.abs(shown - without) <= 1, JSON.stringify({viewport, shown, without}));
      assert.equal(await fields.isVisible(), false);
      assert.equal(await control.getAttribute('aria-expanded'), 'false');
      for (const activate of [() => control.click(), () => control.focus().then(() => f.page.keyboard.press('Enter')),
        () => control.focus().then(() => f.page.keyboard.press('Space'))]) {
        const before = await fields.isVisible();
        await activate();
        assert.equal(await X.getAttribute('open'), '');
        assert.equal(await children.count(), 2);
        assert.equal(await children.nth(0).isVisible(), true);
        assert.equal(await children.nth(1).isVisible(), true);
        assert.equal(await fields.isVisible(), !before);
        assert.equal(await control.getAttribute('aria-expanded'), String(!before));
      }
      assert.equal(await fields.isVisible(), true);
      assert.match(await fields.innerText(), /Revision 2/);
      assert.match(await fields.innerText(), /Field scope value/);
    }
    // The revision dialog has no row control and always shows the fields.
    await row.locator('.history-button').click();
    const dialog = f.page.locator('dialog.notes-dialog');
    const dialogFields = dialog.locator('.object-details');
    await dialogFields.waitFor();
    assert.equal(await X.getAttribute('open'), '');
    assert.equal(await dialog.locator('.object-details > summary').count(), 0);
    assert.equal(await dialogFields.isVisible(), true);
    assert.match(await dialogFields.innerText(), /Revision 2/);
    assert.match(await dialogFields.innerText(), /Field scope value/);
    await dialog.locator('select').selectOption('1');
    await f.page.waitForFunction(() => /Revision 1/.test(document.querySelector('dialog.notes-dialog .object-details')?.innerText));
    assert.equal(await dialogFields.isVisible(), true);
    await dialog.locator('.history-close').click();
    assert.deepEqual(consoleErrors, []);
  } finally { await f.close(); }
});


// A point on the title text, right of the status circle and away from the row controls.
const titlePoint = card => card.locator(':scope > summary').evaluate(summary => {
  const title = summary.querySelector(':scope > .node-title'), range = document.createRange();
  range.selectNodeContents(title.lastChild);
  const text = range.getBoundingClientRect(), box = summary.getBoundingClientRect();
  return {x: text.left + text.width / 2 - box.left, y: text.top + text.height / 2 - box.top};
});


test('compact-open-object-ac2: an open object shows no side line', async () => {
  const f = await fixture({'domain:X': {title: 'Parent X'}, 'capability:Y': {title: 'Child Y', parent: 'domain:X'},
    'requirement:Z': {title: 'Grandchild Z', parent: 'capability:Y'}});
  const row = card => card.locator(':scope > summary');
  const ready = card => card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
  // Every vertical line source that a stylesheet could draw on the left of an open block.
  const lines = card => card.evaluate(node => {
    const found = [];
    const targets = [node, node.querySelector(':scope > summary'), node.querySelector(':scope > .object-body'),
      node.querySelector(':scope > .object-body > .object-children')].filter(Boolean);
    for (const element of targets) {
      const style = getComputedStyle(element);
      if (style.borderLeftStyle !== 'none' && parseFloat(style.borderLeftWidth) > 0 && style.borderLeftColor !== 'rgba(0, 0, 0, 0)')
        found.push(element.className + ' border-left');
      if (style.boxShadow !== 'none') found.push(element.className + ' box-shadow');
      if (style.backgroundImage !== 'none') found.push(element.className + ' background-image');
      for (const pseudo of ['::before', '::after']) {
        const p = getComputedStyle(element, pseudo);
        if (p.content !== 'none' && p.content !== 'normal') found.push(element.className + pseudo);
      }
    }
    const box = node.getBoundingClientRect();
    for (const element of node.querySelectorAll('*')) {
      const r = element.getBoundingClientRect(), style = getComputedStyle(element);
      const painted = style.backgroundColor !== 'rgba(0, 0, 0, 0)' || style.backgroundImage !== 'none';
      if (painted && r.width > 0 && r.width < 4 && r.height > box.height / 2) found.push(element.tagName + ' thin line');
    }
    return found;
  });
  try {
    for (const [width, height] of [[1280, 720], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
      await X.waitFor();
      if (await X.getAttribute('open') === null) await row(X).click({position: {x: 4, y: 4}});
      await ready(X);
      const Y = X.locator(':scope > .object-body > .object-children > .common-object[data-object-address="capability:Y"]');
      await Y.waitFor();
      if (await Y.getAttribute('open') === null) await row(Y).click({position: {x: 4, y: 4}});
      await ready(Y);
      for (const scheme of ['light', 'dark']) {
        await f.page.emulateMedia({colorScheme: scheme});
        assert.deepEqual(await lines(X), [], 'X has no side line at ' + width + ' px, ' + scheme);
        assert.deepEqual(await lines(Y), [], 'Y has no side line at ' + width + ' px, ' + scheme);
        const xLeft = (await row(X).boundingBox()).x, yLeft = (await row(Y).boundingBox()).x;
        assert.ok(yLeft > xLeft, 'the indent shows that Y is inside X: ' + yLeft + ' > ' + xLeft);
      }
    }
  } finally { await f.close(); }
});

const markdownTypes = {...types, task: {title: 'Tasks', role: 'task', parents: ['ac']}};
const markdownBody = ['## Plan', '', '- first', '- second', '', '- [x] done item', '- [ ] open item', '',
  '| Name | Value |', '|---|---|', '| a | b |', '', 'See [docs](https://example.com/docs).', '', '> quoted', '',
  'Use `inline` code.', '', '```', 'let x = 1;', '```', '', 'Given: a', 'When: b'].join('\n');

test('markdown-text-ac1: object text renders as Markdown and titles stay plain', async () => {
  const f = await fixture({'domain:X': {title: '<b>x</b> plan', body: markdownBody},
    'capability:C': {title: 'C', parent: 'domain:X'}, 'requirement:R': {title: 'R', parent: 'capability:C'},
    'ac:A': {title: 'A', parent: 'requirement:R'}}, markdownTypes);
  try {
    await f.api('/commands', {request_id: crypto.randomUUID(), command: {op: 'note', target: 'domain:X', id: 'c1', text: '- one\n- two'}}, f.live.alex);
    await f.command({op: 'save_object', kind: 'task', id: 'T', revision: 0, parent: 'ac:A',
      data: {title: 'T', owner: 'owner', contract: 'Run:\n\n```\ncargo test\n```', purpose: 'implementation'}, reason: 'Fixture Task'});
    const X = await f.open('domain:X');
    const text = X.locator(':scope > .object-body .object-content');
    assert.equal(await text.locator('h2').count(), 1);
    assert.ok(await text.locator('ul > li').count() >= 2);
    const boxes = text.locator('input[type="checkbox"]');
    assert.equal(await boxes.count(), 2);
    assert.equal(await boxes.nth(0).isChecked(), true);
    assert.equal(await boxes.nth(0).isDisabled(), true);
    assert.equal(await text.locator('table th').count(), 2);
    assert.equal(await text.locator('a').first().getAttribute('href'), 'https://example.com/docs');
    assert.equal(await text.locator('blockquote').count(), 1);
    assert.equal(await text.locator('p > code').count(), 1);
    assert.equal(await text.locator('pre > code').count(), 1);
    const plain = await text.innerText();
    assert.ok(!plain.includes('##') && !plain.includes('|---|'), plain);
    const lines = await text.evaluate(node => [...node.querySelectorAll('p')].find(p => p.textContent.includes('Given: a')).innerHTML);
    assert.match(lines, /Given: a<br>\s*When: b/);
    assert.equal(await X.locator(':scope > summary .node-title').innerText(), '<b>x</b> plan');
    assert.equal(await X.locator(':scope > .object-body .note-text ul > li').count(), 2);
    const stored = await f.api('/objects?address=domain:X');
    assert.equal(stored.document.data.body, markdownBody);
    const T = await f.open('task:T');
    assert.equal(await T.locator(':scope > .object-body .object-content pre > code').count(), 1);
  } finally { await f.close(); }
});

test('markdown-text-ac2: active HTML in Markdown stays inert', async () => {
  const hostile = ['<script>window.pwned=1</script>', '<img src=x onerror="window.pwned=2">', '<svg onload="window.pwned=4"></svg>',
    '[run](javascript:window.pwned=3)', '[data](data:text/html,<b>x</b>)', '<iframe src="https://example.com"></iframe>',
    '<style>body{display:none}</style>', '[ext](https://example.com/page)'].join('\n\n');
  const f = await fixture({'domain:H': {title: 'Hostile', body: hostile}});
  try {
    const H = await f.open('domain:H');
    const text = H.locator(':scope > .object-body .object-content');
    for (const link of await text.locator('a').all()) await link.click({modifiers: ['Meta']}).catch(() => {});
    assert.equal(await f.page.evaluate(() => window.pwned), undefined);
    assert.equal(await text.locator('script, iframe, style, svg, [onerror], [onload]').count(), 0);
    const hrefs = await text.locator('a').evaluateAll(nodes => nodes.map(node => node.getAttribute('href')));
    assert.ok(hrefs.every(href => href === null || !/^(javascript|data):/i.test(href)), JSON.stringify(hrefs));
    const external = text.locator('a[href="https://example.com/page"]');
    assert.equal(await external.getAttribute('rel'), 'noopener noreferrer');
  } finally { await f.close(); }
});

test('markdown-text-ac3: the renderer loads once and long content scrolls in its own box', async () => {
  const records = {};
  for (let i = 0; i < 20; i++) records['domain:D' + i] = {title: 'Domain ' + i, body: markdownBody + '\n\n| ' + 'wide '.repeat(60) + '|\n|---|\n| x |'};
  const f = await fixture(records, types, {viewport: {width: 390, height: 844}});
  const scripts = [];
  f.page.on('request', request => { if (new URL(request.url()).pathname === '/vendor/markdown.js') scripts.push(request.url()); });
  try {
    for (const scheme of ['light', 'dark']) {
      scripts.length = 0;
      await f.page.emulateMedia({colorScheme: scheme});
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      for (let i = 0; i < 20; i++) {
        const card = f.page.locator('#collection-domain .common-object[data-object-address="domain:D' + i + '"]');
        await card.waitFor();
        if (await card.getAttribute('open') === null) await card.locator(':scope > summary').click({position: {x: 4, y: 4}});
        await card.locator(':scope > .object-body .object-content h2').waitFor();
      }
      assert.equal(scripts.length, 1, 'one renderer request per page load: ' + scripts.length);
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'the page does not scroll sideways');
      const table = f.page.locator('.object-content table').first();
      assert.equal(await table.evaluate(node => getComputedStyle(node).overflowX), 'auto');
    }
  } finally { await f.close(); }
});

test('compact-open-object-ac4: copy and revisions are icons at the right end of the row', async () => {
  const f = await fixture({'domain:X': {title: 'Release plan outline'}});
  try {
    for (const [width, height] of [[1280, 720], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      for (const scheme of ['light', 'dark']) {
        await f.page.emulateMedia({colorScheme: scheme});
        await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
        const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
        await X.waitFor();
        const row = X.locator(':scope > summary');
        const controls = await row.locator('button').evaluateAll(nodes => nodes.map(node => ({cls: node.className, text: node.innerText.trim(),
          name: node.getAttribute('aria-label'), title: node.getAttribute('title')})));
        const [copy, history] = controls.slice(-2);
        assert.match(copy.cls, /\bcopy\b/); assert.match(history.cls, /\bhistory-button\b/);
        assert.deepEqual([copy.text, history.text], ['', '']);
        assert.deepEqual([copy.name, copy.title], ['Copy ID for agent', 'Copy ID for agent']);
        assert.deepEqual([history.name, history.title], ['Revisions', 'Revisions']);
        const rowBox = await row.boundingBox(), historyBox = await row.locator('.history-button').boundingBox();
        assert.ok(rowBox.x + rowBox.width - (historyBox.x + historyBox.width) <= 8, JSON.stringify({rowBox, historyBox}));
        const lineHeight = await row.evaluate(node => { const before = node.getBoundingClientRect().height;
          const actions = node.querySelector('.item-actions'); actions.style.display = 'none';
          const without = node.getBoundingClientRect().height; actions.style.display = ''; return [before, without]; });
        assert.ok(Math.abs(lineHeight[0] - lineHeight[1]) <= 1, 'the controls add no line: ' + lineHeight);
      }
    }
    const row = f.page.locator('#tree .common-object[data-object-address="domain:X"] > summary');
    await row.locator('.copy').click();
    const itemId = (await f.api('/objects?address=domain:X')).item_id;
    assert.ok((await f.page.evaluate(() => window.copiedIds)).includes(itemId));
    await row.locator('.copy').focus(); await f.page.keyboard.press('Enter');
    assert.ok((await f.page.evaluate(() => window.copiedIds)).filter(id => id === itemId).length >= 2);
    await row.locator('.history-button').click();
    await f.page.locator('dialog.notes-dialog').waitFor();
    await f.page.locator('dialog.notes-dialog .history-close').click();
    await row.locator('.history-button').focus(); await f.page.keyboard.press('Enter');
    await f.page.locator('dialog.notes-dialog').waitFor();
  } finally { await f.close(); }
});

test('compact-open-object-ac5: rows are separated without dashed lines', async (t) => {
  const f = await fixture({'domain:X': {title: 'Domain X'}, 'capability:Y': {title: 'Capability Y', parent: 'domain:X'},
    'requirement:R1': {title: 'Requirement one', parent: 'capability:Y'}, 'requirement:R2': {title: 'Requirement two', parent: 'capability:Y'},
    'requirement:R3': {title: 'Requirement three', parent: 'capability:Y'}});
  const row = card => card.locator(':scope > summary');
  const ready = card => card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
  const rules = {light: 'rgb(217, 211, 199)', dark: 'rgb(68, 73, 81)'};
  try {
    await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    await X.waitFor();
    await row(X).click({position: {x: 4, y: 4}}); await ready(X);
    const Y = X.locator(':scope > .object-body > .object-children > .common-object[data-object-address="capability:Y"]');
    await Y.waitFor();
    await row(Y).click({position: {x: 4, y: 4}}); await ready(Y);
    const requirements = Y.locator(':scope > .object-body > .object-children > .common-object[data-object-address^="requirement:"]');
    await requirements.nth(2).waitFor();
    await Promise.all(f.requests.map(entry => entry.body));
    await f.page.evaluate(() => {
      const name = node => node.tagName.toLowerCase() + (node.id ? '#' + node.id : '') + [...node.classList].map(value => '.' + value).join('') +
        (node.dataset?.objectAddress ? '[' + node.dataset.objectAddress + ']' : '');
      const lines = [...['Top', 'Right', 'Bottom', 'Left'].map(side => ['border-' + side.toLowerCase() + '-style', 'border' + side + 'Style', 'border' + side + 'Width']),
        ['outline-style', 'outlineStyle', 'outlineWidth'], ['text-decoration-style', 'textDecorationStyle', 'textDecorationThickness']];
      // A pseudo-element without content draws nothing, so its outline cannot show focus.
      const drawn = (style, pseudo) => !pseudo || !['none', 'normal'].includes(style.content);
      const indicator = (node, pseudo) => {
        const style = getComputedStyle(node, pseudo || null), label = name(node) + pseudo;
        if (!drawn(style, pseudo)) return null;
        if (style.outlineStyle === 'solid' && parseFloat(style.outlineWidth) >= 1) return label + ' solid outline';
        if (style.textDecorationLine.includes('underline') && style.textDecorationStyle === 'solid') return label + ' solid underline';
        return null;
      };
      window.ac5 = {visited: new Set(),
        // Every element and pseudo-element on the page in its current focus state.
        scan: where => {
          const found = []; let checked = 0;
          for (const node of document.querySelectorAll('*')) for (const pseudo of ['', '::before', '::after', '::marker']) {
            const style = getComputedStyle(node, pseudo || null); checked++;
            for (const [property, key, width] of lines) if (style[key] === 'dashed') found.push(where + ': ' + name(node) + pseudo + ' ' + property + ' dashed ' + style[width]);
          }
          return {found, checked};
        },
        focus: () => {
          const node = document.activeElement;
          if (!node || node === document.body) return null;
          const repeat = ac5.visited.has(node); ac5.visited.add(node);
          const candidates = [[node, ''], [node, '::before'], [node, '::after'], ...[...node.querySelectorAll(':scope > .node-title')].map(title => [title, ''])];
          return {name: name(node), repeat, indicator: candidates.map(([element, pseudo]) => indicator(element, pseudo)).find(Boolean) || null};
        },
        missing: () => [...document.querySelectorAll('a[href],button,input,select,textarea,summary,[tabindex]')]
          .filter(node => node.tabIndex >= 0 && !node.disabled && !node.closest('[inert]') && node.checkVisibility() && !ac5.visited.has(node)).map(name),
        separators: () => {
          const probe = document.createElement('div'); probe.style.color = 'var(--rule)'; document.body.append(probe);
          const rule = getComputedStyle(probe).color; probe.remove();
          return {rule, rows: [...document.querySelectorAll('.common-object[data-object-address^="requirement:"]')].map(node => {
            const style = getComputedStyle(node);
            return {name: name(node), open: node.open, style: style.borderBottomStyle, width: parseFloat(style.borderBottomWidth), color: style.borderBottomColor};
          })};
        }};
    });
    const failures = [];
    let scans = 0, steps = 0;
    for (const [width, height] of [[1280, 720], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      for (const [scheme, background] of [['light', 'rgb(255, 252, 245)'], ['dark', 'rgb(25, 28, 32)']]) {
        const where = 'width ' + width + ', ' + scheme;
        await f.page.emulateMedia({colorScheme: scheme});
        await f.page.mouse.move(0, 0);
        assert.equal(await f.page.evaluate(() => getComputedStyle(document.body).backgroundColor), background, where);
        assert.equal(await X.getAttribute('open'), '', 'X is open, ' + where); assert.equal(await Y.getAttribute('open'), '', 'Y is open, ' + where);
        const separators = await f.page.evaluate(() => ac5.separators());
        assert.equal(separators.rule, rules[scheme], 'the --rule colour of the ' + where + ' theme');
        assert.equal(separators.rows.length, 3, where);
        for (const separator of separators.rows) {
          const text = where + ': ' + separator.name + ' bottom border ' + separator.style + ' ' + separator.width + 'px ' + separator.color;
          t.diagnostic(text);
          if (separator.open || !(separator.style === 'none' ||
            (separator.style === 'solid' && separator.width <= 1 && separator.color === separators.rule))) failures.push(text);
        }
        const idle = await f.page.evaluate(where => ac5.scan(where), where);
        failures.push(...idle.found); scans += idle.checked;
        // Tab from the current position until the focus returns to a visited element.
        await f.page.evaluate(() => { ac5.visited.clear(); });
        let focused = 0;
        for (let press = 0; press < 400; press++) {
          await f.page.keyboard.press('Tab');
          const state = await f.page.evaluate(() => ac5.focus());
          if (!state) continue;
          if (state.repeat) break;
          focused++; steps++;
          const text = where + ', focus ' + state.name;
          if (!state.indicator) failures.push(text + ': no solid outline or solid underline');
          else t.diagnostic(text + ': ' + state.indicator);
          const scan = await f.page.evaluate(where => ac5.scan(where), text);
          failures.push(...scan.found); scans += scan.checked;
        }
        assert.ok(focused > 0, 'Tab reaches the board, ' + where);
        assert.deepEqual(await f.page.evaluate(() => ac5.missing()), [], 'Tab visits every focusable board element, ' + where);
        await f.page.evaluate(() => document.activeElement?.blur());
      }
    }
    t.diagnostic(failures.length + ' dashed or invalid styles in ' + scans + ' element and pseudo-element states over ' + steps + ' focus steps');
    assert.deepEqual(failures, [], 'no board element or pseudo-element shows a dashed line');
  } finally { await f.close(); }
});

test('no-dashed-focus-ac2: keyboard focus shows a solid mark in the link colour on the board and the tree', async (t) => {
  const f = await fixture({'domain:X': {title: 'Domain X'}, 'capability:Y': {title: 'Capability Y', parent: 'domain:X'}});
  const themes = {light: {background: 'rgb(255, 252, 245)', link: 'rgb(0, 92, 239)'}, dark: {background: 'rgb(25, 28, 32)', link: 'rgb(140, 181, 255)'}};
  const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
  const pages = {
    board: {path: '/?workspace=', mouse: X.locator(':scope > summary .copy'), ready: async () => {
      await X.waitFor();
      if (await X.getAttribute('open') === null) await X.locator(':scope > summary').click({position: {x: 4, y: 4}});
      await X.locator(':scope > .object-body > .object-children > .common-object[data-object-address="capability:Y"]').waitFor();
    }},
    tree: {path: '/tree?workspace=', mouse: f.page.locator('#types .type[data-kind="capability"]'),
      ready: () => f.page.locator('#types .type[data-kind="ac"]').waitFor()},
  };
  const install = () => {
    const name = node => node.tagName.toLowerCase() + (node.id ? '#' + node.id : '') + [...node.classList].map(value => '.' + value).join('') +
      (node.dataset?.objectAddress ? '[' + node.dataset.objectAddress + ']' : '') + (node.dataset?.kind ? '[' + node.dataset.kind + ']' : '');
    const colour = value => {
      const probe = document.createElement('div'); probe.style.color = value; document.body.append(probe);
      const resolved = getComputedStyle(probe).color; probe.remove(); return resolved;
    };
    // A pseudo-element without content draws nothing, so it cannot show focus.
    const parts = node => ['', '::before', '::after'].map(pseudo => [pseudo, getComputedStyle(node, pseudo || null)])
      .filter(([pseudo, style]) => !pseudo || !['none', 'normal'].includes(style.content));
    const measure = style => ({outline: style.outlineStyle + ' ' + style.outlineWidth + ' ' + style.outlineColor, boxShadow: style.boxShadow,
      border: ['Top', 'Right', 'Bottom', 'Left'].map(side => style['border' + side + 'Style']).join(' '),
      textDecoration: style.textDecorationLine + ' ' + style.textDecorationStyle + ' ' + style.textDecorationColor});
    const shadow = (style, link) => style.boxShadow !== 'none' && style.boxShadow.split(/,(?![^(]*\))/).some(part =>
      part.includes(link) && (part.replace(link, '').match(/-?[\d.]+px/g) || []).some(length => parseFloat(length) !== 0));
    const mark = (node, link) => parts(node).map(([pseudo, style]) =>
      style.outlineStyle === 'solid' && parseFloat(style.outlineWidth) >= 1 && style.outlineColor === link ? pseudo + ' solid outline' :
      shadow(style, link) ? pseudo + ' solid box-shadow' : null).find(Boolean) || null;
    window.ac2 = {visited: [],
      link: () => colour('var(--link)'),
      focus: () => {
        const node = document.activeElement;
        if (!node || node === document.body) return null;
        const repeat = ac2.visited.includes(node), first = ac2.visited[0] === node;
        if (!repeat) ac2.visited.push(node);
        const link = ac2.link(), measured = parts(node).map(([pseudo, style]) => ({pseudo, ...measure(style)}));
        const dashed = parts(node).flatMap(([pseudo, style]) => [['outline', style.outlineStyle], ['text-decoration', style.textDecorationStyle],
          ...['Top', 'Right', 'Bottom', 'Left'].map(side => ['border-' + side.toLowerCase(), style['border' + side + 'Style']])]
          .filter(([, value]) => value === 'dashed').map(([property]) => pseudo + ' ' + property + ' dashed'));
        return {name: name(node), repeat, first, link, mark: mark(node, link), measured, dashed};
      },
      visits: selector => [...document.querySelectorAll(selector)].map(node => ({name: name(node), visited: ac2.visited.includes(node)})),
      missing: () => [...document.querySelectorAll('a[href],button,input,select,textarea,summary,[tabindex]')]
        .filter(node => node.tabIndex >= 0 && !node.disabled && !node.closest('[inert]') && node.checkVisibility() && !ac2.visited.includes(node)).map(name),
      pointer: () => {
        const node = document.activeElement, link = ac2.link();
        return {name: name(node), visible: node.matches(':focus-visible'), mark: mark(node, link),
          outline: parts(node).some(([, style]) => style.outlineStyle !== 'none' || style.boxShadow !== 'none'), measured: measure(getComputedStyle(node))};
      }};
  };
  try {
    const failures = [];
    let steps = 0;
    for (const [page, setup] of Object.entries(pages)) {
      const response = await f.page.goto(f.live.root + setup.path + f.live.ws);
      assert.equal(response.status(), 200, page);
      await setup.ready();
      await Promise.all(f.requests.map(entry => entry.body));
      await f.page.evaluate(install);
      for (const [scheme, theme] of Object.entries(themes)) {
        const where = page + ', ' + scheme;
        await f.page.evaluate(scheme => { document.documentElement.dataset.theme = scheme; document.activeElement?.blur(); ac2.visited = []; }, scheme);
        await f.page.mouse.move(0, 0);
        assert.equal(await f.page.evaluate(() => getComputedStyle(document.body).backgroundColor), theme.background, 'the ' + where + ' theme is active');
        assert.equal(await f.page.evaluate(() => ac2.link()), theme.link, 'the --link colour of the ' + where + ' theme');
        // Tab from the page start until the focus returns to the first control.
        let focused = 0, returned = false;
        for (let press = 0; press < 400; press++) {
          await f.page.keyboard.press('Tab');
          const state = await f.page.evaluate(() => ac2.focus());
          if (!state) continue;
          if (state.repeat) { returned = state.first; if (!returned) failures.push(where + ': the focus returns to ' + state.name + ', not the first control'); break; }
          focused++; steps++;
          const text = where + ', focus ' + state.name + ': ' + JSON.stringify(state.measured);
          t.diagnostic(text + (state.mark ? ', ' + state.mark : ''));
          if (!state.mark) failures.push(text + ': no solid outline or solid box-shadow in ' + state.link);
          if (state.dashed.length) failures.push(text + ': ' + state.dashed.join(', '));
        }
        assert.ok(focused > 0, 'Tab reaches a control, ' + where);
        assert.ok(returned, 'the focus returns to the first control, ' + where);
        assert.deepEqual(await f.page.evaluate(() => ac2.missing()), [], 'Tab visits every focusable control, ' + where);
        if (page === 'tree') {
          const visits = await f.page.evaluate(() => ac2.visits('.schema-scroll, #types .type'));
          assert.ok(visits.length >= 6, 'the tree shows the diagram and its type cards, ' + where);
          assert.deepEqual(visits.filter(visit => !visit.visited), [], 'Tab visits the diagram and every type card, ' + where);
        }
        // Mouse focus shows no mark.
        await setup.mouse.click();
        const pointer = await f.page.evaluate(() => ac2.pointer());
        t.diagnostic(where + ', mouse focus ' + JSON.stringify(pointer));
        if (pointer.visible || pointer.mark || pointer.outline) failures.push(where + ': mouse focus on ' + pointer.name + ' shows a mark ' + JSON.stringify(pointer.measured));
      }
    }
    t.diagnostic(failures.length + ' focus defects over ' + steps + ' focus steps');
    assert.deepEqual(failures, [], 'every keyboard focus shows a solid mark in the link colour and no dashed line');
  } finally { await f.close(); }
});

});
