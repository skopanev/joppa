// Chromium checks of the edge colours in Flow view (ac:o1x7nevo, ac:4lceb06x): an edge takes its
// own colour, else the colour of its source down the flow. A coloured edge has a marker of its
// own colour, stays coloured when lit and in the dark theme, and the dots keep their tone.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, toneColour, waitMarks, click} = require('./flow_support.cjs');

// The palette of D2: light --flow-teal, light --flow-pink, dark --flow-teal and #aa0000.
const TEAL = 'rgb(15, 148, 136)', PINK = 'rgb(214, 61, 138)', DARK_TEAL = 'rgb(60, 201, 185)', RED = 'rgb(170, 0, 0)';

// The swap edges S→A, A→B, A→C, C→B, C→D, B→F, D→A and E→B. S has the colour teal, D has the
// colour #aa0000 and the edge A→C has the colour pink. No other node or edge has a colour.
const NODES = [
  {id: 's', title: 'Start', relations: {swap: {color: 'teal', edges: [{to: 'step:a'}]}}},
  {id: 'a', title: 'Enter the amount', relations: {swap: {edges: [{to: 'step:b'}, {to: 'step:c', color: 'pink'}]}}},
  {id: 'b', title: 'Check the limit', relations: {swap: {edges: [{to: 'step:f'}]}}},
  {id: 'c', title: 'Ask for a card', relations: {swap: {edges: [{to: 'step:b'}, {to: 'step:d'}]}}},
  {id: 'd', title: 'Send a document', relations: {swap: {color: '#aa0000', edges: [{to: 'step:a'}]}}},
  {id: 'e', title: 'Extra entry', relations: {swap: {edges: [{to: 'step:b'}]}}},
  {id: 'f', title: 'Receipt'},
];
const WANT = {'step:s>step:a': TEAL, 'step:a>step:b': TEAL, 'step:a>step:c': PINK, 'step:c>step:b': PINK,
  'step:c>step:d': PINK, 'step:b>step:f': TEAL, 'step:d>step:a': RED};
const GREY = 'step:e>step:b';
const LIT = ['step:a>step:c', 'step:c>step:b', 'step:c>step:d'];

async function colourWorld(f) {
  await declare(f, {swap: {title: 'Swap', description: 'Client screens'}});
  await build(f, NODES);
}

// In the page: the colour of var(--muted) next to the canvas.
function muted() {
  const canvas = document.querySelector('.flow-canvas');
  const host = canvas instanceof HTMLElement ? canvas : canvas.parentElement;
  const s = document.createElement('span');
  s.style.color = 'var(--muted)';
  host.append(s);
  const out = getComputedStyle(s).color;
  s.remove();
  return out;
}

// In the page: the paint of each .flow-edge under its name data-from>data-to. The opacity is
// the product of the opacities from the drawn line up to the .flow-edge element. marker is the
// ID that the marker-end of the edge names, and head is the fill of the path of that marker.
function edgePaint() {
  const out = {};
  for (const e of document.querySelectorAll('.flow-edge')) {
    const path = e.matches('path,line,polyline') ? e : e.querySelector('path,line,polyline');
    const style = getComputedStyle(path);
    let opacity = 1;
    for (let el = path; el; el = el === e ? null : el.parentElement) opacity *= Number(getComputedStyle(el).opacity);
    const named = [style.markerEnd, getComputedStyle(e).markerEnd, path.getAttribute('marker-end'), e.getAttribute('marker-end')]
      .map(v => /url\(\s*["']?[^#"')]*#([^"')\s]+)/.exec(v || '')).find(Boolean);
    const marker = named ? document.getElementById(named[1]) : null;
    const head = marker && marker.querySelector('path');
    out[e.dataset.from + '>' + e.dataset.to] = {stroke: style.stroke, width: style.strokeWidth, opacity,
      back: e.classList.contains('flow-back'), marker: named ? named[1] : null, tag: marker ? marker.tagName.toLowerCase() : null,
      head: head ? getComputedStyle(head).fill : null};
  }
  return out;
}

// In the page: the address, the data-tone and the computed fill of each dot.
const dotPaint = () => [...document.querySelectorAll('.flow-node')].map(n => {
  const dot = n.matches('circle,ellipse,rect') ? n : n.querySelector('circle,ellipse,rect') || n;
  return {address: n.dataset.address, tone: n.dataset.tone ?? dot.dataset.tone, fill: getComputedStyle(dot).fill};
});

const near = (value, wanted) => Math.abs(Number.parseFloat(value) - wanted) < 0.01;

// Read the edge paint until `ok` holds or 5 seconds pass, so that a CSS transition can end.
async function paint(f, ok) {
  let last;
  for (const end = Date.now() + 5000; Date.now() < end; await f.page.waitForTimeout(100)) {
    last = await f.page.evaluate(edgePaint);
    if (ok(last)) return last;
  }
  return last;
}

