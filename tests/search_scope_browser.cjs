// Chromium checks of the board search scope: with All versions, a click opens the revision
// that the search found, and with All workspaces, a click opens a result of another
// workspace in that workspace. The fixture provider does not answer, so the board search
// uses its full-text half.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const types = {requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []}};

async function search(f, workspace, text, box) {
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(workspace));
  const field = f.page.getByLabel('Search objects');
  await field.waitFor();
  if (box) await f.page.getByLabel(box).check();
  await field.fill(text);
}

test('search-all-versions-board: the board opens the revision that the search found', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:billing', 'Legacy billing rules');
    await f.command({op: 'save_object', kind: 'requirement', id: 'billing', revision: 1, parent: null,
      data: {title: 'Billing rules', owner: 'owner', body: 'Fixture text'}, reason: 'Drop the old word'});
    await search(f, f.live.ws, 'legacy');
    await f.page.getByText('No objects match these filters.').waitFor();
    assert.equal(await f.page.locator('a.search-result').count(), 0, 'the old revision is in the list with the checkbox off');
    await search(f, f.live.ws, 'legacy', 'All versions');
    const row = f.page.locator('a.search-result[data-address="requirement:billing"]');
    await row.waitFor();
    assert.match(await row.textContent(), /revision 1/);
    await row.click();
    await f.page.locator('.object-page-navigation', {hasText: 'Revision 1 · Saved document'}).waitFor();
    assert.equal(await f.page.locator('.object-page .common-object').first().getAttribute('data-object-address'), 'requirement:billing');
  } finally { await f.close(); }
});

test('search-all-workspaces-board: the board opens a result of another workspace in that workspace', async () => {
  const f = await fixture(types);
  try {
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    // alex creates the second workspace in the browser and issues an agent token there.
    const second = await f.page.evaluate(async () => (await fetch('/api/workspaces', {method: 'POST',
      headers: {'Content-Type': 'application/json'}, body: JSON.stringify({title: 'Second board'})})).json());
    const issued = await f.page.evaluate(async id => (await fetch('/api/workspaces/' + id + '/agent-tokens', {method: 'POST',
      headers: {'Content-Type': 'application/json'}, body: JSON.stringify({agent: 'helper'})})).json(), second.id);
    assert.ok(issued.token, JSON.stringify(issued));
    const saved = await fetch(f.live.root + '/api/workspaces/' + second.id + '/commands', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + issued.token, 'Content-Type': 'application/json'},
      body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'save_object', kind: 'domain', id: 'keys', revision: 0,
        parent: null, data: {title: 'Passkey sign in', owner: 'alex'}, reason: 'Define the browser fixture'}})});
    assert.equal(saved.status, 200, await saved.text());
    await search(f, f.live.ws, 'passkey', 'All workspaces');
    const row = f.page.locator('a.search-result[data-address="domain:keys"]');
    await row.waitFor();
    assert.match(await row.textContent(), /Second board/);
    await row.click();
    await f.page.waitForFunction(id => document.querySelector('#workspaces')?.value === id, second.id);
    await f.page.locator('.object-page .common-object[data-object-address="domain:keys"]').waitFor();
  } finally { await f.close(); }
});
