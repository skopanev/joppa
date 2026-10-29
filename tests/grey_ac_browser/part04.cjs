// Part 4 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
/* eslint-disable no-unused-vars */
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {writeFileSync} = require('node:fs');
const {once} = require('node:events');
const http = require('node:http');
const path = require('node:path');

module.exports = shared => {
  const {server, fixture, criterionRecords, types, root, titlePoint, markdownTypes, markdownBody} = shared;

// Alex owns W1. The fixture workspace is W2: the fixture owner owns it and alex is only a member.
test('settings-in-name-menu-ac2: Settings shows members only of owned workspaces', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  try {
    const request = f.page.context().request;
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'W1 ' + crypto.randomUUID()}});
    assert.equal(created.status(), 200);
    const {id: owned, title: w1} = await created.json();
    for (const [suffix, body] of [['/commands', {request_id: crypto.randomUUID(), command: {op: 'member', id: f.live.maria, role: 'human'}}],
      ['/agent-tokens', {agent: 'planner-bot'}]]) {
      const added = await fetch(f.live.root + '/api/workspaces/' + encodeURIComponent(owned) + suffix, {method: 'POST',
        headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'}, body: JSON.stringify(body)});
      assert.equal(added.status, 200, await added.text());
    }
    // A person with an email grant is not yet a member of W1. Maria is a member and also has an email grant.
    for (const email of ['pending@example.com', f.live.maria_email]) {
      const granted = await request.post(f.live.root + '/api/email-grants', {headers: {Origin: f.live.root}, data: {workspace: owned, email}});
      assert.equal(granted.status(), 200);
    }
    const grants = (await (await request.get(f.live.root + '/api/email-grants')).json()).items.find(w => w.id === owned);
    assert.deepEqual(grants.emails.sort(), [f.live.maria_email, 'pending@example.com'].sort());
    const inside = (box, width) => box.x >= 0 && box.x + box.width <= width;
    for (const {width, height} of [{width: 1280, height: 800}, {width: 390, height: 844}]) {
      await f.page.setViewportSize({width, height});
      const reads = [];
      const record = r => { const path = new URL(r.url()).pathname; if (path.startsWith('/api/')) reads.push(path); };
      f.page.on('request', record);
      assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
      const select = f.page.getByLabel('Workspace', {exact: true});
      await select.locator('option[value="' + owned + '"]').waitFor({state: 'attached'});
      await f.page.waitForFunction(() => !document.getElementById('workspace-select').disabled);
      f.page.off('request', record);
      // The page reads two lists, no single workspace and no name of the person.
      assert.deepEqual(reads.sort(), ['/api/access', '/api/workspaces']);
      await select.selectOption(owned);
      // One list holds the members, then the emails that have not signed in. The agent member is not on the page.
      // Maria has a Google identity, so her row shows "name <email>", and her email grant has no second row.
      const members = f.page.getByRole('list', {name: 'People with access', exact: true});
      assert.deepEqual(await members.locator('li > span:first-child').allInnerTexts(),
        ['alex', 'Maria Petrova <' + f.live.maria_email + '>', 'pending@example.com (invited)']);
      // The owner row has no Remove; every other row has one.
      assert.deepEqual(await members.locator('li').evaluateAll(rows => rows.map(r => !!r.querySelector('button.remove'))), [false, true, true]);
      assert.equal(await f.page.locator('#owner-panel ul').count(), 1);
      const panel = f.page.locator('#workspace-panel');
      assert.ok(!(await panel.innerText()).includes('planner-bot'));
      for (const item of [select, members, ...await members.locator('li').all()]) {
        assert.ok(inside(await item.boundingBox(), width));
      }
      // W2 shows no member list and no control.
      await select.selectOption(f.live.ws);
      await f.page.locator('#not-owner').waitFor();
      assert.equal(await f.page.locator('#not-owner').innerText(), 'Only the owner of this workspace manages its people.');
      assert.equal(await panel.locator('ul:visible, form:visible, button:visible, input:visible').count(), 0);
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth) <= width);
    }
    // A failed owned read keeps the drop-down and shows the server error once.
    await f.page.route(f.live.root + '/api/access', route => route.fulfill({status: 500,
      contentType: 'application/json', body: JSON.stringify({error: 'owned read unavailable'})}));
    await f.page.reload();
    await f.page.locator('#workspace-error').filter({hasText: 'owned read unavailable'}).waitFor();
    assert.ok(await f.page.locator('#workspace-select option').count() >= 2);
    assert.equal(await f.page.locator('#owner-panel').isVisible(), false);
  } finally { await f.close(); }
});

