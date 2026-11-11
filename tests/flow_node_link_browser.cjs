// Chromium checks of the Flow link of a node (requirement:board-graph): the card head copies a
// link that opens Flow at the node, the link opens near the node in a wide flow and whole in a
// small one, and the node parameter takes the public item ID of the node.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, swapWorld, open, snap, marked, card, waitCard, click, closeCard} = require('./flow_support.cjs');

// The order of the children of .flow-card-head.
const ORDER = ['toggle', 'circle', 'title', 'copy', 'link', 'open', 'close'];

// In the page: the name of each child of .flow-card-head, from its role, class and aria-label.
function headNames() {
  const head = document.querySelector('.flow-card-head');
  if (!head) return null;
  return [...head.children].map(c => {
    const label = c.getAttribute('aria-label'), cls = String(c.className), tag = c.tagName.toLowerCase();
    if (tag === 'button' && (label === 'Collapse the card' || label === 'Expand the card')) return 'toggle';
    if (/\bstatus-circle\b/.test(cls)) return 'circle';
    if (/\bflow-card-title\b/.test(cls)) return 'title';
    if (tag === 'button' && label === 'Copy ID for agent') return 'copy';
    if (tag === 'button' && label === 'Copy link to node') return 'link';
    if (tag === 'a' && /^Open\b/.test(c.textContent.trim())) return 'open';
    if (c.textContent.trim() === '×') return 'close';
    return tag + '.' + cls + '[' + label + ']';
  });
}

// The query of the page as an object.
const search = page => page.evaluate(() => Object.fromEntries(new URLSearchParams(location.search)));

// Wait until the query of the page satisfies `ok`, or fail with the last query.
async function waitSearch(page, ok, step) {
  let last;
  for (const end = Date.now() + 10000; Date.now() < end; await page.waitForTimeout(100)) {
    last = await search(page);
    if (ok(last)) return last;
  }
  assert.fail(step + ': location.search ' + JSON.stringify(last));
}

// The value of the Relation drop-down.
const relation = page => page.getByLabel('Relation', {exact: true}).inputValue();

// Go to `url` in the page of `g` and wait until the canvas shows `count` nodes.
async function goto(g, url, count) {
  await g.page.goto(url);
  await g.page.locator('.flow-canvas').waitFor();
  await g.page.waitForFunction(n => document.querySelectorAll('.flow-node').length === n, count);
}

// Wait until the canvas draws `count` nodes and two reads 200 ms apart give the same view box
// and dot centres. Give that snapshot.
async function settled(f, count, step) {
  const key = s => JSON.stringify([s.view, s.nodes.map(n => [n.address, n.dx, n.dy])]);
  let last = null, lastKey = '';
  for (const end = Date.now() + 15000; Date.now() < end; await f.page.waitForTimeout(200)) {
    const s = await snap(f);
    if (s.nodes.length !== count) { last = s; lastKey = ''; continue; }
    if (key(s) === lastKey) return s;
    last = s; lastKey = key(s);
  }
  assert.fail(step + ': the drawing did not settle: ' + JSON.stringify({nodes: last?.nodes.length, view: last?.view}));
}

// The dot of a node is inside the view box `view`.
const inside = (view, n) => { const [vx, vy, vw, vh] = view;
  return n.dx - n.dr >= vx && n.dx + n.dr <= vx + vw && n.dy - n.dr >= vy && n.dy + n.dr <= vy + vh; };
// The centre of a dot is outside the view box `view`.
const outside = (view, n) => { const [vx, vy, vw, vh] = view;
  return n.dx < vx || n.dx > vx + vw || n.dy < vy || n.dy > vy + vh; };

// ac:4v583enz
test('4v583enz: The Flow card copies a link that opens Flow at the node', async () => {
  const f = await fixture(types);
  try {
    await f.page.context().grantPermissions(['clipboard-read', 'clipboard-write'], {origin: f.live.root});
    await swapWorld(f);
    await open(f, '&relation=swap', 4);
    await click(f, 'step:b');
    await waitCard(f, 'Check the limit');

    // The head children go in their order, with Copy link between Copy ID and Open.
    const names = await f.page.evaluate(headNames);
    assert.deepEqual(names, ORDER, 'the head children: ' + JSON.stringify(names));
    const linkAt = ORDER.indexOf('link');
    await f.page.locator('.flow-card-head button[aria-label="Copy link to node"]').click();

    // The clipboard holds L, the button gets data-copy-state "copied" and the toast says "Copied L".
    const L = new URL(f.live.root).origin + '/?' + new URLSearchParams({workspace: f.live.ws, view: 'flow', relation: 'swap', node: 'step:b'});
    await f.page.waitForFunction(at => document.querySelector('.flow-card-head')?.children[at]?.dataset.copyState === 'copied', linkAt)
      .catch(async () => assert.fail('copy state: ' + JSON.stringify(await f.page.evaluate(headNames))));
    await f.page.waitForFunction(text => document.querySelector('#toast')?.textContent.trim() === text, 'Copied ' + L)
      .catch(async () => assert.fail('toast: ' + JSON.stringify(await f.page.evaluate(() => document.querySelector('#toast')?.textContent))));
    assert.equal(await f.page.evaluate(() => navigator.clipboard.readText()), L, 'the clipboard text');

    // A new page opens L: Swap, the card of B, B is lit, and location.search equals that of L.
    const page = await f.page.context().newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.setDefaultTimeout(15000);
    const g = {...f, page};
    await goto(g, L, 4);
    await waitCard(g, 'Check the limit');
    assert.equal(await relation(page), 'swap', 'the new page shows Swap');
    assert.equal((await card(g)).open, 'step:b', 'the card shows B');
    assert.ok(marked(await snap(g), 'flow-lit').nodes.includes('step:b'), 'B is .flow-lit');
    assert.equal(await page.evaluate(() => location.search), new URL(L).search, 'location.search of L');

    // A click on C puts the address of C in node.
    await click(g, 'step:c');
    await waitCard(g, 'Ask for a document');
    await waitSearch(page, s => s.node === 'step:c' && s.relation === 'swap', 'the click on C');

    // × removes node.
    await closeCard(g);
    await waitSearch(page, s => !('node' in s) && s.relation === 'swap', '×');

    // Depends on gives relation=depends_on and no node.
    await page.getByLabel('Relation', {exact: true}).selectOption('depends_on');
    await waitSearch(page, s => s.relation === 'depends_on' && !('node' in s), 'Depends on');

    // node=step:none shows Swap with no card and keeps node.
    const none = new URL(L);
    none.searchParams.set('node', 'step:none');
    await goto(g, none.href, 4);
    await page.waitForTimeout(500);
    assert.equal(await relation(page), 'swap', 'node=step:none shows Swap');
    assert.equal(await card(g), null, 'node=step:none shows no card');
    const kept = await search(page);
    assert.equal(kept.node, 'step:none', 'node=step:none stays: ' + JSON.stringify(kept));
    assert.deepEqual(errors, [], 'the new page has no errors');
  } finally { await f.close(); }
});

