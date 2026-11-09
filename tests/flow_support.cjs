// The fixture and the helpers of the Flow view checks (flow_*_browser.cjs, requirement:board-graph).
// The checks find a node by the data-address of its .flow-node or of an element around it, and
// they read each other fact from the drawing: the dot centres, the edge ends and the classes.
const assert = require('node:assert/strict');
const {testType} = require('./board_fixture.cjs');

// A step is a root node or the child of a step. Each flow node of a check is a step, and the
// tree view shows the steps first.
const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []},
  ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['requirement']},
  test: testType(['ac']),
  step: {title: 'Steps', role: 'document', status: 'none', root: true, parents: ['step'], index: 0},
};

// Put the relations `put` (key to {title, description}) into the registry as the owner.
async function declare(f, put) {
  const tree = await f.api('/tree');
  await f.command({op: 'set_tree_schema', revision: tree.current_revision, put_relations: put, reason: 'Declare the flows'}, f.live.owner);
}

// Save the steps `nodes` ({id, title, owner, parent, relations}) in their order, then their
// relations: an edge target and a parent must exist before the save that names them.
async function build(f, nodes) {
  for (const n of nodes) {
    await f.command({op: 'save_object', kind: 'step', id: n.id, revision: 0, parent: n.parent || null,
      data: {title: n.title, owner: n.owner || 'alex', body: 'Fixture text'}, reason: 'Define the flow fixture'});
  }
  for (const n of nodes.filter(n => n.relations)) {
    await f.command({op: 'save_object', kind: 'step', id: n.id, revision: 1, data: {}, relations: n.relations, reason: 'Draw the flow'});
  }
}

// The swap edges A→B (when "limit ok"), A→C, B→D and C→D, and B with depends_on A. B is the
// child of C, so a drawing of parent lines has a line between them. Maria owns C, and only
// the title of D has the word Zebra.
const F5 = [
  {id: 'd', title: 'Zebra receipt'},
  {id: 'c', title: 'Ask for a document', owner: 'maria', relations: {swap: {edges: [{to: 'step:d'}]}}},
  {id: 'b', title: 'Check the limit', parent: 'step:c', relations: {swap: {edges: [{to: 'step:d'}]}, depends_on: {edges: [{to: 'step:a'}]}}},
  {id: 'a', title: 'Enter the amount', relations: {swap: {edges: [{to: 'step:b', when: 'limit ok'}, {to: 'step:c'}]}}},
];
async function swapWorld(f) {
  await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
  await declare(f, {swap: {title: 'Swap', description: 'Client screens'}});
  await build(f, F5);
}

// Open Flow view with `query` and wait until the canvas shows `count` nodes.
async function open(f, query, count) {
  f.page.setDefaultTimeout(15000);
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&view=flow' + query);
  await f.page.locator('.flow-canvas').waitFor();
  await f.page.waitForFunction(n => document.querySelectorAll('.flow-node').length === n, count);
}

