// Chromium checks of the board filters: tabs, the owner, assignee, status and readiness pickers
// and the search box change the visible rows in the same document. The filters read every level
// of the tree, and each picker keeps its value in the board link.
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
    // Maria owns the criterion of beta, one level below the roots.
    for (const [domain, green] of [['alpha', true], ['beta', false], ['gamma', true]]) {
      await f.save('ac:' + domain, 'Criterion of ' + domain, 'domain:' + domain, domain === 'beta' ? 'maria' : 'owner');
      if (green) await f.addTest('ac:' + domain, 'tests/filters.rs > ' + domain);
    }
    await f.mcp('joppa_record_results', {commit: 'c0ffee4', results: [
      {test: 'tests/filters.rs > alpha', status: 'pass'}, {test: 'tests/filters.rs > gamma', status: 'pass'}]});
    await f.save('note:n', 'Incoming note');
    let navigations = 0;
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    // A picker change replaces the link in the same document. Only a new document fires load.
    f.page.on('load', () => navigations++);
    await f.page.locator('#tree .common-object[data-object-address="domain:alpha"]').waitFor();
    const nonce = await f.page.evaluate(() => (window.joppaNonce = crypto.randomUUID()));
    // The rows are the top object cards and the search results.
    const rows = () => [...document.querySelectorAll('#tree .common-object[data-object-address], #tree a.search-result')]
      .filter(row => row.checkVisibility() && !row.parentElement.closest('.common-object'))
      .map(row => row.dataset.objectAddress || row.dataset.address).sort();
    const visible = () => f.page.evaluate(`(${rows})()`);
    const shows = async (wanted, step) => {
      await f.page.waitForFunction(`JSON.stringify((${rows})()) === ${JSON.stringify(JSON.stringify(wanted))}`)
        .catch(async () => assert.fail(step + ': ' + JSON.stringify(await visible())));
    };
    const link = key => f.page.evaluate(key => new URLSearchParams(location.search).get(key), key);
    await f.page.getByRole('tab', {name: 'Inbox'}).click();
    await shows(['note:n'], 'Inbox');
    await f.page.getByRole('tab', {name: 'Domains'}).click();
    await shows(['domain:alpha', 'domain:beta', 'domain:gamma'], 'Domains');
    await f.page.getByLabel('Object owner').selectOption('maria');
    await shows(['ac:beta', 'domain:gamma'], 'owner maria at every level');
    await f.page.getByLabel('Object owner').selectOption('alex');
    await shows(['domain:alpha', 'domain:beta'], 'owner alex');
    assert.equal(await link('owner'), 'alex', 'the link keeps the owner');
    await f.page.getByLabel('Object status').selectOption('green');
    await shows(['domain:alpha'], 'status green');
    assert.equal(await link('status'), 'green', 'the link keeps the status');
    const search = f.page.getByLabel('Search objects');
    await search.fill('zq' + crypto.randomUUID().replaceAll('-', ''));
    await shows([], 'unique search');
    await search.fill('');
    await shows(['domain:alpha'], 'cleared search');
    assert.equal(await f.page.evaluate(() => window.joppaNonce), nonce, 'the original document stays');
    assert.equal(navigations, 0, 'no document navigation after the first load');
    // The board search finds what joppa_search finds, at every level.
    await search.fill('Gamma');
    await f.page.getByLabel('Object owner').selectOption('');
    await f.page.getByLabel('Object status').selectOption('');
    assert.equal(await link('owner'), null, 'All owners leaves the link');
    const found = await f.mcp('joppa_search', {query: 'Gamma', limit: 50});
    const addresses = [...new Set(found.items.map(item => item.address))].sort();
    assert.ok(addresses.includes('domain:gamma') && addresses.includes('ac:gamma'), JSON.stringify(found));
    await shows(addresses, 'search Gamma');
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
    // A criterion that waits for no dependency is ready.
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&assignee=maria&ready=true');
    await f.page.locator('#tree .common-object[data-object-address="ac:one"]').waitFor();
    const rows = await f.page.evaluate(() => [...document.querySelectorAll('#tree .common-object[data-object-address]')]
      .filter(row => row.checkVisibility() && !row.parentElement.closest('.common-object')).map(row => row.dataset.objectAddress));
    assert.deepEqual(rows, ['ac:one']);
    // The pickers show the link values, and a picker change writes its value to the link.
    assert.equal(await f.page.getByLabel('Object assignee').inputValue(), 'maria');
    assert.equal(await f.page.getByLabel('Object readiness').inputValue(), 'true');
    assert.ok(await f.page.locator('.link-filters').isHidden(), 'no link filter is left out of the pickers');
    await f.page.getByLabel('Object readiness').selectOption('false');
    await f.page.getByText('No objects match these filters.').waitFor();
    assert.equal(await f.page.evaluate(() => location.search.includes('assignee=maria&ready=false')), true);
    // A person of a link who is not a member gets an option, and the other link filters show in one line.
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&owner=former&kind=ac');
    await f.page.getByText('No objects match these filters.').waitFor();
    assert.equal(await f.page.getByLabel('Object owner').inputValue(), 'former');
    assert.equal(await f.page.locator('.link-filters').textContent(), 'kind = ac');
  } finally { await f.close(); }
});