// owner-manages-workspace-ac3
test('owner-manages-workspace-ac3: alex removes a member, an email and a domain in Settings', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  const person = await shared.browser.newContext({reducedMotion: 'reduce'});
  try {
    const request = f.page.context().request;
    const {id} = await (await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Shared ' + crypto.randomUUID()}})).json();
    // The person signs in with the granted email and becomes a member.
    assert.equal((await request.post(f.live.root + '/api/email-grants', {headers: {Origin: f.live.root}, data: {workspace: id, email: 'alex@example.com'}})).status(), 200);
    await person.addCookies([{name: 'joppa_session', value: f.live.person_cookie, url: f.live.root}]);
    const page = await person.newPage();
    await page.goto(f.live.root + '/?workspace=' + encodeURIComponent(id));
    await page.locator('#workspaces').waitFor();
    const personRead = async () => (await person.request.get(f.live.root + '/api/workspaces/' + id)).status();
    assert.equal(await personRead(), 200);
    for (const data of [{workspace: id, email: 'lena@example.com'}]) await request.post(f.live.root + '/api/email-grants', {headers: {Origin: f.live.root}, data});
    await request.post(f.live.root + '/api/domain-grants', {headers: {Origin: f.live.root}, data: {workspace: id, domain: 'example.org'}});
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const select = f.page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await select.selectOption(id);
    const rows = f.page.locator('#workspace-people li');
    const texts = () => rows.locator(':scope > span:first-child').allInnerTexts();
    assert.equal((await texts()).length, 4, JSON.stringify(await texts()));
    // The owner row has no Remove.
    assert.equal(await rows.first().locator('button').count(), 0);
    // Remove the member: the person loses the workspace at the next request.
    const member = rows.filter({hasText: 'alex@example.com'}).filter({hasNot: f.page.locator('text=(invited)')});
    await member.getByRole('button', {name: /^Remove /}).click();
    await member.waitFor({state: 'detached'});
    assert.equal(await personRead(), 404);
    // Remove the invited email and the domain.
    await rows.filter({hasText: 'lena@example.com (invited)'}).getByRole('button', {name: /^Remove /}).click();
    await rows.filter({hasText: 'lena@example.com'}).waitFor({state: 'detached'});
    await rows.filter({hasText: 'Everyone at example.org'}).getByRole('button', {name: /^Remove /}).click();
    await rows.filter({hasText: 'example.org'}).waitFor({state: 'detached'});
    assert.equal((await texts()).length, 1);
    const access = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    assert.deepEqual([access.emails, access.domains, access.members.length], [[], [], 1]);
    // The owner cannot remove themself, and a member who is not the owner removes nothing.
    const self = await request.post(f.live.root + '/api/access/remove', {headers: {Origin: f.live.root}, data: {workspace: id, member: 'alex'}});
    assert.equal((await self.json()).error, 'workspace owner cannot be removed');
    const foreign = await request.post(f.live.root + '/api/access/remove', {headers: {Origin: f.live.root}, data: {workspace: f.live.ws, member: 'bot'}});
    assert.equal((await foreign.json()).error, 'workspace owner required');
  } finally { await person.close(); await f.close(); }
});

