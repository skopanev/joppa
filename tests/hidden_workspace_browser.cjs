// Chromium check of a hidden workspace: the drop-down leaves it out, and its direct link opens it.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

// ac:fxsy1uix
test('hidden workspace board: the drop-down leaves it out and its direct link opens it', async () => {
  const f = await fixture({
    note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: []},
  });
  try {
    const select = () => f.page.locator('#workspaces');
    const options = () => select().evaluate(s => [...s.options].map(o => [o.value, o.textContent]));
    const shown = id => f.page.waitForFunction(id => document.getElementById('workspaces').value === id, id, {timeout: 15000});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await shown(f.live.ws);
    // The signed-in member alex creates workspace H in the browser and hides it.
    const h = await f.page.evaluate(async () => {
      const response = await fetch('/api/workspaces', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({title: 'Hidden plans'})});
      return (await response.json()).id;
    });
    const response = await fetch(f.live.root + '/api/workspaces/' + h + '/commands', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
      body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'update_workspace', hidden: true}})});
    assert.equal(response.status, 200, JSON.stringify(await response.json()));

    await f.page.goto(f.live.root + '/');
    await shown(f.live.ws);
    assert.ok(!(await options()).some(([value]) => value === h), JSON.stringify(await options()));

    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(h));
    await shown(h);
    const listed = await options();
    assert.deepEqual(listed.filter(([value]) => value === h), [[h, 'Hidden plans']], JSON.stringify(listed));
    assert.equal(await select().isDisabled(), false);
    const tree = await f.page.locator('#tree').textContent();
    assert.ok(!tree.includes('not available'), tree);

    // A workspace that alex cannot read stays closed.
    await f.page.goto(f.live.root + '/?workspace=no-such-workspace');
    await f.page.locator('#tree').filter({hasText: 'This workspace is not available to your account.'}).waitFor({timeout: 15000});
  } finally { await f.close(); }
});
