// Chromium check of the tree diagram page: its lists match the workspace configuration, and it
// has no control that changes the workspace.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('tree-diagram-page-ac1 tree-config: the tree page lists the workspace configuration exactly', async () => {
  const types = {
    area: {title: 'Areas', role: 'group', status:'children', root: true, parents: []},
    rule: {title: 'Rules', role: 'requirement', status:'children', parents: ['area']},
    check: {title: 'Checks', role: 'ac', status:'children', parents: ['rule']},
    work: {title: 'Work', role: 'task', parents: ['check'], status: 'links'},
    memo: {title: 'Memos', role: 'document', status:'none', root: true, parents: ['*']},
  };
  const f = await fixture(types);
  try {
    const tree = await f.api('/tree');
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, types, priorities: ['urgent', 'normal', 'later'],
      filters: ['tags', 'priority'], reason: 'Set the priority levels and the filter fields'}, f.live.owner);
    // The expected lists come from the configuration above, not from the page.
    const expected = {
      roots: ['Areas', 'Memos'],
      edges: ['Areas → Rules', 'Rules → Checks', 'Workspace → Memos', 'Any object type → Memos', 'Workspace → Areas', 'Checks → Work'].sort(),
      // The page lists each relation key of the registry; the API check below proves the keys.
      relations: ['depends_on', 'related'],
      priorities: ['urgent', 'normal', 'later'],
      filters: ['tags', 'priority'],
      'status-rules': ['Areas: Highest tone of its children', 'Checks: Highest tone of its children', 'Memos: Process status',
        'Rules: Highest tone of its children', 'Work: Highest tone of linking objects'],
    };
    const saved = (await f.api('/tree')).schema;
    assert.deepEqual(saved.priority_levels, expected.priorities);
    assert.equal(saved.link_relations, undefined);
    assert.deepEqual(saved.relations, {
      depends_on: {title: 'Depends on', description: 'The node waits until the target is green. No cycles.', system: true},
      related: {title: 'Related', description: 'A reference between nodes that blocks nothing.', system: true},
    });
    assert.deepEqual(Object.keys(saved.relations).sort(), expected.relations);
    assert.deepEqual(saved.filters, expected.filters);
    assert.deepEqual(saved.status_rules, {area: 'children', check: 'children', memo: 'none', rule: 'children', work: 'links'});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.locator('#tree-schema-link').click();
    await f.page.waitForURL(url => url.pathname === '/tree');
    await f.page.locator('#config-priorities li').first().waitFor();
    const rendered = await f.page.evaluate(() => Object.fromEntries([...document.querySelectorAll('.schema-config [data-category]')]
      .map(list => [list.dataset.category, [...list.children].map(item => item.textContent)])));
    assert.deepEqual({...rendered, roots: [...rendered.roots].sort(), edges: [...rendered.edges].sort(), relations: [...rendered.relations].sort(), 'status-rules': [...rendered['status-rules']].sort()},
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
