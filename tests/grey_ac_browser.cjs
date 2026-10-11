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
  const build = spawnSync('cargo', ['test', '--test', 'browser_server', '--no-run', '--message-format=json'],
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
    await card.locator(':scope > .object-body > .object-secondary').waitFor();
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
    assert.equal(await link.innerText(), 'Suggested method ↗');
    const url = new URL(await link.getAttribute('href'), f.live.root);
    assert.equal(url.searchParams.get('workspace'), f.live.ws);
    assert.equal((await f.page.goto(url.href)).status(), 200);
    await f.page.locator('#workspace-levels #level-entry').waitFor();
    assert.equal(await f.page.locator('h1').innerText(), 'Suggested method');
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
  const choose = async value => {
    await f.page.locator('#identity').click();
    await f.page.locator('#theme').selectOption(value);
    await f.page.locator('#identity').click();
  };
  try {
    await f.open('domain:d');
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await choose('light');
    assert.equal(await background(), 'rgb(255, 252, 245)');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.locator('#theme').inputValue(), 'light');
    await choose('dark');
    await f.page.emulateMedia({colorScheme: 'light'});
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await f.page.locator('#theme').inputValue(), 'dark');
    await choose('system');
    assert.equal(await background(), 'rgb(255, 252, 245)');
    await f.page.emulateMedia({colorScheme: 'dark'});
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await f.page.locator('#theme').inputValue(), 'system');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('joppa-theme')), 'system');
  } finally { await f.close(); }
});

test('account-menu: sign out requires the user menu and ends the real session', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  try {
    await f.open('domain:d');
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.locator('#identity').focus();
    await f.page.keyboard.press('Enter');
    assert.equal(await f.page.locator('#logout').isVisible(), true);
    await f.page.keyboard.press('Escape');
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.locator('#identity').click();
    await f.page.locator('.brand').click();
    assert.equal(await f.page.locator('#logout').isVisible(), false);
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
    await card.locator(':scope > .object-body > .object-secondary').waitFor();
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
    await card.locator(':scope > .object-body > .object-secondary').waitFor();
    const content = await card.locator('.object-content').innerText();
    const current = await f.api('/objects?address=domain:d');
    const mcp = await f.mcp('joppa_read', {item_id: current.item_id});
    assert.equal(mcp.current_revision, 5);
    const fields = card.locator('.object-details');
    await fields.locator(':scope > summary').click();
    assert.match(await fields.innerText(), /Revision 5/);
    const hash = value => createHash('sha256').update(value).digest('hex');
    assert.equal(hash(content), hash(mcp.document.data.body));
    assert.equal(content, body);
  } finally { await f.close(); }
});
});
