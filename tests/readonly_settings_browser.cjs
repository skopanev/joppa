// Chromium checks of Read-only and Editor roles in Settings (ac:pidlyl1y, ac:bynwrfd7,
// ac:zjej545r and ac:5r15xc8v).
const {test} = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const {fixture} = require('./board_fixture.cjs');

async function openSettings(page, root, id) {
  assert.equal((await page.goto(root + '/settings')).status(), 200);
  const select = page.getByLabel('Workspace', {exact: true});
  await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
  await page.waitForFunction(() => !document.getElementById('workspace-select').disabled);
  await select.selectOption(id);
  return select;
}

async function oauthToken(root, cookie) {
  const redirect = 'http://127.0.0.1:32123/callback';
  const reg = await (await fetch(root + '/oauth/register', {method: 'POST',
    headers: {'Content-Type': 'application/json'},
    body: JSON.stringify({client_name: 'Browser MCP', redirect_uris: [redirect], token_endpoint_auth_method: 'none'})})).json();
  const verifier = 'browser-pkce-verifier-abcdefghijklmnopqrstuvwxyz0123456789';
  const challenge = crypto.createHash('sha256').update(verifier).digest('base64url');
  const resource = root + '/mcp';
  const approved = await (await fetch(root + '/oauth/approve', {method: 'POST',
    headers: {Origin: root, Cookie: 'joppa_session=' + cookie, 'Content-Type': 'application/json'},
    body: JSON.stringify({client_id: reg.client_id, redirect_uri: redirect, response_type: 'code',
      code_challenge: challenge, code_challenge_method: 'S256', state: 's', scope: 'joppa', resource})})).json();
  const code = new URL(approved.redirect).searchParams.get('code');
  const body = new URLSearchParams({grant_type: 'authorization_code', code, redirect_uri: redirect,
    client_id: reg.client_id, code_verifier: verifier, resource});
  const tok = await (await fetch(root + '/oauth/token', {method: 'POST',
    headers: {'Content-Type': 'application/x-www-form-urlencoded'}, body})).json();
  assert.ok(tok.access_token, JSON.stringify(tok));
  return tok.access_token;
}

// ac:pidlyl1y
test('owner-adds-members: an Editor picks Read-only or Editor for an added email', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  try {
    const id = f.live.ws, request = f.page.context().request, origin = {Origin: f.live.root};
    await openSettings(f.page, f.live.root, id);
    const roleSelect = f.page.getByLabel('Role', {exact: true});
    const options = await roleSelect.locator('option').evaluateAll(list => list.map(o => [o.value, o.textContent, o.selected]));
    assert.deepEqual(options, [['viewer', 'Read-only', true], ['human', 'Editor', false]]);

    const people = f.page.getByRole('list', {name: 'People with access', exact: true});
    await f.page.getByLabel('Email or domain', {exact: true}).fill(f.live.maria_email);
    await f.page.getByRole('button', {name: 'Add', exact: true}).click();
    await people.locator('li').filter({hasText: f.live.maria_email + ' (invited)'}).waitFor();

    await f.page.getByLabel('Email or domain', {exact: true}).fill('alex@example.com');
    await roleSelect.selectOption('human');
    await f.page.getByRole('button', {name: 'Add', exact: true}).click();
    await people.locator('li').filter({hasText: 'alex@example.com (invited)'}).waitFor();

    const bad = await request.post(f.live.root + '/api/email-grants', {headers: origin, data: {workspace: id, email: 'x@example.com', role: 'agent'}});
    assert.equal(bad.status(), 422);
    assert.deepEqual(await bad.json(), {error: 'role is viewer or human'});

    // maria and alex@example.com load the app with their Google accounts.
    for (const tok of [f.live.maria_cookie, f.live.person_cookie]) {
      const r = await fetch(f.live.root + '/api/me', {headers: {Cookie: 'joppa_session=' + tok}});
      assert.equal(r.status, 200);
    }
    const access = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    const roles = Object.fromEntries(access.members.map(m => [m.id, m.role]));
    assert.equal(roles[f.live.maria], 'viewer');
    assert.equal(roles[f.live.person], 'human');
    assert.equal(access.emails.includes('x@example.com'), false);
  } finally { await f.close(); }
});

