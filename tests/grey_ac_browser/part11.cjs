module.exports = function ({fixture}) {
const {test} = require('node:test');
const assert = require('node:assert/strict');

// ac:z292wew4
// test:69w7776z
test('side-preview-wide-and-narrow: a 1728 px screen expands preview content above 765 px and an 899 px screen hides the pane', async () => {
  const f = await fixture({'domain:X': {title: 'Subscriptions', body: 'Wide preview body text'}});
  try {
    await f.page.setViewportSize({width: 1728, height: 960});
    await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    const pane = f.page.locator('#preview');
    await X.waitFor();
    await X.locator(':scope > summary').click({position: {x: 4, y: 4}});
    await pane.locator('.preview-title').filter({hasText: 'Subscriptions'}).waitFor();
    const wideContent = await pane.locator('.object-content').boundingBox();
    const wideHead = await pane.locator('.preview-head').boundingBox();
    const bodyMax = await f.page.evaluate(() => getComputedStyle(document.body).maxWidth);
    assert.equal(bodyMax, '1480px');
    assert.ok(wideContent.width >= 765 && Math.abs(wideContent.x - wideHead.x) < 1, JSON.stringify({wideContent, wideHead}));

    await f.page.setViewportSize({width: 899, height: 900});
    await f.page.waitForFunction(() => document.getElementById('preview').hidden === true);
    assert.equal(await pane.isVisible(), false);
    assert.equal(await X.locator(':scope > .object-body .object-content').isVisible(), true);
  } finally { await f.close(); }
});
};
