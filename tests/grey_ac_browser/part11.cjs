module.exports = function ({fixture}) {
const {test} = require('node:test');
const assert = require('node:assert/strict');

// ac:z292wew4
// test:69w7776z
test('side-preview-wide-and-narrow: dragging the vertical divider resizes and persists preview width, double-click resets it, and an 899 px screen hides the pane', async () => {
  const f = await fixture({'domain:X': {title: 'Subscriptions', body: 'Wide preview body text'}});
  try {
    await f.page.setViewportSize({width: 1728, height: 960});
    const res = await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    assert.match(res.headers()['cache-control'] || '', /no-store|no-cache/);
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    const pane = f.page.locator('#preview');
    const resizer = f.page.locator('#preview-resizer');
    await X.waitFor();
    assert.equal(await resizer.isVisible(), false);
    await X.locator(':scope > summary').click({position: {x: 4, y: 4}});
    await pane.locator('.preview-title').filter({hasText: 'Subscriptions'}).waitFor();
    assert.equal(await resizer.isVisible(), true);
    const before = await pane.locator('.object-content').boundingBox();
    const head = await pane.locator('.preview-head').boundingBox();
    assert.ok(before.width >= 950 && Math.abs(before.x - head.x) < 1, JSON.stringify({before, head}));

    const handle = await resizer.boundingBox();
    const cx = handle.x + handle.width / 2, cy = handle.y + 80;
    await f.page.mouse.move(cx, cy);
    await f.page.mouse.down();
    await f.page.mouse.move(cx - 240, cy, {steps: 5});
    await f.page.mouse.up();
    const wider = await pane.locator('.object-content').boundingBox();
    assert.ok(wider.width >= before.width + 200, JSON.stringify({before: before.width, wider: wider.width}));

    await f.page.reload();
    await X.waitFor();
    await X.locator(':scope > summary').click({position: {x: 4, y: 4}});
    await pane.locator('.preview-title').filter({hasText: 'Subscriptions'}).waitFor();
    const persisted = await pane.locator('.object-content').boundingBox();
    assert.ok(Math.abs(persisted.width - wider.width) < 4, JSON.stringify({wider: wider.width, persisted: persisted.width}));

    await resizer.dblclick({position: {x: 12, y: 80}});
    const reset = await pane.locator('.object-content').boundingBox();
    assert.ok(Math.abs(reset.width - before.width) < 4, JSON.stringify({before: before.width, reset: reset.width}));

    await f.page.setViewportSize({width: 899, height: 900});
    await f.page.waitForFunction(() => document.getElementById('preview').hidden === true);
    assert.equal(await pane.isVisible(), false);
    assert.equal(await resizer.isVisible(), false);
    assert.equal(await X.locator(':scope > .object-body .object-content').isVisible(), true);
  } finally { await f.close(); }
});
};
