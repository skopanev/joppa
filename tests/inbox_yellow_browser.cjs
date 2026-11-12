// Chromium checks of the tone yellow: only an Inbox input that waits for processing is yellow.
// Criterion B depends on the blue criterion A and is green. No row shows a dependency count
// or a wait badge.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture, testType} = require('./board_fixture.cjs');

const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []},
  ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['requirement']},
  test: testType(['ac']),
  note: {title: 'Inbox', role: 'document', status: 'none', root: true, parents: [], inbox: true},
};
const row = address => '#tree .common-object[data-object-address="' + address + '"]';
const dot = (page, address) => page.locator(row(address) + ' > summary .status-circle').first();
const tone = async (page, address) => {
  await dot(page, address).waitFor();
  return dot(page, address).evaluate(node => [node.dataset.tone, node.getAttribute('aria-label')]);
};

// ac:kk7epj2h
test('yellow_means_only_an_inbox_input_that_waits_for_processing', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Requirement R');
    await f.save('ac:a', 'Criterion A', 'requirement:r');
    await f.addTest('ac:a', 'tests/inbox_yellow.rs > a');
    await f.save('requirement:q', 'Requirement Q');
    await f.command({op: 'save_object', kind: 'ac', id: 'b', revision: 0, parent: 'requirement:q',
      data: {title: 'Criterion B', owner: 'owner', body: 'Fixture text'},
      relations: {depends_on: {edges: [{to: 'ac:a'}]}}, reason: 'B depends on A'});
    await f.addTest('ac:b', 'tests/inbox_yellow.rs > b');
    await f.mcp('joppa_record_results', {commit: 'b'.repeat(40), results: [{test: 'tests/inbox_yellow.rs > b', status: 'pass'}]});
    await f.save('note:i', 'Incoming input');
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.getByRole('tab', {name: 'Requirements'}).click();
    for (const address of ['requirement:r', 'requirement:q']) {
      await f.page.locator(row(address) + ' > summary').click();
    }
    await f.page.locator(row('ac:a')).waitFor();
    await f.page.locator(row('ac:b')).waitFor();
    assert.equal((await tone(f.page, 'requirement:q'))[0], 'green', 'requirement:q is green');
    assert.equal((await tone(f.page, 'ac:b'))[0], 'green', 'ac:b is green');
    assert.equal((await tone(f.page, 'ac:a'))[0], 'blue', 'ac:a is blue');
    // No row shows a wait badge or a count of dependencies.
    assert.equal(await f.page.locator('#tree .waits-for').count(), 0, 'a row shows a wait badge');
    const rows = await f.page.locator('#tree .common-object > summary').allTextContents();
    assert.ok(rows.length >= 4, JSON.stringify(rows));
    for (const text of rows) assert.ok(!/\bdeps?\b|\bwaits?\b|dependenc/i.test(text), 'a row shows a dependency count: ' + text);
    await f.page.getByRole('tab', {name: 'Inbox'}).click();
    const [inbox, label] = await tone(f.page, 'note:i');
    assert.equal(inbox, 'yellow', 'note:i is yellow');
    assert.ok(label.includes('Input awaits processing'), label);
    await f.command({op: 'archive_object', target: 'note:i', revision: 1, reason: 'Processed into the tree'});
    const read = await f.api('/objects?address=note%3Ai');
    assert.equal(read.signal.tone, 'purple', JSON.stringify(read.signal));
  } finally { await f.close(); }
});
