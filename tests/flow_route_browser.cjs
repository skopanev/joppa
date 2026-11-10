// Chromium checks of the layered Flow view layout (requirement:board-graph): a long edge goes
// through a free row of each column between, the barycenter sweeps remove a crossing, each when
// label stands on its first segment with a halo, and two when labels that meet move apart.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, snap, byAddress} = require('./flow_support.cjs');
const {columns, yAtX, pathDistance, pathHitsBox, pathsCross, boxesOverlap} = require('./flow_geometry.cjs');

// The rows of one column are ROW_GAP apart in the drawing, and a dot has the radius RADIUS
// (src/board/flow-layout.js). A when label moves down in steps of WHEN_STEP.
const ROW_GAP = 140, RADIUS = 9, WHEN_STEP = 18;

// Save the steps `titles` (id to object title) in the relation `key` with the edges `edges`
// ([from, to, when]). `named` gives a node its own title in the relation.
async function world(f, titles, edges, key = 'swap', named = {}) {
  await declare(f, {[key]: {title: key === 'swap' ? 'Swap' : key}});
  const out = id => edges.filter(e => e[0] === id).map(([, to, when]) => ({to: 'step:' + to, ...(when ? {when} : {})}));
  await build(f, Object.keys(titles).reverse().map(id => ({id, title: titles[id],
    relations: {[key]: {...(named[id] ? {title: named[id]} : {}), edges: out(id)}}})));
}
const pathOf = (s, name) => s.edges.find(e => e.from + '>' + e.to === name);

// No two .flow-when boxes of a snapshot overlap.
function apart(s) {
  for (const [i, p] of s.whens.entries()) for (const q of s.whens.slice(i + 1)) {
    assert.ok(!boxesOverlap(p.box, q.box), p.text + ' overlaps ' + q.text + ': ' + JSON.stringify([p.box, q.box]));
  }
}

// In the page: the computed paint-order, stroke and stroke width of each .flow-label and
// .flow-when with its --k, and the colour of var(--bg).
const paint = f => f.page.evaluate(() => {
  const probe = document.createElement('span');probe.style.color = 'var(--bg)';document.body.append(probe);
  const bg = getComputedStyle(probe).color;probe.remove();
  return {bg, items: [...document.querySelectorAll('.flow-label, .flow-when')].map(e => {
    const s = getComputedStyle(e);
    return {name: e.classList.contains('flow-label') ? 'flow-label' : 'flow-when', paintOrder: s.paintOrder, stroke: s.stroke,
      width: s.strokeWidth, k: s.getPropertyValue('--k').trim()};
  })};
});

// ac:obbd10e1
// test:20zmzfjn
test('flow-route: a long edge goes through a free row of each column between', async () => {
  const f = await fixture(types);
  try {
    await world(f, {a: 'Start the swap', b: 'Check the results', c: 'Pay'}, [['a', 'b'], ['b', 'c'], ['a', 'c']], 'swap',
      {b: 'Record the result of every test of the criterion'});
    await open(f, '&relation=swap', 3);
    const s = await snap(f), at = byAddress(s), column = columns(s.nodes);
    assert.deepEqual(['a', 'b', 'c'].map(id => column['step:' + id]), [0, 1, 2], 'the columns');
    const long = pathOf(s, 'step:a>step:c'), b = at['step:b'];
    assert.ok(long, 'A→C is drawn');
    assert.equal(b.label.lines.length, 3, 'the label of B has 3 lines: ' + JSON.stringify(b.label.lines));
    // At the x of column 1, the path has the y of row 1, one ROW_GAP below B in row 0.
    const y = yAtX(long.dpoints, b.dx);
    assert.ok(y !== null && Math.abs(y - (b.dy + ROW_GAP)) <= 1, 'A→C at the x of column 1: ' + JSON.stringify({y, b: b.dy}));
    const gap = pathDistance(long.dpoints, [b.dx, b.dy]) - RADIUS;
    assert.ok(gap >= 12, 'A→C keeps 12 from the edge of the dot of B: ' + gap);
    for (const box of b.label.boxes) assert.ok(!pathHitsBox(long.points, box), 'A→C crosses a line of the label of B: ' + JSON.stringify(box));
    // The first view shows the waypoint.
    const [vx, vy, vw, vh] = s.view;
    assert.ok(b.dx >= vx && b.dx <= vx + vw && y >= vy && y <= vy + vh, 'the waypoint is in the view box: ' + JSON.stringify([b.dx, y, s.view]));
  } finally { await f.close(); }
});

