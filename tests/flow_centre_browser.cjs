// Chromium check of the first view of a flow (requirement:board-graph): after each load the view
// box is centred on the dots. The long chain uses a fixed scale, and the small Swap flow shows
// every dot.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, snap} = require('./flow_support.cjs');

// The swap edges of ac:board-graph-ac1: A→B (when "limit ok"), A→C, B→D and C→D, and B with
// depends_on A. B is the child of C, and Maria owns C.
const SWAP = [
  {id: 'd', title: 'Zebra receipt'},
  {id: 'c', title: 'Ask for a document', owner: 'maria', relations: {swap: {edges: [{to: 'step:d'}]}}},
  {id: 'b', title: 'Check the limit', parent: 'step:c', relations: {swap: {edges: [{to: 'step:d'}]}, depends_on: {edges: [{to: 'step:a'}]}}},
  {id: 'a', title: 'Enter the amount', relations: {swap: {edges: [{to: 'step:b', when: 'limit ok'}, {to: 'step:c'}]}}},
];
// The chain N1→N2→…→N30.
const CHAIN = Array.from({length: 30}, (_, i) => ({id: 'n' + (i + 1), title: 'Chain step ' + (i + 1),
  relations: {chain: {edges: i < 29 ? [{to: 'step:n' + (i + 2)}] : []}}}));
const SWAP_NODES = SWAP.map(n => 'step:' + n.id).sort();
const CHAIN_NODES = CHAIN.map(n => 'step:' + n.id).sort();

// Wait until the canvas draws the nodes `addresses` and two reads 200 ms apart give the same
// view box, unit and dot centres. Give that snapshot.
async function settled(f, addresses, step) {
  const key = s => JSON.stringify([s.view, s.unit, s.nodes.map(n => [n.address, n.dx, n.dy])]);
  let last = null, lastKey = '';
  for (const end = Date.now() + 15000; Date.now() < end; await f.page.waitForTimeout(200)) {
    const s = await snap(f), names = s.nodes.map(n => n.address).sort();
    if (JSON.stringify(names) !== JSON.stringify(addresses)) { last = s; lastKey = ''; continue; }
    if (key(s) === lastKey) return s;
    last = s; lastKey = key(s);
  }
  assert.fail(step + ': the drawing did not settle: ' + JSON.stringify({nodes: last?.nodes.map(n => n.address), view: last?.view}));
}

// The view box, the dot centres in the drawing and the units for one pixel of a snapshot.
function evidence(s) {
  const [vx, vy, vw, vh] = s.view, xs = s.nodes.map(n => n.dx), ys = s.nodes.map(n => n.dy);
  const dots = {left: Math.min(...xs), right: Math.max(...xs), top: Math.min(...ys), bottom: Math.max(...ys)};
  return {view: s.view, unit: s.unit, viewCentre: [vx + vw / 2, vy + vh / 2],
    dotCentre: [(dots.left + dots.right) / 2, (dots.top + dots.bottom) / 2], dots: s.nodes.map(n => [n.address, n.dx, n.dy, n.dr])};
}

// The centre of the view box equals the centre of the box of the dot centres within 1 world unit.
function centred(e, step) {
  const gap = Math.hypot(e.viewCentre[0] - e.dotCentre[0], e.viewCentre[1] - e.dotCentre[1]);
  assert.ok(Math.abs(e.viewCentre[0] - e.dotCentre[0]) <= 1 && Math.abs(e.viewCentre[1] - e.dotCentre[1]) <= 1,
    step + ': the view centre is ' + gap + ' from the dot centre: ' + JSON.stringify(e));
}

// For chain, the view uses 1.6 world units for one pixel within 0.01.
function chainScale(e, step) {
  assert.ok(Math.abs(e.unit - 1.6) <= 0.01, step + ': ' + e.unit + ' world units for one pixel: ' + JSON.stringify(e));
}

// ac:e11onja3
test('e11onja3: Each flow opens centred on its nodes', async t => {
  const f = await fixture(types);
  try {
    await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
    await declare(f, {swap: {title: 'Swap', description: 'Client screens'}, chain: {title: 'chain', description: 'A long chain'}});
    await build(f, SWAP);
    await build(f, CHAIN);
    await f.page.setViewportSize({width: 1280, height: 800});
    const relation = f.page.getByLabel('Relation', {exact: true});

    // Load 1: Flow view opens with chain.
    await open(f, '&relation=chain', 30);
    const first = evidence(await settled(f, CHAIN_NODES, 'chain'));
    t.diagnostic('chain: ' + JSON.stringify(first));
    centred(first, 'chain');
    chainScale(first, 'chain');

    // Load 2: alex picks Swap in the Relation drop-down.
    await relation.selectOption('swap');
    const swap = evidence(await settled(f, SWAP_NODES, 'Swap'));
    t.diagnostic('Swap: ' + JSON.stringify(swap));
    centred(swap, 'Swap');
    const [vx, vy, vw, vh] = swap.view;
    for (const [address, x, y, r] of swap.dots) {
      assert.ok(x - r >= vx && x + r <= vx + vw && y - r >= vy && y + r <= vy + vh,
        'Swap: the dot of ' + address + ' is inside the view box: ' + JSON.stringify({x, y, r, view: swap.view}));
    }

    // Load 3: alex picks chain again.
    await relation.selectOption('chain');
    const again = evidence(await settled(f, CHAIN_NODES, 'chain again'));
    t.diagnostic('chain again: ' + JSON.stringify(again));
    centred(again, 'chain again');
    chainScale(again, 'chain again');
  } finally { await f.close(); }
});
