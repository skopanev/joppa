// Chromium checks of the Flow view labels (requirement:board-graph): a node label wraps by words
// into at most 3 lines, and the labels, the transfer badges and the dots never touch.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {wrapLabel, types, declare, build, open, snap, byAddress, waitCard, click} = require('./flow_support.cjs');

// The titles of the nodes in swap, and their lines by the criterion text.
const LABELS = {
  a: 'Check the limit of the client and the account',
  b: 'Record the result of every test of the criterion and turn the status of the requirement green',
  c: 'Pay',
  d: 'Abcdefghijklmnopqrstu',
};
const LINES = {
  a: ['Check the limit of', 'the client and the', 'account'],
  b: ['Record the result', 'of every test of', 'the criterion and…'],
  c: ['Pay'],
  d: ['Abcdefghijklmnopq…'],
};

// The swap edges A→B, A→C, B→D and C→D with the titles of LABELS. `money` gives A and B the
// relation swap-money too, and `when` gives A→B a when label.
async function world(f, money = false, when) {
  await declare(f, {swap: {title: 'Swap'}, ...(money ? {'swap-money': {title: 'Swap money'}} : {})});
  const swap = (id, edges) => ({swap: {title: LABELS[id], edges: edges.map(to => ({to: 'step:' + to}))},
    ...(money && 'ab'.includes(id) ? {'swap-money': {edges: []}} : {})});
  const relations = {a: swap('a', ['b', 'c']), b: swap('b', ['d']), c: swap('c', ['d']), d: swap('d', [])};
  if (when) relations.a.swap.edges[0].when = when;
  await build(f, ['d', 'c', 'b', 'a'].map(id => ({id, title: 'Step ' + id.toUpperCase(), relations: relations[id]})));
}

// True when two screen boxes overlap.
const overlap = (p, q) => p.left < q.right && q.left < p.right && p.top < q.bottom && q.top < p.bottom;

// ac:uyrc0eaz
// test:nzztp6bh
test('flow-label-wrap: a node label wraps by words into at most 3 lines', async () => {
  const f = await fixture(types);
  try {
    await world(f);
    await open(f, '&relation=swap', 4);
    const at = byAddress(await snap(f));
    for (const id of Object.keys(LABELS)) {
      const node = at['step:' + id], label = node.label;
      assert.ok(label, id + ' has a label');
      assert.deepEqual([label.tag, label.flow], ['text', true], id + ': the label is a <text> with .flow-label');
      assert.deepEqual(label.lines, LINES[id], id + ' lines');
      assert.deepEqual(label.lines, wrapLabel(LABELS[id]), id + ' lines by the rule');
      assert.equal(label.xs.length, LINES[id].length, id + ': one <tspan> for each line');
      for (const x of label.xs) assert.ok(x !== null && Math.abs(x - node.x) < 1, id + ': a line at the x of its dot: ' + JSON.stringify([label.xs, node.x]));
      for (let i = 1; i < label.boxes.length; i++) assert.ok(label.boxes[i].top > label.boxes[i - 1].top, id + ': line ' + (i + 1) + ' is lower');
      assert.equal(node.title, LABELS[id], id + ': the dot <title> gives the full label');
    }
    for (const id of ['a', 'b']) {
      await click(f, 'step:' + id);
      await waitCard(f, LABELS[id]);
    }
  } finally { await f.close(); }
});

// ac:sona3cwy
// test:mynmmnyr
test('flow-label-space: labels and transfer badges never touch', async () => {
  const f = await fixture(types);
  try {
    const when = 'the limit of the client is ok';
    await world(f, true, when);
    await open(f, '&relation=swap', 4);
    const s = await snap(f), at = byAddress(s);
    // Each first badge of A and B is below the last line of the label, and the last badge of B
    // is above the dot of C.
    const badges = id => s.badges.filter(b => b.address === 'step:' + id).map(b => b.box).sort((p, q) => p.top - q.top);
    for (const id of ['a', 'b']) {
      const list = badges(id), last = at['step:' + id].label.boxes.at(-1);
      assert.ok(list.length > 0, id + ' has a badge');
      assert.ok(list[0].top > last.bottom, id + ': the badge is below the label: ' + JSON.stringify([list[0], last]));
    }
    const lastB = badges('b').at(-1), dotC = at['step:c'].box;
    assert.ok(lastB.bottom < dotC.top, 'the last badge of B is above the dot of C: ' + JSON.stringify([lastB, dotC]));
    // No two label boxes of nodes in neighbour columns overlap.
    const xs = s.nodes.map(n => n.x).sort((p, q) => p - q).filter((x, i, all) => i === 0 || x > all[i - 1] + 1);
    const column = n => xs.findIndex(x => Math.abs(x - n.x) <= 1);
    assert.deepEqual(['a', 'b', 'c', 'd'].map(id => column(at['step:' + id])), [0, 1, 1, 2], 'the columns');
    for (const p of s.nodes) for (const q of s.nodes) {
      if (column(q) - column(p) !== 1) continue;
      assert.ok(!overlap(p.label.box, q.label.box), p.address + ' and ' + q.address + ': the labels overlap');
    }
    // The rows of one column are 140 apart in the drawing.
    const gap = at['step:c'].dy - at['step:b'].dy;
    assert.ok(Math.abs(gap - 140) < 0.5, 'B and C are 140 apart: ' + gap);
    // The when label keeps one line: no more than one <tspan>, and no higher than two lines of C.
    const label = s.whens.find(w => w.edge === 'step:a>step:b');
    assert.ok(label, 'A→B has its when label: ' + JSON.stringify(s.whens));
    const line = at['step:c'].label.boxes[0].bottom - at['step:c'].label.boxes[0].top;
    assert.ok(label.spans <= 1 && label.height < 2 * line, 'the when label keeps one line: ' + JSON.stringify([label, line]));
  } finally { await f.close(); }
});