// ac:gn9x8s2h
// test:t0cbuxbf
test('flow-sweep: the barycenter sweeps remove a crossing', async () => {
  const f = await fixture(types);
  try {
    await world(f, {a: 'Start', b: 'Begin', c: 'Close', d: 'Done'}, [['a', 'd'], ['b', 'c']]);
    await open(f, '&relation=swap', 4);
    const s = await snap(f), at = byAddress(s), column = columns(s.nodes);
    assert.deepEqual(['a', 'b', 'c', 'd'].map(id => column['step:' + id]), [0, 0, 1, 1], 'the columns');
    assert.ok(at['step:a'].y < at['step:b'].y, 'A stands above B');
    assert.ok(at['step:d'].y < at['step:c'].y, 'D stands above C');
    const ad = pathOf(s, 'step:a>step:d'), bc = pathOf(s, 'step:b>step:c');
    assert.ok(ad && bc, 'A→D and B→C are drawn');
    assert.ok(!pathsCross(ad.points, bc.points), 'the paths of A→D and B→C cross');
  } finally { await f.close(); }
});

// ac:4f18z6k9
// test:xjmcyjlv
test('flow-when: each when label stands on its first segment and touches no other', async () => {
  const f = await fixture(types);
  try {
    const whens = {'limit ok': 'step:a>step:b', 'limit too high': 'step:a>step:c', 'documents ok': 'step:b>step:d', 'documents missing': 'step:c>step:d'};
    await world(f, {a: 'Enter the amount', b: 'Check the limit', c: 'Ask for a document', d: 'Done'},
      [['a', 'b', 'limit ok'], ['a', 'c', 'limit too high'], ['b', 'd', 'documents ok'], ['c', 'd', 'documents missing']]);
    await open(f, '&relation=swap', 4);
    const s = await snap(f), at = byAddress(s), column = columns(s.nodes);
    assert.deepEqual(s.whens.map(w => w.text).sort(), Object.keys(whens).sort());
    for (const w of s.whens) {
      const from = whens[w.text].split('>')[0], next = s.nodes.find(n => column[n.address] === column[from] + 1);
      assert.equal(w.edge, whens[w.text], w.text + ': the nearest path is its own edge');
      assert.ok(at[from].x < w.x && w.x < next.x, w.text + ': between the source and the next column: ' + JSON.stringify([at[from].x, w.x, next.x]));
    }
    apart(s);
    // A halo of the background colour, 4·k px wide, under each label and when label.
    const {bg, items} = await paint(f);
    assert.deepEqual(['flow-label', 'flow-when'].map(name => items.filter(i => i.name === name).length), [4, 4]);
    for (const item of items) {
      const k = Number(item.k);
      assert.ok(item.k !== '' && k > 0, item.name + ' has --k: ' + JSON.stringify(item));
      assert.match(item.paintOrder, /^stroke\b/, item.name + ' paint-order');
      assert.equal(item.stroke, bg, item.name + ' stroke colour');
      assert.ok(Math.abs(parseFloat(item.width) - 4 * k) < 0.01, item.name + ' stroke width 4·k px: ' + JSON.stringify(item));
    }
  } finally { await f.close(); }
});

// ac:yswtalb0
// test:qafac055
test('flow-when-apart: two when labels that meet move apart', async () => {
  const f = await fixture(types);
  try {
    await world(f, {a: 'Start', b: 'Begin', c: 'Close', d: 'Done'},
      [['a', 'c'], ['a', 'd', 'limit ok'], ['b', 'c', 'limit too high'], ['b', 'd']], 'swap2');
    await open(f, '&relation=swap2', 4);
    const s = await snap(f), of = text => s.whens.find(w => w.text === text);
    const [ad, bc] = [of('limit ok'), of('limit too high')];
    assert.ok(ad && bc, 'both when labels show: ' + JSON.stringify(s.whens.map(w => w.text)));
    assert.ok(Math.abs(ad.ax - bc.ax) < 0.5, 'the labels have the same x: ' + JSON.stringify([ad.ax, bc.ax]));
    const n = (bc.ay - ad.ay) / WHEN_STEP;
    assert.ok(n > 0.97 && Math.abs(n - Math.round(n)) < 0.03, 'B→C is lower by 18·n, n ≥ 1: ' + JSON.stringify([ad.ay, bc.ay, n]));
    apart(s);
  } finally { await f.close(); }
});
