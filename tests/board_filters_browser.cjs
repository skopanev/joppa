// Chromium checks of the board filters: tabs, the owner and status filters and the search box
// change the visible rows in the same document.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('board-filters-and-theme-ac1 filters: tabs, filters and search update the same document', async () => {
  const types = {
    domain: {title: 'Domains', role: 'group', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', parents: ['domain']},
    note: {title: 'Inbox', role: 'document', root: true, parents: [], inbox: true},
  };
  const f = await fixture(types);
  try {
    await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
    await f.save('domain:alpha', 'Alpha', null, 'alex');
    await f.save('domain:beta', 'Beta', null, 'alex');
    await f.save('domain:gamma', 'Gamma', null, 'maria');
    // A green domain has a criterion with a recorded pass. Beta has a criterion without tests.
    for (const [domain, green] of [['alpha', true], ['beta', false], ['gamma', true]]) {
      await f.save('ac:' + domain, 'Criterion of ' + domain, 'domain:' + domain);
      if (green) await f.command({op: 'set_ac_tests', target: 'ac:' + domain, tests: ['tests/filters.rs > ' + domain], reason: 'Bind'});
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
    assert.ok(found.items.some(item => item.kind === 'domain' && item.id === 'gamma'), JSON.stringify(found));
  } finally { await f.close(); }
});
