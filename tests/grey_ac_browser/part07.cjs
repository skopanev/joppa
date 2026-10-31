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

test('check-list-frozen-ac3: an archived red sibling leaves the requirement grey, not yellow', async () => {
  const f = await fixture({...criterionRecords,
    'ac:B': {title: 'Red sibling', parent: 'requirement:r'},
    'requirement:mixed': {title: 'Mixed requirement', parent: 'capability:c'},
    'ac:M1': {title: 'Passing criterion', parent: 'requirement:mixed'},
    'ac:M2': {title: 'Criterion without tests', parent: 'requirement:mixed'}});
  try {
    await f.bind('ac:A', ['X']); await f.bind('ac:B', ['Y']); await f.bind('ac:M1', ['X']);
    await f.record('eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee', [['X'], ['Y', 'failed check']]);
    await f.command({op: 'archive_object', target: 'ac:B', revision: 2, reason: 'Drop the red criterion'}, true);
    const card = await f.open('capability:c');
    const r = card.locator('.common-object[data-object-address="requirement:r"] > summary');
    assert.equal(await r.locator('.verification-chip').innerText(), '1 AC');
    assert.equal(await r.locator('.status-circle').getAttribute('data-tone'), 'grey');
    const mixed = card.locator('.common-object[data-object-address="requirement:mixed"] > summary');
    assert.equal(await mixed.locator('.verification-chip').innerText(), '1 / 2 ACs');
    // type-status-rules: a requirement takes the first of red, grey, green that a child has.
    assert.equal(await mixed.locator('.status-circle').getAttribute('data-tone'), 'grey');
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
    assert.equal(comment.actor_type, 'person');
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

test('same-revision-everywhere-ac1: web and MCP read the same saved revision', async () => {
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
// revision of its blocking link, and the revision gates nothing.
test('blocking-pins-revision-ac1: a blocking link keeps its stored target revision', async () => {
  const f = await fixture({'domain:t': {title: 'Target'}});
  try {
    for (const revision of [1, 2]) await f.save('domain:t', {title: 'Target', body: 'Target at revision ' + (revision + 1)}, revision);
    const link = (id, revision, links) => f.command({op: 'save_object', kind: 'domain', id, revision, parent: null,
      data: {title: 'Source ' + id, owner: 'owner', body: 'Linked source'}, links, reason: 'Link the target'});
    await link('s', 0, [{target: 'domain:t', relation: 'depends_on', revision: 3, blocking: true}]);
    await link('u', 0, [{target: 'domain:t', relation: 'related'}]);
    await link('u', 1, [{target: 'domain:t', relation: 'depends_on', revision: 3}]);
    const read = async suffix => {
      const value = await f.api('/objects?address=' + suffix);
      assert.equal(value.links.length, 1);
      const [item] = value.links;
      assert.deepEqual(item.link, value.document.links[0]);
      assert.deepEqual([item.title, item.kind, item.present, item.metadata_scope], ['Target', 'domain', true, 'current']);
      return value;
    };
    const pinned = await read('domain:s');
    assert.deepEqual([pinned.links[0].blocking, pinned.links[0].revision, pinned.links[0].current_revision], [true, 3, 3]);
    assert.equal(pinned.review_required, false);
    const unpinned = await read('domain:u&revision=1');
    assert.deepEqual([unpinned.historical, unpinned.links[0].blocking, unpinned.links[0].revision], [true, false, null]);
    assert.deepEqual(unpinned.links[0].link, {target: 'domain:t', relation: 'related', revision: null, blocking: false});
    const later = await read('domain:u');
    assert.deepEqual([later.links[0].blocking, later.links[0].revision], [false, 3]);
    await f.save('domain:t', {title: 'Target', body: 'Target at revision 4'}, 3);
    const stale = await read('domain:s');
    assert.deepEqual([stale.links[0].blocking, stale.links[0].revision, stale.links[0].current_revision], [true, 3, 4]);
    assert.deepEqual(stale.document.links, pinned.document.links);
    assert.equal(stale.review_required, false);
  } finally { await f.close(); }
});

// The bot marks A ready. Alex reads the rows of A and B under the requirement.
test('readiness-per-criterion-ac1: only the ready criterion A shows Ready for work', async t => {
  const f = await fixture({...criterionRecords, 'ac:B': {title: 'Second condition', parent: 'requirement:r'}});
  // Alex reads the status of each criterion over HTTP and MCP; both give the same label and tone.
  const signal = async address => {
    const http = await f.api('/objects?address=' + encodeURIComponent(address), undefined, f.live.alex);
    const mcp = await f.mcp('joppa_objects', {address}, f.live.alex);
    assert.deepEqual(mcp.status, {label: http.signal.label, tone: http.signal.tone}, address);
    return http;
  };
  try {
    for (const address of ['ac:A', 'ac:B']) assert.equal((await signal(address)).signal.assessment.state, 'unassessed', address);
    const a = await signal('ac:A');
    await f.command({op: 'confirm', req: a.process.req, revision: a.process.revision}, true);
    await f.command({op: 'assess', req: a.process.req, revision: a.process.revision, ac: a.process.criterion,
      assessment: {ready: true, reason: 'Known scope'}});
    const [A, B] = [await signal('ac:A'), await signal('ac:B')];
    assert.deepEqual([A.signal.assessment.state, A.signal.assessment.reason], ['ready', 'Known scope']);
    assert.equal(A.process.readiness_blocker, null);
    assert.equal(A.signal.label, 'Ready for work');
    assert.equal(B.signal.assessment.state, 'unassessed');
    assert.notEqual(B.signal.label, 'Ready for work');
    // Each width is its own subtest, so the output names both runs.
    for (const viewport of [{width: 1280, height: 720}, {width: 390, height: 844}]) await t.test('readiness-per-criterion-ac1: ' + viewport.width + ' px', async () => {
      await f.page.setViewportSize(viewport);
      const card = await f.open('requirement:r');
      const row = address => card.locator('.common-object[data-object-address="' + address + '"] > summary');
      await row('ac:B').waitFor();
      // The board gives each row its test status last. Neither criterion has tests.
      for (const address of ['ac:A', 'ac:B']) await row(address).getByRole('img', {name: /No tests/}).waitFor();
      // The status dot carries the row status as its name and shows it on hover.
      const status = async address => {
        const dot = row(address).locator('.status-circle');
        assert.equal(await dot.isVisible(), true, address);
        await dot.hover();
        const hint = f.page.locator('.status-hint');
        await hint.waitFor();
        return {label: await dot.getAttribute('aria-label'), hint: await hint.innerText(), text: await row(address).innerText()};
      };
      const shownA = await status('ac:A');
      assert.match(shownA.label, /^Status: Ready for work · No tests(\n|$)/, JSON.stringify(viewport));
      assert.match(shownA.hint, /^Status: Ready for work · No tests(\n|$)/, JSON.stringify(viewport));
      assert.equal(await row('ac:A').getByRole('img', {name: /Ready for work/}).count(), 1);
      const shownB = await status('ac:B');
      for (const text of Object.values(shownB)) assert.ok(!text.includes('Ready for work'), JSON.stringify([viewport, text]));
      assert.equal(await row('ac:B').getByRole('img', {name: /Ready for work/}).count(), 0);
      assert.ok(await f.page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1), 'no sideways scroll');
    });
  } finally { await f.close(); }
});

test('dependencies-before-priority-ac5: alex sets and removes a dependency from the row of B', async t => {
  const records = {'domain:d': {title: 'Domain'}, 'capability:c': {title: 'Capability', parent: 'domain:d'},
    'requirement:A': {title: 'Sign-in rule', parent: 'capability:c'}, 'requirement:B': {title: 'Billing rule', parent: 'capability:c'},
    'ac:A1': {title: 'Sign-in condition', parent: 'requirement:A'}, 'ac:B1': {title: 'Billing condition', parent: 'requirement:B'}};
  const f = await fixture(records);
  const read = address => f.api('/objects?address=' + encodeURIComponent(address));
  const unstarted = async () => {
    const http = await f.api('/objects?work_filter=unstarted&limit=100');
    const mcp = await f.mcp('joppa_objects', {work_filter: 'unstarted', limit: 100});
    assert.deepEqual(mcp.items.map(item => item.address), http.items.map(item => item.address));
    return http.items.map(item => item.address);
  };
  const card = () => f.page.locator('.object-page > .common-object[data-object-address="requirement:B"]');
  const waiting = () => card().locator(':scope > summary > .waits-for');
  try {
    // B1 is ready and its work is in the queue. A is not green.
    const b1 = await read('ac:B1');
    await f.command({op: 'confirm', req: b1.process.req, revision: b1.process.revision}, true);
    await f.command({op: 'assess', req: b1.process.req, revision: b1.process.revision, ac: b1.process.criterion,
      assessment: {ready: true, reason: 'Known scope'}}, true);
    assert.ok((await unstarted()).includes('ac:B1'));
    const readiness = (await read('ac:B1')).process.assessment.state;
    assert.equal(readiness, 'ready');
    // Save the blocking dependency on B via MCP/API command; the board displays it read-only.
    const currentB = await read('requirement:B');
    await f.command({op: 'save_object', kind: 'requirement', id: 'B', revision: currentB.current_revision,
      parent: 'capability:c', data: currentB.document.data,
      links: [{target: 'requirement:A', relation: 'depends_on', blocking: true}], reason: 'Add blocking dependency'});
    assert.ok(!(await unstarted()).includes('ac:B1'), 'the work of B leaves the queue');
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
        parent: 'capability:c', data: before.document.data, links: [], reason: 'Cookie write'}}});
    assert.equal(commands.status(), 403);
    await unchanged('cookie commands');
  } finally { await f.close(); }
});
};
