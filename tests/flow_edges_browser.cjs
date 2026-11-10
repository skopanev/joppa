// Chromium check of the edge paint in Flow view (requirement:board-graph): each edge and the arrow
// head are light grey (var(--muted)) until a click, and a click paints the edges of the picked node
// in var(--heading) while the other edges keep the light stroke.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, swapWorld, open, snap, waitMarks, click} = require('./flow_support.cjs');

// In the page: the computed colours of var(--muted) and var(--heading). The probe sits next to
// the canvas, so it gets the same custom properties as the drawing.
function themeColours() {
  const canvas = document.querySelector('.flow-canvas');
  const host = canvas instanceof HTMLElement ? canvas : canvas.parentElement;
  const colour = name => {
    const s = document.createElement('span');
    s.style.color = 'var(' + name + ')';
    host.append(s);
    const out = getComputedStyle(s).color;
    s.remove();
    return out;
  };
  return {muted: colour('--muted'), heading: colour('--heading')};
}

// In the page: the paint of each .flow-edge in document order, and the fill of each
// .flow-arrow-head. The stroke and the width come from the drawn line of the edge. The opacity
// is the product of the opacities from the drawn line up to the .flow-edge element.
function edgePaint() {
  const edges = [...document.querySelectorAll('.flow-edge')].map(e => {
    const path = e.matches('path,line,polyline') ? e : e.querySelector('path,line,polyline');
    const style = getComputedStyle(path);
    let opacity = 1;
    for (let el = path; el; el = el === e ? null : el.parentElement) opacity *= Number(getComputedStyle(el).opacity);
    return {stroke: style.stroke, width: style.strokeWidth, opacity};
  });
  const heads = [...document.querySelectorAll('.flow-arrow-head')].map(h => getComputedStyle(h).fill);
  return {edges, heads};
}

const near = (value, wanted) => Math.abs(Number.parseFloat(value) - wanted) < 0.01;

// Read the paint of the edges under their names (from>to). Poll until `ok` holds or 5 seconds
// pass, so that a CSS transition can end. Give the last reading.
async function paint(f, ok) {
  let last;
  for (const end = Date.now() + 5000; Date.now() < end; await f.page.waitForTimeout(100)) {
    const s = await snap(f), p = await f.page.evaluate(edgePaint);
    last = {heads: p.heads, edges: Object.fromEntries(s.edges.map((e, i) => [e.from + '>' + e.to, p.edges[i]]))};
    if (s.edges.length === p.edges.length && ok(last)) return last;
  }
  return last;
}

const ALL = ['step:a>step:b', 'step:a>step:c', 'step:b>step:d', 'step:c>step:d'];
const LIT = ['step:a>step:b', 'step:b>step:d'];

// ac:12h9x0nm
test('12h9x0nm: Flow edges stay light grey until a click lights them', async () => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    await open(f, '&relation=swap', 4);
    const {muted, heading} = await f.page.evaluate(themeColours);
    assert.ok(/^rgba?\(/.test(muted) && /^rgba?\(/.test(heading), 'the theme colours resolve: ' + JSON.stringify({muted, heading}));
    assert.notEqual(muted, heading, 'var(--muted) and var(--heading) are different colours');

    // Before the click: each edge is light, and each arrow head has the fill of var(--muted).
    const light = e => e && e.stroke === muted && near(e.width, 1.25);
    const before = await paint(f, p => ALL.every(name => light(p.edges[name]) && near(p.edges[name].opacity, 0.38))
      && p.heads.length > 0 && p.heads.every(fill => fill === muted));
    assert.deepEqual(Object.keys(before.edges).sort(), ALL, 'the swap edges: ' + JSON.stringify(before.edges));
    for (const name of ALL) {
      const e = before.edges[name];
      assert.equal(e.stroke, muted, name + ' has the stroke colour of var(--muted) before the click');
      assert.ok(near(e.width, 1.25), name + ' has stroke-width 1.25px before the click: ' + e.width);
      assert.ok(near(e.opacity, 0.38), name + ' has opacity 0.38 before the click: ' + e.opacity);
    }
    assert.ok(before.heads.length > 0, 'the drawing has .flow-arrow-head');
    for (const fill of before.heads) assert.equal(fill, muted, '.flow-arrow-head has the fill colour of var(--muted)');

    // The click on B lights A→B and B→D.
    await click(f, 'step:b');
    await waitMarks(f, {lit: {nodes: ['step:a', 'step:b', 'step:d'], edges: LIT}}, 'B');
    const strong = e => e && e.stroke === heading && near(e.width, 2) && near(e.opacity, 1);
    const after = await paint(f, p => LIT.every(name => strong(p.edges[name]))
      && ALL.filter(name => !LIT.includes(name)).every(name => light(p.edges[name])));
    for (const name of LIT) {
      const e = after.edges[name];
      assert.ok(e, name + ' is drawn after the click: ' + JSON.stringify(after.edges));
      assert.equal(e.stroke, heading, name + ' has the stroke colour of var(--heading) after the click');
      assert.ok(near(e.width, 2), name + ' has stroke-width 2px after the click: ' + e.width);
      assert.ok(near(e.opacity, 1), name + ' has opacity 1 after the click: ' + e.opacity);
    }
    // The other edges keep the light stroke. .flow-faint sets their opacity, so the check
    // reads only the stroke colour and the width.
    for (const name of ALL.filter(name => !LIT.includes(name))) {
      const e = after.edges[name];
      assert.ok(e, name + ' is drawn after the click: ' + JSON.stringify(after.edges));
      assert.equal(e.stroke, muted, name + ' keeps the stroke colour of var(--muted) after the click');
      assert.ok(near(e.width, 1.25), name + ' keeps stroke-width 1.25px after the click: ' + e.width);
    }
  } finally { await f.close(); }
});