// ac:bynwrfd7
test('owner-adds-members: a domain grant admits Read-only, and a personal Editor email wins', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  try {
    const id = f.live.ws, request = f.page.context().request;
    await openSettings(f.page, f.live.root, id);
    const people = f.page.getByRole('list', {name: 'People with access', exact: true});
    await f.page.getByLabel('Email or domain', {exact: true}).fill(f.live.hd_domain);
    await f.page.getByRole('button', {name: 'Add', exact: true}).click();
    const domainRow = people.locator('li[data-kind="domain"]').filter({hasText: 'Everyone at ' + f.live.hd_domain});
    await domainRow.waitFor();
    assert.equal(await domainRow.locator('.role-text').innerText(), 'Read-only');
    assert.equal(await domainRow.locator('select').count(), 0);

    await f.page.getByLabel('Email or domain', {exact: true}).fill(f.live.lena_hd_email);
    await f.page.getByLabel('Role', {exact: true}).selectOption('human');
    await f.page.getByRole('button', {name: 'Add', exact: true}).click();
    await people.locator('li').filter({hasText: f.live.lena_hd_email + ' (invited)'}).waitFor();

    for (const tok of [f.live.maria_hd_cookie, f.live.lena_hd_cookie]) {
      const r = await fetch(f.live.root + '/api/me', {headers: {Cookie: 'joppa_session=' + tok}});
      assert.equal(r.status, 200);
    }
    const access = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    const roles = Object.fromEntries(access.members.map(m => [m.id, m.role]));
    assert.equal(roles[f.live.maria_hd], 'viewer');
    assert.equal(roles[f.live.lena_hd], 'human');
  } finally { await f.close(); }
});

// ac:zjej545r
test('owner-adds-members: an Editor changes the role of a member and of an invited email', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  try {
    const id = f.live.ws, request = f.page.context().request, origin = {Origin: f.live.root};
    assert.equal((await request.post(f.live.root + '/api/email-grants', {headers: origin, data: {workspace: id, email: f.live.maria_email}})).status(), 200);
    assert.equal((await request.post(f.live.root + '/api/email-grants', {headers: origin, data: {workspace: id, email: 'alex@example.com'}})).status(), 200);
    assert.equal((await fetch(f.live.root + '/api/me', {headers: {Cookie: 'joppa_session=' + f.live.maria_cookie}})).status, 200);
    const mariaMcp = await oauthToken(f.live.root, f.live.maria_cookie);

    await openSettings(f.page, f.live.root, id);
    const people = f.page.getByRole('list', {name: 'People with access', exact: true});
    const mariaRow = people.locator('li[data-kind="member"]').filter({hasText: f.live.maria_email});
    const invitedRow = people.locator('li[data-kind="email"]').filter({hasText: 'alex@example.com (invited)'});
    await mariaRow.waitFor();
    await invitedRow.waitFor();
    for (const row of [mariaRow, invitedRow]) {
      const opts = await row.locator('select.role option').evaluateAll(list => list.map(o => [o.value, o.textContent]));
      assert.deepEqual(opts, [['viewer', 'Read-only'], ['human', 'Editor']]);
      assert.equal(await row.locator('select.role').inputValue(), 'viewer');
    }

    let changed = f.page.waitForResponse(r => r.url().endsWith('/api/access/role') && r.status() === 200);
    await mariaRow.locator('select.role').selectOption('human');
    await changed;
    await f.page.waitForFunction(email => {
      const row = [...document.querySelectorAll('#workspace-people li')].find(li => li.textContent.includes(email));
      return row && row.querySelector('select.role')?.value === 'human' && !row.querySelector('select.role').disabled;
    }, f.live.maria_email);

    changed = f.page.waitForResponse(r => r.url().endsWith('/api/access/role') && r.status() === 200);
    await invitedRow.locator('select.role').selectOption('human');
    await changed;

    // maria saves an object with no new sign-in, and alex@example.com joins as human.
    const saveCall = idSuffix => fetch(f.live.root + '/mcp', {method: 'POST',
      headers: {Authorization: 'Bearer ' + mariaMcp, 'Content-Type': 'application/json'},
      body: JSON.stringify({jsonrpc: '2.0', id: 1, method: 'tools/call', params: {name: 'joppa_command',
        arguments: {workspace: id, request_id: 'r-' + idSuffix, command: {op: 'save_object', kind: 'note', id: idSuffix,
          revision: 0, parent: null, data: {title: 'Note ' + idSuffix, owner: f.live.maria}, reason: 'Test'}}}})});
    const saved = await (await saveCall('one')).json();
    assert.notEqual(saved.result?.isError, true, JSON.stringify(saved));

    assert.equal((await fetch(f.live.root + '/api/me', {headers: {Cookie: 'joppa_session=' + f.live.person_cookie}})).status, 200);
    const access = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    assert.equal(access.members.find(m => m.id === f.live.person)?.role, 'human');

    // Demote maria back to Read-only: her next save is refused with 403 / Read-only error.
    await f.page.reload();
    await openSettings(f.page, f.live.root, id);
    changed = f.page.waitForResponse(r => r.url().endsWith('/api/access/role') && r.status() === 200);
    await mariaRow.locator('select.role').selectOption('viewer');
    await changed;

    const mcpRefused = await (await saveCall('two')).json();
    assert.equal(mcpRefused.result?.isError, true);
    assert.equal(mcpRefused.result.content[0].text, 'a Read-only member cannot write');

    const httpRefused = await fetch(f.live.root + '/api/workspaces/' + encodeURIComponent(id) + '/commands', {method: 'POST',
      headers: {Origin: f.live.root, Cookie: 'joppa_session=' + f.live.maria_cookie, 'Content-Type': 'application/json'},
      body: JSON.stringify({request_id: 'http-two', command: {op: 'note', target: 'note:one', id: 'c1', text: 'no'}})});
    assert.equal(httpRefused.status, 403);
    assert.deepEqual(await httpRefused.json(), {error: 'a Read-only member cannot write'});
  } finally { await f.close(); }
});

