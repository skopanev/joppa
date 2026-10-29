// Part 6 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

// settings-in-name-menu-ac4
test('settings-in-name-menu-ac4: Settings follows the board theme, links back to the board and opens New workspace from the header', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  try {
    const created = await f.page.context().request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Themed space'}});
    const {id} = await created.json();
    // The board stores the dark theme; Settings opens dark at once.
    await f.page.goto(f.live.root + '/settings');
    await f.page.evaluate(() => localStorage.setItem('theme', 'dark'));
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    const background = () => f.page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await f.page.locator('meta[name="theme-color"]').getAttribute('content'), '#191c20');
    // One icon shows at a time: the sun in the dark theme.
    assert.deepEqual([await f.page.locator('#sun-svg').isVisible(), await f.page.locator('#moon-svg').isVisible()], [true, false]);
    // The theme button switches to light and stores the choice for the board.
    await f.page.getByRole('button', {name: 'Switch to light theme'}).click();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'light');
    // The header holds the workspace drop-down, the pencil and New workspace; the forms open on demand.
    const header = f.page.locator('header.app-header');
    assert.equal(await header.locator('.workspace-label #workspace-select').count(), 1);
    assert.equal(await header.getByRole('button', {name: '+ New workspace'}).isVisible(), true);
    assert.equal(await f.page.locator('#create-workspace').isVisible(), false);
    assert.equal(await f.page.locator('#rename-workspace').isVisible(), false);
    await header.getByRole('button', {name: '+ New workspace'}).click();
    assert.equal(await f.page.getByLabel('Workspace title').evaluate(e => e === document.activeElement), true);
    await f.page.locator('#create-workspace').getByRole('button', {name: 'Cancel'}).click();
    assert.equal(await f.page.locator('#create-workspace').isVisible(), false);
    // Back to board opens the selected workspace.
    const workspace = f.page.getByLabel('Workspace', {exact: true});
    await workspace.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await workspace.selectOption(id);
    assert.equal(await f.page.getByRole('link', {name: '← Back to board'}).getAttribute('href'), '/?workspace=' + encodeURIComponent(id));
    // A field and its button share one row, and Delete workspace uses the failure color.
    const input = await f.page.getByLabel('Email or domain').boundingBox(), add = await f.page.getByRole('button', {name: 'Add', exact: true}).boundingBox();
    assert.ok(Math.abs((input.y + input.height / 2) - (add.y + add.height / 2)) < 4);
    const fail = await f.page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--fail').trim());
    const color = await f.page.getByRole('button', {name: 'Delete workspace'}).evaluate(e => getComputedStyle(e).color);
    const probe = await f.page.evaluate(value => { const e = document.createElement('i'); e.style.color = value; document.body.append(e); const c = getComputedStyle(e).color; e.remove(); return c; }, fail);
    assert.equal(color, probe);
    await f.page.getByRole('link', {name: '← Back to board'}).click();
    await f.page.waitForURL(url => url.searchParams.get('workspace') === id);
  } finally { await f.close(); }
});

// workspace-drop-down-ac1
test('workspace-drop-down-ac1: the board drop-down shows no placeholder while the workspace list loads', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}});
  try {
    await f.open('domain:d');
    const title = await f.page.locator('#workspaces option:checked').innerText();
    // Hold the list on reload: the drop-down shows the last title and never "Choose a workspace".
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route(f.live.root + '/api/workspaces', async route => { await held; await route.continue(); });
    const reloaded = f.page.reload();
    // The board modules run after readyState leaves 'loading'. Wait for the first option that they add.
    await f.page.locator('#workspaces option').first().waitFor({state: 'attached'});
    assert.deepEqual(await f.page.locator('#workspaces option').allInnerTexts(), [title]);
    assert.ok(!(await f.page.content()).includes('Choose a workspace'));
    release();
    await reloaded;
    await f.page.unroute(f.live.root + '/api/workspaces');
  } finally { await f.close(); }
});

