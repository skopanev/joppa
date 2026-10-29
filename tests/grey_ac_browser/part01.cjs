// Part 1 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

test('suggested-method: the guide reads custom workspace levels and questions', async () => {
  const configured = {
    ledger: {title: 'Ledgers', role: 'document', status:'none', root: true, parents: [], question: 'Which records belong together?'},
    entry: {title: 'Entries', role: 'document', status:'none', parents: ['ledger', 'entry'], question: 'Which fact does this entry preserve?'},
    observation: {title: 'Observations', role: 'document', status:'none', root: true, parents: ['*'], question: 'Which source provided this observation?'},
  };
  const f = await fixture({'ledger:one': {title: 'Product ledger'}, 'entry:one': {title: 'Recorded fact', parent: 'ledger:one'}}, configured);
  try {
    await f.open('ledger:one');
    const link = f.page.locator('#suggested-method-link');
    assert.equal(await link.innerText(), 'How it works ↗');
    const url = new URL(await link.getAttribute('href'), f.live.root);
    assert.equal(url.searchParams.get('workspace'), f.live.ws);
    assert.equal((await f.page.goto(url.href)).status(), 200);
    await f.page.locator('#workspace-levels #level-entry').waitFor();
    assert.equal(await f.page.locator('h1').innerText(), 'How it works');
    assert.equal(await f.page.title(), 'How it works · joppa');
    assert.ok((await f.page.locator('h1 + p').innerText()).startsWith('This is a suggested method.'));
    assert.deepEqual(await f.page.locator('.brand-word').evaluateAll(words => words.map(word => {
      const style = getComputedStyle(word);
      return {text: word.textContent, font: style.fontFamily, weight: style.fontWeight,
        size: Math.round(parseFloat(style.fontSize) / parseFloat(getComputedStyle(word.parentElement).fontSize) * 100) / 100,
        height: parseFloat(style.lineHeight) / parseFloat(style.fontSize), alignment: style.verticalAlign,
        color: style.color, dot: getComputedStyle(word.firstElementChild).color};
    })), Array.from({length: 2}, () => ({text: 'joppa.', font: 'Verdana, sans-serif', weight: '400',
      size: 1.15, height: 1, alignment: 'baseline', color: 'rgb(34, 34, 34)', dot: 'rgb(75, 120, 88)'})));
    assert.equal(await f.page.locator('#default-tree').isVisible(), false);
    assert.deepEqual(await f.page.locator('#workspace-levels h3').allTextContents(), ['Ledgers', 'Entries', 'Observations']);
    for (const [kind, type] of Object.entries(configured)) {
      const level = f.page.locator('#level-' + kind + ' > .level-card');
      assert.ok((await level.innerText()).includes(type.question));
    }
    assert.equal(await f.page.locator('#level-ledger > ol > #level-entry').count(), 1);
    assert.equal(await f.page.locator('#level-entry a[href="#level-entry"]').innerText(), 'Entries');
    assert.ok((await f.page.locator('#level-observation').innerText()).includes('Any level'));
    assert.ok(f.requests.some(request => new URL(request.url).pathname === '/api/workspaces/' + f.live.ws + '/tree'));
    f.requests.length = 0;
    await f.page.goto(f.live.root + '/guide');
    await f.page.waitForLoadState('networkidle');
    assert.equal(await f.page.locator('#default-tree').isVisible(), true);
    assert.equal(await f.page.locator('#workspace-levels').isVisible(), false);
    assert.equal(await f.page.locator('#level-intro').innerText(),
      'Suggested default tree: Domain → Capability → REQ → AC. Each workspace schema sets its own levels and the question each level answers.');
    assert.equal(f.requests.length, 0);
    const response = await f.page.goto(f.live.root + '/guide?workspace=missing');
    assert.equal(response.status(), 200);
    await f.page.waitForFunction(() => document.getElementById('level-intro').textContent === 'Workspace not found');
    assert.equal(await f.page.locator('#default-tree').isVisible(), false);
    assert.equal(await f.page.locator('#workspace-levels .level-card').count(), 0);
  } finally { await f.close(); }
});

test('guide-teaches-reports-ac1: the guide teaches test lists and records, not legacy Checks', async () => {
  const f = await fixture({});
  try {
    assert.equal((await f.page.goto(f.live.root + '/guide')).status(), 200);
    const text = await f.page.locator('body').innerText();
    for (const legacy of [/Expected and observed/i, /runner (records|seals)/i, /check list/i, /manual check/i, /start at n\/a/i])
      assert.doesNotMatch(text, legacy);
    assert.match(text, /every test in its list passes in its latest record/);
    assert.match(text, /A failed test makes the criterion red/);
    assert.doesNotMatch(text, /full report|JUnit|pre-push/i);
    assert.match(text, /status rule: tests, children, links or none/);
  } finally { await f.close(); }
});