// ac:5r15xc8v
test('m78flwn8: a Read-only member sees only Hide and New workspace in Settings', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  const viewerCtx = await f.page.context().browser().newContext({reducedMotion: 'reduce', colorScheme: 'light'});
  let mineId = null;
  try {
    const id = f.live.ws, request = f.page.context().request, origin = {Origin: f.live.root};
    assert.equal((await request.post(f.live.root + '/api/email-grants', {headers: origin, data: {workspace: id, email: f.live.maria_email}})).status(), 200);
    await viewerCtx.addCookies([{name: 'joppa_session', value: f.live.maria_cookie, url: f.live.root}]);
    const page = await viewerCtx.newPage(), consoleErrors = [], pageErrors = [];
    page.on('console', m => { if (m.type() === 'error') consoleErrors.push(m.text()); });
    page.on('pageerror', e => pageErrors.push(e.message));

    await openSettings(page, f.live.root, id);
    await page.waitForLoadState('networkidle');
    assert.equal(await page.locator('#hide-workspace').isVisible(), true);
    assert.equal(await page.locator('#create-open').isVisible(), true);
    for (const sel of ['#rename-open', '#read-key-panel', '#write-key-panel', '#people-panel', '#delete-button']) {
      assert.equal(await page.locator(sel).isVisible(), false, sel + ' should be hidden');
    }
    assert.deepEqual(consoleErrors, []);
    assert.deepEqual(pageErrors, []);

    const hid = page.waitForResponse(r => r.url().endsWith('/hide') && r.request().method() === 'POST');
    await page.locator('#hide-workspace').check();
    assert.equal((await hid).status(), 200);

    // Create workspace "Mine" with + New workspace.
    await page.locator('#create-open').click();
    await page.locator('#workspace-title').fill('Mine');
    await Promise.all([
      page.waitForURL(u => u.searchParams.has('workspace')),
      page.locator('#create-workspace-button').click(),
    ]);
    mineId = new URL(page.url()).searchParams.get('workspace');
    const mariaAccess = await (await viewerCtx.request.get(f.live.root + '/api/access')).json();
    const mineItem = mariaAccess.items.find(w => w.id === mineId);
    assert.equal(mineItem?.members?.find(m => m.id === f.live.maria)?.role, 'human');

    const before = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    const adminCalls = [
      ['/api/workspaces/' + encodeURIComponent(id) + '/rename', {title: 'Nope'}],
      ['/api/workspaces/' + encodeURIComponent(id) + '/delete', undefined],
      ['/api/workspaces/' + encodeURIComponent(id) + '/read-key', undefined],
      ['/api/workspaces/' + encodeURIComponent(id) + '/read-key/revoke', undefined],
      ['/api/workspaces/' + encodeURIComponent(id) + '/write-key', undefined],
      ['/api/workspaces/' + encodeURIComponent(id) + '/write-key/revoke', undefined],
      ['/api/email-grants', {workspace: id, email: 'intruder@example.com'}],
      ['/api/domain-grants', {workspace: id, domain: 'intruder.org'}],
      ['/api/access/remove', {workspace: id, member: 'alex'}],
    ];
    for (const [path, data] of adminCalls) {
      const res = await viewerCtx.request.post(f.live.root + path, {headers: origin, ...(data ? {data} : {})});
      assert.notEqual(res.status(), 200, path);
      assert.deepEqual(await res.json(), {error: 'a human member of the workspace is required'}, path);
    }
    const after = (await (await request.get(f.live.root + '/api/access')).json()).items.find(w => w.id === id);
    assert.deepEqual(after, before);
    assert.deepEqual(consoleErrors, []);
    assert.deepEqual(pageErrors, []);
  } finally {
    if (mineId) await viewerCtx.request.post(f.live.root + '/api/workspaces/' + encodeURIComponent(mineId) + '/delete', {headers: {Origin: f.live.root}}).catch(() => {});
    await viewerCtx.close();
    await f.close();
  }
});
