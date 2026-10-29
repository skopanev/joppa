const {test, describe, before, after} = require('node:test');
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {writeFileSync} = require('node:fs');
const {once} = require('node:events');
const http = require('node:http');
const path = require('node:path');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
// These checks read the earlier full shape of the read tools (requirement:read-contract).

const root = path.join(__dirname, '..');
const types = {
  domain: {title: 'Domains', role: 'group', status:'children', root: true, parents: [], badge: 'DOM'},
  capability: {title: 'Capabilities', role: 'group', status:'children', parents: ['domain'], badge: 'CAP'},
  requirement: {title: 'Requirements', role: 'requirement', status:'children', parents: ['capability'], badge: 'REQ'},
  ac: {title: 'Criteria', role: 'ac', status:'tests', parents: ['requirement'], badge: 'AC'},
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
    const timer = setTimeout(() => { child.stdin.end(); reject(Error('browser server timed out')); }, 45000);
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
  // Playwright never settles the body of a response whose document the page left.
  // Each new main-frame document ends the wait for the bodies of the document that it replaced.
  let leave, left = new Promise(resolve => { leave = resolve; });
  await context.exposeBinding('joppaNewDocument', ({frame}) => {
    if (frame !== page.mainFrame()) return;
    leave();
    left = new Promise(resolve => { leave = resolve; });
  });
  await context.addInitScript(() => window.joppaNewDocument());
  page.on('response', response => {
    if (!response.url().includes('/api/')) return;
    const entry = {url: response.url(), status: response.status()};
    entry.body = Promise.race([response.json().then(value => { entry.value = value; }).catch(() => {}), left]);
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
  // Record one commit: each case is [name] for a pass or [name, message] for a failure.
  const record = (commit, cases) => mcp('joppa_record_results', {commit, results: cases.map(([name, message]) =>
    message ? {test: 'checks.cjs > ' + name, status: 'fail', message} : {test: 'checks.cjs > ' + name, status: 'pass'})});
  const bind = (address, tests) => command({op: 'set_ac_tests', target: address,
    tests: tests.map(name => 'checks.cjs > ' + name), reason: 'Use the exact recorded checks'});
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
    await api('/agent-tokens', {agent: 'bot'}, live.owner);
    await command({op: 'member', id: 'alex', role: 'human'}, true);
    const schema = await api('/tree');
    // Only the workspace owner sets the tree schema, archives and restores.
    await command({op: 'set_tree_schema', revision: schema.current_revision, types: configured, reason: 'Configure the browser tree'}, true);
    for (const [address, record] of Object.entries(records)) await save(address, record);
  } catch (error) { await close(); throw error; }
  return {page, requests, open, close, api, command, save, record, bind, mcp, live};
}

const criterionRecords = {
  'domain:d': {title: 'Domain'},
  'capability:c': {title: 'Capability', parent: 'domain:d'},
  'requirement:r': {title: 'Requirement', parent: 'capability:c'},
  'ac:A': {title: 'Acceptance condition', parent: 'requirement:r'},
};

// A point on the title text, right of the status circle and away from the row controls.
const titlePoint = card => card.locator(':scope > summary').evaluate(summary => {
  const title = summary.querySelector(':scope > .node-title'), range = document.createRange();
  range.selectNodeContents(title.lastChild);
  const text = range.getBoundingClientRect(), box = summary.getBoundingClientRect();
  return {x: text.left + text.width / 2 - box.left, y: text.top + text.height / 2 - box.top};
});
const markdownTypes = {...types, task: {title: 'Tasks', role: 'task', status:'none', parents: ['ac']}};
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

const shared = {server, fixture, criterionRecords, types, root, titlePoint, markdownTypes, markdownBody, get browser() { return browser; }};

describe('tests/grey_ac_browser.cjs', () => {
  require('./grey_ac_browser/part01.cjs')(shared);
  require('./grey_ac_browser/part02.cjs')(shared);
  require('./grey_ac_browser/part03.cjs')(shared);
  require('./grey_ac_browser/part04.cjs')(shared);
  require('./grey_ac_browser/part05.cjs')(shared);
  require('./grey_ac_browser/part06.cjs')(shared);
  require('./grey_ac_browser/part07.cjs')(shared);
  require('./grey_ac_browser/part08.cjs')(shared);
  require('./grey_ac_browser/part09.cjs')(shared);
  require('./grey_ac_browser/part10.cjs')(shared);
});
