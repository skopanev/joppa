// Chromium checks of the Flow view controls (requirement:board-graph): a click that lights a
// node and its neighbours with its card, the filters and the search that fade the other nodes,
// and the drag, wheel and pinch of the camera.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, swapWorld, open, snap, byAddress, marked, waitMarks, NONE, card, waitCard, closeCard, click,
  emptyPoint, waitText, link} = require('./flow_support.cjs');

// ac:board-graph-ac2
// test:gselk9wz
test('flow-pick: a click lights the node and its neighbours and opens its card', async () => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    await open(f, '&relation=swap', 4);
    await click(f, 'step:b');
    await waitMarks(f, {lit: {nodes: ['step:a', 'step:b', 'step:d'], edges: ['step:a>step:b', 'step:b>step:d']},
      faint: {nodes: ['step:c'], edges: ['step:a>step:c', 'step:c>step:d']}}, 'B');
    const s = await snap(f);
    for (const item of [...s.nodes, ...s.edges].filter(x => x.marks.includes('flow-faint'))) assert.equal(item.faint, '0.08');
    await waitCard(f, 'Check the limit');
    const picked = await card(f);
    assert.ok(picked.text.includes('step:b'), 'the card gives the address: ' + picked.text);
    assert.equal(picked.open, 'step:b');
    const {Previous: previous, Next: next} = picked.facts;
    assert.ok(previous?.includes('Enter the amount') && previous.includes('limit ok') && !previous.includes('Zebra receipt'), 'Previous: ' + previous);
    assert.ok(next?.includes('Zebra receipt') && !next.includes('Enter the amount'), 'Next: ' + next);
    // × clears every mark.
    await closeCard(f);
    await waitMarks(f, {lit: NONE, faint: NONE}, 'after ×');
    assert.equal(await card(f), null, 'the card closes');
    // A click on the empty canvas clears every mark.
    await click(f, 'step:c');
    await waitCard(f, 'Ask for a document');
    const empty = await emptyPoint(f);
    assert.ok(empty, 'the canvas has an empty point');
    await f.page.mouse.click(empty.x, empty.y);
    await waitMarks(f, {lit: NONE, faint: NONE}, 'after the canvas click');
  } finally { await f.close(); }
});

// ac:board-graph-ac3
// test:xxdoxogd
test('flow-filters: a filter or a search fades the other nodes', async () => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    await open(f, '&relation=swap&owner=maria', 4);
    await waitMarks(f, {lit: NONE, faint: {nodes: ['step:a', 'step:b', 'step:d'], edges: []}}, 'owner maria');
    assert.equal(link(f).get('owner'), 'maria');
    assert.equal(link(f).get('relation'), 'swap');
    await f.page.getByLabel('Object owner').selectOption('');
    await waitMarks(f, {lit: NONE, faint: NONE}, 'no filter');
    const search = f.page.getByLabel('Search objects');
    await search.fill('Zebra');
    await waitMarks(f, {lit: {nodes: []}, faint: {nodes: ['step:a', 'step:b', 'step:c']}}, 'search Zebra');
    await search.fill('Nothing');
    await waitMarks(f, {lit: {nodes: []}, faint: {nodes: ['step:a', 'step:b', 'step:c', 'step:d']}}, 'search Nothing');
    await waitText(f, 'No node matches these filters.');
  } finally { await f.close(); }
});

// ac:board-graph-ac4
// test:mb610fgy
test('flow-controls: a drag pans the view box, a wheel pans and a pinch zooms', async () => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    await open(f, '&relation=swap', 4);
    const start = await snap(f), u = start.unit;
    assert.equal(start.select, 'none', 'the canvas has user-select none');
    const drag = async (x, y) => {
      await f.page.mouse.move(x, y);await f.page.mouse.down();
      for (let i = 1; i <= 10; i++) await f.page.mouse.move(x + 12 * i, y + 8 * i);
      await f.page.mouse.up();
    };
    const near = (got, wanted, what) => assert.ok(Math.abs(got - wanted) < 1.5 * u, what + ': ' + got + ' for ' + wanted);
    // Each drag moves the view box by the pointer, and each dot keeps its place in the drawing.
    const panned = async (before, x, y, what) => {
      await drag(x, y);
      const after = await snap(f), at = byAddress(before);
      near(after.view[0], before.view[0] - 120 * u, what + ' x');
      near(after.view[1], before.view[1] - 80 * u, what + ' y');
      assert.equal(after.view[2], before.view[2], what + ' width');
      for (const n of after.nodes) assert.ok(Math.hypot(n.dx - at[n.address].dx, n.dy - at[n.address].dy) < 0.5, what + ': ' + n.address + ' moved');
      return after;
    };
    const b = byAddress(start)['step:b'];
    const dotDrag = await panned(start, b.x, b.y, 'drag of B');
    assert.deepEqual(marked(dotDrag, 'flow-lit'), NONE, 'the drag of B picks nothing');
    assert.equal(await card(f), null, 'the drag of B opens no card');
    const empty = await emptyPoint(f);
    assert.ok(empty, 'the canvas has an empty point');
    const canvasDrag = await panned(dotDrag, empty.x, empty.y, 'drag of the canvas');
    // A wheel pans by its deltas. A wheel with ctrlKey (a pinch) zooms in.
    const wheel = options => f.page.evaluate(o => {
      const c = document.querySelector('.flow-canvas'), r = c.getBoundingClientRect();
      c.dispatchEvent(new WheelEvent('wheel', {bubbles: true, cancelable: true, clientX: r.x + r.width / 2, clientY: r.y + r.height / 2, ...o}));
    }, options);
    await wheel({deltaX: 40, deltaY: 30});
    const swiped = await snap(f);
    near(swiped.view[0], canvasDrag.view[0] + 40 * u, 'wheel x');
    near(swiped.view[1], canvasDrag.view[1] + 30 * u, 'wheel y');
    await wheel({deltaY: -50, ctrlKey: true});
    const zoomed = await snap(f);
    assert.ok(zoomed.view[2] < swiped.view[2] && zoomed.view[3] < swiped.view[3], 'the pinch makes the view box smaller: ' + JSON.stringify([swiped.view, zoomed.view]));
    await f.page.setViewportSize({width: 390, height: 800});
    assert.equal(await f.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true, 'no horizontal scroll at 390 px');
  } finally { await f.close(); }
});
