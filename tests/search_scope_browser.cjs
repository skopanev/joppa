// Chromium checks of the board search scope: each scope checkbox is visible, and its label
// stays on one line.
// With All versions, a click opens the revision that the search found, and with All
// workspaces, a click opens a result of another workspace in that workspace. The fixture
// provider does not answer, so the board search uses its full-text half.
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
    // alex creates the second workspace in the browser and writes in it with a service token.
    const second = await f.page.evaluate(async () => (await fetch('/api/workspaces', {method: 'POST',
      headers: {'Content-Type': 'application/json'}, body: JSON.stringify({title: 'Second board'})})).json());
    const saved = await fetch(f.live.root + '/api/workspaces/' + second.id + '/commands', {method: 'POST',
      headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
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

test('search-scope-labels-one-line: each scope checkbox is visible and its label stays on one line at every width', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:billing', 'Billing rules');
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.getByLabel('All workspaces').waitFor();
    for (const width of [390, 600, 800, 1000, 1280]) {
      await f.page.setViewportSize({width, height: 900});
      const lines = await f.page.evaluate(() => [...document.querySelectorAll('label.filter-check')].map(label => {
        // The text node only: the checkbox has its own box.
        const range = document.createRange();
        range.selectNodeContents(label.lastChild);
        const tops = new Set([...range.getClientRects()].filter(r => r.width > 0).map(r => Math.round(r.top)));
        const box = label.querySelector('input[type=checkbox]').getBoundingClientRect();
        return [label.textContent.trim(), tops.size, box.width >= 12 && box.height >= 12];
      }));
      assert.deepEqual(lines, [['All versions', 1, true], ['All workspaces', 1, true]], 'width ' + width);
    }
  } finally { await f.close(); }
});