test('owner-adds-members-ac8: alex@example.com reloads the board and reads W with no new sign-in', async () => {
  const f = await fixture({'domain:d': {title: 'Existing domain'}});
  const person = await shared.browser.newContext({reducedMotion: 'reduce'});
  try {
    const request = f.page.context().request;
    const create = async title => {
      const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title}});
      assert.equal(created.status(), 200);
      return (await created.json()).id;
    };
    const id = await create('Shared space'), control = await create('Control space');
    await person.addCookies([{name: 'joppa_session', value: f.live.person_cookie, url: f.live.root}]);
    const page = await person.newPage(), signIns = [], errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('request', r => { if (new URL(r.url()).pathname.startsWith('/auth/')) signIns.push(r.url()); });
    const read = async workspace => {
      const response = await person.request.get(f.live.root + '/api/workspaces/' + workspace);
      return {status: response.status(), value: await response.json()};
    };
    // Before the grant the person opens W and has no access.
    await page.goto(f.live.root + '/?workspace=' + encodeURIComponent(id));
    await page.locator('#tree').filter({hasText: 'This workspace is not available to your account.'}).waitFor();
    assert.deepEqual(await read(id), {status: 404, value: {error: 'Workspace not found'}});
    // The owner adds alex@example.com to W in Settings.
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const workspace = f.page.getByLabel('Workspace', {exact: true});
    await workspace.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await workspace.selectOption(id);
    await f.page.getByLabel('Email or domain').fill('alex@example.com');
    const granted = f.page.waitForResponse(response => response.url().endsWith('/api/email-grants') && response.request().method() === 'POST');
    await f.page.getByRole('button', {name: 'Add', exact: true}).click();
    assert.equal((await granted).status(), 200);
    // The person reloads, and the board of W opens with the same session.
    const board = page.waitForResponse(response => response.url().endsWith('/api/workspaces/' + encodeURIComponent(id) + '/board'));
    await page.reload();
    assert.equal((await board).status(), 200);
    assert.equal(await page.locator('#workspaces').inputValue(), id);
    assert.equal(await page.locator('#login-button').isVisible(), false);
    assert.deepEqual(signIns, []);
    const cookies = (await person.cookies()).filter(cookie => cookie.name === 'joppa_session');
    assert.deepEqual(cookies.map(cookie => cookie.value), [f.live.person_cookie]);
    const index = (await read(id)).value.index;
    assert.equal(index.members[f.live.person], 'human');
    // The person who signed in shows once, as a member, and the grant is not listed again.
    await f.page.reload();
    await f.page.waitForFunction(() => !document.getElementById('workspace-select').disabled);
    await f.page.getByLabel('Workspace', {exact: true}).selectOption(id);
    const people = await f.page.locator('#workspace-people li').allInnerTexts();
    assert.equal(people.filter(text => text.includes('alex@example.com')).length, 1, JSON.stringify(people));
    assert.ok(!people.some(text => text.includes('(invited)')), JSON.stringify(people));
    assert.deepEqual(await read(control), {status: 404, value: {error: 'Workspace not found'}});
    assert.deepEqual(errors, []);
  } finally { await person.close(); await f.close(); }
});

