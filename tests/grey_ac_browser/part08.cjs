// Part 8 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
/* eslint-disable no-unused-vars */
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {writeFileSync} = require('node:fs');
const {once} = require('node:events');
const http = require('node:http');
const path = require('node:path');

module.exports = shared => {
  const {server, fixture, criterionRecords, types, root, titlePoint, markdownTypes, markdownBody} = shared;

test('compact-open-object-ac1: the object row holds the control for its fields', async () => {
  const title = 'Release plan outline';
  assert.equal(title.length, 20);
  const f = await fixture({'domain:X': {title}, 'capability:a': {title: 'First child', parent: 'domain:X'},
    'capability:b': {title: 'Second child', parent: 'domain:X'}});
  const consoleErrors = [];
  f.page.on('console', message => { if (message.type() === 'error') consoleErrors.push(message.text()); });
  try {
    await f.command({op: 'save_object', kind: 'domain', id: 'X', revision: 1, parent: null,
      data: {title, owner: 'owner', body: 'Current source', scope: 'Field scope value'}, reason: 'Add a document field'});
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    const row = X.locator(':scope > summary');
    const control = row.getByRole('button', {name: 'Properties', exact: true});
    const fields = X.locator(':scope > .object-body > .object-secondary > .object-details');
    const children = X.locator(':scope > .object-body > .object-children > .common-object');
    for (const viewport of [null, {width: 390, height: 844}]) {
      if (viewport) await f.page.setViewportSize(viewport);
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      await row.waitFor();
      if (await X.getAttribute('open') === null) await row.click({position: {x: 4, y: 4}});
      await children.nth(1).waitFor();
      assert.equal(await X.getAttribute('open'), '');
      assert.equal(await control.count(), 1);
      assert.equal(await control.evaluate(node => node.tagName + ' ' + node.type), 'BUTTON button');
      // The control is an icon with the tooltip Properties.
      assert.deepEqual(await control.evaluate(node => [node.title, node.querySelectorAll('svg').length, node.textContent.trim()]), ['Properties', 1, '']);
      // Copy and Revisions stay the last two controls of the row.
      assert.deepEqual((await row.locator('button').evaluateAll(nodes => nodes.map(node => node.className))).slice(-3),
        ['fields-button', 'copy', 'history-button']);
      assert.equal(await X.locator(':scope > .object-body .object-details > summary').count(), 0);
      assert.equal(await fields.evaluate(node => node.tagName), 'DIV');
      assert.deepEqual((await X.locator(':scope > .object-body > .object-secondary > details > summary').allTextContents())
        .filter(text => /details|fields/i.test(text)), []);
      // The control adds no line to the row.
      const shown = (await row.boundingBox()).height;
      // A role query skips a hidden button, so the class selects it while it is hidden.
      const plain = row.locator('.fields-button');
      await plain.evaluate(node => { node.style.display = 'none'; });
      const without = (await row.boundingBox()).height;
      await plain.evaluate(node => { node.style.display = ''; });
      assert.ok(Math.abs(shown - without) <= 1, JSON.stringify({viewport, shown, without}));
      assert.equal(await fields.isVisible(), false);
      assert.equal(await control.getAttribute('aria-expanded'), 'false');
      for (const activate of [() => control.click(), () => control.focus().then(() => f.page.keyboard.press('Enter')),
        () => control.focus().then(() => f.page.keyboard.press('Space'))]) {
        const before = await fields.isVisible();
        await activate();
        assert.equal(await X.getAttribute('open'), '');
        assert.equal(await children.count(), 2);
        assert.equal(await children.nth(0).isVisible(), true);
        assert.equal(await children.nth(1).isVisible(), true);
        assert.equal(await fields.isVisible(), !before);
        assert.equal(await control.getAttribute('aria-expanded'), String(!before));
      }
      assert.equal(await fields.isVisible(), true);
      assert.match(await fields.innerText(), /Revision 2/);
      assert.match(await fields.innerText(), /Field scope value/);
    }
    // The revision dialog has no row control and always shows the fields.
    await row.locator('.history-button').click();
    const dialog = f.page.locator('dialog.notes-dialog');
    const dialogFields = dialog.locator('.object-details');
    await dialogFields.waitFor();
    assert.equal(await X.getAttribute('open'), '');
    assert.equal(await dialog.locator('.object-details > summary').count(), 0);
    assert.equal(await dialogFields.isVisible(), true);
    assert.match(await dialogFields.innerText(), /Revision 2/);
    assert.match(await dialogFields.innerText(), /Field scope value/);
    await dialog.locator('select').selectOption('1');
    await f.page.waitForFunction(() => /Revision 1/.test(document.querySelector('dialog.notes-dialog .object-details')?.innerText));
    assert.equal(await dialogFields.isVisible(), true);
    await dialog.locator('.history-close').click();
    assert.deepEqual(consoleErrors, []);
  } finally { await f.close(); }
});

test('compact-open-object-ac2: an open object shows no side line', async () => {
  const f = await fixture({'domain:X': {title: 'Parent X'}, 'capability:Y': {title: 'Child Y', parent: 'domain:X'},
    'requirement:Z': {title: 'Grandchild Z', parent: 'capability:Y'}});
  const row = card => card.locator(':scope > summary');
  const ready = card => card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
  // Every vertical line source that a stylesheet could draw on the left of an open block.
  const lines = card => card.evaluate(node => {
    const found = [];
    const targets = [node, node.querySelector(':scope > summary'), node.querySelector(':scope > .object-body'),
      node.querySelector(':scope > .object-body > .object-children')].filter(Boolean);
    for (const element of targets) {
      const style = getComputedStyle(element);
      if (style.borderLeftStyle !== 'none' && parseFloat(style.borderLeftWidth) > 0 && style.borderLeftColor !== 'rgba(0, 0, 0, 0)')
        found.push(element.className + ' border-left');
      if (style.boxShadow !== 'none') found.push(element.className + ' box-shadow');
      if (style.backgroundImage !== 'none') found.push(element.className + ' background-image');
      for (const pseudo of ['::before', '::after']) {
        const p = getComputedStyle(element, pseudo);
        if (p.content !== 'none' && p.content !== 'normal') found.push(element.className + pseudo);
      }
    }
    const box = node.getBoundingClientRect();
    for (const element of node.querySelectorAll('*')) {
      const r = element.getBoundingClientRect(), style = getComputedStyle(element);
      const painted = style.backgroundColor !== 'rgba(0, 0, 0, 0)' || style.backgroundImage !== 'none';
      if (painted && r.width > 0 && r.width < 4 && r.height > box.height / 2) found.push(element.tagName + ' thin line');
    }
    return found;
  });
  try {
    for (const [width, height] of [[1280, 720], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
      await X.waitFor();
      if (await X.getAttribute('open') === null) await row(X).click({position: {x: 4, y: 4}});
      await ready(X);
      const Y = X.locator(':scope > .object-body > .object-children > .common-object[data-object-address="capability:Y"]');
      await Y.waitFor();
      if (await Y.getAttribute('open') === null) await row(Y).click({position: {x: 4, y: 4}});
      await ready(Y);
      for (const scheme of ['light', 'dark']) {
        await f.page.emulateMedia({colorScheme: scheme});
        assert.deepEqual(await lines(X), [], 'X has no side line at ' + width + ' px, ' + scheme);
        assert.deepEqual(await lines(Y), [], 'Y has no side line at ' + width + ' px, ' + scheme);
        const xLeft = (await row(X).boundingBox()).x, yLeft = (await row(Y).boundingBox()).x;
        assert.ok(yLeft > xLeft, 'the indent shows that Y is inside X: ' + yLeft + ' > ' + xLeft);
      }
    }
  } finally { await f.close(); }
});

test('markdown-text-ac2: active HTML in Markdown stays inert', async () => {
  const hostile = ['<script>window.pwned=1</script>', '<img src=x onerror="window.pwned=2">', '<svg onload="window.pwned=4"></svg>',
    '[run](javascript:window.pwned=3)', '[data](data:text/html,<b>x</b>)', '<iframe src="https://example.com"></iframe>',
    '<style>body{display:none}</style>', '[ext](https://example.com/page)'].join('\n\n');
  const f = await fixture({'domain:H': {title: 'Hostile', body: hostile}});
  try {
    const H = await f.open('domain:H');
    const text = H.locator(':scope > .object-body .object-content');
    for (const link of await text.locator('a').all()) await link.click({modifiers: ['Meta']}).catch(() => {});
    assert.equal(await f.page.evaluate(() => window.pwned), undefined);
    assert.equal(await text.locator('script, iframe, style, svg, [onerror], [onload]').count(), 0);
    const hrefs = await text.locator('a').evaluateAll(nodes => nodes.map(node => node.getAttribute('href')));
    assert.ok(hrefs.every(href => href === null || !/^(javascript|data):/i.test(href)), JSON.stringify(hrefs));
    const external = text.locator('a[href="https://example.com/page"]');
    assert.equal(await external.getAttribute('rel'), 'noopener noreferrer');
  } finally { await f.close(); }
});

test('markdown-text-ac3: the renderer loads once and long content scrolls in its own box', async () => {
  const records = {};
  for (let i = 0; i < 20; i++) records['domain:D' + i] = {title: 'Domain ' + i, body: markdownBody + '\n\n| ' + 'wide '.repeat(60) + '|\n|---|\n| x |'};
  const f = await fixture(records, types, {viewport: {width: 390, height: 844}});
  const scripts = [];
  f.page.on('request', request => { if (new URL(request.url()).pathname === '/vendor/markdown.js') scripts.push(request.url()); });
  try {
    for (const scheme of ['light', 'dark']) {
      scripts.length = 0;
      await f.page.emulateMedia({colorScheme: scheme});
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      for (let i = 0; i < 20; i++) {
        const card = f.page.locator('#collection-domain .common-object[data-object-address="domain:D' + i + '"]');
        await card.waitFor();
        if (await card.getAttribute('open') === null) await card.locator(':scope > summary').click({position: {x: 4, y: 4}});
        await card.locator(':scope > .object-body .object-content h2').waitFor();
      }
      assert.equal(scripts.length, 1, 'one renderer request per page load: ' + scripts.length);
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'the page does not scroll sideways');
      const table = f.page.locator('.object-content table').first();
      assert.equal(await table.evaluate(node => getComputedStyle(node).overflowX), 'auto');
    }
  } finally { await f.close(); }
});

test('compact-open-object-ac4: copy and revisions are icons at the right end of the row', async () => {
  const f = await fixture({'domain:X': {title: 'Release plan outline'}});
  try {
    for (const [width, height] of [[1280, 720], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      for (const scheme of ['light', 'dark']) {
        await f.page.emulateMedia({colorScheme: scheme});
        await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
        const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
        await X.waitFor();
        const row = X.locator(':scope > summary');
        const controls = await row.locator('button').evaluateAll(nodes => nodes.map(node => ({cls: node.className, text: node.innerText.trim(),
          name: node.getAttribute('aria-label'), title: node.getAttribute('title')})));
        const [copy, history] = controls.slice(-2);
        assert.match(copy.cls, /\bcopy\b/); assert.match(history.cls, /\bhistory-button\b/);
        assert.deepEqual([copy.text, history.text], ['', '']);
        assert.deepEqual([copy.name, copy.title], ['Copy ID for agent', 'Copy ID for agent']);
        assert.deepEqual([history.name, history.title], ['Revisions', 'Revisions']);
        const rowBox = await row.boundingBox(), historyBox = await row.locator('.history-button').boundingBox();
        assert.ok(rowBox.x + rowBox.width - (historyBox.x + historyBox.width) <= 8, JSON.stringify({rowBox, historyBox}));
        const lineHeight = await row.evaluate(node => { const before = node.getBoundingClientRect().height;
          const actions = node.querySelector('.item-actions'); actions.style.display = 'none';
          const without = node.getBoundingClientRect().height; actions.style.display = ''; return [before, without]; });
        assert.ok(Math.abs(lineHeight[0] - lineHeight[1]) <= 1, 'the controls add no line: ' + lineHeight);
      }
    }
    const row = f.page.locator('#tree .common-object[data-object-address="domain:X"] > summary');
    await row.locator('.copy').click();
    const itemId = (await f.api('/objects?address=domain:X')).item_id;
    assert.ok((await f.page.evaluate(() => window.copiedIds)).includes(itemId));
    await row.locator('.copy').focus(); await f.page.keyboard.press('Enter');
    assert.ok((await f.page.evaluate(() => window.copiedIds)).filter(id => id === itemId).length >= 2);
    await row.locator('.history-button').click();
    await f.page.locator('dialog.notes-dialog').waitFor();
    await f.page.locator('dialog.notes-dialog .history-close').click();
    await row.locator('.history-button').focus(); await f.page.keyboard.press('Enter');
    await f.page.locator('dialog.notes-dialog').waitFor();
  } finally { await f.close(); }
});
};
