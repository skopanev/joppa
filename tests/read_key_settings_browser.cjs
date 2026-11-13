// Chromium check of the Read key section in Settings: a member creates, copies, regenerates
// and revokes the read key of a workspace. A new key ends the old one at once, and a change of
// the workspace clears the secret. The same holds at 390 px.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const SECRET = /^jpr_[0-9a-f]{64}$/;

// ac:oiu88oiv
test('read-key-settings: a member creates, copies, regenerates and revokes the read key', async () => {
  const f = await fixture({box: {title: 'Boxes', role: 'document', status: 'none', root: true, parents: []}});
  try {
    const root = f.live.root, page = f.page;
    await page.context().grantPermissions(['clipboard-read', 'clipboard-write'], {origin: root});
    page.on('dialog', dialog => dialog.accept());
    // The fixture member alex creates W1 and W2 and so is a human member of both.
    const create = async title => {
      const created = await page.context().request.post(root + '/api/workspaces', {headers: {Origin: root}, data: {title}});
      assert.equal(created.status(), 200, await created.text());
      return f.track((await created.json()).id);
    };
    const suffix = crypto.randomUUID().slice(0, 8);
    const w1 = await create('Read key one ' + suffix), w2 = await create('Read key two ' + suffix);

    const panel = page.locator('#workspace-panel');
    const button = name => panel.getByRole('button', {name, exact: true});
    // The visible buttons of the Read key section, among the four names of the criterion.
    const shown = async () => {
      const names = [];
      for (const name of ['Create read key', 'Copy', 'Regenerate read key', 'Revoke read key']) {
        if (await button(name).count() > 0 && await button(name).first().isVisible()) names.push(name);
      }
      return names;
    };
    // Each secret that the panel shows as a text or as the value of a field.
    const secrets = () => panel.evaluate(node => {
      const found = new Set();
      for (const e of [node, ...node.querySelectorAll('*')]) {
        if (!e.checkVisibility()) continue;
        const text = ('value' in e && typeof e.value === 'string' ? e.value : e.textContent).trim();
        if (/^jpr_[0-9a-f]{64}$/.test(text)) found.add(text);
      }
      return [...found];
    });
    const waitSecret = async (not, step) => {
      for (let i = 0; i < 150; i++) {
        const list = await secrets();
        if (list.length > 0 && !list.includes(not)) return list;
        await page.waitForTimeout(100);
      }
      assert.fail(step + ': the section shows no new secret');
    };
    const dataCode = async key => (await fetch(root + '/api/data', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + key, 'Content-Type': 'application/json'}, body: '{}'})).status;
    const inside = async (width, name) => {
      const edge = await button(name).first().boundingBox();
      assert.ok(edge && edge.x >= 0 && edge.x + edge.width <= width, width + ': "' + name + '" is outside the page');
    };
    const select = page.getByLabel('Workspace', {exact: true});
    const choose = async id => {
      await select.locator('option[value="' + id + '"]').waitFor({state: 'attached', timeout: 15000});
      await select.selectOption(id);
    };

    for (const [width, height] of [[1280, 900], [390, 844]]) {
      await page.setViewportSize({width, height});
      assert.equal((await page.goto(root + '/settings')).status(), 200);
      await page.waitForFunction(() => !document.getElementById('workspace-select').disabled, null, {timeout: 15000});
      await choose(w1);
      await button('Create read key').waitFor({state: 'visible', timeout: 15000});
      assert.deepEqual(await shown(), ['Create read key'], width + ': before the create');
      assert.deepEqual(await secrets(), [], width + ': a secret before the create');
      await inside(width, 'Create read key');

      await button('Create read key').click();
      const [first] = await waitSecret(null, width + ': create');
      assert.deepEqual(await secrets(), [first], width + ': one secret after the create');
      assert.match(first, SECRET);
      await button('Revoke read key').waitFor({state: 'visible', timeout: 15000});
      const after = await shown();
      for (const name of ['Regenerate read key', 'Revoke read key']) {
        assert.ok(after.includes(name), width + ': "' + name + '" after the create ' + JSON.stringify(after));
        await inside(width, name);
      }
      await inside(width, 'Copy');
      await button('Copy').click();
      assert.equal(await page.evaluate(() => navigator.clipboard.readText()), first, width + ': the clipboard');
      assert.equal(await dataCode(first), 200, width + ': the first secret before the regenerate');

      await button('Regenerate read key').click();
      const [second] = await waitSecret(first, width + ': regenerate');
      assert.deepEqual(await secrets(), [second], width + ': one secret after the regenerate');
      assert.match(second, SECRET);
      assert.equal(await dataCode(first), 401, width + ': the first secret after the regenerate');

      await choose(w2);
      await choose(w1);
      await button('Revoke read key').waitFor({state: 'visible', timeout: 15000});
      assert.deepEqual(await secrets(), [], width + ': a secret after the selection of W2 and W1');

      await button('Revoke read key').click();
      await button('Create read key').waitFor({state: 'visible', timeout: 15000});
      assert.deepEqual(await shown(), ['Create read key'], width + ': after the revoke');
      assert.deepEqual(await secrets(), [], width + ': a secret after the revoke');
      assert.equal(await dataCode(second), 401, width + ': the second secret after the revoke');
    }
  } finally { await f.close(); }
});
