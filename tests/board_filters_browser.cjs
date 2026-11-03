// Chromium checks of the board filters: tabs, the owner and status filters and the search box
// change the visible rows in the same document.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture, testType} = require('./board_fixture.cjs');

test('board-filters-and-theme-ac1 filters: tabs, filters and search update the same document', async () => {
  const types = {
    domain: {title: 'Domains', role: 'group', status:'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status:'children', parents: ['domain']},
    test: testType(['ac']),
    note: {title: 'Inbox', role: 'document', status:'none', root: true, parents: [], inbox: true},
  };
  const f = await fixture(types);
  try {
    await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
    await f.save('domain:alpha', 'Alpha', null, 'alex');
    await f.save('domain:beta', 'Beta', null, 'alex');
    await f.save('domain:gamma', 'Gamma', null, 'maria');
    // A green domain has a criterion with a recorded pass. Beta has an orange criterion without tests.
    for (const [domain, green] of [['alpha', true], ['beta', false], ['gamma', true]]) {
      await f.save('ac:' + domain, 'Criterion of ' + domain, 'domain:' + domain);
      if (green) await f.addTest('ac:' + domain, 'tests/filters.rs > ' + domain);
    }
    await f.mcp('joppa_record_results', {commit: 'c0ffee4', results: [
      {test: 'tests/filters.rs > alpha', status: 'pass'}, {test: 'tests/filters.rs > gamma', status: 'pass'}]});
    await f.save('note:n', 'Incoming note');
    let navigations = 0;
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    f.page.on('framenavigated', frame => { if (frame === f.page.mainFrame()) navigations++; });
    await f.page.locator('#tree .common-object[data-object-address="domain:alpha"]').waitFor();
    const nonce = await f.page.evaluate(() => (window.joppaNonce = crypto.randomUUID()));
    const visible = () => f.page.evaluate(() => [...document.querySelectorAll('#tree .common-object[data-object-address]')]
      .filter(row => row.checkVisibility() && !row.parentElement.closest('.common-object'))
      .map(row => row.dataset.objectAddress).sort());
    const shows = async (wanted, step) => {
      await f.page.waitForFunction(wanted => JSON.stringify([...document.querySelectorAll('#tree .common-object[data-object-address]')]
        .filter(row => row.checkVisibility() && !row.parentElement.closest('.common-object'))
        .map(row => row.dataset.objectAddress).sort()) === JSON.stringify(wanted), wanted)
        .catch(async () => assert.fail(step + ': ' + JSON.stringify(await visible())));
    };
    await f.page.getByRole('tab', {name: 'Inbox'}).click();
    await shows(['note:n'], 'Inbox');
    await f.page.getByRole('tab', {name: 'Domains'}).click();
    await shows(['domain:alpha', 'domain:beta', 'domain:gamma'], 'Domains');
    await f.page.getByLabel('Object owner').selectOption('alex');
    await shows(['domain:alpha', 'domain:beta'], 'owner alex');
    await f.page.getByLabel('Object status').selectOption('green');
    await shows(['domain:alpha'], 'status green');
    const search = f.page.getByLabel('Search objects');
    await search.fill('zq' + crypto.randomUUID().replaceAll('-', ''));
    await shows([], 'unique search');
    await search.fill('');
    await shows(['domain:alpha'], 'cleared search');
    assert.equal(await f.page.evaluate(() => window.joppaNonce), nonce, 'the original document stays');
    assert.equal(navigations, 0, 'no document navigation after the first load');
    // The board search finds what joppa_search finds.
    await search.fill('Gamma');
    await f.page.getByLabel('Object owner').selectOption('');
    await f.page.getByLabel('Object status').selectOption('');
    await shows(['domain:gamma'], 'search Gamma');
    const found = await f.mcp('joppa_search', {query: 'Gamma'});
    assert.ok(found.items.some(item => item.address === 'domain:gamma'), JSON.stringify(found));
  } finally { await f.close(); }
});

test('board-link-filters: a board link with node filters opens that list', async () => {
  const types = {
    domain: {title: 'Domains', role: 'group', status: 'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['domain']},
    test: testType(['ac']),
  };
  const f = await fixture(types);
  try {
    await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
    await f.save('domain:alpha', 'Alpha');
    for (const [id, assignee] of [['one', 'maria'], ['two', 'alex']]) {
      const saved = await f.command({op: 'save_object', kind: 'ac', id, revision: 0, parent: 'domain:alpha',
        data: {title: 'Criterion ' + id, owner: 'owner', body: 'Fixture text', assignee}, reason: 'Define the browser fixture'});
      assert.ok(!saved.error, JSON.stringify(saved));
    }
    // One link names the assignee and the readiness: the board opens with that list only.
    // A criterion without an assessment that waits for nothing is ready.
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&assignee=maria&ready=true');
    await f.page.locator('#tree .common-object[data-object-address="ac:one"]').waitFor();
    const rows = await f.page.evaluate(() => [...document.querySelectorAll('#tree .common-object[data-object-address]')]
      .filter(row => row.checkVisibility() && !row.parentElement.closest('.common-object')).map(row => row.dataset.objectAddress));
    assert.deepEqual(rows, ['ac:one']);
    const line = f.page.locator('.link-filters');
    assert.ok(await line.isVisible(), 'the link filters show');
    assert.match(await line.textContent(), /assignee = .+ · ready = true/);
  } finally { await f.close(); }
});