// In the page: a snapshot of the canvas. A position is the centre in screen px (x, y) and
// in the coordinates of the drawing (dx, dy). An edge goes from data-from to data-to, or else
// between the nodes nearest to its ends. drawn names the nodes nearest to its ends. marks are the classes of the element and of
// its ancestors inside the canvas. faint is the opacity of the element that has .flow-faint.
function snapshot() {
  const canvas = document.querySelector('.flow-canvas');
  const svg = canvas && (canvas.matches('svg') ? canvas : canvas.querySelector('svg'));
  // A relation switch draws the canvas again: until then the snapshot is empty.
  if (!svg) return {nodes: [], edges: [], whens: [], badges: [], lines: 0, view: [], unit: 1, arrowHead: false, select: ''};
  const address = el => (el.closest('[data-address]') || el.querySelector('[data-address]'))?.dataset.address;
  const marks = el => { const out = new Set(); for (let e = el; e && e !== canvas; e = e.parentElement) for (const c of e.classList) out.add(c); return [...out].sort(); };
  const faint = el => { for (let e = el; e && e !== canvas; e = e.parentElement) if (e.classList.contains('flow-faint')) return getComputedStyle(e).opacity; return null; };
  const inverse = svg.getScreenCTM().inverse();
  const shown = el => el.checkVisibility({opacityProperty: true, visibilityProperty: true}) && el.getBoundingClientRect().width > 0;
  const nodes = [...document.querySelectorAll('.flow-node')].map(n => {
    const dot = n.matches('circle,ellipse,rect') ? n : n.querySelector('circle,ellipse,rect') || n, b = dot.getBoundingClientRect();
    const x = b.x + b.width / 2, y = b.y + b.height / 2, p = new DOMPoint(x, y).matrixTransform(inverse), a = address(n);
    const labels = [...svg.querySelectorAll('text')].filter(t => address(t) === a && !t.closest('.flow-transfer,.flow-when'));
    return {address: a, x, y, dx: p.x, dy: p.y, fill: getComputedStyle(dot).fill, marks: marks(n), faint: faint(n),
      labels: labels.map(t => t.textContent.trim()), labelsShown: labels.every(shown)};
  });
  const near = (x, y) => nodes.reduce((best, n) => Math.hypot(n.x - x, n.y - y) < Math.hypot(best.x - x, best.y - y) ? n : best).address;
  const edges = [...document.querySelectorAll('.flow-edge')].map(e => {
    const path = e.matches('path,line,polyline') ? e : e.querySelector('path,line,polyline'), length = path.getTotalLength(), ctm = path.getScreenCTM();
    const points = Array.from({length: 41}, (_, i) => path.getPointAtLength(length * i / 40).matrixTransform(ctm));
    const marker = [path, e].map(el => getComputedStyle(el).markerEnd + ' ' + (el.getAttribute('marker-end') || '')).join(' ');
    const drawn = near(points[0].x, points[0].y) + '>' + near(points[40].x, points[40].y), [x, y] = drawn.split('>');
    return {from: e.dataset.from ?? x, to: e.dataset.to ?? y, drawn, marks: [...new Set([...marks(e), ...marks(path)])].sort(),
      faint: faint(path), marker, top: Math.min(...points.map(p => p.y)), points: points.map(p => [p.x, p.y])};
  });
  const whens = [...document.querySelectorAll('.flow-when')].map(w => {
    const b = w.getBoundingClientRect(), x = b.x + b.width / 2, y = b.y + b.height / 2;
    const gap = edge => Math.min(...edge.points.map(([px, py]) => Math.hypot(px - x, py - y)));
    const edge = edges.reduce((best, e) => gap(e) < gap(best) ? e : best, edges[0]);
    return {text: w.textContent.trim(), shown: shown(w), edge: edge && edge.from + '>' + edge.to};
  });
  const badges = [...document.querySelectorAll('.flow-transfer')].map(b => ({address: address(b), text: b.textContent.trim()}));
  // Each line of the drawing that is no edge, no dot and no badge, such as a parent line.
  const lines = [...svg.querySelectorAll('line,path,polyline')].filter(l => !l.closest('defs,marker,.flow-edge,.flow-node,.flow-transfer')).length;
  const box = svg.viewBox.baseVal;
  return {nodes, edges, whens, badges, lines,
    view: [box.x, box.y, box.width, box.height], unit: 1 / svg.getScreenCTM().a,
    arrowHead: Boolean(document.querySelector('#flow-arrow .flow-arrow-head')), select: getComputedStyle(svg).userSelect};
}
const snap = f => f.page.evaluate(snapshot);
// The computed colour of var(--dot-<tone>).
const toneColour = (f, tone) => f.page.evaluate(tone => {
  const s = document.createElement('span');s.style.color = 'var(--dot-' + tone + ')';document.body.append(s);
  const colour = getComputedStyle(s).color;s.remove();return colour;
}, tone);
const byAddress = s => Object.fromEntries(s.nodes.map(n => [n.address, n]));
const edgeNames = s => s.edges.map(e => e.from + '>' + e.to).sort();
// Each edge starts at the dot of its source and ends at the dot of its target.
const drawnAsNamed = s => assert.deepEqual(s.edges.map(e => e.drawn).sort(), edgeNames(s), 'the edge ends');

// The addresses of the nodes and the names of the edges that have the class `name`.
const marked = (s, name) => ({nodes: s.nodes.filter(n => n.marks.includes(name)).map(n => n.address).sort(),
  edges: s.edges.filter(e => e.marks.includes(name)).map(e => e.from + '>' + e.to).sort()});