test('board-filter-pages: a filter list and a search show 25 rows, and Load more adds the next 25', async () => {
  const types = {
    domain: {title: 'Domains', role: 'group', status: 'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['domain']},
    test: testType(['ac']),
  };
  const f = await fixture(types);
  try {
    for (let i = 0; i < 30; i++) await f.save('domain:r' + String(i).padStart(2, '0'), 'Region ' + i, null, 'alex');
    const reads = [];
    f.page.on('request', request => { const url = new URL(request.url()); if (/\/(objects|search)$/.test(url.pathname)) reads.push(url); });
    const rows = () => f.page.locator('.object-filter-results .common-object, .object-filter-results a.search-result');
    const more = f.page.locator('.object-filter-results button.filter-more');
    for (const [query, path] of [['owner=alex', '/objects'], ['', '/search']]) {
      reads.length = 0;
      await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + (query ? '&' + query : ''));
      if (!query) { await f.page.locator('#tree .common-object').first().waitFor(); reads.length = 0; await f.page.getByLabel('Search objects').fill('Region'); }
      await f.page.waitForFunction(() => document.querySelectorAll('.object-filter-results .common-object, .object-filter-results a.search-result').length === 25);
      await more.waitFor();
      const first = reads.filter(url => url.pathname.endsWith(path));
      assert.equal(first.length, 1, path + ' reads one page: ' + first.join(' '));
      assert.equal(first[0].searchParams.get('limit'), '25');
      await more.click();
      await f.page.waitForFunction(() => document.querySelectorAll('.object-filter-results .common-object, .object-filter-results a.search-result').length === 30);
      assert.equal(await rows().count(), 30);
      assert.equal(await more.count(), 0, path + ': the last page offers no Load more');
      const pages = reads.filter(url => url.pathname.endsWith(path));
      assert.equal(pages.length, 2, path + ' reads two pages');
      assert.ok(pages[1].searchParams.get('cursor'), path + ': the second read names the cursor');
    }
  } finally { await f.close(); }
});

test('filter-fields-ac2 field-filters: each schema filter field is a drop-down of its values and a part of the data filter', async () => {
  const types = {domain: {title: 'Domains', role: 'group', status: 'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['domain']}, test: testType(['ac'])};
  const f = await fixture(types);
  try {
    const tree = await f.api('/tree');
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, filters: ['tags', 'priority'], reason: 'Offer two filters'}, f.live.owner);
    const domain = (id, data) => f.command({op: 'save_object', kind: 'domain', id, revision: 0, parent: null, data: {title: id, owner: 'owner', body: 'Fixture text', ...data}, reason: 'Define the browser fixture'});
    await domain('swap', {tags: ['swap', 'fx'], priority: 'p0'});
    await domain('fx', {tags: ['fx']});
    await domain('plain', {});
    const rows = () => f.page.evaluate(() => [...document.querySelectorAll('.object-filter-results .common-object')].map(row => row.dataset.objectAddress).sort());
    const shows = async (wanted, step) => f.page.waitForFunction(w => JSON.stringify([...document.querySelectorAll('.object-filter-results .common-object')].map(r => r.dataset.objectAddress).sort()) === w, JSON.stringify(wanted))
      .catch(async () => assert.fail(step + ': ' + JSON.stringify(await rows())));
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const tags = f.page.getByLabel('Filter tags'), priority = f.page.getByLabel('Filter priority');
    await tags.waitFor();
    assert.deepEqual(await tags.locator('option').allTextContents(), ['All tags', 'fx', 'swap']);
    assert.deepEqual(await priority.locator('option').allTextContents(), ['All priority', 'p0']);
    await tags.selectOption({label: 'fx'});
    await shows(['domain:fx', 'domain:swap'], 'tag fx');
    await priority.selectOption({label: 'p0'});
    await shows(['domain:swap'], 'tag fx and priority p0');
    assert.deepEqual(JSON.parse(new URL(f.page.url()).searchParams.get('data')), {tags: ['fx'], priority: 'p0'});
    // A link names the choice of each drop-down. A part that no drop-down shows stays in the filter.
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&data=' + encodeURIComponent('{"tags":["swap"],"owner":"owner"}'));
    await shows(['domain:swap'], 'the link filter');
    await tags.waitFor();
    assert.equal(await tags.inputValue(), '{"tags":["swap"]}');
    assert.equal(await f.page.locator('.link-filters').textContent(), 'data = {"owner":"owner"}');
    await tags.selectOption('');
    await shows(['domain:fx', 'domain:plain', 'domain:swap'], 'only the rest of the link filter');
    assert.equal(new URL(f.page.url()).searchParams.get('data'), '{"owner":"owner"}');
  } finally { await f.close(); }
});