// Each dot has data-tone blue and the fill of var(--dot-blue) in the current theme.
async function blueDots(f, step) {
  const blue = await toneColour(f, 'blue'), dots = await f.page.evaluate(dotPaint);
  assert.equal(dots.length, NODES.length, step + ': the dots ' + JSON.stringify(dots));
  for (const dot of dots) {
    assert.equal(dot.tone, 'blue', step + ': ' + dot.address + ' has data-tone blue');
    assert.equal(dot.fill, blue, step + ': ' + dot.address + ' has the fill of var(--dot-blue)');
  }
}

// ac:o1x7nevo
test('o1x7nevo: Flow edges take the colour of their source down the flow', async () => {
  const f = await fixture(types);
  try {
    await colourWorld(f);
    await open(f, '&relation=swap', NODES.length);
    const grey = await f.page.evaluate(muted);
    assert.ok(/^rgba?\(/.test(grey), 'var(--muted) resolves: ' + grey);
    const p = await paint(f, p => Object.entries(WANT).every(([name, stroke]) => p[name]?.stroke === stroke && p[name].head === stroke)
      && p[GREY]?.stroke === grey);
    assert.deepEqual(Object.keys(p).sort(), [...Object.keys(WANT), GREY].sort(), 'the swap edges: ' + JSON.stringify(p));
    assert.ok(p['step:d>step:a'].back, 'D→A is the back edge (class flow-back): ' + JSON.stringify(p['step:d>step:a']));
    for (const [name, stroke] of Object.entries(WANT)) {
      const e = p[name];
      assert.equal(e.stroke, stroke, name + ' has the stroke colour ' + stroke + ': ' + JSON.stringify(e));
      assert.ok(near(e.opacity, 0.6), name + ' has opacity 0.6: ' + e.opacity);
      assert.ok(e.marker && e.tag === 'marker', name + ' has a marker-end that names a marker: ' + JSON.stringify(e));
      assert.equal(e.head, stroke, name + ': the path of marker #' + e.marker + ' has the fill colour of the stroke');
    }
    // E→B has no colour: it keeps the light grey style of ac:12h9x0nm and the shared marker.
    const e = p[GREY];
    assert.equal(e.stroke, grey, 'E→B has the stroke colour of var(--muted)');
    assert.ok(near(e.width, 1.25), 'E→B has stroke-width 1.25px: ' + e.width);
    assert.ok(near(e.opacity, 0.38), 'E→B has opacity 0.38: ' + e.opacity);
    assert.equal(e.marker, 'flow-arrow', 'E→B has the shared marker #flow-arrow');
    assert.equal(e.head, grey, 'the path of #flow-arrow has the fill of var(--muted)');
  } finally { await f.close(); }
});

// ac:4lceb06x
test('4lceb06x: A coloured edge stays coloured when lit and in the dark theme, and dots keep their tone', async () => {
  const f = await fixture(types);
  try {
    await colourWorld(f);
    await open(f, '&relation=swap', NODES.length);
    await paint(f, p => p['step:a>step:c']?.stroke === PINK);
    await blueDots(f, 'before the click');

    // The click on C lights A→C, C→B and C→D, and each keeps the colour pink.
    await click(f, 'step:c');
    await waitMarks(f, {lit: {nodes: ['step:a', 'step:b', 'step:c', 'step:d'], edges: LIT}}, 'C');
    const strong = e => e && e.stroke === PINK && near(e.width, 2) && near(e.opacity, 1);
    const lit = await paint(f, p => LIT.every(name => strong(p[name])));
    for (const name of LIT) {
      const e = lit[name];
      assert.ok(e, name + ' is drawn after the click: ' + JSON.stringify(lit));
      assert.equal(e.stroke, PINK, name + ' keeps the stroke colour #d63d8a after the click');
      assert.ok(near(e.width, 2), name + ' has stroke-width 2px after the click: ' + e.width);
      assert.ok(near(e.opacity, 1), name + ' has opacity 1 after the click: ' + e.opacity);
    }
    await blueDots(f, 'after the click');

    // With data-theme dark, S→A takes the dark --flow-teal, and D→A keeps #aa0000.
    await f.page.evaluate(() => { document.documentElement.dataset.theme = 'dark'; });
    const dark = await paint(f, p => p['step:s>step:a']?.stroke === DARK_TEAL && p['step:d>step:a']?.stroke === RED);
    assert.equal(dark['step:s>step:a']?.stroke, DARK_TEAL, 'S→A has #3cc9b9 in the dark theme: ' + JSON.stringify(dark));
    assert.equal(dark['step:d>step:a']?.stroke, RED, 'D→A keeps #aa0000 in the dark theme: ' + JSON.stringify(dark));
    await blueDots(f, 'in the dark theme');
  } finally { await f.close(); }
});