// Wait until the nodes (and the edges, when `wanted` names them) with .flow-lit and
// .flow-faint are `wanted`: {lit: {nodes, edges}, faint: {nodes, edges}}.
async function waitMarks(f, wanted, step) {
  const shape = s => Object.fromEntries(Object.entries(wanted).map(([key, w]) => {
    const all = marked(s, key === 'lit' ? 'flow-lit' : 'flow-faint');
    return [key, w.edges ? all : {nodes: all.nodes}];
  }));
  let last;
  for (const end = Date.now() + 10000; Date.now() < end; await f.page.waitForTimeout(100)) {
    last = shape(await snap(f));
    if (JSON.stringify(last) === JSON.stringify(wanted)) return;
  }
  assert.fail(step + ': ' + JSON.stringify(last));
}
const NONE = {nodes: [], edges: []};

// In the page: the open card, found from its Open link (the text starts with Open) and its
// button ×. head is the text of its first heading, and facts gives the text next to each label.
function readCard() {
  const link = [...document.querySelectorAll('a')].find(a => /^Open\b/.test(a.textContent.trim()) && /[?&]object=/.test(a.getAttribute('href') || ''));
  if (!link || !link.checkVisibility()) return null;
  // The card is the widest element around the link without the canvas.
  let card = link.parentElement;
  while (card.parentElement && card.parentElement !== document.body && !card.parentElement.querySelector('.flow-canvas')) card = card.parentElement;
  if (![...card.querySelectorAll('button')].some(b => b.textContent.trim() === '×')) return null;
  const fact = name => {
    const label = [...card.querySelectorAll('*')].find(e => e.children.length === 0 && e.textContent.trim() === name);
    if (!label) return null;
    if (label.tagName === 'DT') { let text = '', e = label.nextElementSibling; while (e && e.tagName !== 'DT') { text += ' ' + e.textContent; e = e.nextElementSibling; } return text.trim(); }
    return (label.nextElementSibling ? label.nextElementSibling.textContent : label.parentElement.textContent.replace(name, '')).trim();
  };
  const head = card.querySelector('h1,h2,h3,h4,h5,h6,[role=heading]');
  return {head: head?.textContent.trim(), text: card.textContent, open: new URLSearchParams(link.getAttribute('href').split('?')[1] || '').get('object'),
    facts: Object.fromEntries(['Object', 'Previous', 'Next'].map(name => [name, fact(name)]))};
}
const card = f => f.page.evaluate(readCard);
async function waitCard(f, head) {
  await f.page.waitForFunction(([source, head]) => eval('(' + source + ')')()?.head === head, [String(readCard), head])
    .catch(async () => assert.fail('card ' + head + ': ' + JSON.stringify(await card(f))));
}
const closeCard = f => f.page.locator('button:visible', {hasText: /^\s*×\s*$/}).first().click();

// Click the dot of `address`.
async function click(f, address) {
  const node = byAddress(await snap(f))[address];
  assert.ok(node, address + ' is drawn');
  await f.page.mouse.click(node.x, node.y);
}

// A point of the canvas in screen px with no node, edge, label or badge under it.
const emptyPoint = f => f.page.evaluate(() => {
  const canvas = document.querySelector('.flow-canvas'), b = canvas.getBoundingClientRect();
  for (let y = b.top + 8; y < b.bottom; y += 16) for (let x = b.left + 8; x < b.right; x += 16) {
    const hit = document.elementFromPoint(x, y);
    if (hit && canvas.contains(hit) && !hit.closest('.flow-node,.flow-edge,.flow-transfer,.flow-when,text,[data-address]')) return {x, y};
  }
  return null;
});

// In the page: the deepest visible element whose whole text is `text`, or null.
function shownText(text) {
  return [...document.querySelectorAll('body *')].find(e => e.textContent.trim() === text
    && [...e.children].every(c => c.textContent.trim() !== text) && e.checkVisibility()) || null;
}
// Wait until a visible element, such as the status line, gives the whole text `text`.
const waitText = (f, text) => f.page.waitForFunction(([source, text]) => Boolean(eval('(' + source + ')')(text)), [String(shownText), text])
  .catch(() => assert.fail('no visible text ' + JSON.stringify(text)));
const link = f => new URL(f.page.url()).searchParams;

module.exports = {types, declare, build, swapWorld, open, snap, toneColour, byAddress, edgeNames, drawnAsNamed, marked, waitMarks, NONE,
  card, waitCard, closeCard, click, emptyPoint, shownText, waitText, link};
