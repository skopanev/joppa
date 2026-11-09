// Chromium check of the per-member hidden flag: a member who is not the owner hides a workspace
// in Settings, and only the board drop-down of that member leaves it out.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

// The fixture member alex plays the owner O: alex creates and owns W and W2. maria, a new Google
// person of each fixture, plays the member alex of the criterion: a member of W and W2, no owner.
// ac:5m7fqi0h
// test:TBD
test('hide-from-my-list: each member hides a workspace in Settings for the own drop-down', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: []}});
  const member = await f.page.context().browser().newContext({reducedMotion: 'reduce', colorScheme: 'light'});
  try {
    const root = f.live.root;
    const create = async title => {
      const created = await f.page.context().request.post(root + '/api/workspaces', {headers: {Origin: root}, data: {title}});
      assert.equal(created.status(), 200, await created.text());
      return (await created.json()).id;
    };
    const suffix = crypto.randomUUID().slice(0, 8);
    const w = await create('Shared plans ' + suffix), w2 = await create('Side plans ' + suffix);
    for (const id of [w, w2]) {
      const added = await fetch(root + '/api/workspaces/' + encodeURIComponent(id) + '/commands', {method: 'POST',
        headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
        body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'member', id: f.live.maria, role: 'human'}})});
      assert.equal(added.status, 200, await added.text());
      // A Google person reads only granted workspaces, so the owner also grants the email.
      const granted = await f.page.context().request.post(root + '/api/email-grants', {headers: {Origin: root},
        data: {workspace: id, email: f.live.maria_email}});
      assert.equal(granted.status(), 200, await granted.text());
    }
    await member.addCookies([{name: 'joppa_session', value: f.live.maria_cookie, url: root}]);
    const page = await member.newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));

    // The workspace IDs of the board drop-down on / of one page.
    const dropDown = async p => {
      assert.equal((await p.goto(root + '/')).status(), 200);
      await p.waitForFunction(() => { const s = document.getElementById('workspaces'); return s && s.options.length > 0 && s.value; },
        null, {timeout: 15000});
      return p.locator('#workspaces').evaluate(s => [...s.options].map(o => o.value));
    };
    // Wait until the plain web list of the member lists W (`listed`) or leaves it out.
    const stored = async listed => {
      for (let i = 0; i < 100; i++) {
        const list = await (await member.request.get(root + '/api/workspaces')).json();
        if ((list.items || list).some(item => item.id === w) === listed) return;
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      assert.fail('GET /api/workspaces of the member: W listed should be ' + listed);
    };
    // Open Settings, select W and give the checkbox when it shows the hidden flag `hidden`.
    const settings = async (width, hidden) => {
      assert.equal((await page.goto(root + '/settings')).status(), 200);
      await page.waitForFunction(() => !document.getElementById('workspace-select').disabled, null, {timeout: 15000});
      const select = page.getByLabel('Workspace', {exact: true});
      const values = await select.locator('option').evaluateAll(options => options.map(o => o.value));
      assert.ok(values.includes(w) && values.includes(w2), width + ': the Settings select ' + JSON.stringify(values));
      await select.selectOption(w);
      const box = page.locator('#hide-workspace');
      await box.waitFor({state: 'visible', timeout: 15000});
      assert.equal(await page.getByLabel('Hide from my list', {exact: true}).getAttribute('id'), 'hide-workspace');
      const edge = await box.boundingBox();
      assert.ok(edge.x >= 0 && edge.x + edge.width <= width, width + ': the checkbox is outside the page');
      await page.waitForFunction(want => document.getElementById('hide-workspace').checked === want, hidden, {timeout: 5000})
        .catch(() => assert.fail(width + ': the checkbox should show hidden ' + hidden));
      return box;
    };

    for (const [width, height] of [[1280, 900], [390, 844]]) {
      await page.setViewportSize({width, height});
      await f.page.setViewportSize({width, height});
      await (await settings(width, false)).check();
      await stored(false);
      const mine = await dropDown(page);
      assert.ok(!mine.includes(w) && mine.includes(w2), width + ': the drop-down of the member ' + JSON.stringify(mine));
      const owners = await dropDown(f.page);
      assert.ok(owners.includes(w), width + ': the drop-down of the owner ' + JSON.stringify(owners));
      // Settings still lists W, and the checkbox shows the flag of the member when Settings opens.
      await (await settings(width, true)).uncheck();
      await stored(true);
      const again = await dropDown(page);
      assert.ok(again.includes(w) && again.includes(w2), width + ': the drop-down after the uncheck ' + JSON.stringify(again));
    }
    assert.deepEqual(errors, []);
  } finally { await member.close(); await f.close(); }
});
