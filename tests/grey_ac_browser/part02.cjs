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

test('child-heading: a repeated parent title shows the configured type and status', async () => {
  const f = await fixture({'domain:d': {title: 'Shared title'},
    'capability:c': {title: 'Shared title', parent: 'domain:d'},
    'capability:other': {title: 'Different title', parent: 'domain:d'}});
  try {
    const card = await f.open('domain:d');
    const titles = card.locator(':scope > .object-body > .object-children > .common-object > summary > .node-title');
    await titles.nth(1).waitFor();
    // A capability without children is orange, so its status reads No children.
    assert.deepEqual(await titles.allTextContents(), ['Capabilities · No children', 'Different title']);
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
    const [x, y] = await f.bind('ac:A', ['X', 'Y']);
    await f.record(commit, [['X'], ['Y', 'expected 403, got 200']]);
    const card = await f.open('ac:A');
    const rows = card.locator(':scope > .object-body > .object-children > .common-object');
    await rows.nth(1).waitFor();
    // Each test is a child row of the criterion: its title is the test name and its dot the result.
    const shown = await rows.evaluateAll(list => list.map(row => [row.dataset.objectAddress,
      row.querySelector(':scope > summary .node-title').textContent, row.querySelector(':scope > summary .status-circle').dataset.tone]));
    assert.deepEqual(shown.sort(), [[x, 'checks.cjs > X', 'green'], [y, 'checks.cjs > Y', 'red']].sort());
    // The Result section of each test row gives the status, the commit and the failure message.
    const result = async address => {
      const row = card.locator('.common-object[data-object-address="' + address + '"]');
      await row.locator(':scope > summary').click({position: {x: 4, y: 4}});
      const group = row.locator(':scope > .object-body .test-result');
      await group.waitFor({state: 'attached'});
      if (await group.getAttribute('open') === null) await group.locator(':scope > summary').click();
      return group;
    };
    const passed = await result(x), failed = await result(y);
    assert.equal(await passed.locator('.test-result-line').innerText(), 'pass · commit ccccccc');
    assert.equal(await passed.locator('pre.test-message').count(), 0);
    assert.equal(await failed.locator('.test-result-line').innerText(), 'fail · commit ccccccc');
    assert.equal(await failed.locator('pre.test-message').innerText(), 'expected 403, got 200');
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

test('oldest-commit-card: a test node shows the commit of its own result after newer records of other tests', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const f = await fixture(criterionRecords);
  try {
    const [x] = await f.bind('ac:A', ['X', 'Y']);
    await f.record(commit, [['X'], ['Y']]);
    for (let order = 2; order <= 6; order++) await f.record(String(order).repeat(40), [['other']]);
    // A record changes only the tests that it names: X keeps the result of the first commit.
    const read = await f.api('/objects?address=' + encodeURIComponent(x));
    assert.deepEqual([read.result.status, read.result.commit], ['pass', commit]);
    const card = await f.open(x), group = card.locator(':scope > .object-body .test-result');
    await group.waitFor({state: 'attached'});
    if (await group.getAttribute('open') === null) await group.locator(':scope > summary').click();
    const line = await group.locator('.test-result-line').innerText();
    assert.equal(line, 'pass · commit ccccccc');
    assert.doesNotMatch(await card.locator(':scope > .object-body').innerText(), /behind/);
  } finally { await f.close(); }
});

};
