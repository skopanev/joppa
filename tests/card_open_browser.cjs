// Chromium checks of a board row: a click opens a closed card at once with its loading line,
// before its reads end, and each row shows the counts of its direct children by type.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: [], badge: 'REQ', index: 1},
  ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['requirement'], badge: 'AC', index: 2},
  test: {title: 'Tests', role: 'test', status: 'own', parents: ['ac'], badge: 'TEST', index: 3, title_preview_length: 400},
};
const node = address => '.common-object[data-object-address="' + address + '"]';
const row = address => '#tree ' + node(address);

async function board(f) {
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
  await f.page.locator(row('requirement:r')).waitFor();
}

// ac:vo82tcdi
test('card-opens-at-once: a click opens a closed card before its reads end', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Paid access');
    await f.save('ac:a', 'Access stays after sign-in', 'requirement:r');
    await board(f);
    // Hold each read of the card until the open card shows.
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route(url => /[?&](address|parent)=requirement%3Ar/.test(url.search), async route => { await held; await route.continue(); });
    const card = f.page.locator(row('requirement:r'));
    await card.locator(':scope > summary').click();
    await f.page.waitForFunction(selector => document.querySelector(selector)?.open, row('requirement:r'));
    assert.equal(await card.locator(':scope > .object-body > .object-loading').isVisible(), true, 'no loading line');
    assert.equal(await card.locator(node('ac:a')).count(), 0, 'the children showed before the reads ended');
    release();
    await card.locator(node('ac:a')).waitFor();
    assert.equal(await card.locator(':scope > .object-body > .object-loading').count(), 0, 'the loading line stayed');
    assert.equal(await card.evaluate(node => node.open), true);
  } finally { await f.close(); }
});

// ac:pl7zblgo
test('child-counts-row: each row shows its direct children of each type and the green ones', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Paid access');
    await f.save('ac:a', 'Access stays after sign-in', 'requirement:r');
    await f.save('ac:b', 'Access ends after a refund', 'requirement:r');
    for (const name of ['tests/x.rs > one', 'tests/x.rs > two']) await f.addTest('ac:a', name);
    await f.mcp('joppa_record_results', {commit: '1'.repeat(40),
      results: [{test: 'tests/x.rs > one', status: 'pass'}, {test: 'tests/x.rs > two', status: 'pass'}]});
    await board(f);
    const chip = f.page.locator(row('requirement:r') + ' > summary > .verification-chip');
    assert.deepEqual(await chip.allTextContents(), ['1 / 2 ACs']);
    assert.equal(await chip.getAttribute('aria-label'), '1 of 2 ACs passed');
    assert.equal(await chip.evaluate(node => node.classList.contains('has-passes')), false);
    await f.page.locator(row('requirement:r') + ' > summary').click();
    const tests = f.page.locator(row('ac:a') + ' > summary > .verification-chip');
    await tests.waitFor();
    assert.deepEqual(await tests.allTextContents(), ['2 / 2 TESTs']);
    assert.equal(await tests.evaluate(node => node.classList.contains('has-passes')), true);
    assert.equal(await f.page.locator(row('ac:b') + ' > summary > .verification-chip').count(), 0, 'a row without children shows a count');
  } finally { await f.close(); }
});
