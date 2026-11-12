// Chromium checks of the tone blue (requirement:vqlbur46): the status circle of a criterion
// without results, the card of a node that is bound to another workspace, and the status
// filter. Each test starts its own browser_server fixture with the default status list.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture, testType} = require('./board_fixture.cjs');

const LIGHT = 'rgb(111, 168, 220)', DARK = 'rgb(139, 187, 232)';
const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []},
  ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['requirement']},
  test: testType(['ac']),
  component: {title: 'Components', role: 'document', status: 'none', parents: ['requirement']},
};

// The tone and the background of each status circle on the page: no circle shows grey.
const circles = page => page.locator('.status-circle').evaluateAll(list => list.map(node => ({
  tone: node.dataset.tone || node.className, background: getComputedStyle(node).backgroundColor})));

// ac:idxkripi
// test:jw0dsiit
test('idxkripi blue-circle: a criterion without results has a blue circle in both themes, and the card of V says not all criteria green', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Requirement R');
    await f.save('ac:a', 'Criterion A', 'requirement:r');
    await f.addTest('ac:a', 'tests/blue.rs > t');
    // The signed-in member alex creates workspace V; the node component:v stands for it.
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const v = await f.page.evaluate(async () => {
      const response = await fetch('/api/workspaces', {method: 'POST', headers: {'Content-Type': 'application/json'}, body: JSON.stringify({title: 'Bound'})});
      return (await response.json()).id;
    });
    f.track(v);
    await f.command({op: 'save_object', kind: 'component', id: 'v', revision: 0, parent: 'requirement:r',
      data: {title: 'Bound workspace', owner: 'alex', workspace: v}, reason: 'Bind V'}, f.live.alex);
    const read = await f.api('/objects?address=ac%3Aa');
    assert.equal(read.signal.tone, 'blue');
    await f.page.reload();
    const r = f.page.locator('#tree .common-object[data-object-address="requirement:r"]');
    await r.locator(':scope > summary').click();
    const dot = r.locator('.common-object[data-object-address="ac:a"] > summary .status-circle').first();
    await dot.waitFor();
    await f.page.waitForFunction(() => [...document.querySelectorAll('#tree .status-circle[data-tone]')].every(node => getComputedStyle(node).backgroundColor !== ''));
    const colour = () => dot.evaluate(node => [node.dataset.tone, getComputedStyle(node).backgroundColor]);
    assert.deepEqual(await colour(), ['blue', LIGHT]);
    assert.ok((await circles(f.page)).every(c => !String(c.tone).includes('grey')), JSON.stringify(await circles(f.page)));
    await f.page.locator('#theme').click();
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.deepEqual(await colour(), ['blue', DARK]);
    // The card of the node bound to V names the blue status of V.
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&object=component%3Av');
    const section = f.page.locator('.workspace-links').first();
    await section.waitFor({timeout: 15000});
    const shown = await section.evaluate(node => node.textContent);
    assert.ok(shown.includes('not all criteria green'), shown);
    assert.ok(!/grey|undefined/i.test(shown), shown);
  } finally { await f.close(); }
});

// ac:i5vjohp0
// test:xcayjtv2
test('i5vjohp0 blue-filter: the status filter gives Awaits checks with a blue circle and no grey option', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Requirement R');
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const select = f.page.getByLabel('Object status');
    await select.waitFor();
    const options = () => select.locator('option').evaluateAll(list => list.map(option => ({value: option.value, label: option.textContent.trim(),
      dot: option.querySelector('.status-circle') && getComputedStyle(option.querySelector('.status-circle')).backgroundColor})));
    const light = await options();
    assert.ok(!light.some(option => option.value === 'grey' || /grey/i.test(option.label)), JSON.stringify(light));
    assert.deepEqual(light.find(option => option.value === 'blue'), {value: 'blue', label: 'Awaits checks', dot: LIGHT});
    // board.css sets --dot-blue from the light and the dark colour of the tone.
    const variable = () => f.page.evaluate(() => {
      const probe = document.createElement('i');
      probe.style.color = 'var(--dot-blue)';
      document.body.append(probe);
      const colour = getComputedStyle(probe).color;
      probe.remove();
      return colour;
    });
    assert.equal(await variable(), LIGHT);
    await f.page.locator('#theme').click();
    assert.equal(await f.page.evaluate(() => document.documentElement.dataset.theme), 'dark');
    assert.equal((await options()).find(option => option.value === 'blue').dot, DARK);
    assert.equal(await variable(), DARK);
    // A pick of the option keeps the value blue in the link.
    await select.click();
    await f.page.locator('.status-option[data-tone="blue"]').click();
    assert.equal(await select.inputValue(), 'blue');
    await f.page.waitForFunction(() => new URL(location.href).searchParams.get('status') === 'blue');
  } finally { await f.close(); }
});
