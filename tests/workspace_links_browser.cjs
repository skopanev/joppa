// Chromium check of objects that point to other workspaces: a node shows its child workspace
// and an object shows its direct links to objects of another workspace.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('node-workspace-ac7 board: a node shows its child workspace and its links to another workspace', async () => {
  const f = await fixture({
    component: {title: 'Components', role: 'group', root: true, parents: []},
    note: {title: 'Notes', role: 'document', root: true, parents: ['*']},
  });
  try {
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    // The signed-in member alex creates workspace C in the browser and adds one object to it.
    const c = await f.page.evaluate(async () => {
      const response = await fetch('/api/workspaces', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({title: 'Backend'})});
      return (await response.json()).id;
    });
    const inC = async (command) => {
      const response = await fetch(f.live.root + '/api/workspaces/' + c + '/commands', {method: 'POST',
        headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
        body: JSON.stringify({request_id: crypto.randomUUID(), command})});
      assert.equal(response.status, 200, JSON.stringify(await response.json()));
    };
    await inC({op: 'save_object', kind: 'domain', id: 'api', revision: 0, parent: null, data: {title: 'API contract', owner: 'alex'}, reason: 'Fixture'});
    await f.command({op: 'save_object', kind: 'component', id: 'backend', revision: 0, parent: null,
      data: {title: 'Backend', owner: 'alex', workspace: c}, reason: 'Fixture'}, f.live.alex);
    await f.command({op: 'save_object', kind: 'note', id: 'mobile', revision: 0, parent: null,
      data: {title: 'Mobile login', owner: 'alex', body: 'Uses the API'}, reason: 'Fixture'}, f.live.alex);
    await f.command({op: 'set_remote_links', target: 'note:mobile', reason: 'Mobile uses the API',
      links: [{workspace: c, target: 'domain:api', relation: 'related', revision: 1, blocking: false}]}, f.live.alex);

    for (const [address, text, href] of [
      ['component:backend', 'Child workspace', '/?workspace=' + c],
      ['note:mobile', 'Links to other workspaces', '/?workspace=' + c + '&object=domain%3Aapi'],
    ]) {
      await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&object=' + encodeURIComponent(address));
      const section = f.page.locator('.workspace-links').first();
      await section.waitFor({timeout: 15000});
      assert.ok(await section.evaluate(n => n.checkVisibility()), address + ': the section is visible');
      assert.match(await section.evaluate(n => n.textContent), new RegExp(text));
      const links = await section.locator('a').evaluateAll(nodes => nodes.map(n => n.getAttribute('href')));
      assert.ok(links.includes(href), address + ': ' + JSON.stringify(links));
    }
  } finally { await f.close(); }
});
