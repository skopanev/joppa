// Chromium check of the board filter by a value of a board field of the result shape: the
// filter lists the values, and each row dot takes the tone of the selected value.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('board-field-filter: the board shows the tone of each row for the selected value', async () => {
  const types = {
    requirement: {title: 'Requirements', role: 'requirement', status:'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status:'tests', parents: ['requirement']},
  };
  const f = await fixture(types);
  try {
    // The command sends the whole block of test rules with the added board field.
    const tree = await f.api('/tree');
    const block = tree.schema.test_results;
    block.fields = [...block.fields, {name: 'app', title: 'App', type: 'enum', values: ['ios', 'android'], required: true, board: true}];
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, reason: 'Add the app board field', test_results: block}, f.live.owner);
    await f.save('requirement:r', 'Sign in');
    await f.save('ac:a', 'Email sign-in works', 'requirement:r');
    await f.command({op: 'set_ac_tests', target: 'ac:a', tests: ['tests/app.rs > sign_in'], reason: 'Bind'});
    await f.mcp('joppa_record_results', {commit: 'c0ffee6', results: [
      {test: 'tests/app.rs > sign_in', status: 'pass', app: 'ios'},
      {test: 'tests/app.rs > sign_in', status: 'fail', app: 'android', message: 'crash'}]});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    const dot = address => f.page.locator('#tree .common-object[data-object-address="' + address + '"] > summary .status-circle').first();
    const toneIs = async (address, tone, step) => {
      await f.page.waitForFunction(([address, tone]) => document.querySelector('#tree .common-object[data-object-address="' + address + '"] > summary .status-circle')?.dataset.tone === tone, [address, tone])
        .catch(async () => assert.fail(step + ': ' + address + ' is ' + await dot(address).getAttribute('data-tone')));
    };
    await toneIs('requirement:r', 'red', 'all values');
    const filter = f.page.getByLabel('Result App');
    assert.deepEqual(await filter.locator('option').allTextContents(), ['All App', 'App: ios', 'App: android']);
    await filter.selectOption('app:ios');
    await toneIs('requirement:r', 'green', 'ios');
    await f.page.locator('#tree .common-object[data-object-address="requirement:r"] > summary').click();
    await toneIs('ac:a', 'green', 'ios criterion');
    await filter.selectOption('app:android');
    await toneIs('requirement:r', 'red', 'android');
    await filter.selectOption('');
    await toneIs('requirement:r', 'red', 'all values again');
  } finally { await f.close(); }
});
