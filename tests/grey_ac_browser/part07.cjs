// Part 7 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
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

test('check-list-frozen-ac3: an archived red sibling leaves the requirement green', async () => {
  const f = await fixture({...criterionRecords,
    'ac:B': {title: 'Red sibling', parent: 'requirement:r'},
    'requirement:mixed': {title: 'Mixed requirement', parent: 'capability:c'},
    'ac:M1': {title: 'Passing criterion', parent: 'requirement:mixed'},
    'ac:M2': {title: 'Criterion without tests', parent: 'requirement:mixed'}});
  try {
    await f.bind('ac:A', ['X']); const [y] = await f.bind('ac:B', ['Y']); await f.bind('ac:M1', ['X']);
    await f.record('eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', [['X'], ['Y', 'failed check']]);
    // The owner archives the test node of B before B.
    const archive = async target => f.command({op: 'archive_object', target,
      revision: (await f.api('/objects?address=' + encodeURIComponent(target))).current_revision, reason: 'Drop the red criterion'}, true);
    await archive(y); await archive('ac:B');
    const card = await f.open('capability:c');
    const r = card.locator('.common-object[data-object-address="requirement:r"] > summary');
    // The archive resets only ac:B and below, so ac:A keeps its pass.
    assert.equal(await r.locator('.status-circle').getAttribute('data-tone'), 'green');
    const mixed = card.locator('.common-object[data-object-address="requirement:mixed"] > summary');
    // A criterion without test nodes is orange, and orange wins over the green M1.
    assert.equal(await mixed.locator('.status-circle').getAttribute('data-tone'), 'orange');
  } finally { await f.close(); }
});

test('comments-on-object-ac2: a comment shows its author and server time', async () => {
  const configured = {record: {title: 'Records', role: 'document', status:'none', root: true, parents: []}};
  const address = 'record:one', text = 'Clarify this object boundary';
  const f = await fixture({[address]: {title: 'A source record'}}, configured,
    {locale: 'en-GB', timezoneId: 'Europe/Berlin'});
  try {
    await f.mcp('joppa_command',
      {request_id: crypto.randomUUID(), command: {op: 'note', target: address, id: 'comment', text}}, f.live.alex);
    const current = await f.api('/objects?address=' + encodeURIComponent(address));
    const comment = current.annotations.comments.comment;
    assert.equal(comment.author, 'alex');
    assert.equal(comment.actor_type, undefined);
    assert.equal(comment.created_at, comment.at);
    const instant = new Date(comment.created_at.replace(' ', 'T').replace(/\+00$/, 'Z'));
    assert.ok(Number.isFinite(instant.getTime()), 'the server timestamp must parse');
    const expected = new Intl.DateTimeFormat('en-GB',
      {dateStyle: 'medium', timeStyle: 'short', timeZone: 'Europe/Berlin'}).format(instant);
    const card = await f.open(address), comments = card.locator('.object-comments');
    assert.equal(await comments.getAttribute('open'), null);
    await comments.locator(':scope > summary').click();
    const line = comments.locator('.note');
    await line.locator('.note-text').waitFor();
    assert.equal(await line.locator('.note-text').innerText(), text);
    assert.equal(await line.locator('.note-meta').innerText(), 'alex · ' + expected);
    assert.equal(await f.page.evaluate(() => Intl.DateTimeFormat().resolvedOptions().timeZone), 'Europe/Berlin');
  } finally { await f.close(); }
});

test('edit-makes-revision-ac1: web and MCP read the same saved revision', async () => {
  const f = await fixture({'domain:d': {title: 'Shared object', body: 'Body at revision 1'}});
  try {
    await f.open('domain:d');
    for (let revision = 1; revision < 4; revision++) await f.save('domain:d', {title: 'Shared object', body: 'Body at revision ' + (revision + 1)}, revision, true);
    const body = 'Body at revision 5 from MCP';
    const saved = await f.save('domain:d', {title: 'Shared object', body}, 4, true);
    assert.equal(saved.object.revision, 5);
    await f.page.reload();
    const card = f.page.locator('.object-page > .common-object[data-object-address="domain:d"]');
    await card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
    const content = await card.locator('.object-content').innerText();
    const current = await f.api('/objects?address=domain:d');
    const mcp = await f.mcp('joppa_objects', {address: 'domain:d'});
    assert.equal(mcp.revision, 5);
    const fields = card.locator('.object-details');
    await card.locator(':scope > summary').getByRole('button', {name: 'Properties', exact: true}).click();
    assert.match(await fields.innerText(), /Revision 5/);
    const hash = value => createHash('sha256').update(value).digest('hex');
    assert.equal(hash(content), hash(mcp.data.body));
    assert.equal(content, body);
  } finally { await f.close(); }
});

