// Part 2 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

test('account-menu: sign out requires the user menu and ends the real session', async () => {
  const configured = {...types,
    incoming: {title: 'Inbox', role: 'document', status:'none', root: true, parents: [], inbox: true},
    audience: {title: 'Consumers', role: 'document', status:'none', root: true, parents: []},
  };
  const f = await fixture({'domain:d': {title: 'Domain'},
    'incoming:one': {title: 'Incoming change'}, 'audience:one': {title: 'Product user'}}, configured);
  try {
    await f.open('domain:d');
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.locator('#identity').focus();
    await f.page.keyboard.press('Enter');
    assert.equal(await f.page.locator('#logout').isVisible(), true);
    assert.deepEqual(await f.page.locator('.account-options > *').allTextContents(), ['Settings', 'Sign out']);
    await f.page.keyboard.press('Escape');
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.locator('#identity').click();
    await f.page.locator('.brand').click();
    assert.equal(await f.page.locator('#logout').isVisible(), false);
    await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    await f.page.locator('#tab-domain').waitFor();
    const closedMenu = async () => {
      assert.equal(await f.page.locator('#account-menu').getAttribute('open'), null);
      assert.equal(await f.page.locator('.account-options').isVisible(), false);
      assert.equal(await f.page.locator('#logout').isVisible(), false);
    };
    await f.page.locator('#identity').click();
    await f.page.locator('#tab-incoming').click();
    await f.page.locator('#collection-incoming .common-object').waitFor();
    await closedMenu();
    await f.page.locator('#identity').click();
    await f.page.locator('#tab-audience').click();
    await f.page.locator('#collection-audience .common-object').waitFor();
    await closedMenu();
    await f.page.locator('#identity').click();
    await f.page.locator('#tab-domain').focus();
    await f.page.keyboard.press('Home');
    await closedMenu();
    await f.page.evaluate(() => history.pushState(null, '', '#menu-history-check'));
    await f.page.locator('#identity').click();
    await f.page.evaluate(() => history.back());
    await f.page.waitForFunction(() => location.hash === '' && !document.getElementById('account-menu').open);
    await closedMenu();
    await f.page.locator('#identity').click();
    await f.open('domain:d');
    await closedMenu();
    await f.page.goBack();
    await f.page.locator('#tab-domain').waitFor();
    await closedMenu();
    await f.page.setViewportSize({width: 390, height: 844});
    await f.page.locator('#identity').click();
    const menu = await f.page.locator('.account-options').boundingBox();
    assert.ok(menu.x >= 0 && menu.x + menu.width <= 390);
    const logout = f.page.waitForResponse(r => new URL(r.url()).pathname === '/auth/logout');
    await f.page.locator('#logout').click();
    assert.equal((await logout).status(), 200);
    await f.page.locator('#login-button').waitFor({state: 'visible'});
    assert.equal(await f.page.locator('.app-header').isVisible(), false);
    assert.ok(!(await f.page.context().cookies()).some(cookie => cookie.name === 'joppa_session'));
  } finally { await f.close(); }
});

test('direct-child-counts: each row uses the shared next-level count', async () => {
  const configured = {
    bundle: {title: 'Bundles', role: 'group', status:'children', root: true, parents: []},
    branch: {title: 'Branches', role: 'group', status:'children', parents: ['bundle']},
    sheet: {title: 'Sheets', role: 'requirement', status:'children', parents: ['branch']},
    proof: {title: 'Proofs', role: 'ac', status:'tests', parents: ['sheet']},
    extra: {title: 'Extra', role: 'document', status:'none', parents: ['bundle']},
  };
  const records = {
    'bundle:root': {title: 'Root'},
    'branch:good': {title: 'Good', parent: 'bundle:root'},
    'branch:empty': {title: 'Empty', parent: 'bundle:root'},
    'branch:bad': {title: 'Bad', parent: 'bundle:root'},
    'extra:raw': {title: 'Unrelated', parent: 'bundle:root'},
    'sheet:one': {title: 'First sheet', parent: 'branch:good'},
    'sheet:two': {title: 'Second sheet', parent: 'branch:good'},
    'sheet:bad': {title: 'Failed sheet', parent: 'branch:bad'},
    'proof:a': {title: 'First proof', parent: 'sheet:one'},
    'proof:b': {title: 'Second proof', parent: 'sheet:two'},
    'proof:bad': {title: 'Failed proof', parent: 'sheet:bad'},
  };
  const f = await fixture(records, configured);
  try {
    await f.bind('proof:a', ['pass']); await f.bind('proof:b', ['pass']); await f.bind('proof:bad', ['fail']);
    await f.record('cccccccc', [['pass'], ['fail', 'failed check']]);
    const card = await f.open('bundle:root');
    assert.equal(await card.locator(':scope > summary .verification-chip').innerText(), '1 / 3 Branches');
    assert.equal(await card.locator('.common-object[data-object-address="branch:good"] > summary .verification-chip').innerText(), '2 / 2 Sheets');
    assert.equal(await card.locator('.common-object[data-object-address="branch:empty"] > summary .verification-chip').innerText(), '0 Sheets');
    assert.equal(f.requests.filter(r => new URL(r.url).pathname.endsWith('/tests')).length, 0);
  } finally { await f.close(); }
});

test('child-heading: a repeated parent title shows the configured type and status', async () => {
  const f = await fixture({'domain:d': {title: 'Shared title'},
    'capability:c': {title: 'Shared title', parent: 'domain:d'},
    'capability:other': {title: 'Different title', parent: 'domain:d'}});
  try {
    const card = await f.open('domain:d');
    const titles = card.locator(':scope > .object-body > .object-children > .common-object > summary > .node-title');
    await titles.nth(1).waitFor();
    assert.deepEqual(await titles.allTextContents(), ['Capabilities · Current document', 'Different title']);
  } finally { await f.close(); }
});

