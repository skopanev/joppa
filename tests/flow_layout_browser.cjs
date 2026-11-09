// Chromium checks of the Flow view drawing (requirement:board-graph): the columns from left to
// right, the arrows and their when labels, the back arc of a cycle, the title of a node in the
// relation, and the transfer badge to another flow.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, swapWorld, open, snap, toneColour, byAddress, edgeNames, waitMarks, NONE,
  card, waitCard, click, link} = require('./flow_support.cjs');

// ac:board-graph-ac1
// test:wisx4max
test('flow-layout: the flow draws left to right with the arrow of the relation', async () => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    await open(f, '&relation=swap', 4);
    const first = await snap(f), at = byAddress(first);
    const [a, b, c, d] = ['step:a', 'step:b', 'step:c', 'step:d'].map(address => at[address]);
    const columns = JSON.stringify(first.nodes.map(n => [n.address, Math.round(n.x), Math.round(n.y)]));
    assert.ok(a.x < b.x - 1 && Math.abs(b.x - c.x) < 1 && c.x < d.x - 1, 'columns 0, 1, 1 and 2: ' + columns);
    assert.ok(b.y < c.y, 'column 1 goes down in address order: ' + columns);
    assert.deepEqual(edgeNames(first), ['step:a>step:b', 'step:a>step:c', 'step:b>step:d', 'step:c>step:d']);
    for (const edge of first.edges) assert.match(edge.marker, /#flow-arrow\b/, edge.from + '>' + edge.to + ' marker');
    assert.ok(first.arrowHead, 'the marker #flow-arrow holds .flow-arrow-head');
    // The when label sits on its edge, and every label shows.
    assert.deepEqual(first.whens, [{text: 'limit ok', shown: true, edge: 'step:a>step:b'}]);
    for (const n of first.nodes) assert.ok(n.labels.length > 0 && n.labelsShown, n.address + ' label: ' + JSON.stringify(n.labels));
    assert.equal(first.lines, 0, 'no parent line');
    // Each dot has the colour of its stored tone.
    const list = await f.api('/objects?parent=*&limit=100&fields=signal');
    const tones = Object.fromEntries(list.items.map(item => [item.address, item.signal.tone]));
    for (const n of first.nodes) assert.equal(n.fill, await toneColour(f, tones[n.address]), n.address + ' fill');
    // The dots keep their place.
    await f.page.waitForTimeout(2000);
    for (const n of (await snap(f)).nodes) {
      assert.ok(Math.hypot(n.dx - at[n.address].dx, n.dy - at[n.address].dy) < 0.5, n.address + ' moved after 2 s');
    }
    // depends_on draws the arrow from the blocker A to the node B that waits.
    await f.page.getByLabel('Relation', {exact: true}).selectOption('depends_on');
    await f.page.waitForFunction(() => document.querySelectorAll('.flow-node').length === 2);
    const depends = await snap(f);
    assert.deepEqual(depends.nodes.map(n => n.address).sort(), ['step:a', 'step:b']);
    assert.deepEqual(edgeNames(depends), ['step:a>step:b']);
    assert.match(depends.edges[0].marker, /#flow-arrow\b/);
    assert.equal(depends.lines, 0, 'no parent line');
  } finally { await f.close(); }
});

// ac:00160lr5
// test:otbohcwf
test('flow-cycle: a cycle draws a back arc above its ends', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap'}});
    await build(f, [
      {id: 'a', title: 'Start', relations: {swap: {edges: [{to: 'step:b'}]}}},
      {id: 'b', title: 'Middle', relations: {swap: {edges: [{to: 'step:c'}]}}},
      {id: 'c', title: 'Choice', relations: {swap: {edges: [{to: 'step:a'}, {to: 'step:d'}]}}},
      {id: 'd', title: 'End'},
    ]);
    await open(f, '&relation=swap', 4);
    const s = await snap(f), at = byAddress(s);
    const xs = ['step:a', 'step:b', 'step:c', 'step:d'].map(address => at[address].x);
    assert.ok(xs.every((x, i) => i === 0 || x > xs[i - 1] + 1), 'columns 0, 1, 2 and 3: ' + JSON.stringify(xs));
    assert.deepEqual(edgeNames(s), ['step:a>step:b', 'step:b>step:c', 'step:c>step:a', 'step:c>step:d']);
    const back = s.edges.filter(e => e.marks.includes('flow-back'));
    assert.deepEqual(back.map(e => e.from + '>' + e.to), ['step:c>step:a'], 'only C→A is .flow-back');
    assert.ok(back[0].top < at['step:a'].y && back[0].top < at['step:c'].y,
      'the arc rises above A and C: ' + JSON.stringify([back[0].top, at['step:a'].y, at['step:c'].y]));
  } finally { await f.close(); }
});

// ac:9t8ve58s
// test:dhmjva0y
test('flow-title: a node shows its own title in the relation on its label and its card', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap'}});
    await build(f, [
      {id: 't', title: 'Done screen'},
      {id: 'a', title: 'Amount screen', relations: {swap: {title: 'Enter amount', edges: [{to: 'step:t'}]}}},
      {id: 'b', title: 'Confirm screen', relations: {swap: {edges: [{to: 'step:t'}]}}},
    ]);
    await open(f, '&relation=swap', 3);
    const at = byAddress(await snap(f));
    assert.ok(at['step:a'].labels.includes('Enter amount') && !at['step:a'].labels.includes('Amount screen'), JSON.stringify(at['step:a'].labels));
    assert.ok(at['step:b'].labels.includes('Confirm screen'), JSON.stringify(at['step:b'].labels));
    assert.ok(at['step:t'].labels.includes('Done screen'), JSON.stringify(at['step:t'].labels));
    await click(f, 'step:a');
    await waitCard(f, 'Enter amount');
    const open1 = await card(f);
    assert.equal(open1.facts.Object, 'Amount screen', JSON.stringify(open1.facts));
    assert.equal(open1.open, 'step:a');
  } finally { await f.close(); }
});

// ac:uxd7pa0s
// test:ktnq4qao
test('flow-transfer: a transfer badge opens the other flow with the node picked', async () => {
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap'}, 'swap-money': {title: 'Swap money'}});
    await build(f, [
      {id: 'c', title: 'Confirm screen', relations: {swap: {edges: []}}},
      {id: 'b', title: 'Bank screen', relations: {swap: {edges: [{to: 'step:c'}]}, 'swap-money': {edges: []}}},
    ]);
    await open(f, '&relation=swap', 2);
    assert.deepEqual((await snap(f)).badges, [{address: 'step:b', text: '⇄ Swap money'}]);
    await f.page.locator('.flow-transfer', {hasText: 'Swap money'}).click();
    await f.page.waitForURL(url => url.searchParams.get('relation') === 'swap-money');
    assert.equal(link(f).get('relation'), 'swap-money');
    assert.equal(await f.page.getByLabel('Relation', {exact: true}).inputValue(), 'swap-money');
    await waitMarks(f, {lit: {nodes: ['step:b'], edges: []}, faint: NONE}, 'B after the transfer');
    await waitCard(f, 'Bank screen');
    assert.equal((await card(f)).open, 'step:b');
  } finally { await f.close(); }
});
