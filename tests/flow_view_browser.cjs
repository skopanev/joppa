// Chromium checks of the Flow view of the board (requirement:board-graph): the view drop-down,
// the removed graph modules, the relation picker, and the load of a large and an empty flow.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, shownText, waitText, link} = require('./flow_support.cjs');

// Open the tree view with `query` and wait for the root step A.
async function tree(f, query) {
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + query);
  await f.page.locator('#tree .common-object[data-object-address="step:a"]').waitFor();
}
const AB = [{id: 'b', title: 'Pay screen'}, {id: 'a', title: 'Amount screen', relations: {swap: {edges: [{to: 'step:b'}]}}}];

// ac:ogcg47cr
// test:squ97bjj
test('flow-view: the board offers Flow view, and view=graph opens the tree view', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap'}});
    await build(f, AB);
    await tree(f, '');
    const view = f.page.getByLabel('Board view');
    const options = await view.locator('option').evaluateAll(list => list.map(o => [o.value, o.textContent.trim()]));
    assert.deepEqual(options, [['tree', 'Tree view'], ['flow', 'Flow view']]);
    await view.selectOption('flow');
    await f.page.locator('.flow-canvas').waitFor();
    assert.equal(link(f).get('view'), 'flow');
    assert.equal(await f.page.title(), 'Flow · Joppa');
    assert.equal(await view.inputValue(), 'flow');
    // Flow view has no Object root, no Detail depth, no trail and no Focus flow.
    assert.equal(await f.page.getByLabel('Object root').count(), 0, 'Object root');
    assert.equal(await f.page.getByLabel('Detail depth').count(), 0, 'Detail depth');
    assert.equal(await f.page.getByLabel('Path steps').count(), 0, 'trail');
    assert.equal(await f.page.getByRole('button', {name: 'Focus flow'}).count(), 0, 'Focus flow');
    // The old view name has no alias: it opens the tree view.
    await tree(f, '&view=graph');
    assert.equal(await f.page.getByLabel('Board view').inputValue(), 'tree');
    assert.equal(await f.page.locator('.flow-canvas').count(), 0);
  } finally { await f.close(); }
});

// ac:jk7n4f2w
// test:jpszvol6
test('flow-removal: the board modules graph-force.js and graph.js give 404', async () => {
  const f = await fixture(types);
  try {
    const get = async name => (await fetch(f.live.root + '/board/' + name, {headers: {Authorization: 'Bearer ' + f.live.agent}})).status;
    assert.equal(await get('board.js'), 200, 'a present board module');
    assert.equal(await get('graph-force.js'), 404, 'graph-force.js');
    assert.equal(await get('graph.js'), 404, 'graph.js');
  } finally { await f.close(); }
});

// ac:0lp5jzyh
// test:7hq03l5j
test('flow-picker: the Relation picker lists the registry in key order and keeps a picked key in the link', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap', description: 'Client screens'}, 'swap-money': {title: 'Swap money'}});
    await build(f, AB);
    await open(f, '', 2);
    const picker = f.page.getByLabel('Relation', {exact: true});
    const options = await picker.locator('option').evaluateAll(list => list.map(o => [o.value, o.textContent.trim()]));
    assert.deepEqual(options, [['depends_on', 'Depends on'], ['related', 'Related'], ['swap', 'Swap'], ['swap-money', 'Swap money']]);
    // The first view is the first key that is not a system key, and the link names no relation.
    assert.equal(await picker.inputValue(), 'swap');
    assert.equal(link(f).has('relation'), false, 'the first view writes no relation');
    const description = f.page.locator('.flow-description');
    assert.equal(await description.isVisible(), true);
    assert.equal((await description.textContent()).trim(), 'Client screens');
    await picker.selectOption('swap-money');
    await f.page.waitForURL(url => url.searchParams.get('relation') === 'swap-money');
    await description.waitFor({state: 'hidden'});
    await picker.selectOption('depends_on');
    await f.page.waitForURL(url => url.searchParams.get('relation') === 'depends_on');
    await f.page.reload();
    assert.equal(await f.page.getByLabel('Relation', {exact: true}).inputValue(), 'depends_on');
    assert.equal(link(f).get('relation'), 'depends_on');
    // An unknown key shows the first view and stays in the link.
    await open(f, '&relation=nope', 2);
    assert.equal(await picker.inputValue(), 'swap');
    assert.equal(link(f).get('relation'), 'nope');
    // The tree view ignores relation.
    await tree(f, '&relation=swap');
    const withRelation = await f.page.locator('#tree').innerText();
    await tree(f, '');
    assert.equal(withRelation, await f.page.locator('#tree').innerText());
  } finally { await f.close(); }
});

// ac:8n222i8j
// test:ggc2dpjv
test('flow-load: a large flow reads every page of 25, and an empty flow says so', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap'}, empty: {title: 'Empty'}});
    const step = i => ({command: {op: 'save_object', kind: 'step', id: 'n' + String(i).padStart(3, '0'), revision: 0, parent: null,
      data: {title: 'Screen ' + i, owner: 'alex', body: 'Fixture text'}, relations: {swap: {edges: []}}, reason: 'Define the large flow'}});
    for (const start of [0, 60])
      await f.mcp('joppa_batch', {request_id: crypto.randomUUID(), operations: Array.from({length: 60}, (_, i) => step(start + i))});
    // Each member read of swap waits 400 ms, so the status line shows each step of the load.
    const swap = url => url.searchParams.get('in_relation') === 'swap';
    await f.page.route(swap, async route => { await new Promise(done => setTimeout(done, 400)); await route.continue(); });
    const pages = [];
    f.page.on('response', response => { if (swap(new URL(response.url()))) pages.push(response.json().then(v => v.items.length)); });
    f.page.setDefaultTimeout(30000);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&view=flow&relation=swap');
    await waitText(f, 'Loading nodes… 25 of 120');
    await f.page.evaluate(([source, text]) => { eval('(' + source + ')')(text).dataset.check = 'status'; }, [String(shownText), 'Loading nodes… 25 of 120']);
    await f.page.waitForFunction(() => document.querySelectorAll('.flow-node').length === 120);
    assert.deepEqual(await Promise.all(pages), [25, 25, 25, 25, 20], 'five pages of 25');
    // After the load, the status line is hidden.
    await f.page.waitForFunction(() => {
      const line = document.querySelector('[data-check=status]');
      return !line || !line.checkVisibility();
    }).catch(() => assert.fail('the status line shows after the load'));
    assert.equal(await f.page.evaluate(() => [...document.querySelectorAll('body *')]
      .some(e => e.checkVisibility() && /^Loading nodes/.test(e.textContent.trim()))), false);
    await f.page.getByLabel('Relation', {exact: true}).selectOption('empty');
    await waitText(f, 'No node holds this relation.');
    assert.equal(await f.page.locator('.flow-node').count(), 0);
    assert.equal(link(f).get('relation'), 'empty');
  } finally { await f.close(); }
});
