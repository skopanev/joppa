// Chromium check of the first view of a flow (requirement:board-graph): after each load the view
// box holds every dot and every edge path, and it is centred on the dots. The first view has no
// limit of world units for one pixel, so the long chain shows all of its dots.
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

// In the page: the box of the path of each .flow-edge in the coordinates of the drawing, from
// the four corners of its getBBox().
function pathBoxes() {
  const canvas = document.querySelector('.flow-canvas');
  const svg = canvas && (canvas.matches('svg') ? canvas : canvas.querySelector('svg'));
  if (!svg) return [];
  const inverse = svg.getScreenCTM().inverse();
  return [...document.querySelectorAll('.flow-edge')].map(e => {
    const path = e.matches('path,line,polyline') ? e : e.querySelector('path,line,polyline'), b = path.getBBox(), ctm = path.getScreenCTM();
    const corners = [[b.x, b.y], [b.x + b.width, b.y], [b.x, b.y + b.height], [b.x + b.width, b.y + b.height]]
      .map(([x, y]) => new DOMPoint(x, y).matrixTransform(ctm).matrixTransform(inverse));
    const xs = corners.map(p => p.x), ys = corners.map(p => p.y);
    return {from: e.dataset.from ?? null, to: e.dataset.to ?? null,
      left: Math.min(...xs), right: Math.max(...xs), top: Math.min(...ys), bottom: Math.max(...ys)};
  });
}

// Wait until the canvas draws the nodes `addresses` and `edges` edge paths, and two reads 200 ms
// apart give the same view box, dot centres and path boxes. Give that snapshot with its path boxes.
async function settled(f, addresses, edges, step) {
  const read = async () => ({...await snap(f), paths: await f.page.evaluate(pathBoxes)});
  const key = s => JSON.stringify([s.view, s.nodes.map(n => [n.address, n.dx, n.dy]), s.paths]);
  let last = null, lastKey = '';
  for (const end = Date.now() + 15000; Date.now() < end; await f.page.waitForTimeout(200)) {
    const s = await read(), names = s.nodes.map(n => n.address).sort();
    if (JSON.stringify(names) !== JSON.stringify(addresses) || s.paths.length !== edges) { last = s; lastKey = ''; continue; }
    if (key(s) === lastKey) return s;
    last = s; lastKey = key(s);
  }
  assert.fail(step + ': the drawing did not settle: ' + JSON.stringify({nodes: last?.nodes.map(n => n.address), paths: last?.paths.length, view: last?.view}));
}

// The view box, the dot centres in the drawing (with the radius) and the edge path boxes.
function evidence(s) {
  const [vx, vy, vw, vh] = s.view, xs = s.nodes.map(n => n.dx), ys = s.nodes.map(n => n.dy);
  const dots = {left: Math.min(...xs), right: Math.max(...xs), top: Math.min(...ys), bottom: Math.max(...ys)};
  return {view: s.view, unit: s.unit, viewCentre: [vx + vw / 2, vy + vh / 2],
    dotCentre: [(dots.left + dots.right) / 2, (dots.top + dots.bottom) / 2],
    dots: s.nodes.map(n => [n.address, n.dx, n.dy, n.dr]), paths: s.paths};
}

// Every dot and the box of every .flow-edge path are inside the view box, and the centre of the
// view box equals the centre of the box of the dot centres within 1 world unit.
function wholeAndCentred(e, step) {
  const [vx, vy, vw, vh] = e.view;
  const inside = b => b.left >= vx && b.right <= vx + vw && b.top >= vy && b.bottom <= vy + vh;
  for (const [address, x, y, r] of e.dots) {
    assert.ok(inside({left: x - r, right: x + r, top: y - r, bottom: y + r}),
      step + ': the dot of ' + address + ' is inside the view box: ' + JSON.stringify({x, y, r, view: e.view}));
  }
  for (const p of e.paths) {
    assert.ok(inside(p), step + ': the path ' + p.from + '>' + p.to + ' is inside the view box: ' + JSON.stringify({path: p, view: e.view}));
  }
  const gap = Math.hypot(e.viewCentre[0] - e.dotCentre[0], e.viewCentre[1] - e.dotCentre[1]);
  assert.ok(Math.abs(e.viewCentre[0] - e.dotCentre[0]) <= 1 && Math.abs(e.viewCentre[1] - e.dotCentre[1]) <= 1,
    step + ': the view centre is ' + gap + ' from the dot centre: ' + JSON.stringify({view: e.view, viewCentre: e.viewCentre, dotCentre: e.dotCentre}));
}

// ac:e11onja3
test('e11onja3: Each flow opens whole and centred', async t => {
  const f = await fixture(types);
  try {
    await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
    await declare(f, {swap: {title: 'Swap', description: 'Client screens'}, chain: {title: 'chain', description: 'A long chain'}});
    await build(f, SWAP);
    await build(f, CHAIN);
    await f.page.setViewportSize({width: 1280, height: 800});
    const relation = f.page.getByLabel('Relation', {exact: true});

    // Load 1: Flow view opens with chain, and all 30 dots show.
    await open(f, '&relation=chain', 30);
    const first = evidence(await settled(f, CHAIN_NODES, 29, 'chain'));
    t.diagnostic('chain: ' + JSON.stringify(first));
    wholeAndCentred(first, 'chain');

    // Load 2: alex picks Swap in the Relation drop-down.
    await relation.selectOption('swap');
    const swap = evidence(await settled(f, SWAP_NODES, 4, 'Swap'));
    t.diagnostic('Swap: ' + JSON.stringify(swap));
    wholeAndCentred(swap, 'Swap');

    // Load 3: alex picks chain again.
    await relation.selectOption('chain');
    const again = evidence(await settled(f, CHAIN_NODES, 29, 'chain again'));
    t.diagnostic('chain again: ' + JSON.stringify(again));
    wholeAndCentred(again, 'chain again');
  } finally { await f.close(); }
});