// dependencies-before-priority narrows the pin to Task links: a domain keeps the stored
// revision of its depends_on edge, and the revision gates nothing. A related edge also keeps
// a pin. No edge has a blocking flag.
test('blocking-pins-revision-ac1: a blocking link keeps its stored target revision', async () => {
  const f = await fixture({'domain:t': {title: 'Target'}});
  try {
    for (const revision of [1, 2]) await f.save('domain:t', {title: 'Target', body: 'Target at revision ' + (revision + 1)}, revision);
    const link = (id, revision, relations) => f.command({op: 'save_object', kind: 'domain', id, revision, parent: null,
      data: {title: 'Source ' + id, owner: 'owner', body: 'Linked source'}, relations, reason: 'Link the target'});
    await link('s', 0, {depends_on: {edges: [{to: 'domain:t', revision: 3}]}});
    await link('u', 0, {related: {edges: [{to: 'domain:t'}]}});
    await link('u', 1, {related: {edges: [{to: 'domain:t', revision: 3}]}});
    // The read gives the edge of `key` with the title of its target, and the stored edge.
    const read = async (suffix, key) => {
      const value = await f.api('/objects?address=' + suffix);
      const edges = value.relations[key].edges;
      assert.equal(edges.length, 1);
      const [edge] = edges;
      assert.equal(edge.to, value.document.relations[key].edges[0].to);
      assert.deepEqual([edge.to, edge.title, edge.present ?? true, edge.blocking], ['domain:t', 'Target', true, undefined]);
      return {value, edge, stored: value.document.relations[key].edges[0]};
    };
    const pinned = await read('domain:s', 'depends_on');
    assert.deepEqual([pinned.edge.revision, pinned.edge.current_revision, pinned.stored.revision], [3, 3, 3]);
    assert.equal(pinned.value.review_required, undefined);
    const unpinned = await read('domain:u&revision=1', 'related');
    assert.deepEqual([unpinned.value.historical, unpinned.edge.revision ?? null, unpinned.stored.revision ?? null], [true, null, null]);
    const later = await read('domain:u', 'related');
    assert.deepEqual([later.edge.revision, later.stored.revision], [3, 3]);
    await f.save('domain:t', {title: 'Target', body: 'Target at revision 4'}, 3);
    const stale = await read('domain:s', 'depends_on');
    assert.deepEqual([stale.edge.revision, stale.edge.current_revision], [3, 4]);
    assert.deepEqual(stale.value.document.relations, pinned.value.document.relations);
    assert.equal(stale.value.review_required, undefined);
  } finally { await f.close(); }
});