test('lazy-children-ac1: a capability reads only its twenty direct requirements', async () => {
  const records = {'domain:d': {title: 'Domain'}, 'capability:X': {title: 'Capability', parent: 'domain:d'}};
  for (let index = 0; index < 20; index++) {
    const id = String(index).padStart(2, '0');
    records['requirement:r' + id] = {title: 'Requirement ' + id, parent: 'capability:X'};
    records['ac:a' + id] = {title: 'Criterion ' + id, parent: 'requirement:r' + id};
  }
  const f = await fixture(records);
  try {
    const card = await f.open('capability:X');
    await card.locator(':scope > .object-body > .object-children > .common-object').nth(19).waitFor();
    await Promise.all(f.requests.map(entry => entry.body));
    const children = f.requests.filter(r => new URL(r.url).searchParams.get('parent') === 'capability:X');
    assert.equal(children.length, 1);
    assert.equal(children[0].value.items.length, 20);
    assert.ok(children[0].value.items.every(item => item.kind === 'requirement'));
    assert.equal(f.requests.filter(r => new URL(r.url).searchParams.get('address')?.startsWith('ac:')).length, 0);
    assert.equal(f.requests.filter(r => new URL(r.url).searchParams.get('address')?.startsWith('requirement:')).length, 0);
  } finally { await f.close(); }
});

test('old-results-history-ac2: the criterion shows both results and the failure message', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const f = await fixture(criterionRecords);
  try {
    await f.bind('ac:A', ['X', 'Y']);
    await f.record(commit, [['X'], ['Y', 'expected 403, got 200']]);
    const card = await f.open('ac:A'), group = card.locator('.object-tests');
    await group.locator(':scope > summary').click();
    await group.locator('.ac-check').nth(1).waitFor();
    const rows = group.locator('.ac-check');
    assert.equal(await rows.count(), 2);
    assert.match(await rows.nth(0).innerText(), /checks.cjs > X\s+pass · commit ccccccc/);
    assert.match(await rows.nth(1).innerText(), /checks.cjs > Y\s+fail · commit ccccccc/);
    assert.equal(await rows.nth(1).locator('pre').innerText(), 'expected 403, got 200');
  } finally { await f.close(); }
});

test('stable-url-ac1: the original URL keeps the item ID after rename and move', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'},
    'capability:old': {title: 'Old capability', parent: 'domain:d'},
    'capability:new': {title: 'New capability', parent: 'domain:d'},
    'requirement:r': {title: 'Original title', parent: 'capability:old'}});
  try {
    let card = await f.open('requirement:r');
    const originalUrl = f.page.url(), original = await f.api('/objects?address=requirement:r');
    await card.locator(':scope > summary .copy').click();
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), [original.item_id]);
    await f.save('requirement:r', {title: 'Renamed requirement', parent: 'capability:new'}, 1);
    const response = await f.page.goto(originalUrl);
    assert.equal(response.status(), 200);
    card = f.page.locator('.object-page > .common-object[data-object-address="requirement:r"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
    assert.equal(await card.locator(':scope > summary .node-title').innerText(), 'Renamed requirement');
    await card.locator(':scope > summary .copy').click();
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), [original.item_id]);
    const current = await f.api('/objects?address=requirement:r');
    assert.equal(current.document.parent, 'capability:new');
    assert.equal(current.item_id, original.item_id);
  } finally { await f.close(); }
});

test('time-not-stale-ac2: the oldest test commit shows five newer commits', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const f = await fixture(criterionRecords);
  try {
    await f.bind('ac:A', ['X', 'Y']);
    await f.record(commit, [['X'], ['Y']]);
    for (let order = 2; order <= 6; order++) await f.record(String(order).repeat(40), [['other']]);
    const card = await f.open('ac:A'), group = card.locator('.object-tests');
    await group.locator(':scope > summary').click();
    await group.locator('.ac-check').nth(1).waitFor();
    assert.match(await group.locator(':scope > .node-meta').innerText(), /commit ccccccc · 5 commits behind/);
  } finally { await f.close(); }
});

test('done-is-not-green-ac3: a done Task under a green criterion shows Checks passed', async () => {
  const configured = {...types, task: {title: 'Tasks', role: 'task', status:'none', parents: ['ac']}};
  const f = await fixture(criterionRecords, configured);
  try {
    await f.bind('ac:A', ['X']);
    await f.command({op: 'save_object', kind: 'task', id: 'T', revision: 0, parent: 'ac:A',
      data: {title: 'Implement the criterion', owner: 'owner', contract: 'Implement the criterion and record the result', purpose: 'implementation'},
      reason: 'Check completion under a green criterion'});
    await f.command({op: 'claim_task', id: 'task:T', revision: 1});
    await f.command({op: 'complete_task', commit: 'c0ffee0', id: 'task:T', revision: 2, evidence: 'Implemented the criterion'});
    await f.record('dddddddddddddddddddddddddddddddddddddddd', [['X']]);
    const card = await f.open('ac:A');
    const circle = card.locator('.common-object[data-object-address="task:T"] > summary .status-circle');
    assert.equal(await circle.getAttribute('data-tone'), 'green');
    assert.match(await circle.getAttribute('data-hint'), /^Status: Work complete(\n|$)/);
  } finally { await f.close(); }
});
};
