// Chromium checks of Flow group filtering and the filter-free Flow header (requirement:board-graph).
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, waitText, link} = require('./flow_support.cjs');

const opts = locator => locator.locator('option').evaluateAll(list => list.map(o => [o.value, o.textContent.trim()]));

// ac:jjsxw7vn
// test:2zxxwp71
test('flow-group: selecting a group narrows the flow list, keeps the link and resets on a cross-group transfer', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {
      swap: {title: 'Swap', group: 'Money'},
      'swap-money': {title: 'Swap money', group: 'Money'},
      chain: {title: 'Chain', group: 'Steps'},
      pay: {title: 'Pay'},
    });
    await build(f, [
      {id: 'b', title: 'Receipt', relations: {pay: {edges: [{to: 'step:a'}]}}},
      {id: 'a', title: 'Start', relations: {swap: {edges: [{to: 'step:b'}]}, chain: {edges: [{to: 'step:b'}]}}},
    ]);
    await open(f, '', 2);
    const controls = await f.page.locator('.filter-fields > select').evaluateAll(list => list.map(s => [s.name, s.getAttribute('aria-label')]));
    assert.deepEqual(controls, [['group', 'Flow group'], ['relation', 'Relation']]);
    const grouper = f.page.getByLabel('Flow group', {exact: true});
    const picker = f.page.getByLabel('Relation', {exact: true});
    assert.deepEqual(await opts(grouper), [['', 'All groups'], ['Money', 'Money'], ['Steps', 'Steps'], ['-', 'Ungrouped']]);
    assert.deepEqual(await opts(picker), [['chain', 'Chain'], ['pay', 'Pay'], ['swap', 'Swap'], ['swap-money', 'Swap money']]);
    assert.equal(await picker.inputValue(), 'chain');

    await grouper.selectOption('Money');
    await f.page.waitForURL(url => url.searchParams.get('group') === 'Money' && url.searchParams.get('relation') === 'swap');
    assert.deepEqual(await opts(picker), [['swap', 'Swap'], ['swap-money', 'Swap money']]);
    assert.equal(await picker.inputValue(), 'swap');
    assert.equal(link(f).has('node'), false);

    await f.page.reload();
    await f.page.locator('.flow-canvas').waitFor();
    assert.equal(await grouper.inputValue(), 'Money');
    assert.equal(await picker.inputValue(), 'swap');

    // Clicking the transfer badge to Chain (in Steps) resets Flow group to All groups.
    await f.page.locator('.flow-transfer').filter({hasText: 'Chain'}).click();
    await f.page.waitForURL(url => !url.searchParams.has('group') && url.searchParams.get('relation') === 'chain');
    assert.equal(await grouper.inputValue(), '');
    assert.equal(await picker.inputValue(), 'chain');

    await grouper.selectOption('-');
    await f.page.waitForURL(url => url.searchParams.get('group') === '-' && url.searchParams.get('relation') === 'pay');
    assert.deepEqual(await opts(picker), [['pay', 'Pay']]);

    await grouper.selectOption('');
    await f.page.waitForURL(url => !url.searchParams.has('group'));
    assert.deepEqual(await opts(picker), [['chain', 'Chain'], ['pay', 'Pay'], ['swap', 'Swap'], ['swap-money', 'Swap money']]);

    await open(f, '&group=nope', 2);
    assert.equal(await grouper.inputValue(), '');
    assert.equal(link(f).get('group'), 'nope');

    await open(f, '&group=Steps&relation=swap', 2);
    assert.equal(await grouper.inputValue(), 'Steps');
    assert.equal(await picker.inputValue(), 'chain');
    assert.equal(link(f).get('group'), 'Steps');
    assert.equal(link(f).get('relation'), 'swap');
  } finally { await f.close(); }
});

// ac:0sc3ky1s
// test:iy5maixk
test('flow-no-filters: Flow view omits tree filters and system relations, and says when no flow exists', async () => {
  const f1 = await fixture(types);
  try {
    await f1.command({op: 'member', id: 'maria', role: 'human'}, f1.live.owner);
    await declare(f1, {swap: {title: 'Swap'}});
    await build(f1, [
      {id: 'b', title: 'Finish'},
      {id: 'a', title: 'Begin', owner: 'maria', relations: {
        swap: {edges: [{to: 'step:b'}]},
        depends_on: {edges: [{to: 'step:b'}]},
        related: {edges: [{to: 'step:b'}]},
      }},
    ]);
    await open(f1, '', 2);
    for (const label of ['Object owner', 'Object assignee', 'Object status', 'Search objects', 'All versions', 'All workspaces', 'Flow group']) {
      assert.equal(await f1.page.getByLabel(label, {exact: true}).count(), 0, label);
    }
    const controls = await f1.page.locator('.filter-fields > *').evaluateAll(list => list.map(e => e.getAttribute('name')));
    assert.deepEqual(controls, ['relation']);
    assert.deepEqual(await opts(f1.page.getByLabel('Relation', {exact: true})), [['swap', 'Swap']]);
    assert.equal(await f1.page.locator('.flow-transfer').count(), 0);
  } finally { await f1.close(); }

  const f2 = await fixture(types);
  try {
    await build(f2, [
      {id: 'b', title: 'Target'},
      {id: 'a', title: 'Source', relations: {depends_on: {edges: [{to: 'step:b'}]}}},
    ]);
    await f2.page.goto(f2.live.root + '/?workspace=' + encodeURIComponent(f2.live.ws) + '&view=flow');
    await waitText(f2, 'No flows in this workspace.');
    assert.equal(await f2.page.getByLabel('Flow group', {exact: true}).count(), 0);
    assert.equal(await f2.page.getByLabel('Relation', {exact: true}).count(), 0);
    assert.equal(await f2.page.locator('.flow-canvas').count(), 0);
  } finally { await f2.close(); }
});
