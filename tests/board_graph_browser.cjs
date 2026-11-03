// Chromium checks of the board graph view (src/board/graph*.js): the dots, the parent lines and
// the depends_on arrows, the path of a picked dot with its card, the filters that keep only
// their path, and the pointer, wheel and pinch controls.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {world, ALL, open, classes, inView, waitClasses} = require('./board_graph_support.cjs');

test('board-graph-ac1 graph: each node is a dot of its tone and level with parent lines and depends_on arrows', async () => {
  const f = await fixture(world.types);
  try {
    await world.build(f);
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.locator('#tree .common-object[data-object-address="domain:pay"]').waitFor();
    const view = f.page.getByLabel('Board view');
    assert.equal(await view.inputValue(), 'tree');
    await view.selectOption('graph');
    await f.page.locator('.graph-canvas[data-state=rest]').waitFor();
    assert.equal(new URL(f.page.url()).searchParams.get('view'), 'graph');
    assert.equal(await view.inputValue(), 'graph');
    // Each dot has the stored tone of its node, the colour of that tone and the size of its level.
    const list = await f.api('/objects?parent=*&limit=100&fields=signal');
    const tones = Object.fromEntries(list.items.map(item => [item.address, item.signal.tone]));
    const dots = await f.page.evaluate(() => [...document.querySelectorAll('.graph-node')].map(dot => {
      const probe = document.createElement('span');probe.style.color = 'var(--dot-' + dot.dataset.tone + ')';document.body.append(probe);
      const wanted = getComputedStyle(probe).color;probe.remove();
      return {address: dot.dataset.address, tone: dot.dataset.tone, fill: getComputedStyle(dot).fill, wanted, r: Number(dot.getAttribute('r')), opacity: getComputedStyle(dot).opacity};
    }));
    assert.equal(dots.length, 8);
    const radius = Object.fromEntries(dots.map(d => [d.address.startsWith('test:') ? 'test:' : d.address, d.r]));
    for (const dot of dots) {
      assert.equal(dot.tone, tones[dot.address], dot.address + ' tone');
      assert.equal(dot.fill, dot.wanted, dot.address + ' colour');
      assert.equal(dot.opacity, '0.88', dot.address + ' without a pick');
    }
    for (const [a, b, ratio] of [['domain:pay', 'ac:a', 12 / 8.5], ['ac:a', 'test:', 8.5 / 6], ['domain:ship', 'domain:pay', 1]]) {
      assert.ok(Math.abs(radius[a] / radius[b] - ratio) < 0.03, a + ' / ' + b + ': ' + JSON.stringify(radius));
    }
    assert.equal(tones['ac:a'], 'green');
    assert.equal(await f.page.locator('.graph-parent').count(), 6, 'one parent line for each node with a parent');
    const arrows = await f.page.evaluate(() => [...document.querySelectorAll('.graph-depends')]
      .map(path => [path.dataset.from, path.dataset.to, getComputedStyle(path).markerEnd.includes('graph-arrow')]));
    assert.deepEqual(arrows.sort(), [['ac:a', 'ac:b', true], ['ac:b', 'ac:c', true]]);
    assert.deepEqual(await classes(f), {hidden: [], lit: [], faint: []});
    // The roots are named first, and no two shown labels overlap.
    const labels = await f.page.evaluate(() => [...document.querySelectorAll('.graph-label')].filter(l => getComputedStyle(l).display !== 'none')
      .map(l => {const b = l.getBoundingClientRect();return {address: l.dataset.address, box: [b.left, b.top, b.right, b.bottom]};}));
    for (const root of ['domain:pay', 'domain:ship']) assert.ok(labels.some(l => l.address === root), root + ' has a label');
    for (const [i, a] of labels.entries()) for (const b of labels.slice(i + 1)) {
      assert.ok(!(a.box[0] < b.box[2] && a.box[2] > b.box[0] && a.box[1] < b.box[3] && a.box[3] > b.box[1]), a.address + ' overlaps ' + b.address);
    }
    assert.ok(await inView(f, ALL), 'the camera shows every node');
    await f.page.setViewportSize({width: 390, height: 800});
    assert.equal(await f.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  } finally { await f.close(); }
});

test('board-graph-ac2 graph-pick: a click lights the flow in steps, shows the card and fades the rest', async () => {
  const f = await fixture(world.types);
  try {
    await world.build(f);
    await open(f, '');
    const dot = address => f.page.locator('.graph-node[data-address="' + address + '"]');
    await dot('ac:b').click();
    const flow = ['ac:a', 'ac:b', 'ac:c'], rest = ['ac:d', 'ac:e', 'domain:pay', 'domain:ship', 'test:'];
    await waitClasses(f, {hidden: [], lit: flow, faint: rest}, 'b: a upstream and c downstream');
    const look = await f.page.evaluate(() => ({
      steps: Object.fromEntries([...document.querySelectorAll('.graph-step.graph-step-shown')].map(s => [s.dataset.address, s.textContent])),
      trail: [...document.querySelectorAll('.graph-picked .graph-hop')].map(h => h.dataset.address),
      faint: getComputedStyle(document.querySelector('.graph-node[data-address="ac:d"]')).opacity,
      grown: Number(document.querySelector('.graph-node[data-address="ac:b"]').getAttribute('r')) / Number(document.querySelector('.graph-node[data-address="ac:d"]').getAttribute('r')),
      arrow: getComputedStyle(document.querySelector('.graph-depends[data-to="ac:b"]')).strokeWidth}));
    assert.deepEqual(look.steps, {'ac:a': '1', 'ac:b': '2', 'ac:c': '3'});
    assert.deepEqual(look.trail, flow);
    assert.deepEqual([look.faint, look.arrow], ['0.08', '2.5px']);
    assert.ok(Math.abs(look.grown - 10 / 8.5) < 0.03, String(look.grown));
    assert.ok(await inView(f, flow), 'the camera shows the flow');
    // The card names the node, its parent, its data and its steps of one hop.
    const card = f.page.locator('.graph-card');
    await card.locator('.graph-card-facts dt', {hasText: 'owner'}).waitFor();
    assert.equal(await card.locator('.graph-card-title').textContent(), 'Refund the capture');
    const facts = await card.evaluate(c => Object.fromEntries([...c.querySelectorAll('dt')].map(dt => [dt.textContent, dt.nextElementSibling.textContent])));
    assert.equal(facts.Address, 'ac:b');
    assert.equal(facts.Parent, 'Payments');
    assert.equal(facts.owner, 'alex');
    assert.equal(new URL(await card.locator('a.graph-open').getAttribute('href')).searchParams.get('object'), 'ac:b');
    const hops = title => card.locator('.graph-card-step', {hasText: title}).locator('.graph-hop').evaluateAll(h => h.map(x => x.dataset.address));
    assert.deepEqual([await hops('Depends on'), await hops('Needed by')], [['ac:a'], ['ac:c']]);
    // Hide folds the details, and Details opens them again.
    await card.getByRole('button', {name: 'Hide ▴'}).click();
    assert.equal(await card.locator('.graph-card-details').isHidden(), true);
    await card.getByRole('button', {name: 'Details ▾'}).click();
    // A step of the card picks its node. × clears the pick.
    await card.locator('.graph-card-step', {hasText: 'Needed by'}).locator('.graph-hop').click();
    await f.page.waitForFunction(() => document.querySelector('.graph-card .graph-card-title')?.textContent === 'Ship after the refund');
    await waitClasses(f, {hidden: [], lit: flow, faint: rest}, 'c: b and a upstream');
    await card.locator('.graph-card-close').click();
    await waitClasses(f, {hidden: [], lit: [], faint: []}, 'the close button clears the pick');
    assert.equal(await card.isHidden(), true);
    await dot('ac:d').click();
    await waitClasses(f, {hidden: [], lit: ['ac:d'], faint: ['ac:a', 'ac:b', 'ac:c', 'ac:e', 'domain:pay', 'domain:ship', 'test:']}, 'd has no flow');
    // Focus flow keeps only the flow of the node. A click on the empty canvas resets the graph.
    await card.getByRole('button', {name: 'Focus flow'}).click();
    await waitClasses(f, {hidden: ['ac:a', 'ac:b', 'ac:c', 'ac:e', 'domain:pay', 'domain:ship', 'test:'], lit: ['ac:d'], faint: []}, 'focus on d');
    await f.page.locator('.graph-canvas').click({position: {x: 3, y: 3}});
    await waitClasses(f, {hidden: [], lit: [], faint: []}, 'a click on the empty canvas resets the graph');
  } finally { await f.close(); }
});

test('board-graph-ac3 graph-filters: a filter, a field, a root or a search keeps only its subgraph and flow', async () => {
  const f = await fixture(world.types);
  try {
    await world.build(f);
    await open(f, '&owner=maria');
    const flow = ['ac:a', 'ac:b', 'ac:c', 'test:'];
    assert.equal(await f.page.getByLabel('Object owner').inputValue(), 'maria');
    await waitClasses(f, {hidden: ['ac:d', 'ac:e', 'domain:pay', 'domain:ship'], lit: flow, faint: []}, 'owner maria: c, its depends_on nodes and the branch of a');
    assert.equal(await f.page.evaluate(() => getComputedStyle(document.querySelector('.graph-node[data-address="domain:pay"]')).display), 'none');
    assert.ok(await inView(f, flow), 'the camera shows the subgraph');
    const labels = await f.page.evaluate(() => [...document.querySelectorAll('.graph-label')].filter(l => getComputedStyle(l).display !== 'none').map(l => l.dataset.address.startsWith('test:') ? 'test:' : l.dataset.address).sort());
    assert.deepEqual(labels, flow, 'a small flow names every node');
    await f.page.getByLabel('Object status').selectOption('green');
    await waitClasses(f, {hidden: ALL, lit: [], faint: []}, 'owner maria and status green match nothing');
    assert.equal(await f.page.locator('.graph-status').textContent(), 'No node matches these filters.');
    await f.page.getByLabel('Object owner').selectOption('');
    await waitClasses(f, {hidden: ['ac:d', 'ac:e', 'domain:pay', 'domain:ship'], lit: flow, faint: []}, 'status green: the test node and a, with b and c');
    await f.page.getByLabel('Object status').selectOption('');
    await waitClasses(f, {hidden: [], lit: [], faint: []}, 'no filter');
    // The schema filter field tags gives a drop-down of its values.
    const tags = f.page.getByLabel('Filter tags');
    assert.deepEqual(await tags.locator('option').allTextContents(), ['All tags', 'label', 'swap']);
    await tags.selectOption({label: 'swap'});
    await waitClasses(f, {hidden: ['ac:d', 'ac:e', 'domain:pay', 'domain:ship'], lit: flow, faint: []}, 'tag swap');
    assert.equal(new URL(f.page.url()).searchParams.get('data'), '{"tags":["swap"]}');
    await tags.selectOption('');
    // The root picker lists the roots with children and their children with children.
    assert.deepEqual(await f.page.getByLabel('Object root').locator('option').allTextContents(), ['All roots', 'Payments', '  ↳ Zebra card capture', 'Shipping']);
    assert.deepEqual(await f.page.getByLabel('Detail depth').locator('option').allTextContents(), ['Top 3 levels', 'Top 2 levels', 'All levels']);
    await f.page.getByLabel('Object root').selectOption({label: 'Shipping'});
    await waitClasses(f, {hidden: ['ac:e', 'domain:pay'], lit: ['ac:a', 'ac:b', 'ac:c', 'ac:d', 'domain:ship', 'test:'], faint: []}, 'root ship: its branch, b and a');
    assert.equal(new URL(f.page.url()).searchParams.get('parent'), 'domain:ship');
    await f.page.getByLabel('Object root').selectOption('');
    // The board search finds what joppa_search finds: a by the word Zebra.
    const found = (await f.mcp('joppa_search', {query: 'Zebra', limit: 50})).items.map(item => item.address.startsWith('test:') ? 'test:' : item.address);
    assert.ok(found.includes('ac:a') && found.every(a => a === 'ac:a' || a === 'test:'), JSON.stringify(found));
    await f.page.getByLabel('Search objects').fill('Zebra');
    await waitClasses(f, {hidden: ['ac:d', 'ac:e', 'domain:pay', 'domain:ship'], lit: flow, faint: []}, 'search Zebra');
    // A click inside a filter lights the flow of that node inside the filter.
    await f.page.locator('.graph-node[data-address="ac:c"]').click();
    await waitClasses(f, {hidden: ['ac:d', 'ac:e', 'domain:pay', 'domain:ship'], lit: ['ac:a', 'ac:b', 'ac:c'], faint: ['test:']}, 'c inside the search');
    await f.page.getByLabel('Search objects').fill('');
    await waitClasses(f, {hidden: [], lit: [], faint: []}, 'cleared search');
    // The view drop-down keeps the node filters of the link.
    await f.page.getByLabel('Object owner').selectOption('maria');
    await f.page.getByLabel('Board view').selectOption('tree');
    await f.page.waitForURL(url => !url.searchParams.has('view'));
    assert.equal(new URL(f.page.url()).searchParams.get('owner'), 'maria');
  } finally { await f.close(); }
});

test('board-graph-ac4 graph-controls: a dot drag pulls its links, other drags pan, the wheel pans and a pinch zooms', async () => {
  const f = await fixture(world.types);
  try {
    await world.build(f);
    await open(f, '');
    const at = address => f.page.evaluate(a => {const d = document.querySelector('.graph-node[data-address="' + a + '"]'), b = d.getBoundingClientRect();
      return {x: b.x + b.width / 2, y: b.y + b.height / 2, cx: Number(d.getAttribute('cx')), cy: Number(d.getAttribute('cy'))};}, address);
    const view = () => f.page.evaluate(() => document.querySelector('.graph-canvas').getAttribute('viewBox').split(' ').map(Number));
    assert.deepEqual(await f.page.evaluate(() => {const s = getComputedStyle(document.querySelector('.graph-canvas'));return [s.userSelect, s.webkitUserSelect];}), ['none', 'none']);
    // A pointer over b marks b, its neighbours and its edges, and names them.
    const b = await at('ac:b');
    await f.page.mouse.move(b.x, b.y);
    const near = () => f.page.evaluate(() => [...document.querySelectorAll('.graph-node.graph-near')].map(d => d.dataset.address).sort());
    await f.page.waitForFunction(() => document.querySelectorAll('.graph-node.graph-near').length > 0);
    assert.deepEqual(await near(), ['ac:a', 'ac:b', 'ac:c', 'domain:pay']);
    assert.equal(await f.page.locator('.graph-edge.graph-near').count(), 3);
    assert.equal(await f.page.locator('.graph-label.graph-near').count(), 4);
    // A drag of d holds it at the pointer, and its root ship follows it.
    const d = await at('ac:d'), ship = await at('domain:ship'), before = await view();
    await f.page.mouse.move(d.x, d.y);await f.page.mouse.down();
    for (let i = 1; i <= 10; i++) await f.page.mouse.move(d.x + 12 * i, d.y + 8 * i);
    await f.page.waitForFunction(([x, y]) => {const s = document.querySelector('.graph-node[data-address="domain:ship"]');return Math.hypot(Number(s.getAttribute('cx')) - x, Number(s.getAttribute('cy')) - y) > 2;}, [ship.cx, ship.cy]);
    const held = await at('ac:d');
    assert.ok(Math.hypot(held.x - (d.x + 120), held.y - (d.y + 80)) < 3, JSON.stringify(held));
    await f.page.mouse.up();
    assert.deepEqual(await view(), before, 'a dot drag does not pan');
    assert.equal(await f.page.evaluate(() => getSelection().toString()), '', 'a drag selects no text');
    assert.equal(await f.page.locator('.graph-node.graph-lit').count(), 0, 'a drag does not pick');
    // A drag that starts on a line pans: the pointer down is cancelled, so no text selection starts.
    const pan = await f.page.evaluate(() => {
      const canvas = document.querySelector('.graph-canvas'), line = canvas.querySelector('.graph-parent'), box = canvas.getBoundingClientRect();
      const send = (target, type, x) => target.dispatchEvent(new PointerEvent(type, {bubbles: true, cancelable: true, button: 0, pointerId: 7, clientX: box.x + x, clientY: box.y + 50}));
      const kept = send(line, 'pointerdown', 100);send(canvas, 'pointermove', 140);send(canvas, 'pointermove', 180);send(canvas, 'pointerup', 180);
      return kept;
    });
    assert.equal(pan, false, 'the pointer down on a line is cancelled');
    const panned = await view();
    assert.ok(panned[0] < before[0] && panned[2] === before[2], JSON.stringify([before, panned]));
    // A wheel or a two-finger swipe pans. A pinch (ctrlKey) zooms.
    const wheel = options => f.page.evaluate(o => {const c = document.querySelector('.graph-canvas'), b = c.getBoundingClientRect();
      c.dispatchEvent(new WheelEvent('wheel', {bubbles: true, cancelable: true, clientX: b.x + b.width / 2, clientY: b.y + b.height / 2, ...o}));}, options);
    await wheel({deltaX: 40, deltaY: 30});
    const swiped = await view();
    assert.ok(swiped[0] > panned[0] && swiped[1] > panned[1] && swiped[2] === panned[2], JSON.stringify([panned, swiped]));
    await wheel({deltaY: -50, ctrlKey: true});
    const zoomed = await view();
    assert.ok(Math.abs(zoomed[2] / swiped[2] - Math.exp(-0.5)) < 0.01, JSON.stringify([swiped, zoomed]));
  } finally { await f.close(); }
});
