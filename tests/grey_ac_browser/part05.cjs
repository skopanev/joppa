// Part 5 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

test('create-workspace-ac1: alex creates Billing on the settings page and owns it', async () => {
  const f = await fixture({'domain:d': {title: 'Existing domain'}});
  try {
    const request = f.page.context().request;
    const titles = async () => (await (await request.get(f.live.root + '/api/workspaces')).json()).map(w => w.title);
    assert.ok(!(await titles()).includes('Billing'));
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    // New workspace in the header opens the form.
    await f.page.getByRole('button', {name: '+ New workspace'}).click();
    const title = f.page.getByLabel('Workspace title'), create = f.page.getByRole('button', {name: 'Create'});
    // A failed request keeps the form and shows the server error.
    await title.fill('  ');
    await create.click();
    await f.page.getByRole('alert').filter({hasText: 'text must not be empty'}).waitFor();
    assert.equal(await create.isDisabled(), false);
    assert.equal(await title.inputValue(), '  ');
    // Hold the creation to observe the pending state.
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route(f.live.root + '/api/workspaces', async route => { await held; await route.continue(); });
    await title.fill('Billing');
    await create.click();
    assert.equal(await create.isDisabled(), true);
    const board = f.page.waitForResponse(response => /\/api\/workspaces\/[^/]+\/board$/.test(response.url()));
    release();
    const opened = await board;
    assert.equal(opened.status(), 200);
    const id = new URL(f.page.url()).searchParams.get('workspace');
    assert.ok(opened.url().endsWith('/api/workspaces/' + encodeURIComponent(id) + '/board'));
    assert.notEqual(id, f.live.ws);
    assert.equal(await f.page.locator('#workspaces').inputValue(), id);
    // The access rules name only the fixture workspace, so ownership gives alex Billing.
    const list = await (await request.get(f.live.root + '/api/workspaces')).json();
    assert.deepEqual(list.filter(w => w.title === 'Billing'), [{id, title: 'Billing'}]);
    const read = await request.get(f.live.root + '/api/workspaces/' + id);
    assert.equal(read.status(), 200);
    const index = (await read.json()).index;
    assert.equal(index.owner, 'alex');
    assert.deepEqual(Object.keys(index.members), ['alex']);
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + id + '/board')).status(), 200);
    // The fixture owner is not a member of Billing.
    const other = await fetch(f.live.root + '/api/workspaces/' + id, {headers: {Authorization: 'Bearer ' + f.live.owner}});
    assert.equal(other.status, 404);
    assert.deepEqual(await other.json(), {error: 'Workspace not found'});
  } finally { await f.close(); }
});

test('owner-adds-members-ac1: alex adds maria@example.com to an owned workspace in Settings', async () => {
  const f = await fixture({'domain:d': {title: 'Existing domain'}});
  try {
    const request = f.page.context().request;
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Team space'}});
    assert.equal(created.status(), 200);
    const {id} = await created.json();
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const workspace = f.page.getByLabel('Workspace', {exact: true}), email = f.page.getByLabel('Email or domain');
    const add = f.page.getByRole('button', {name: 'Add', exact: true});
    await workspace.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    // The fixture workspace is offered, but alex does not own it, so it has no add field.
    await workspace.selectOption(f.live.ws);
    await f.page.locator('#not-owner').waitFor();
    assert.equal(await email.isVisible(), false);
    await workspace.selectOption(id);
    // A value with "@" is an email, so a malformed email gets the email error.
    await email.fill('maria@');
    await add.click();
    await f.page.getByRole('alert').filter({hasText: 'a valid email is required'}).waitFor();
    assert.equal(await add.isDisabled(), false);
    await email.fill('maria@example.com');
    const granted = f.page.waitForResponse(response => response.url().endsWith('/api/email-grants') && response.request().method() === 'POST');
    await add.click();
    assert.equal((await granted).status(), 200);
    const emails = f.page.locator('#workspace-people li[data-kind="email"] > span');
    await emails.filter({hasText: 'maria@example.com'}).waitFor();
    assert.deepEqual(await emails.allInnerTexts(), ['maria@example.com (invited)']);
    assert.equal(await workspace.inputValue(), id);
    const listed = (await (await request.get(f.live.root + '/api/email-grants')).json()).items.filter(item => item.id === id);
    assert.deepEqual(listed, [{id, title: 'Team space', emails: ['maria@example.com']}]);
  } finally { await f.close(); }
});