test('dependencies-before-priority-ac5: alex sets and removes a dependency from the row of B', async t => {
  const records = {'domain:d': {title: 'Domain'}, 'capability:c': {title: 'Capability', parent: 'domain:d'},
    'requirement:A': {title: 'Sign-in rule', parent: 'capability:c'}, 'requirement:B': {title: 'Billing rule', parent: 'capability:c'},
    'ac:A1': {title: 'Sign-in condition', parent: 'requirement:A'}, 'ac:B1': {title: 'Billing condition', parent: 'requirement:B'}};
  const f = await fixture(records);
  const read = address => f.api('/objects?address=' + encodeURIComponent(address));
  const readyWork = async () => {
    const http = await f.api('/objects?kind=ac&ready=true&limit=100');
    const mcp = await f.mcp('joppa_objects', {kind: 'ac', ready: true, limit: 100});
    assert.deepEqual(mcp.items.map(item => item.address), http.items.map(item => item.address));
    return http.items.map(item => item.address);
  };
  const card = () => f.page.locator('.object-page > .common-object[data-object-address="requirement:B"]');
  const waiting = () => card().locator(':scope > summary > .waits-for');
  try {
    // B1 waits for no dependency, so its work is in the queue. A is not green.
    assert.ok((await readyWork()).includes('ac:B1'));
    // Save the depends_on edge of B via MCP/API command; the board displays it read-only.
    const currentB = await read('requirement:B');
    await f.command({op: 'save_object', kind: 'requirement', id: 'B', revision: currentB.current_revision,
      parent: 'capability:c', data: currentB.document.data,
      relations: {depends_on: {edges: [{to: 'requirement:A'}]}}, reason: 'Add blocking dependency'});
    assert.ok(!(await readyWork()).includes('ac:B1'), 'the work of B leaves the queue');
    // Each width is its own subtest, so the output names both runs.
    for (const viewport of [{width: 1280, height: 720}, {width: 390, height: 844}]) await t.test('dependencies-before-priority-ac5: ' + viewport.width + ' px', async () => {
      await f.page.setViewportSize(viewport);
      const B = await f.open('requirement:B');
      await waiting().waitFor();
      assert.equal(await waiting().innerText(), '1 dep', JSON.stringify(viewport));
      assert.equal(await waiting().getAttribute('title'), 'Waits for Sign-in rule');
      assert.equal(await waiting().isVisible(), true);
      const depLink = B.locator('.object-dependencies .dependency-row .object-link');
      await depLink.waitFor();
      assert.equal(await depLink.innerText(), 'Sign-in rule');
      assert.equal(await B.locator('.depends-on-button, .dependency-add, .dependency-remove, .dependency-picker').count(), 0);
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'no sideways scroll');
    });

    // Browser POST /dependencies and /commands are refused with 403 Forbidden.
    const before = await read('requirement:B');
    const route = f.live.root + '/api/workspaces/' + f.live.ws;
    const body = () => ({request_id: crypto.randomUUID(), source: 'requirement:B', target: 'requirement:A', action: 'remove'});
    const unchanged = async label => {
      const after = await read('requirement:B');
      assert.deepEqual([after.current_revision, after.document, after.position], [before.current_revision, before.document, before.position], label);
    };
    const sameOrigin = await f.page.request.post(route + '/dependencies', {headers: {Origin: f.live.root}, data: body()});
    assert.equal(sameOrigin.status(), 403);
    await unchanged('same-origin cookie dependencies');
    const foreign = await f.page.request.post(route + '/dependencies', {headers: {Origin: 'https://other.example'}, data: body()});
    assert.equal(foreign.status(), 403);
    await unchanged('foreign Origin');
    const commands = await f.page.request.post(route + '/commands', {headers: {Origin: f.live.root},
      data: {request_id: crypto.randomUUID(), command: {op: 'save_object', kind: 'requirement', id: 'B', revision: before.current_revision,
        parent: 'capability:c', data: before.document.data, relations: {depends_on: null}, reason: 'Cookie write'}}});
    assert.equal(commands.status(), 403);
    await unchanged('cookie commands');
  } finally { await f.close(); }
});

// Alex points at the status dot of the row of A, once at each width.
test('status-hint-on-hover: a hover on a status dot shows its status label', async t => {
  const f = await fixture(criterionRecords);
  try {
    const detail = await f.api('/objects?address=' + encodeURIComponent('ac:A'), undefined, f.live.alex);
    // Each width is its own subtest, so the output names both runs.
    for (const viewport of [{width: 1280, height: 720}, {width: 390, height: 844}]) await t.test('status-hint-on-hover: ' + viewport.width + ' px', async () => {
      await f.page.setViewportSize(viewport);
      const card = await f.open('requirement:r');
      const row = card.locator('.common-object[data-object-address="ac:A"] > summary');
      await row.waitFor();
      const dot = row.locator('.status-circle');
      assert.equal(await dot.isVisible(), true, JSON.stringify(viewport));
      await dot.hover();
      const hint = f.page.locator('.status-hint');
      await hint.waitFor();
      const text = await hint.innerText();
      assert.ok(text.startsWith('Status: ' + detail.signal.label), JSON.stringify([viewport, text]));
      assert.equal(await dot.getAttribute('aria-label'), await dot.getAttribute('data-hint'));
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'no sideways scroll');
    });
  } finally { await f.close(); }
});
};
