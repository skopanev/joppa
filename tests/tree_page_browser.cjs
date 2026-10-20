// Chromium check of the tree diagram page: its lists match the workspace configuration, and it
// has no control that changes the workspace.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('tree-diagram-page-ac1 tree-config: the tree page lists the workspace configuration exactly', async () => {
  const types = {
    area: {title: 'Areas', role: 'group', root: true, parents: []},
    rule: {title: 'Rules', role: 'requirement', parents: ['area']},
    check: {title: 'Checks', role: 'ac', parents: ['rule']},
    work: {title: 'Work', role: 'task', parents: ['check'], status: 'links'},
    memo: {title: 'Memos', role: 'document', root: true, parents: ['*']},
  };
  const f = await fixture(types);
  try {
    const tree = await f.api('/tree');
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, types, priorities: ['urgent', 'normal', 'later'],
      reason: 'Set the priority levels'}, f.live.owner);
    // The expected lists come from the configuration above, not from the page.
    const expected = {
      roots: ['Areas', 'Memos'],
      edges: ['Areas → Rules', 'Rules → Checks', 'Workspace → Memos', 'Any object type → Memos', 'Workspace → Areas', 'Checks → Work'].sort(),
      relations: ['related', 'depends_on'],
      priorities: ['urgent', 'normal', 'later'],
      'status-rules': ['Areas: First status of its children', 'Checks: Test results of its own list', 'Memos: Process status',
        'Rules: First status of its children', 'Work: First status of linking objects'],
    };
    const saved = (await f.api('/tree')).schema;
    assert.deepEqual(saved.priority_levels, expected.priorities);
    assert.deepEqual(saved.link_relations, expected.relations);
    assert.deepEqual(saved.status_rules, {area: 'children', check: 'tests', memo: 'none', rule: 'children', work: 'links'});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.locator('#tree-schema-link').click();
    await f.page.waitForURL(url => url.pathname === '/tree');
    await f.page.locator('#config-priorities li').first().waitFor();
    const rendered = await f.page.evaluate(() => Object.fromEntries([...document.querySelectorAll('.schema-config [data-category]')]
      .map(list => [list.dataset.category, [...list.children].map(item => item.textContent)])));
    assert.deepEqual({...rendered, roots: [...rendered.roots].sort(), edges: [...rendered.edges].sort(), 'status-rules': [...rendered['status-rules']].sort()},
      {...expected, roots: [...expected.roots].sort(), 'status-rules': [...expected['status-rules']].sort()});
    // The diagram draws one line for each parent rule and each root type.
    const lines = await f.page.locator('.schema-edge:not(.reference-edge)').evaluateAll(paths => paths.map(path => path.dataset.source + '>' + path.dataset.target).sort());
    assert.deepEqual(lines, ['@workspace>area', '@workspace>memo', 'area>rule', 'check>work', 'rule>check']);
    // No control submits a change: no button, no form and no submit input.
    const controls = await f.page.evaluate(() => ({buttons: document.querySelectorAll('button, input[type=submit], input[type=button]').length,
      forms: document.querySelectorAll('form').length, selects: [...document.querySelectorAll('select')].map(select => select.id).sort()}));
    assert.deepEqual(controls, {buttons: 0, forms: 0, selects: ['revision', 'workspace']});
  } finally { await f.close(); }
});