// board-opens-top-level-ac1
test('board-opens-top-level-ac1: a reload of the board shows only the top level', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}, 'capability:c': {title: 'Capability', parent: 'domain:d'}});
  try {
    assert.equal((await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws))).status(), 200);
    const top = f.page.locator('#tree .common-object[data-object-address="domain:d"]');
    await top.waitFor();
    // Alex opens the domain and sees the capability below it.
    await top.locator(':scope > summary').click();
    await top.locator('.common-object[data-object-address="capability:c"]').waitFor();
    assert.equal(await top.evaluate(e => e.open), true);
    await f.page.reload();
    await top.waitFor();
    await f.page.waitForFunction(() => document.querySelector('#tree .common-object') !== null);
    // After the reload no branch is open.
    assert.equal(await f.page.locator('#tree .common-object[open]').count(), 0);
    assert.equal(await f.page.evaluate(() => Object.keys(sessionStorage).filter(k => k.startsWith('joppa-open-')).length), 0);
  } finally { await f.close(); }
});

// type-status-rules-ac4
test('type-status-rules-ac4: the board dot of each node has the colour of its status and R has a label that agrees', async () => {
  const f = await fixture({});
  try {
    const tree = await f.api('/tree');
    const types = structuredClone(tree.schema.types);
    types.area = {title: 'Areas', role: 'group', root: true, parents: ['area'], status: 'children'};
    types.rule = {title: 'Rules', role: 'requirement', parents: ['area'], status: 'children'};
    types.check = {title: 'Checks', role: 'ac', parents: ['rule'], status: 'children'};
    types.test = {...types.test, parents: [...types.test.parents, 'check']};
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, types, reason: 'Set the status rules'}, true);
    for (const [address, parent] of [['area:g1', null], ['area:g2', 'area:g1'], ['rule:r', 'area:g2'], ['check:a', 'rule:r'], ['check:b', 'rule:r']]) {
      await f.save(address, {title: address.split(':')[1].toUpperCase(), parent});
    }
    const nodes = ['area:g1', 'area:g2', 'rule:r'];
    let order = 0;
    const report = cases => f.record('c' + String(++order).padStart(6, '0'), cases);
    const labels = [];
    // Each dot shows the tone of the API in the light and the dark theme.
    const check = async (fixture, expected) => {
      for (const theme of ['light', 'dark']) {
        await f.page.goto(f.live.root + '/settings');
        await f.page.evaluate(t => localStorage.setItem('theme', t), theme);
        for (const [i, address] of nodes.entries()) {
          const read = await f.api('/objects?address=' + encodeURIComponent(address));
          assert.equal(read.signal.tone, expected[i], fixture + ' ' + address);
          const card = await f.open(address);
          const dot = card.locator(':scope > summary .status-circle').first();
          assert.equal(await dot.getAttribute('data-tone'), expected[i], fixture + ' ' + theme + ' ' + address);
          const colours = await dot.evaluate(e => {
            const probe = document.createElement('i');
            probe.style.color = 'var(--dot-' + e.dataset.tone + ')';
            document.body.append(probe);
            const want = getComputedStyle(probe).color;
            probe.remove();
            return [getComputedStyle(e).backgroundColor, want];
          });
          assert.equal(colours[0], colours[1], fixture + ' ' + theme + ' ' + address);
          if (address === 'rule:r') {
            const label = (await dot.getAttribute('aria-label')).split('\n')[0];
            // The dot shows the label of the API, and the label agrees with the tone.
            assert.equal(label, 'Status: ' + read.signal.label, fixture + ' ' + theme);
            labels.push(expected[i] + ': ' + label);
          }
        }
      }
    };
    await f.bind('check:a', ['a']);
    await report([['a']]);
    // B has no test node, so B is orange and R takes orange over the green A.
    await check('fixture 1', ['orange', 'orange', 'orange']);
    await f.bind('check:b', ['b']);
    await check('fixture 2', ['blue', 'blue', 'blue']);
    await report([['a'], ['b']]);
    await check('fixture 3', ['green', 'green', 'green']);
    await report([['a'], ['b', 'boom']]);
    await check('fixture 4', ['red', 'red', 'red']);
    await f.bind('check:a', []);
    await check('fixture 5', ['red', 'red', 'red']);
    await f.bind('check:a', ['a']);
    await report([['a'], ['b', 'boom']]);
    await f.save('rule:r2', {title: 'R2', parent: 'area:g2'});
    await f.save('check:d', {title: 'D', parent: 'rule:r2'});
    // One rollup decides every tone: the orange R2 does not put orange over red.
    await check('fixture 6', ['red', 'red', 'red']);
    // An orange R says that a child has no tests, a blue R that its children await checks, a
    // green R that its children passed, and a red R that a child failed, in every theme.
    assert.deepEqual([...new Set(labels)], [
      'orange: Status: A child has no tests',
      'blue: Status: Children await checks',
      'green: Status: Children checks passed',
      'red: Status: Child checks failed',
    ], JSON.stringify(labels));
  } finally { await f.close(); }
});

};
