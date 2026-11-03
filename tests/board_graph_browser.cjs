// Chromium checks of the board graph view (src/board/graph.js): the dots, the parent edges and
// the depends_on arrows of the present nodes, and the light of a picked dot, the filter bar
// and the search on the matches and their full depends_on chain.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture, testType} = require('./board_fixture.cjs');

const types = {
  domain: {title: 'Domains', role: 'group', status: 'children', root: true, parents: []},
  ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['domain']},
  test: testType(['ac']),
};

// Domains pay and ship. Criterion c depends on b, and b depends on a. Criteria d and e have
// no link. Maria owns c, and only the title of a has the word Zebra.
async function world(f) {
  await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
  await f.save('domain:pay', 'Payments', null, 'alex');
  await f.save('domain:ship', 'Shipping', null, 'alex');
  const criterion = (id, title, parent, owner, target) => f.command({op: 'save_object', kind: 'ac', id, revision: 0, parent,
    data: {title, owner, body: 'Fixture text'}, reason: 'Define the graph fixture',
    links: target ? [{target, relation: 'depends_on', blocking: true}] : []});
  await criterion('a', 'Zebra card capture', 'domain:pay', 'alex');
  await criterion('b', 'Refund the capture', 'domain:pay', 'alex', 'ac:a');
  await criterion('c', 'Ship after the refund', 'domain:ship', 'maria', 'ac:b');
  await criterion('d', 'Print the label', 'domain:ship', 'alex');
  await criterion('e', 'Show the receipt', 'domain:pay', 'alex');
  await f.addTest('ac:a', 'tests/graph.rs > a');
  await f.mcp('joppa_record_results', {commit: 'c0ffee5', results: [{test: 'tests/graph.rs > a', status: 'pass'}]});
}

const ALL = ['ac:a', 'ac:b', 'ac:c', 'ac:d', 'ac:e', 'domain:pay', 'domain:ship', 'test:'];

// The addresses of the lit dots and of the lit depends_on arrows, sorted.
const lit = f => f.page.evaluate(() => {
  const on = element => getComputedStyle(element).opacity === '1';
  const dots = [...document.querySelectorAll('.graph-node')].filter(on).map(d => d.dataset.address.startsWith('test:') ? 'test:' : d.dataset.address);
  const arrows = [...document.querySelectorAll('.graph-depends')].filter(on).map(p => p.dataset.from + '>' + p.dataset.to);
  return {dots: dots.sort(), arrows: arrows.sort()};
});
async function lights(f, dots, arrows, step) {
  await f.page.waitForFunction(([dots, arrows]) => {
    const on = element => getComputedStyle(element).opacity === '1';
    const now = [...document.querySelectorAll('.graph-node')].filter(on).map(d => d.dataset.address.startsWith('test:') ? 'test:' : d.dataset.address).sort();
    const lines = [...document.querySelectorAll('.graph-depends')].filter(on).map(p => p.dataset.from + '>' + p.dataset.to).sort();
    return JSON.stringify(now) === JSON.stringify(dots) && JSON.stringify(lines) === JSON.stringify(arrows);
  }, [[...dots].sort(), [...arrows].sort()]).catch(async () => assert.fail(step + ': ' + JSON.stringify(await lit(f))));
}

