// Part 3 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

test('pins-ignore-process-events-ac4: the board marks a link changed only when the server asks for review', async () => {
  const configured = {...types, task: {title: 'Tasks', role: 'task', status:'none', parents: ['ac']}};
  const f = await fixture(criterionRecords, configured);
  const task = (id, contract, revision = 0, links = []) => f.command({op: 'save_object', kind: 'task', id, revision, parent: 'ac:A',
    data: {title: 'Task ' + id, owner: 'owner', contract, purpose: 'implementation'}, links, reason: 'Define the Task'});
  const meta = async () => {
    const card = await f.open('task:B');
    const text = await card.locator(':scope > .object-body .object-links').textContent();
    assert.match(text, /Links to · depends_on · revision 1/);
    return {text};
  };
  try {
    await task('A', 'Implement the first part');
    await task('B', 'Implement the second part', 0, [{target: 'task:A', relation: 'depends_on', revision: 1, blocking: true}]);
    // Process events move A to revision 3; the server keeps the pin of B current.
    await f.command({op: 'claim_task', id: 'task:A', revision: 1});
    await f.command({op: 'release_task', id: 'task:A', revision: 2, reason: 'Hand the work back'});
    let read = await f.mcp('joppa_objects', {address: 'task:B'});
    assert.equal(read.review_required, undefined, JSON.stringify(read));
    assert.doesNotMatch((await meta()).text, /changed/);
    // An authored contract change of A asks for review of B.
    const a = await f.mcp('joppa_objects', {address: 'task:A'});
    await task('A', 'Implement the first part and record the result', a.revision);
    read = await f.mcp('joppa_objects', {address: 'task:B'});
    assert.equal(read.review_required, true, JSON.stringify(read));
    assert.match((await meta()).text, /Links to · depends_on · revision 1 · changed/);
  } finally { await f.close(); }
});

test('retired-tasks-hidden-ac1: the board tree hides a retired Task and its page still opens', async () => {
  const configured = {...types, task: {title: 'Tasks', role: 'task', status:'none', parents: ['ac']}};
  const f = await fixture(criterionRecords, configured);
  try {
    for (const id of ['open', 'old']) await f.command({op: 'save_object', kind: 'task', id, revision: 0, parent: 'ac:A',
      data: {title: 'Task ' + id, owner: 'owner', contract: 'Implement the ' + id + ' part', purpose: 'implementation'}, reason: 'Define the Task'});
    await f.command({op: 'retire_task', id: 'task:old', revision: 1, reason: 'A new Task replaces this work'}, true);
    const card = await f.open('ac:A');
    await card.locator('.common-object[data-object-address="task:open"]').waitFor();
    assert.equal(await card.locator('.common-object[data-object-address="task:old"]').count(), 0);
    const retired = await f.open('task:old');
    const circle = retired.locator(':scope > summary .status-circle');
    assert.match(await circle.getAttribute('data-hint'), /^Status: Retired(\n|$)/);
  } finally { await f.close(); }
});

test('tests-only-from-reports-ac1: object pages and board rows show no legacy checks block', async () => {
  const configured = {...types, task: {title: 'Tasks', role: 'task', status:'none', parents: ['ac']}};
  const f = await fixture(criterionRecords, configured);
  try {
    await f.bind('ac:A', ['X']);
    await f.command({op: 'save_object', kind: 'task', id: 'T', revision: 0, parent: 'ac:A',
      data: {title: 'Implement the criterion', owner: 'owner', contract: 'Implement the criterion and record the result', purpose: 'implementation'},
      reason: 'Check the object page under a criterion'});
    await f.record('dddddddddddddddddddddddddddddddddddddddd', [['X']]);
    const addresses = ['requirement:r', 'ac:A', 'task:T'];
    const noLegacy = async (card, address) => {
      const sections = await card.locator(':scope > .object-body > .object-secondary > .object-section > summary').allTextContents();
      assert.ok(!sections.some(title => /^Checks|run plan/i.test(title)), address + ': ' + sections.join(', '));
      assert.equal(await card.locator('.object-checks,.verification-record').count(), 0, address);
      const text = await card.locator(':scope > .object-body').textContent();
      for (const forbidden of ['Expected', 'Observed', 'Missing result', 'Not checked']) assert.ok(!text.includes(forbidden), address + ': ' + forbidden);
    };
    const testsShown = async card => {
      const group = card.locator(':scope > .object-body > .object-secondary > .object-tests');
      await group.locator(':scope > summary').filter({hasText: /^Tests 1$/}).waitFor();
      if (await group.getAttribute('open') === null) await group.locator(':scope > summary').click();
      await group.locator('.ac-check').waitFor();
      assert.match(await group.locator('.ac-check').innerText(), /checks.cjs > X\s+pass · commit ddddddd/);
    };
    for (const address of addresses) {
      const card = await f.open(address);
      await noLegacy(card, address);
      if (address === 'ac:A') await testsShown(card);
    }
    await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    const cards = {};
    for (const address of ['domain:d', 'capability:c', ...addresses]) {
      const card = f.page.locator('#tree .common-object[data-object-address="' + address + '"]');
      await card.waitFor();
      if (await card.getAttribute('open') === null) await card.locator(':scope > summary').click({position: {x: 4, y: 4}});
      await card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
      cards[address] = card;
    }
    await Promise.all(f.requests.map(entry => entry.body));
    for (const address of addresses) await noLegacy(cards[address], address);
    await testsShown(cards['ac:A']);
  } finally { await f.close(); }
});