// owner-manages-workspace-ac1
test('owner-manages-workspace-ac1: alex renames an owned workspace in Settings', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  try {
    const request = f.page.context().request;
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Old name'}});
    const {id} = await created.json();
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const select = f.page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await select.selectOption(id);
    // The pencil next to the drop-down opens the rename form with the current title.
    await f.page.locator('header').getByRole('button', {name: 'Rename workspace'}).click();
    const title = f.page.getByLabel('New title'), rename = f.page.getByRole('button', {name: 'Rename', exact: true});
    assert.equal(await title.inputValue(), 'Old name');
    // A blank title shows the server error and keeps the name.
    await title.fill('  ');
    await rename.click();
    await f.page.locator('#rename-error').filter({hasText: 'text must not be empty'}).waitFor();
    await title.fill('New name');
    const saved = f.page.waitForResponse(r => r.url().endsWith('/rename') && r.request().method() === 'POST');
    await rename.click();
    assert.equal((await saved).status(), 200);
    await select.locator('option[value="' + id + '"]').filter({hasText: 'New name'}).waitFor({state: 'attached'});
    assert.equal(await select.inputValue(), id);
    const listed = (await (await request.get(f.live.root + '/api/workspaces')).json()).filter(w => w.id === id);
    assert.deepEqual(listed, [{id, title: 'New name'}]);
    await f.page.locator('#rename-workspace').waitFor({state: 'hidden'});
    // W2 is not owned by alex, so the pencil is gone.
    await select.selectOption(f.live.ws);
    assert.equal(await f.page.getByRole('button', {name: 'Rename workspace'}).isVisible(), false);
    // A member who is not the owner cannot rename W2.
    const refused = await request.post(f.live.root + '/api/workspaces/' + encodeURIComponent(f.live.ws) + '/rename',
      {headers: {Origin: f.live.root}, data: {title: 'Taken'}});
    assert.notEqual(refused.status(), 200);
    assert.ok(!(await (await request.get(f.live.root + '/api/workspaces')).json()).some(w => w.title === 'Taken'));
  } finally { await f.close(); }
});

// owner-manages-workspace-ac2
test('owner-manages-workspace-ac2: alex deletes an owned workspace in Settings and nobody reads it', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  try {
    const request = f.page.context().request;
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Doomed ' + crypto.randomUUID()}});
    const {id, title} = await created.json();
    const added = await fetch(f.live.root + '/api/workspaces/' + encodeURIComponent(id) + '/commands', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
      body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'member', id: 'owner', role: 'human'}})});
    assert.equal(added.status, 200, await added.text());
    assert.equal((await fetch(f.live.root + '/api/workspaces/' + id, {headers: {Authorization: 'Bearer ' + f.live.owner}})).status, 200);
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const select = f.page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await select.selectOption(id);
    // The first click only asks; Cancel keeps the workspace.
    await f.page.getByRole('button', {name: 'Delete workspace'}).click();
    await f.page.getByRole('button', {name: 'Cancel'}).click();
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + id)).status(), 200);
    await f.page.getByRole('button', {name: 'Delete workspace'}).click();
    const deleted = f.page.waitForResponse(r => r.url().endsWith('/delete') && r.request().method() === 'POST');
    await f.page.getByRole('button', {name: 'Delete for everyone'}).click();
    assert.equal((await deleted).status(), 200);
    await select.locator('option[value="' + id + '"]').waitFor({state: 'detached'});
    // Neither the owner nor the other member lists or reads it.
    assert.ok(!(await (await request.get(f.live.root + '/api/workspaces')).json()).some(w => w.id === id || w.title === title));
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + id)).status(), 404);
    assert.equal((await fetch(f.live.root + '/api/workspaces/' + id, {headers: {Authorization: 'Bearer ' + f.live.owner}})).status, 404);
    // A member who is not the owner cannot delete W2.
    const refused = await request.post(f.live.root + '/api/workspaces/' + encodeURIComponent(f.live.ws) + '/delete', {headers: {Origin: f.live.root}});
    assert.notEqual(refused.status(), 200);
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + f.live.ws)).status(), 200);
  } finally { await f.close(); }
});

test('settings-in-name-menu-ac1: open does not wait for the response body of a page that it left', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}, 'domain:e': {title: 'Other domain'}});
  // This server sends the headers of an /api/ response and never ends its body, like a fetch that a navigation cuts off.
  const sockets = new Set();
  const stalled = http.createServer((request, response) => {
    response.writeHead(200, {'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*'});
    response.write('{');
  });
  stalled.on('connection', socket => sockets.add(socket));
  stalled.listen(0, '127.0.0.1');
  await once(stalled, 'listening');
  try {
    const url = 'http://127.0.0.1:' + stalled.address().port + '/api/stalled';
    await f.open('domain:d');
    const headers = f.page.waitForResponse(url);
    await f.page.evaluate(target => { fetch(target).catch(() => {}); }, url);
    await headers;
    assert.ok(f.requests.some(entry => entry.url === url));
    // The old document still waits for the body. The new document must open without it.
    await f.open('domain:e');
  } finally {
    for (const socket of sockets) socket.destroy();
    stalled.close();
    await f.close();
  }
});
};
