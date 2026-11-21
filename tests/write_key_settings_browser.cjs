// Chromium check of the Write key section in Settings: an Editor creates, copies, regenerates
// and revokes the write key of a workspace. A new key ends the old one on /mcp at once, and a
// switch of the workspace clears the shown secret.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const SECRET = /^jpt_[0-9a-f]{64}$/;

test('write-key-settings: a member creates, copies, regenerates and revokes the write key', async () => {
  const f = await fixture({box: {title: 'Boxes', role: 'document', status: 'none', root: true, parents: []}});
  try {
    const root = f.live.root, page = f.page;
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write'], {origin: root});
    page.on('dialog', dialog => dialog.accept());
    const create = async title => {
      const created = await page.context().request.post(root + '/api/workspaces', {headers: {Origin: root}, data: {title}});
      assert.equal(created.status(), 200, await created.text());
      return f.track((await created.json()).id);
    };
    const suffix = crypto.randomUUID().slice(0, 8);
    const w1 = await create('Write key one ' + suffix), w2 = await create('Write key two ' + suffix);

    const section = page.locator('#write-key-panel');
    const button = name => section.getByRole('button', {name, exact: true});
    const mcpStatus = async key => (await fetch(root + '/mcp', {
      method: 'POST',
      headers: {'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'},
      body: JSON.stringify({jsonrpc: '2.0', id: 1, method: 'ping'}),
    })).status;
    const select = page.getByLabel('Workspace', {exact: true});
    const choose = async id => {
      await select.locator('option[value="' + id + '"]').waitFor({state: 'attached', timeout: 15000});
      await select.selectOption(id);
    };

    await page.setViewportSize({width: 390, height: 844});
    assert.equal((await page.goto(root + '/settings')).status(), 200);
    await page.waitForFunction(() => !document.getElementById('workspace-select').disabled, null, {timeout: 15000});
    await choose(w1);
    await button('Create write key').waitFor({state: 'visible', timeout: 15000});

    await button('Create write key').click();
    await page.locator('#write-key-value').waitFor({state: 'visible', timeout: 15000});
    const first = (await page.locator('#write-key-value').textContent()).trim();
    assert.match(first, SECRET);
    await button('Copy').click();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), first);
    assert.equal(await mcpStatus(first), 200);

    await button('Regenerate write key').click();
    await page.waitForFunction(prev => {
      const text = document.getElementById('write-key-value')?.textContent?.trim() || '';
      return text.length > 0 && text !== prev;
    }, first, {timeout: 15000});
    const second = (await page.locator('#write-key-value').textContent()).trim();
    assert.match(second, SECRET);
    assert.equal(await mcpStatus(first), 401);
    assert.equal(await mcpStatus(second), 200);

    await choose(w2);
    await button('Create write key').waitFor({state: 'visible', timeout: 15000});
    assert.equal(await page.locator('#write-key-secret').isVisible(), false);

    await choose(w1);
    await button('Revoke write key').waitFor({state: 'visible', timeout: 15000});
    assert.equal(await page.locator('#write-key-secret').isVisible(), false);
    await button('Revoke write key').click();
    await button('Create write key').waitFor({state: 'visible', timeout: 15000});
    assert.equal(await mcpStatus(second), 401);
  } finally {
    await f.close();
  }
});

test('write-key-settings: a Read-only viewer sees no Write key section and POST write-key gets 403', async () => {
  const f = await fixture({box: {title: 'Boxes', role: 'document', status: 'none', root: true, parents: []}});
  const viewerCtx = await f.page.context().browser().newContext();
  try {
    const id = f.live.ws, root = f.live.root, origin = {Origin: root};
    assert.equal((await f.page.context().request.post(root + '/api/email-grants', {headers: origin, data: {workspace: id, email: f.live.maria_email}})).status(), 200);
    await viewerCtx.addCookies([{name: 'joppa_session', value: f.live.maria_cookie, url: root}]);
    const page = await viewerCtx.newPage();
    assert.equal((await page.goto(root + '/settings')).status(), 200);
    const select = page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + id + '"]').waitFor({state: 'attached', timeout: 15000});
    await select.selectOption(id);
    await page.waitForLoadState('networkidle');
    assert.equal(await page.locator('#write-key-panel').isVisible(), false);
    const res = await viewerCtx.request.post(root + '/api/workspaces/' + encodeURIComponent(id) + '/write-key', {headers: origin});
    assert.equal(res.status(), 403);
  } finally {
    await viewerCtx.close();
    await f.close();
  }
});
