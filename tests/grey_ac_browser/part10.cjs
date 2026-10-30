// Part 10 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

test('no-dashed-focus-ac1: a mouse click leaves no focus mark on the history Close button or a board dropdown', async (t) => {
  const f = await fixture({'domain:X': {title: 'Domain X'}});
  const themes = {light: 'rgb(255, 252, 245)', dark: 'rgb(25, 28, 32)'};
  const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
  const dialog = f.page.locator('dialog.notes-dialog');
  const status = f.page.locator('#tree .filter-fields select[aria-label="Object status"]');
  const install = () => {
    const read = node => { const style = getComputedStyle(node); return {outlineStyle: style.outlineStyle, textDecorationStyle: style.textDecorationStyle}; };
    window.ac1 = {read, closes: []};
    // Close removes the dialog, so a capturing listener reads the button before its own click handler runs.
    document.addEventListener('click', event => {
      const close = event.target.closest?.('dialog.notes-dialog .history-close');
      if (close) ac1.closes.push(read(close));
    }, true);
  };
  const failures = [];
  const check = (where, measured) => {
    t.diagnostic(where + ': ' + JSON.stringify(measured));
    if (!measured?.outlineStyle || !measured.textDecorationStyle) failures.push(where + ': empty style ' + JSON.stringify(measured));
    else if (measured.outlineStyle !== 'none' || measured.textDecorationStyle === 'dashed') failures.push(where + ': focus mark ' + JSON.stringify(measured));
  };
  try {
    for (const [scheme, background] of Object.entries(themes)) {
      const response = await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      assert.equal(response.status(), 200, scheme);
      await X.waitFor();
      await Promise.all(f.requests.map(entry => entry.body));
      await f.page.evaluate(install);
      await f.page.evaluate(scheme => { document.documentElement.dataset.theme = scheme; }, scheme);
      assert.equal(await f.page.evaluate(() => getComputedStyle(document.body).backgroundColor), background, 'the ' + scheme + ' theme is active');
      // The history dialog opens and closes with mouse clicks.
      await X.locator(':scope > summary .history-button').click();
      await dialog.waitFor();
      await dialog.locator('.history-close').click();
      await dialog.waitFor({state: 'detached'});
      const closes = await f.page.evaluate(() => ac1.closes);
      assert.equal(closes.length, 1, 'the click listener reads the Close button once, ' + scheme);
      check(scheme + ', history Close click', closes[0]);
      // The status dropdown opens, takes an option and loses focus with mouse clicks.
      assert.equal(await status.inputValue(), '', scheme);
      await status.click();
      assert.equal(await status.evaluate(node => node.matches(':open')), true, 'the status list opens, ' + scheme);
      check(scheme + ', dropdown open click', await status.evaluate(node => ac1.read(node)));
      const option = await status.locator('option[value="green"]').boundingBox();
      assert.ok(option?.width && option?.height, 'the mouse reaches a status option, ' + scheme + ': ' + JSON.stringify(option));
      await f.page.mouse.click(option.x + option.width / 2, option.y + option.height / 2);
      assert.equal(await status.inputValue(), 'green', 'the option click changes the status value, ' + scheme);
      assert.equal(await status.evaluate(node => node.matches(':open')), false, 'the status list closes, ' + scheme);
      check(scheme + ', dropdown option click', await status.evaluate(node => ac1.read(node)));
      await f.page.waitForFunction(() => !document.querySelector('#tree .object-filter-results .object-loading'));
      await Promise.all(f.requests.map(entry => entry.body));
      await f.page.locator('.app-header .brand').click();
      check(scheme + ', outside click', await status.evaluate(node => ac1.read(node)));
    }
    assert.deepEqual(failures, [], 'a mouse click leaves no focus mark');
  } finally { await f.close(); }
});

