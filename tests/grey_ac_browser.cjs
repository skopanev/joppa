const {test, describe, before, after} = require('node:test');
const assert = require('node:assert/strict');
const {readFileSync} = require('node:fs');
const path = require('node:path');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');

const root = path.join(__dirname, '..');
const html = readFileSync(path.join(root, 'src/board.html'), 'utf8')
  .replace('/* BOARD_STYLE */', readFileSync(path.join(root, 'src/board.css'), 'utf8'))
  .replace('/* BOARD_SCRIPT */', readFileSync(path.join(root, 'src/board.js'), 'utf8'));
const types = {
  domain: {title: 'Domains', role: 'group', root: true, parents: [], badge: 'DOM'},
  capability: {title: 'Capabilities', role: 'group', parents: ['domain'], badge: 'CAP'},
  requirement: {title: 'Requirements', role: 'requirement', parents: ['capability'], badge: 'REQ'},
  ac: {title: 'Criteria', role: 'ac', parents: ['requirement'], badge: 'AC'},
};
let browser;
before(async () => { browser = await chromium.launch({headless: true}); });
after(async () => { await browser?.close(); });

async function fixture(records, statuses = {}, objectTypes = types) {
  const context = await browser.newContext({reducedMotion: 'reduce'});
  const page = await context.newPage();
  const requests = [], errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await context.addInitScript(() => {
    window.copiedIds = [];
    Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => window.copiedIds.push(text)}});
  });
  const row = address => {
    const record = records[address];
    return {address, kind: address.split(':')[0], id: address.split(':')[1], title: record.title,
      parent: record.parent || null, revision: record.revision || 1, item_id: record.item_id || 'W-' + address,
      verification_summary: record.verification_summary,
      present: true, work_ready: true, signal: {tone: 'grey', label: 'Current document'}};
  };
  await context.route('http://joppa.test/**', async route => {
    const url = new URL(route.request().url());
    if (url.pathname === '/') return route.fulfill({contentType: 'text/html', body: html});
    if (url.pathname === '/favicon.svg') return route.fulfill({contentType: 'image/svg+xml', body: '<svg/>'});
    let value, status = 200;
    if (url.pathname === '/api/me') value = {actor: 'owner', email: 'owner@example.test'};
    else if (url.pathname === '/api/workspaces') value = [{id: 'w', title: 'Test workspace'}];
    else if (url.pathname.endsWith('/board')) value = {workspace: 'w', lazy: true, object_types: objectTypes,
      state: {title: 'Test workspace', members: {owner: 'human'}, requirements: {}}, launch: {configured: false}, system_ids: {}};
    else if (url.pathname.endsWith('/tree')) value = {schema: {revision: 1, types: objectTypes}};
    else if (url.pathname.endsWith('/tests')) {
      const address = url.searchParams.get('ac');
      value = address ? statuses[address] : {items: []};
      if (!value) value = {address, status: 'grey', reason: 'no tests', tests: [], tests_draft: false, commit: null, commits_behind: null};
    } else if (url.pathname.endsWith('/objects')) {
      const address = url.searchParams.get('address');
      if (address && records[address]) {
        const record = records[address], current = row(address);
        value = {...current, current_revision: current.revision, revision_count: current.revision, historical: false,
          document: {revision: current.revision, parent: current.parent, present: true, at: '2026-10-10T10:00:00Z',
            data: {title: record.title, owner: 'owner', body: record.body || 'Current source'}, links: []},
          links: [], incoming: [], annotations: {}, process: null};
      } else if (address) { status = 404; value = {error: 'Object not found'}; }
      else {
        const parent = url.searchParams.get('parent'), kind = url.searchParams.get('kind');
        value = {items: Object.keys(records).filter(key => (!kind || row(key).kind === kind)
          && (parent === '*' || row(key).parent === (parent || null))).map(row), next_cursor: null};
      }
    } else { status = 404; value = {error: 'Not found'}; }
    requests.push({url: url.href, status, value});
    return route.fulfill({status, json: value});
  });
  const open = async address => {
    const response = await page.goto('http://joppa.test/?workspace=w&object=' + encodeURIComponent(address));
    assert.equal(response.status(), 200);
    const card = page.locator('.object-page > .common-object[data-object-address="' + address + '"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor();
    return card;
  };
  const close = async () => { assert.deepEqual(errors, []); await context.close(); };
  return {page, context, requests, open, close};
}