test('removal-at-once-ac4: a removed member reloads without workspace data', async () => {
  const secret = 'Private source for the browser member';
  const f = await fixture({'domain:d': {title: 'Private domain', body: secret}});
  try {
    const card = await f.open('domain:d', false);
    assert.equal(await card.locator('.object-content').innerText(), secret);
    writeFileSync(f.live.access_file, JSON.stringify({emails: {'alex@example.test': []}}));
    await f.page.reload();
    await f.page.waitForLoadState('networkidle');
    assert.equal(await f.page.locator('.common-object').count(), 0);
    assert.ok(!(await f.page.locator('body').innerText()).includes(secret));
    assert.match(await f.page.locator('#tree').innerText(), /No workspace access/);
    const request = f.page.context().request;
    const response = await request.get(f.live.root + '/api/workspaces/' + f.live.ws);
    const absent = await request.get(f.live.root + '/api/workspaces/absent-' + f.live.ws);
    assert.equal(response.status(), 404);
    assert.equal(absent.status(), 404);
    assert.deepEqual(await response.json(), {error: 'Workspace not found'});
    assert.deepEqual(await response.json(), await absent.json());
  } finally { await f.close(); }
});

// Maria is new in each fixture and is a member of W1 and W2 only. Alex owns W1 and W2.
// Another person owns the control workspace W3.
test('settings-in-name-menu-ac1: Settings in the name menu opens the settings page with the member workspaces', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  const person = await shared.browser.newContext({reducedMotion: 'reduce'});
  const maria = await shared.browser.newContext({reducedMotion: 'reduce'});
  try {
    const create = async (context, title) => {
      const created = await context.request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title}});
      assert.equal(created.status(), 200);
      return (await created.json()).id;
    };
    const suffix = crypto.randomUUID(), w1 = 'W1 ' + suffix, w2 = 'W2 ' + suffix, w3 = 'W3 ' + suffix;
    const shared = [await create(f.page.context(), w1), await create(f.page.context(), w2)];
    // Alex adds maria as a human member. A Google person reads only granted workspaces, so alex also grants the email.
    for (const id of shared) {
      const added = await fetch(f.live.root + '/api/workspaces/' + encodeURIComponent(id) + '/commands', {method: 'POST',
        headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
        body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'member', id: f.live.maria, role: 'human'}})});
      assert.equal(added.status, 200, await added.text());
      const granted = await f.page.context().request.post(f.live.root + '/api/email-grants', {headers: {Origin: f.live.root},
        data: {workspace: id, email: f.live.maria_email}});
      assert.equal(granted.status(), 200);
    }
    await person.addCookies([{name: 'joppa_session', value: f.live.person_cookie, url: f.live.root}]);
    await create(person, w3);
    await maria.addCookies([{name: 'joppa_session', value: f.live.maria_cookie, url: f.live.root}]);
    // Maria is a member of W1 and W2, but not the owner.
    const access = (await (await f.page.context().request.get(f.live.root + '/api/access')).json()).items;
    for (const id of shared) {
      assert.deepEqual(access.find(w => w.id === id).members.filter(m => m.id === f.live.maria).map(m => m.owner), [false]);
    }
    const page = await maria.newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const inside = (box, width, height) => box.x >= 0 && box.y >= 0 && box.x + box.width <= width && box.y + box.height <= height;
    for (const {width, height} of [{width: 1280, height: 800}, {width: 390, height: 844}]) {
      await page.setViewportSize({width, height});
      assert.equal((await page.goto(f.live.root + '/?workspace=' + encodeURIComponent(shared[0]))).status(), 200);
      await page.locator('#identity').filter({hasText: /\S/}).waitFor();
      await page.locator('#identity').click();
      const items = page.locator('.account-options > *');
      assert.deepEqual(await items.allInnerTexts(), ['Settings', 'Sign out']);
      for (const item of await items.all()) assert.ok(inside(await item.boundingBox(), width, height));
      await items.first().click();
      await page.waitForURL(f.live.root + '/settings');
      // One drop-down holds the titles of W1 and W2 and no other workspace.
      const list = page.getByLabel('Workspace', {exact: true});
      await page.waitForFunction(() => !document.getElementById('workspace-select').disabled);
      assert.deepEqual((await list.locator('option').allInnerTexts()).sort(), [w1, w2]);
      assert.ok(inside(await list.boundingBox(), width, Infinity));
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth) <= width);
    }
    // A failed list read shows the server error and no list.
    await page.route(f.live.root + '/api/workspaces', route => route.request().method() === 'GET'
      ? route.fulfill({status: 500, contentType: 'application/json', body: JSON.stringify({error: 'workspace list unavailable'})})
      : route.continue());
    await page.reload();
    await page.getByRole('alert').filter({hasText: 'workspace list unavailable'}).waitFor();
    assert.equal(await page.locator('#workspace-select option').count(), 0);
    assert.equal(await page.locator('#workspace-panel').isVisible(), false);
    assert.deepEqual(errors, []);
  } finally { await maria.close(); await person.close(); await f.close(); }
});
};
