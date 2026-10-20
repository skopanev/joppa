// The browser_server fixture of the board Chromium checks: one fixture server per test, a
// signed-in page, and helpers for the API, commands, MCP calls and saves.
const {before, after} = require('node:test');
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
  const save = (address, title, parent = null, owner = 'owner') => {
    const [kind, id] = address.split(':');
    return command({op: 'save_object', kind, id, revision: 0, parent, data: {title, owner, body: 'Fixture text'}, reason: 'Define the browser fixture'});
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
    // An agent enrolls through its token; the member command takes only people.
    await api('/agent-tokens', {agent: 'bot'}, live.owner);
    await command({op: 'member', id: 'alex', role: 'human'}, live.owner);
    const tree = await api('/tree');
    await command({op: 'set_tree_schema', revision: tree.current_revision, types, ...(statuses ? {statuses} : {}), reason: 'Configure the browser tree'});
  } catch (error) { await close(); throw error; }
  return {live, page, api, command, mcp, save, close};
}

module.exports = {fixture, root};