// ac:kwkcrtes
test('kwkcrtes: A link to a node opens near it in a wide flow and whole in a small one', async t => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    await declare(f, {chain: {title: 'chain', description: 'A long chain'}});
    await build(f, Array.from({length: 30}, (_, i) => ({id: 'n' + (i + 1), title: 'Chain step ' + (i + 1),
      relations: {chain: {edges: i < 29 ? [{to: 'step:n' + (i + 2)}] : []}}})));
    await f.page.setViewportSize({width: 1280, height: 800});

    // The wide flow: the view holds N14, N15 and N16 and leaves out N1 and N30.
    await open(f, '&relation=chain&node=step:n15', 30);
    await waitCard(f, 'Chain step 15');
    const chain = await settled(f, 30, 'chain');
    const at = Object.fromEntries(chain.nodes.map(n => [n.address, n]));
    t.diagnostic('chain: ' + JSON.stringify({view: chain.view, unit: chain.unit}));
    assert.equal((await card(f)).open, 'step:n15', 'the card shows N15');
    for (const a of ['step:n14', 'step:n15', 'step:n16']) {
      assert.ok(inside(chain.view, at[a]), 'the dot of ' + a + ' is inside the view box: ' + JSON.stringify({dot: at[a], view: chain.view}));
    }
    for (const a of ['step:n1', 'step:n30']) {
      assert.ok(outside(chain.view, at[a]), a + ' is outside the view box: ' + JSON.stringify({x: at[a].dx, y: at[a].dy, view: chain.view}));
    }
    assert.ok(chain.unit >= 1 - 1e-6, 'the wide flow uses at least 1 world unit for one pixel: ' + chain.unit);

    // The small flow: every dot is inside the view box, and the view is centred on the dots.
    await open(f, '&relation=swap&node=step:b', 4);
    await waitCard(f, 'Check the limit');
    const swap = await settled(f, 4, 'Swap');
    t.diagnostic('Swap: ' + JSON.stringify({view: swap.view, unit: swap.unit}));
    assert.equal((await card(f)).open, 'step:b', 'the card shows B');
    for (const n of swap.nodes) assert.ok(inside(swap.view, n), 'the dot of ' + n.address + ' is inside the view box: ' + JSON.stringify({dot: n, view: swap.view}));
    const [vx, vy, vw, vh] = swap.view, xs = swap.nodes.map(n => n.dx), ys = swap.nodes.map(n => n.dy);
    const centre = [(Math.min(...xs) + Math.max(...xs)) / 2, (Math.min(...ys) + Math.max(...ys)) / 2];
    assert.ok(Math.abs(vx + vw / 2 - centre[0]) <= 1 && Math.abs(vy + vh / 2 - centre[1]) <= 1,
      'the view centre equals the dot centre: ' + JSON.stringify({view: swap.view, centre}));
    assert.ok(swap.unit <= 1 + 1e-6, 'the small flow uses at most 1 world unit for one pixel: ' + swap.unit);
  } finally { await f.close(); }
});

// ac:truc29es
test('truc29es: A flow link takes the public item ID of the node', async () => {
  const f = await fixture(types);
  try {
    await swapWorld(f);
    // P is the public item ID of B.
    const P = (await f.api('/objects?address=step:b')).item_id;
    assert.ok(typeof P === 'string' && P.length > 0, 'B has a public item ID: ' + JSON.stringify(P));
    await open(f, '&relation=swap&node=' + encodeURIComponent(P), 4);

    // The page shows Swap with the card of B, B is lit, and node stays P.
    await waitCard(f, 'Check the limit');
    assert.equal(await relation(f.page), 'swap', 'the page shows Swap');
    assert.equal((await card(f)).open, 'step:b', 'the card shows B');
    assert.ok(marked(await snap(f), 'flow-lit').nodes.includes('step:b'), 'B is .flow-lit');
    await f.page.waitForTimeout(500);
    const before = await search(f.page);
    assert.equal(before.node, P, 'node stays P until the click: ' + JSON.stringify(before));

    // A click on C puts the address of C in node.
    await click(f, 'step:c');
    await waitCard(f, 'Ask for a document');
    await waitSearch(f.page, s => s.node === 'step:c', 'the click on C');
  } finally { await f.close(); }
});