test('board-graph-ac1 graph: each node is a dot of its tone with parent edges and depends_on arrows', async () => {
  const f = await fixture(types);
  try {
    await world(f);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.locator('#tree .common-object[data-object-address="domain:pay"]').waitFor();
    assert.equal(await f.page.locator('#tree-view-link').getAttribute('aria-current'), 'page');
    await f.page.locator('#graph-link').click();
    await f.page.locator('.graph-node').nth(7).waitFor();
    assert.equal(new URL(f.page.url()).searchParams.get('view'), 'graph');
    assert.equal(await f.page.locator('#graph-link').getAttribute('aria-current'), 'page');
    // Each dot has the stored tone of its node and the colour of that tone.
    const list = await f.api('/objects?parent=*&limit=100&fields=signal');
    const tones = Object.fromEntries(list.items.map(item => [item.address, item.signal.tone]));
    const dots = await f.page.evaluate(() => [...document.querySelectorAll('.graph-node')].map(dot => {
      const probe = document.createElement('span');probe.style.color = 'var(--dot-' + dot.dataset.tone + ')';document.body.append(probe);
      const wanted = getComputedStyle(probe).color;probe.remove();
      return {address: dot.dataset.address, tone: dot.dataset.tone, fill: getComputedStyle(dot).fill, wanted};
    }));
    assert.equal(dots.length, 8);
    for (const dot of dots) {
      assert.equal(dot.tone, tones[dot.address], dot.address + ' tone');
      assert.equal(dot.fill, dot.wanted, dot.address + ' colour');
    }
    assert.equal(tones['ac:a'], 'green');
    assert.equal(await f.page.locator('.graph-parent').count(), 6, 'one parent edge for each node with a parent');
    const arrows = await f.page.evaluate(() => [...document.querySelectorAll('.graph-depends')]
      .map(path => [path.dataset.from, path.dataset.to, path.getAttribute('marker-end')]));
    assert.deepEqual(arrows.sort(), [['ac:b', 'ac:a', 'url(#graph-arrow)'], ['ac:c', 'ac:b', 'url(#graph-arrow)']]);
    await lights(f, ALL, ['ac:b>ac:a', 'ac:c>ac:b'], 'no filter lights every node');
    // The page has no horizontal scroll at phone width.
    await f.page.setViewportSize({width: 390, height: 800});
    assert.equal(await f.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  } finally { await f.close(); }
});

test('board-graph-ac2 graph-pick: a click on a dot lights its node and its full depends_on chain', async () => {
  const f = await fixture(types);
  try {
    await world(f);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&view=graph');
    const dot = address => f.page.locator('.graph-node[data-address="' + address + '"]');
    await dot('ac:b').waitFor();
    await dot('ac:b').click();
    await lights(f, ['ac:a', 'ac:b', 'ac:c'], ['ac:b>ac:a', 'ac:c>ac:b'], 'b with a upstream and c downstream');
    assert.equal(await f.page.evaluate(() => getComputedStyle(document.querySelector('.graph-node[data-address="ac:d"]')).opacity), '0.15');
    const open = f.page.locator('.graph-picked a.graph-open');
    assert.equal(await open.textContent(), 'Refund the capture');
    assert.equal(new URL(await open.getAttribute('href')).searchParams.get('object'), 'ac:b');
    await dot('ac:a').click();
    await lights(f, ['ac:a', 'ac:b', 'ac:c'], ['ac:b>ac:a', 'ac:c>ac:b'], 'a with its two dependents');
    await dot('ac:d').click();
    await lights(f, ['ac:d'], [], 'd has no chain');
    // A click on the empty canvas clears the pick.
    await f.page.locator('.graph-canvas').click({position: {x: 5, y: 5}});
    await lights(f, ALL, ['ac:b>ac:a', 'ac:c>ac:b'], 'cleared pick');
    assert.equal(await f.page.locator('.graph-picked').isHidden(), true);
  } finally { await f.close(); }
});

test('board-graph-ac3 graph-filters: the filter bar and the search light the matches and their chain', async () => {
  const f = await fixture(types);
  try {
    await world(f);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&view=graph&owner=maria');
    await f.page.locator('.graph-node').nth(7).waitFor();
    // The link filter owner=maria matches c. Its chain goes up through b to a.
    assert.equal(await f.page.getByLabel('Object owner').inputValue(), 'maria');
    await lights(f, ['ac:a', 'ac:b', 'ac:c'], ['ac:b>ac:a', 'ac:c>ac:b'], 'owner maria');
    assert.match(await f.page.locator('#tree-view-link').getAttribute('href'), /owner=maria/);
    await f.page.getByLabel('Object status').selectOption('green');
    await lights(f, [], [], 'owner maria and status green match nothing');
    await f.page.getByLabel('Object owner').selectOption('');
    await lights(f, ['test:', 'ac:a', 'ac:b', 'ac:c'], ['ac:b>ac:a', 'ac:c>ac:b'], 'status green: the test node and a with its dependents');
    assert.equal(new URL(f.page.url()).searchParams.get('status'), 'green');
    await f.page.getByLabel('Object status').selectOption('');
    await lights(f, ALL, ['ac:b>ac:a', 'ac:c>ac:b'], 'no filter');
    // The board search finds what joppa_search finds: a by the word Zebra. Its dependents light too.
    const found = (await f.mcp('joppa_search', {query: 'Zebra', limit: 50})).items.map(item => item.address.startsWith('test:') ? 'test:' : item.address);
    assert.ok(found.includes('ac:a') && !found.includes('ac:d'), JSON.stringify(found));
    await f.page.getByLabel('Search objects').fill('Zebra');
    await lights(f, [...new Set([...found, 'ac:b', 'ac:c'])], ['ac:b>ac:a', 'ac:c>ac:b'], 'search Zebra');
    await f.page.getByLabel('Search objects').fill('');
    await lights(f, ALL, ['ac:b>ac:a', 'ac:c>ac:b'], 'cleared search');
  } finally { await f.close(); }
});