// workspace-timeline-ac2
test('workspace-timeline-ac2: alex reads the newest change first and opens R at the revision of its move', async () => {
  const configured = {...types, signal: {title: 'Signals', role: 'document', status:'none', root: true, parents: [], inbox: true, badge: 'SIG'}};
  const f = await fixture({
    'domain:d': {title: 'Domain'},
    'capability:c1': {title: 'First capability', parent: 'domain:d'},
    'capability:c2': {title: 'Second capability', parent: 'domain:d'},
    'requirement:q': {title: 'Requirement of A', parent: 'capability:c1'},
    'ac:A': {title: 'Criterion A', parent: 'requirement:q'},
    'signal:n': {title: 'Signal N'},
  }, configured);
  try {
    // The five changes of ac1: bot saves R, P moves R, archives N, marks A ready, bot records test results.
    const criterion = await f.api('/objects?address=ac:A');
    await f.command({op: 'confirm', req: criterion.process.req, revision: criterion.process.revision}, true);
    await f.save('requirement:R', {title: 'Requirement R', parent: 'capability:c1'});
    await f.command({op: 'save_object', kind: 'requirement', id: 'R', revision: 1, parent: 'capability:c2',
      data: {title: 'Requirement R', owner: 'owner', body: 'Current source'}, reason: 'Move R to the second capability'}, true);
    await f.command({op: 'archive_object', target: 'signal:n', revision: 1, reason: 'Signal N is handled'}, true);
    const ready = await f.api('/objects?address=ac:A');
    await f.command({op: 'assess', req: ready.process.req, revision: ready.process.revision, ac: ready.process.criterion,
      assessment: {ready: true, reason: 'The scope of A is known'}}, true);
    await f.record('0123abc', [['one']]);
    const newest = (await f.api('/timeline?order=newest&limit=5')).items;
    assert.deepEqual(newest.map(item => item.change), ['results', 'readiness', 'archive', 'move', 'save']);
    const move = newest[3];
    assert.deepEqual([move.address, move.revision, newest[4].address, newest[4].revision], ['requirement:R', 2, 'requirement:R', 1]);
    for (const viewport of [{width: 1280, height: 900}, {width: 390, height: 844}]) {
      await f.page.setViewportSize(viewport);
      // alex opens the timeline page from the header link.
      await f.open('domain:d');
      const link = f.page.locator('#timeline-link');
      assert.equal(new URL(await link.getAttribute('href'), f.live.root).search, '?workspace=' + f.live.ws + '&view=timeline');
      await link.click();
      const rows = f.page.locator('.timeline-list > .timeline-entry');
      await rows.nth(4).waitFor();
      assert.equal(await f.page.locator('.timeline-title').innerText(), 'Timeline');
      for (const [index, item] of newest.entries()) {
        const row = rows.nth(index);
        assert.equal(await row.getAttribute('data-position'), String(item.position), viewport.width + ': row ' + (index + 1));
        const expected = await f.page.evaluate(async item => {
          const {eventDate, personLabel} = await import('board/format.js');
          return [eventDate(item.at), personLabel(item.actor), item.title, item.change];
        }, item);
        assert.deepEqual(await row.locator('.timeline-time, .timeline-actor, .timeline-object, .timeline-change').allInnerTexts(),
          expected, viewport.width + ': row ' + (index + 1));
      }
      assert.deepEqual(await rows.locator('.timeline-object').evaluateAll(nodes => nodes.slice(0, 5).map(node => node.textContent)),
        ['Test results 0123abc', 'Criterion A', 'Signal N', 'Requirement R', 'Requirement R']);
      for (const index of [0, 4]) {
        const box = await rows.nth(index).boundingBox();
        assert.ok(box.x >= 0 && box.x + box.width <= viewport.width, viewport.width + ': ' + JSON.stringify(box));
      }
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), viewport.width + ': no horizontal scroll');
      // The move row opens R at the revision of the move.
      await rows.nth(3).locator('a.timeline-row').click();
      await f.page.waitForURL(url => url.searchParams.get('object') === 'requirement:R' && url.searchParams.get('revision') === String(move.revision));
      const card = f.page.locator('.object-page > .common-object[data-object-address="requirement:R"]');
      await card.waitFor();
      assert.ok((await f.page.locator('.object-page-navigation > .hint').innerText()).startsWith('Revision ' + move.revision), viewport.width);
      await Promise.all(f.requests.map(entry => entry.body));
    }
  } finally { await f.close(); }
});

test('compact-open-object-ac7: the Properties icon opens a closed object with its properties first', async () => {
  const f = await fixture({'domain:X': {title: 'Release plan outline'}, 'capability:a': {title: 'First child', parent: 'domain:X'},
    'capability:b': {title: 'Second child', parent: 'domain:X'}});
  try {
    await f.command({op: 'save_object', kind: 'domain', id: 'X', revision: 1, parent: null,
      data: {title: 'Release plan outline', owner: 'owner', body: 'Current source', scope: 'Field scope value'}, reason: 'Add a document field'});
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    const row = X.locator(':scope > summary');
    const control = row.getByRole('button', {name: 'Properties', exact: true});
    const fields = X.locator(':scope > .object-body > .object-details');
    const children = X.locator(':scope > .object-body > .object-children > .common-object');
    // A row click closes X; the close can animate, so wait until X is closed.
    const closed = () => f.page.waitForFunction(() => !document.querySelector('#tree .common-object[data-object-address="domain:X"]').open);
    // The properties are the first block of the body, above the children.
    const shownFirst = async () => {
      await children.nth(1).waitFor();
      assert.equal(await X.getAttribute('open'), '');
      assert.equal(await fields.isVisible(), true);
      assert.equal(await control.getAttribute('aria-expanded'), 'true');
      assert.equal(await X.locator(':scope > .object-body').evaluate(body => body.firstElementChild.className), 'object-details');
      assert.ok((await fields.boundingBox()).y < (await children.nth(0).boundingBox()).y);
      assert.match(await fields.innerText(), /Field scope value/);
    };
    for (const viewport of [null, {width: 390, height: 844}]) {
      if (viewport) await f.page.setViewportSize(viewport);
      await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
      await row.waitFor();
      // A closed object that is not loaded yet opens with its properties shown.
      if (await X.getAttribute('open') !== null) await row.click({position: {x: 4, y: 4}});
      await closed();
      await control.click();
      await shownFirst();
      // A loaded object with hidden properties: close it, then the icon opens it with them shown.
      await control.click();
      assert.equal(await fields.isVisible(), false);
      await row.click({position: {x: 4, y: 4}});
      await closed();
      await control.click();
      await shownFirst();
    }
  } finally { await f.close(); }
});
};
