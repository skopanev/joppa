// Chromium check of an alias: the board shows the alias of the agent next to its account.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

// ac:crwplj1r
test('actor alias board: a comment and History show the account and the alias', async () => {
  const f = await fixture({
    note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: []},
  });
  try {
    // alex writes under the alias p1 over MCP.
    const call = async (name, args) => {
      const response = await fetch(f.live.root + '/mcp', {method: 'POST',
        headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
        body: JSON.stringify({jsonrpc: '2.0', id: 1, method: 'tools/call',
          params: {name, arguments: {workspace: f.live.ws, alias: 'p1', ...args}}})});
      const reply = await response.json();
      assert.notEqual(reply.result?.isError, true, JSON.stringify(reply));
    };
    await call('joppa_command', {request_id: crypto.randomUUID(), command: {op: 'save_object', kind: 'note',
      id: 'plan', revision: 0, parent: null, data: {title: 'Release plan', owner: 'alex', body: 'Plan'}, reason: 'Fixture'}});
    await call('joppa_command', {request_id: crypto.randomUUID(), command: {op: 'note', target: 'note:plan', id: 'c1', text: 'Checked'}});

    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&object=note%3Aplan');
    const meta = f.page.locator('.note-meta').first();
    // The comment group starts closed: read the text of the line.
    await meta.waitFor({state: 'attached', timeout: 15000});
    assert.match(await meta.textContent(), /^alex · p1 · /);

    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.getByRole('tab', {name: 'History'}).click();
    const entry = f.page.locator('.history-collection .history-entry').first();
    await entry.waitFor({timeout: 15000});
    await entry.locator(':scope > summary').click();
    assert.match(await entry.locator('.history-actor').innerText(), /^alex · p1/);
  } finally { await f.close(); }
});
