// Chromium checks of requirement:member-rights: no page names a workspace owner, and Settings
// keeps the last human member of a workspace.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('member-rights: the board, /tree and Settings name no workspace owner', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  // eve is signed in and has no workspace: the fixture person maria plays eve.
  const eve = await f.page.context().browser().newContext({reducedMotion: 'reduce', colorScheme: 'light'});
  try {
    await eve.addCookies([{name: 'joppa_session', value: f.live.maria_cookie, url: f.live.root}]);
    const page = await eve.newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));
    assert.equal((await page.goto(f.live.root + '/')).status(), 200);
    const tree = page.locator('#tree');
    await tree.filter({hasText: 'No workspace access'}).waitFor();
    assert.equal(await tree.locator('p').innerText(), 'Ask a member of the workspace to grant access.');
    assert.doesNotMatch(await page.locator('body').innerText(), /owner/i);
    assert.equal((await page.goto(f.live.root + '/tree')).status(), 200);
    const error = page.locator('#error');
    await error.filter({hasText: /\S/}).waitFor();
    assert.equal(await error.innerText(), 'No workspace access. Ask a member of the workspace for access.');
    assert.doesNotMatch(await page.locator('body').innerText(), /owner/i);
    assert.deepEqual(errors, []);
    // alex reads Settings of a workspace that alex created: the page has no word owner.
    const request = f.page.context().request;
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Plain ' + crypto.randomUUID()}});
    assert.equal(created.status(), 200);
    const {id} = await created.json();
    for (const viewport of [{width: 1280, height: 800}, {width: 390, height: 844}]) {
      await f.page.setViewportSize(viewport);
      assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
      const select = f.page.getByLabel('Workspace', {exact: true});
      await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
      await f.page.waitForFunction(() => !document.getElementById('workspace-select').disabled);
      await select.selectOption(id);
      await f.page.getByRole('list', {name: 'People with access', exact: true}).locator('li').first().waitFor();
      assert.doesNotMatch(await f.page.locator('body').innerText(), /owner/i);
      assert.equal(await f.page.locator('.owner-mark').count(), 0);
      assert.equal(await f.page.locator('#not-owner').count(), 0);
    }
  } finally { await eve.close(); await f.close(); }
});

test('member-rights: Settings keeps the last human member of a workspace', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  try {
    // bob (actor owner) created the fixture workspace W. alex and bob are its human members.
    const id = f.live.ws, request = f.page.context().request;
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const select = f.page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await select.selectOption(id);
    const people = f.page.getByRole('list', {name: 'People with access', exact: true});
    const bob = people.locator('li').filter({hasText: 'owner'});
    await bob.waitFor();
    // alex removes bob in Settings: bob loses W at once.
    await bob.getByRole('button', {name: /^Remove /}).click();
    await bob.waitFor({state: 'detached'});
    const bobRead = await fetch(f.live.root + '/api/workspaces/' + id, {headers: {Authorization: 'Bearer ' + f.live.owner}});
    assert.equal(bobRead.status, 404);
    // alex is the only human member, and the row of alex has no Remove.
    assert.deepEqual(await people.locator('li > span:first-child').allInnerTexts(), ['alex']);
    assert.equal(await people.getByRole('button', {name: /^Remove /}).count(), 0);
    const self = await request.post(f.live.root + '/api/access/remove', {headers: {Origin: f.live.root}, data: {workspace: id, member: 'alex'}});
    assert.notEqual(self.status(), 200);
    assert.equal((await self.json()).error, 'the last human member cannot be removed');
    await f.page.reload();
    await select.selectOption(id);
    await people.locator('li').first().waitFor();
    assert.deepEqual(await people.locator('li > span:first-child').allInnerTexts(), ['alex']);
    const access = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    assert.deepEqual(access.members.map(m => m.id), ['alex']);
  } finally { await f.close(); }
});
