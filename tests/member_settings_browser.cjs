// Chromium check of requirement:member-rights in Settings: each member of a workspace sees the
// same controls, and a member who did not create the workspace uses each of them.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

/// Open Settings on `page`, select workspace `id` and read its controls and people rows.
async function controls(page, root, id) {
  assert.equal((await page.goto(root + '/settings')).status(), 200);
  const select = page.getByLabel('Workspace', {exact: true});
  await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
  await page.waitForFunction(() => !document.getElementById('workspace-select').disabled);
  await select.selectOption(id);
  const people = page.getByRole('list', {name: 'People with access', exact: true});
  await people.locator('li').filter({hasText: 'example.org'}).waitFor();
  const shown = async locator => (await locator.count()) === 1 && locator.isVisible();
  return {
    shown: {
      pencil: await shown(page.locator('header').getByRole('button', {name: 'Rename workspace'})),
      people: await people.isVisible(),
      field: await shown(page.getByLabel('Email or domain', {exact: true})),
      hide: await shown(page.locator('#hide-workspace')),
      remove: (await people.getByRole('button', {name: /^Remove /}).count()) > 0,
      delete: await shown(page.getByRole('button', {name: 'Delete workspace', exact: true})),
    },
    marks: [await page.locator('.owner-mark').count(), await page.locator('#not-owner').count()],
    rows: await people.locator('li > span:first-child').allInnerTexts(),
    removable: await people.locator('li').evaluateAll(rows => rows.map(r => !!r.querySelector('button.remove'))),
    wide: await page.evaluate(() => document.documentElement.scrollWidth),
  };
}

test('member-rights: Settings is the same page for each member of the workspace', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  const bobContext = await f.page.context().browser().newContext({reducedMotion: 'reduce', colorScheme: 'light'});
  try {
    // bob (actor owner) created the fixture workspace W. alex is a human member who did not create W.
    const id = f.live.ws, request = f.page.context().request, origin = {Origin: f.live.root};
    assert.equal((await request.post(f.live.root + '/api/email-grants', {headers: origin, data: {workspace: id, email: 'lena@example.com'}})).status(), 200);
    assert.equal((await request.post(f.live.root + '/api/domain-grants', {headers: origin, data: {workspace: id, domain: 'example.org'}})).status(), 200);
    await bobContext.addCookies([{name: 'joppa_session', value: f.live.owner_cookie, url: f.live.root}]);
    const bobPage = await bobContext.newPage(), bobErrors = [];
    bobPage.on('pageerror', error => bobErrors.push(error.message));
    const rows = ['alex', 'owner', 'lena@example.com (invited)', 'Everyone at example.org'];
    for (const viewport of [{width: 1280, height: 800}, {width: 390, height: 844}]) {
      await f.page.setViewportSize(viewport);
      await bobPage.setViewportSize(viewport);
      const alex = await controls(f.page, f.live.root, id);
      const bob = await controls(bobPage, f.live.root, id);
      const all = {pencil: true, people: true, field: true, hide: true, remove: true, delete: true};
      assert.deepEqual(alex.shown, all, viewport.width + ': controls of alex');
      assert.deepEqual(bob.shown, alex.shown, viewport.width + ': controls of bob');
      assert.deepEqual([alex.marks, bob.marks], [[0, 0], [0, 0]]);
      assert.deepEqual([alex.rows, bob.rows], [rows, rows]);
      // The row of the signed-in person has no Remove, and each other row has Remove.
      assert.deepEqual(alex.removable, [false, true, true, true]);
      assert.deepEqual(bob.removable, [true, false, true, true]);
      assert.ok(alex.wide <= viewport.width && bob.wide <= viewport.width, JSON.stringify([alex.wide, bob.wide]));
    }
    await f.page.setViewportSize({width: 1280, height: 800});
    await controls(f.page, f.live.root, id);
    // alex renames W with the pencil.
    const title = 'Renamed ' + crypto.randomUUID();
    await f.page.locator('header').getByRole('button', {name: 'Rename workspace'}).click();
    await f.page.getByLabel('New title').fill(title);
    const renamed = f.page.waitForResponse(r => r.url().endsWith('/rename') && r.request().method() === 'POST');
    await f.page.getByRole('button', {name: 'Rename', exact: true}).click();
    assert.equal((await renamed).status(), 200);
    const select = f.page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + id + '"]').filter({hasText: title}).waitFor({state: 'attached'});
    // alex adds maria@example.com in the field "Email or domain".
    const people = f.page.getByRole('list', {name: 'People with access', exact: true});
    await f.page.getByLabel('Email or domain', {exact: true}).fill('maria@example.com');
    const granted = f.page.waitForResponse(r => r.url().endsWith('/api/email-grants') && r.request().method() === 'POST');
    await f.page.getByRole('button', {name: 'Add', exact: true}).click();
    assert.equal((await granted).status(), 200);
    await people.locator('li').filter({hasText: 'maria@example.com (invited)'}).waitFor();
    // alex removes the domain example.org.
    await people.locator('li').filter({hasText: 'Everyone at example.org'}).getByRole('button', {name: /^Remove /}).click();
    await people.locator('li').filter({hasText: 'example.org'}).waitFor({state: 'detached'});
    const access = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    assert.deepEqual([access.emails.sort(), access.domains], [['lena@example.com', 'maria@example.com'], []]);
    assert.ok(access.members.every(m => !('owner' in m)), JSON.stringify(access.members));
    // alex deletes W after the exact title (ac:jfz3m79d).
    await f.page.getByRole('button', {name: 'Delete workspace', exact: true}).click();
    const confirm = f.page.locator('#delete-confirm-button');
    await f.page.locator('#delete-title').fill(title.toUpperCase());
    assert.equal(await confirm.isDisabled(), true);
    await f.page.locator('#delete-title').fill(title);
    assert.equal(await confirm.isDisabled(), false);
    const deleted = f.page.waitForResponse(r => r.url().endsWith('/delete') && r.request().method() === 'POST');
    await confirm.click();
    assert.equal((await deleted).status(), 200);
    await select.locator('option[value="' + id + '"]').waitFor({state: 'detached'});
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + id)).status(), 404);
    assert.equal((await bobContext.request.get(f.live.root + '/api/workspaces/' + id)).status(), 404);
    assert.deepEqual(bobErrors, []);
  } finally { await bobContext.close(); await f.close(); }
});
