// Chromium check of the Settings deletion: the page names the workspaces that link to the
// workspace, shows a spinner while the request runs, and the links leave those workspaces.
// The Settings header shows the brand line and no name of the signed-in person.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('owner-manages-workspace-ac4: Settings names the linking workspaces, shows a spinner and removes the links', async () => {
  const f = await fixture({
    component: {title: 'Components', role: 'group', root: true, parents: []},
    note: {title: 'Notes', role: 'document', root: true, parents: ['*']},
  });
  try {
    const request = f.page.context().request;
    // The signed-in member alex creates workspace D and adds one object to it.
    const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: 'Doomed ' + crypto.randomUUID()}});
    const {id: d} = await created.json();
    const inD = await fetch(f.live.root + '/api/workspaces/' + d + '/commands', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
      body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'save_object', kind: 'domain', id: 'api', revision: 0, parent: null,
        data: {title: 'API contract', owner: 'alex'}, reason: 'Fixture'}})});
    assert.equal(inD.status, 200, await inD.text());
    // The fixture workspace links to D twice: a node bound to D and a note with a remote link.
    await f.command({op: 'save_object', kind: 'component', id: 'backend', revision: 0, parent: null,
      data: {title: 'Backend', owner: 'alex', workspace: d}, reason: 'Fixture'}, f.live.alex);
    await f.command({op: 'save_object', kind: 'note', id: 'mobile', revision: 0, parent: null,
      data: {title: 'Mobile login', owner: 'alex', body: 'Uses the API'}, reason: 'Fixture'}, f.live.alex);
    await f.command({op: 'set_remote_links', target: 'note:mobile', revision: 0, reason: 'Mobile uses the API',
      links: [{workspace: d, target: 'domain:api', relation: 'related', revision: 1, blocking: false}]}, f.live.alex);
    const title = (await (await request.get(f.live.root + '/api/workspaces')).json()).find(w => w.id === f.live.ws).title;

    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    // The header keeps the brand line and shows no name of the person.
    const header = f.page.locator('header.app-header');
    assert.equal(await header.locator('.brand .tagline').textContent(), 'AI first product specification');
    assert.equal(await header.locator('.brand .tagline').isVisible(), true);
    assert.equal(await header.locator('#identity').count(), 0);
    assert.equal(await header.getByRole('button', {name: 'Sign out'}).isVisible(), true);
    const me = await (await request.get(f.live.root + '/api/me')).json();
    for (const name of [me.name, me.email].filter(Boolean)) assert.ok(!(await header.innerText()).includes(name), name);

    const select = f.page.getByLabel('Workspace', {exact: true});
    await select.locator('option[value="' + d + '"]').waitFor({state: 'attached'});
    await select.selectOption(d);
    await f.page.getByRole('button', {name: 'Delete workspace'}).click();
    const list = f.page.locator('#delete-references');
    await list.waitFor();
    assert.match(await list.innerText(), /These workspaces link to this workspace:/);
    assert.deepEqual(await list.locator('li').allInnerTexts(), [title + ' (2 objects)']);
    assert.match(await list.innerText(), /After the deletion, these links are removed from those workspaces\./);
    // Hold the deletion: the confirmation button shows a spinner until the answer comes.
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route('**/api/workspaces/' + d + '/delete', async route => { await held; await route.continue(); });
    const confirm = f.page.getByRole('button', {name: 'Delete for everyone'});
    await confirm.click();
    await confirm.locator('.spinner').waitFor();
    assert.equal(await confirm.getAttribute('aria-busy'), 'true');
    assert.equal(await confirm.isDisabled(), true);
    const deleted = f.page.waitForResponse(r => r.url().endsWith('/delete') && r.request().method() === 'POST');
    release();
    const reply = await (await deleted).json();
    assert.deepEqual(reply.removed_references.map(w => [w.title, w.objects]), [[title, ['component:backend', 'note:mobile']]]);
    await select.locator('option[value="' + d + '"]').waitFor({state: 'detached'});
    // The links left the fixture workspace with new revisions.
    const note = await f.api('/objects?address=' + encodeURIComponent('note:mobile'));
    assert.deepEqual(note.remote_links, []);
    const node = await f.api('/objects?address=' + encodeURIComponent('component:backend'));
    assert.equal(node.document.data.workspace, undefined);
    assert.equal(node.child_workspace, undefined);
  } finally { await f.close(); }
});

test('long-operation-spinner: a board button shows a spinner while its request runs', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', root: true, parents: ['*']}});
  try {
    await f.command({op: 'save_object', kind: 'note', id: 'mobile', revision: 0, parent: null,
      data: {title: 'Mobile login', owner: 'alex', body: 'Uses the API'}, reason: 'Fixture'}, f.live.alex);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const history = f.page.locator('[data-object-address="note:mobile"] > summary .history-button');
    await history.waitFor();
    // Hold the read of the revisions: the button shows a spinner until the answer comes.
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route(url => url.pathname.endsWith('/objects') && url.searchParams.get('address') === 'note:mobile',
      async route => { await held; await route.continue(); });
    await history.click();
    await history.locator('.spinner').waitFor();
    assert.equal(await history.getAttribute('aria-busy'), 'true');
    release();
    await f.page.locator('dialog.notes-dialog').waitFor();
    await history.locator('.spinner').waitFor({state: 'detached'});
    assert.equal(await history.getAttribute('aria-busy'), 'false');
  } finally { await f.close(); }
});
