// The private browser_server fixture with three workspaces for the workspace switch checks.
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {once} = require('node:events');
const path = require('node:path');
// These checks read the earlier full shape of the read tools (requirement:read-contract).

const root = path.join(__dirname, '..');
const types = {
  domain: {title: 'Domains', role: 'group', status:'children', root: true, parents: [], badge: 'DOM'},
  capability: {title: 'Capabilities', role: 'group', status:'children', parents: ['domain'], badge: 'CAP'},
  requirement: {title: 'Requirements', role: 'requirement', status:'children', parents: ['capability'], badge: 'REQ'},
  ac: {title: 'Criteria', role: 'ac', status:'tests', parents: ['requirement'], badge: 'AC'},
};

/// Build the release fixture and start it. The fixture prints its address, workspace and tokens.
async function server() {
  const build = spawnSync('cargo', ['test', '--release', '--test', 'browser_server', '--no-run', '--message-format=json'],
    {cwd: root, encoding: 'utf8', maxBuffer: 1 << 28});
  assert.equal(build.status, 0, build.stderr);
  const artifact = build.stdout.split('\n').filter(Boolean).map(line => JSON.parse(line))
    .find(value => value.reason === 'compiler-artifact' && value.target.name === 'browser_server' && value.executable);
  assert.ok(artifact, 'browser fixture executable required');
  const child = spawn(artifact.executable, ['--ignored', '--exact', 'browser_server', '--nocapture'],
    {cwd: root, env: {...process.env, JOPPA_BROWSER_HELPER: '1'}, stdio: ['pipe', 'pipe', 'pipe']});
  let output = '', errors = '';
  child.stderr.on('data', chunk => { errors += chunk; });
  const ready = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => { child.stdin.end(); reject(Error('browser server timed out')); }, 45000);
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
    assert.equal((await exit)[0], 0, 'browser fixture cleanup failed: ' + errors);
  }};
}

/// One MCP tool call with a fixture token.
async function mcp(live, token, name, args) {
  const response = await fetch(live.root + '/mcp', {method: 'POST',
    headers: {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'},
    body: JSON.stringify({jsonrpc: '2.0', id: 1, method: 'tools/call', params: {name, arguments: {...args}}})});
  assert.equal(response.status, 200);
  const reply = await response.json();
  assert.equal(reply.error, undefined, JSON.stringify(reply));
  assert.notEqual(reply.result.isError, true, JSON.stringify(reply));
  return JSON.parse(reply.result.content[0].text);
}

/// Save the records in batches of 100 operations. A parent comes before its children.
async function saveAll(live, token, workspace, owner, records) {
  const operations = records.map(([address, title, parent]) => {
    const [kind, id] = address.split(':');
    return {command: {op: 'save_object', kind, id, revision: 0, parent: parent || null,
      data: {title, owner, body: 'Fixture object for the switch checks'}, reason: 'Define the switch fixture'}};
  });
  for (let i = 0; i < operations.length; i += 100)
    await mcp(live, token, 'joppa_batch', {workspace, request_id: crypto.randomUUID(), operations: operations.slice(i, i + 100)});
}

/// A tree of 5 domains. Each level has `width` children: capabilities, requirements and criteria.
function domainTree(widths) {
  const records = [];
  const grow = (parent, level, prefix) => {
    if (level >= widths.length) return;
    const kind = ['capability', 'requirement', 'ac'][level];
    for (let n = 1; n <= widths[level]; n++) {
      const address = kind + ':' + prefix + kind[0] + n;
      records.push([address, kind + ' ' + prefix + n, parent]);
      grow(address, level + 1, prefix + kind[0] + n);
    }
  };
  for (let d = 1; d <= 5; d++) {
    records.push(['domain:d' + d, 'Domain ' + d]);
    grow('domain:d' + d, 0, 'd' + d);
  }
  return records;
}

/// Fill the fixture workspace with `records`. Alex creates two small workspaces through the browser
/// API, so alex can select three workspaces. Each workspace uses the same tree.
async function workspaces(live, browser, records) {
  const api = async (token, workspace, suffix, body) => {
    const response = await fetch(live.root + '/api/workspaces/' + workspace + suffix, {method: body ? 'POST' : 'GET',
      headers: {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}, body: body && JSON.stringify(body)});
    const value = await response.json();
    assert.equal(response.status, 200, JSON.stringify(value));
    return value;
  };
  const command = (token, workspace, value) => api(token, workspace, '/commands', {request_id: crypto.randomUUID(), command: value});
  const configure = async (token, workspace) => {
    const tree = await api(token, workspace, '/tree');
    await command(token, workspace, {op: 'set_tree_schema', revision: tree.current_revision, types, reason: 'Configure the switch tree'});
  };
  await api(live.owner, live.ws, '/agent-tokens', {agent: 'bot'});
  await command(live.owner, live.ws, {op: 'member', id: 'alex', role: 'human'});
  // Only the workspace owner sets the tree schema.
  await configure(live.owner, live.ws);
  await saveAll(live, live.agent, live.ws, 'owner', records);
  const context = await browser.newContext({reducedMotion: 'reduce', viewport: {width: 1440, height: 1000}});
  await context.addCookies([{name: 'joppa_session', value: live.cookie, url: live.root}]);
  const small = {};
  for (const name of ['other', 'third']) {
    const created = await context.request.post(live.root + '/api/workspaces', {headers: {Origin: live.root}, data: {title: 'Switch ' + name}});
    assert.equal(created.status(), 200, await created.text());
    const id = (await created.json()).id;
    await configure(live.alex, id);
    const rows = [1, 2].map(n => ['domain:' + name + n, name + ' domain ' + n]);
    await saveAll(live, live.alex, id, 'alex', rows);
    small[name] = {ws: id, rows: rows.map(([address]) => address)};
  }
  const big = {ws: live.ws, rows: [1, 2, 3, 4, 5].map(n => 'domain:d' + n)};
  const all = [big, small.other, small.third];
  const expect = target => ({ws: target.ws, rows: target.rows,
    foreign: all.filter(other => other !== target).flatMap(other => other.rows)});
  return {context, big, other: small.other, third: small.third, expect};
}

/// Wait until the page shows every top-level row of `target` and no row of another workspace.
async function shows(page, wanted) {
  await page.waitForFunction(wanted => wanted.rows.every(address => document.querySelector('#tree .common-object[data-object-address="' + address + '"]')?.checkVisibility())
    && !wanted.foreign.some(address => document.querySelector('#tree [data-object-address="' + address + '"]')), wanted)
    .catch(async error => {
      const state = await page.evaluate(() => ({url: location.href, tree: document.getElementById('tree')?.innerText.slice(0, 600),
        rows: [...document.querySelectorAll('#tree [data-object-address]')].slice(0, 12).map(row => row.dataset.objectAddress)}));
      throw Error('rows of ' + wanted.ws + ' did not show: ' + JSON.stringify(state) + ' ' + error.message);
    });
}

const build = () => spawnSync('git', ['rev-parse', 'HEAD'], {cwd: root, encoding: 'utf8'}).stdout.trim();

module.exports = {server, mcp, saveAll, domainTree, workspaces, shows, build};