test('owner-adds-members-ac7: alex adds healthium.one to W in Settings', async () => {
  const f = await fixture({'domain:d': {title: 'Existing domain'}});
  try {
    const request = f.page.context().request;
    const create = async title => {
      const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title}});
      assert.equal(created.status(), 200);
      return (await created.json()).id;
    };
    const id = await create('Domain space'), control = await create('Domain control');
    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    // The same field takes a domain: a value without "@" is a domain.
    const workspace = f.page.getByLabel('Workspace', {exact: true}), domain = f.page.getByRole('textbox', {name: 'Email or domain'});
    const add = f.page.getByRole('button', {name: 'Add', exact: true});
    await workspace.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await workspace.selectOption(id);
    // A failed save shows the server error.
    await domain.fill('healthium');
    await add.click();
    await f.page.getByRole('alert').filter({hasText: 'a valid email domain is required'}).waitFor();
    assert.equal(await add.isDisabled(), false);
    await domain.fill('healthium.one');
    const granted = f.page.waitForResponse(response => response.url().endsWith('/api/domain-grants') && response.request().method() === 'POST');
    await add.click();
    assert.equal((await granted).status(), 200);
    const domains = f.page.locator('#workspace-people li[data-kind="domain"] > span');
    await domains.filter({hasText: 'healthium.one'}).waitFor();
    assert.deepEqual(await domains.allInnerTexts(), ['Everyone at healthium.one']);
    assert.equal(await workspace.inputValue(), id);
    // The control workspace has no domain.
    await workspace.selectOption(control);
    assert.equal(await domains.count(), 0);
    const listed = (await (await request.get(f.live.root + '/api/domain-grants')).json()).items
      .filter(item => item.id === id || item.id === control);
    assert.deepEqual(listed, [{id: control, title: 'Domain control', domains: []}, {id, title: 'Domain space', domains: ['healthium.one']}]);
  } finally { await f.close(); }
});

test('owner-adds-members-ac5: the sign-in page offers one Google sign-in control', async () => {
  const live = await server();
  const context = await shared.browser.newContext({reducedMotion: 'reduce'});
  try {
    const page = await context.newPage(), starts = [], errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('request', r => { const {pathname} = new URL(r.url()); if (pathname.startsWith('/auth/')) starts.push(r.method() + ' ' + pathname); });
    // The fixture has no Google client, so the start reply stands in for Google's authorize URL.
    const google = 'https://accounts.google.com/o/oauth2/v2/auth?client_id=joppa-browser-check';
    await context.route(live.root + '/auth/login', route => route.fulfill({json: {verification_url: google, expires_in: 300}}));
    await context.route('https://accounts.google.com/**', route => route.fulfill({contentType: 'text/html', body: '<title>Google</title>'}));
    assert.equal((await page.goto(live.root + '/')).status(), 200);
    const login = page.locator('#login-screen');
    await login.locator('#login-button').waitFor({state: 'visible'});
    const controls = login.locator('button:visible, a:visible, input:visible, [role="button"]:visible');
    assert.equal(await controls.count(), 1);
    assert.equal(await controls.first().getAttribute('id'), 'login-button');
    assert.match(await controls.first().innerText(), /Continue with Google/);
    const popup = page.waitForEvent('popup');
    await controls.first().click();
    const googleWindow = await popup;
    await googleWindow.waitForURL(url => url.href.startsWith(google));
    assert.deepEqual(starts, ['POST /auth/login']);
    // Cancel ends this attempt: no other start and no poll follow, even after the poll interval.
    const cancel = login.locator('#login-cancel');
    await cancel.waitFor({state: 'visible'});
    await cancel.click();
    assert.equal(await cancel.isVisible(), false);
    assert.equal(await login.locator('#login-button').isDisabled(), false);
    await page.waitForTimeout(2500);
    assert.deepEqual(starts, ['POST /auth/login']);
    assert.deepEqual(errors, []);
  } finally { await context.close(); await live.stop(); }
});

test('req-has-criteria-ac3: a row without criteria shows zero and a grey dot', async () => {
  const configured = {...types, ac: {...types.ac, title: 'Proofs'}};
  const f = await fixture({'domain:d': {title: 'Domain'},
    'capability:c': {title: 'Capability', parent: 'domain:d'},
    'requirement:empty': {title: 'Empty requirement', parent: 'capability:c'}}, configured);
  try {
    const card = await f.open('capability:c');
    const row = card.locator('.common-object[data-object-address="requirement:empty"] > summary');
    assert.equal(await row.locator('.verification-chip').innerText(), '0 Proofs');
    assert.equal(await row.locator('.status-circle').getAttribute('data-tone'), 'grey');
  } finally { await f.close(); }
});
};