describe('tests/grey_ac_browser.cjs', () => {
test('direct-child-counts: each row uses the shared next-level count', async () => {
  const configured = {
    bundle: {title: 'Bundles', role: 'group', root: true, parents: []},
    branch: {title: 'Branches', role: 'group', parents: ['bundle']},
    sheet: {title: 'Sheets', role: 'requirement', parents: ['branch']},
    proof: {title: 'Proofs', role: 'ac', parents: ['sheet']},
    extra: {title: 'Extra', role: 'document', parents: ['bundle']},
  };
  const records = {
    'bundle:root': {title: 'Root', verification_summary: {total: 3, passed: 1, failed: 1}},
    'branch:good': {title: 'Good', parent: 'bundle:root', verification_summary: {total: 2, passed: 2, failed: 0}},
    'branch:empty': {title: 'Empty', parent: 'bundle:root', verification_summary: {total: 0, passed: 0, failed: 0}},
  };
  const f = await fixture(records, {}, configured);
  try {
    const card = await f.open('bundle:root');
    assert.equal(await card.locator(':scope > summary .verification-chip').innerText(), '1 / 3 Branches');
    assert.equal(await card.locator('.common-object[data-object-address="branch:good"] > summary .verification-chip').innerText(), '2 / 2 Sheets');
    assert.equal(await card.locator('.common-object[data-object-address="branch:empty"] > summary .verification-chip').innerText(), '0 Sheets');
    assert.equal(f.requests.filter(r => new URL(r.url).pathname.endsWith('/tests')).length, 0);
  } finally { await f.close(); }
});
test('lazy-children-ac1: a capability reads only its twenty direct requirements', async () => {
  const records = {'domain:d': {title: 'Domain'}, 'capability:X': {title: 'Capability', parent: 'domain:d'}};
  for (let index = 0; index < 20; index++) {
    const id = String(index).padStart(2, '0');
    records['requirement:r' + id] = {title: 'Requirement ' + id, parent: 'capability:X'};
    records['ac:a' + id] = {title: 'Criterion ' + id, parent: 'requirement:r' + id};
  }
  const f = await fixture(records);
  try {
    const card = await f.open('capability:X');
    await card.locator(':scope > .object-body > .object-children > .common-object').nth(19).waitFor();
    const children = f.requests.filter(r => new URL(r.url).searchParams.get('parent') === 'capability:X');
    assert.equal(children.length, 1);
    assert.equal(children[0].value.items.length, 20);
    assert.ok(children[0].value.items.every(item => item.kind === 'requirement'));
    assert.equal(f.requests.filter(r => new URL(r.url).searchParams.get('address')?.startsWith('ac:')).length, 0);
    assert.equal(f.requests.filter(r => new URL(r.url).searchParams.get('address')?.startsWith('requirement:')).length, 0);
  } finally { await f.close(); }
});

test('old-results-history-ac2: the criterion shows both results and the failure message', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const records = {'ac:A': {title: 'Acceptance condition', parent: 'requirement:r'}};
  const f = await fixture(records, {'ac:A': {address: 'ac:A', status: 'red', commit, commits_behind: 0, tests_draft: false,
    tests: [{address: 'X', status: 'pass', commit}, {address: 'Y', status: 'fail', commit, message: 'expected 403, got 200'}]}});
  try {
    const card = await f.open('ac:A');
    const group = card.locator('.object-tests');
    await group.locator(':scope > summary').click();
    await group.locator('.ac-check').nth(1).waitFor();
    const rows = group.locator('.ac-check');
    assert.equal(await rows.count(), 2);
    assert.match(await rows.nth(0).innerText(), /X\s+pass · commit ccccccc/);
    assert.match(await rows.nth(1).innerText(), /Y\s+fail · commit ccccccc/);
    assert.equal(await rows.nth(1).locator('pre').innerText(), 'expected 403, got 200');
  } finally { await f.close(); }
});

test('stable-url-ac1: the original URL keeps the item ID after rename and move', async () => {
  const records = {'requirement:r': {title: 'Original title', parent: 'capability:old', item_id: 'W-R-STABLE'}};
  const f = await fixture(records);
  try {
    let card = await f.open('requirement:r');
    const originalUrl = f.page.url();
    await card.locator(':scope > summary .copy').click();
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), ['W-R-STABLE']);
    records['requirement:r'].title = 'Renamed requirement';
    records['requirement:r'].parent = 'capability:new';
    records['requirement:r'].revision = 2;
    const response = await f.page.goto(originalUrl);
    assert.equal(response.status(), 200);
    card = f.page.locator('.object-page > .common-object[data-object-address="requirement:r"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor();
    assert.equal(await card.locator(':scope > summary .node-title').innerText(), 'Renamed requirement');
    await card.locator(':scope > summary .copy').click();
    assert.deepEqual(await f.page.evaluate(() => window.copiedIds), ['W-R-STABLE']);
    const reads = f.requests.filter(r => new URL(r.url).searchParams.get('address') === 'requirement:r');
    assert.equal(reads.at(-1).value.document.parent, 'capability:new');
    assert.equal(reads.at(-1).value.item_id, 'W-R-STABLE');
  } finally { await f.close(); }
});

test('time-not-stale-ac2: the oldest test commit shows five newer commits', async () => {
  const commit = 'cccccccccccccccccccccccccccccccccccccccc';
  const newer = 'dddddddddddddddddddddddddddddddddddddddd';
  const records = {'ac:A': {title: 'Acceptance condition', parent: 'requirement:r'}};
  const f = await fixture(records, {'ac:A': {address: 'ac:A', status: 'green', commit, commits_behind: 5, tests_draft: false,
    tests: [{address: 'X', status: 'pass', commit}, {address: 'Y', status: 'pass', commit: newer}]}});
  try {
    const card = await f.open('ac:A'), group = card.locator('.object-tests');
    await group.locator(':scope > summary').click();
    await group.locator('.ac-check').nth(1).waitFor();
    assert.match(await group.locator(':scope > .node-meta').innerText(), /commit ccccccc · 5 commits behind/);
  } finally { await f.close(); }
});
});