// guide-mcp-url
test('guide-mcp-url: the guide shows the MCP endpoint of this host and copies it with the board copy icon', async () => {
  const f = await fixture({});
  try {
    const origin = new URL(f.live.root).origin;
    assert.equal((await f.page.goto(f.live.root + '/guide')).status(), 200);
    const section = f.page.locator('#agent-work');
    assert.equal(await section.locator('#mcp-url').innerText(), origin + '/mcp');
    // One line: the URL and the copy icon of the board, with no text and no frame.
    assert.doesNotMatch(await section.innerText(), /Streamable HTTP|Bearer/);
    const button = section.locator('#mcp-copy');
    assert.equal(await button.innerText(), '');
    assert.equal(await button.locator('svg.copy-symbol').isVisible(), true);
    assert.equal(await button.locator('svg.copy-check').isVisible(), false);
    assert.deepEqual(await button.evaluate(e => { const s = getComputedStyle(e); return [s.borderTopWidth, s.backgroundColor]; }), ['0px', 'rgba(0, 0, 0, 0)']);
    await button.click();
    // The fixture clipboard keeps each copied text in window.copiedIds.
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), [origin + '/mcp']);
    assert.equal(await button.locator('svg.copy-check').isVisible(), true);
    assert.equal(await button.locator('svg.copy-symbol').isVisible(), false);
    await f.page.waitForTimeout(3200);
    assert.equal(await button.locator('svg.copy-symbol').isVisible(), true);
  } finally { await f.close(); }
});

test('type-badge: a click on a type badge toggles its row and opens no popover', async () => {
  const question = 'Which observation does this record preserve?';
  const configured = {record: {title: 'Records', role: 'document', status:'none', root: true, parents: ['record'], badge: 'REC', question}};
  const f = await fixture({'record:root': {title: 'Source'}, 'record:child': {title: 'Detail', parent: 'record:root'},
    'record:leaf': {title: 'Leaf', parent: 'record:child'}}, configured);
  try {
    const card = await f.open('record:root');
    const child = card.locator('.common-object[data-object-address="record:child"]');
    await child.waitFor();
    const badge = child.locator(':scope > summary > .type-badge');
    assert.equal(await badge.evaluate(node => node.tagName), 'ABBR');
    assert.equal(await child.locator(':scope > summary button.type-badge').count(), 0);
    assert.equal(await badge.getAttribute('title'), 'Records\n' + question);
    await badge.hover();
    assert.equal(await f.page.locator('.status-hint').innerText(), 'Records\n' + question);
    assert.equal(await child.getAttribute('open'), null);
    await badge.click();
    await child.locator('.common-object[data-object-address="record:leaf"]').waitFor();
    assert.equal(await child.getAttribute('open'), '');
    assert.equal(await f.page.locator(':popover-open').count(), 0);
    await badge.click();
    await f.page.waitForFunction(() => !document.querySelector('.common-object[data-object-address="record:child"]').open);
    assert.equal(await f.page.locator(':popover-open').count(), 0);
  } finally { await f.close(); }
});

test('theme: the saved choice overrides or follows the system theme', async () => {
  const f = await fixture({'domain:d': {title: 'Domain'}}, types, {colorScheme: 'dark'});
  const background = () => f.page.evaluate(() => getComputedStyle(document.body).backgroundColor);
  const toggle = f.page.locator('button#theme');
  try {
    await f.open('domain:d');
    assert.equal(await f.page.locator('select#theme').count(), 0);
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), null);
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await toggle.getAttribute('aria-label'), 'Switch to light theme');
    assert.equal(await toggle.locator('.theme-sun').isVisible(), true);
    assert.equal(await toggle.locator('.theme-moon').isVisible(), false);
    await f.page.emulateMedia({colorScheme: 'light'});
    await f.page.waitForFunction(() => document.getElementById('theme').dataset.dark === 'false');
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await toggle.getAttribute('aria-label'), 'Switch to dark theme');
    assert.equal(await toggle.locator('.theme-moon').isVisible(), true);
    await f.page.emulateMedia({colorScheme: 'dark'});
    await toggle.click();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'light');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await toggle.getAttribute('aria-label'), 'Switch to dark theme');
    await toggle.focus();
    await f.page.keyboard.press('Space');
    await f.page.emulateMedia({colorScheme: 'light'});
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'dark');
    assert.equal(await background(), 'rgb(25, 28, 32)');
    await f.page.setViewportSize({width: 390, height: 844});
    await toggle.click();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.locator('meta[name="theme-color"]').getAttribute('content'), '#fffcf5');
    const box = await toggle.boundingBox();
    assert.ok(box.width >= 44 && box.height >= 44 && box.x >= 0 && box.x + box.width <= 390);
    await f.page.evaluate(() => localStorage.removeItem('theme'));
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    await f.page.emulateMedia({colorScheme: 'dark'});
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), null);
    await f.page.evaluate(() => localStorage.setItem('joppa-theme', 'light'));
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(255, 252, 245)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'light');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('joppa-theme')), null);
    await toggle.click();
    await f.page.evaluate(() => localStorage.setItem('joppa-theme', 'light'));
    await f.page.reload();
    await f.page.locator('body.authenticated').waitFor();
    assert.equal(await background(), 'rgb(25, 28, 32)');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('theme')), 'dark');
    assert.equal(await f.page.evaluate(() => localStorage.getItem('joppa-theme')), null);
  } finally { await f.close(); }
});
};
