// Chromium checks of the relations of a node on its card (requirement:q438uw0s): the card groups
// the edges by the relation title of the registry, shows when, and loads each group in pages
// of 25, at 1280 px and at 390 px.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []},
  doc: {title: 'Documents', role: 'document', status: 'none', root: true, parents: []},
};

function save(id, title, relations) {
  const command = {op: 'save_object', kind: 'doc', id, revision: 0, parent: null,
    data: {title, owner: 'owner', body: 'Fixture text'}, reason: 'Define the browser fixture'};
  return relations ? {...command, relations} : command;
}

// Wait until the card shows `count` rows: leaf elements whose whole text matches `source`.
function rows(f, source, count) {
  return f.page.waitForFunction(([source, count]) => [...document.querySelectorAll('.object-page *')]
    .filter(e => e.children.length === 0 && new RegExp(source).test(e.textContent)).length === count, [source, count]);
}

// Open the group with the title `title` when it shows no row that matches `row`.
async function openGroup(f, title, row) {
  const page = f.page.locator('.object-page');
  if (await page.getByText(row).count() === 0) await page.getByText(title).first().click();
  await page.getByText(row).first().waitFor();
}

// ac:1nywudsy
// test:4h530ybl
test('the board reads relations: the card groups the edges by relation title and pages them', async () => {
  const f = await fixture(types);
  try {
    const tree = await f.api('/tree');
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, reason: 'Declare the flow',
      put_relations: {swap: {title: 'Client swap', description: 'Client screens'}}}, f.live.owner);
    for (let i = 0; i < 30; i++) await f.command(save('r' + i, 'Ref ' + String(i).padStart(2, '0')));
    for (let i = 0; i < 3; i++) await f.command(save('s' + i, 'Screen ' + i));
    const related = Array.from({length: 30}, (_, i) => ({to: 'doc:r' + i}));
    const swap = [{to: 'doc:s0', when: 'limit ok'}, {to: 'doc:s1'}, {to: 'doc:s2'}];
    await f.command(save('a', 'Payment flow', {related: {edges: related}, swap: {edges: swap}}));
    for (const [width, height] of [[1280, 900], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      const reads = [];
      const log = request => { if (request.url().includes('relations_of=')) reads.push(new URL(request.url())); };
      f.page.on('request', log);
      await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&object=doc%3Aa');
      await f.page.locator('.object-page .common-object[data-object-address="doc:a"]').waitFor();
      // The group titles come from the registry.
      await openGroup(f, /^\s*Client swap\b/, /^\s*Screen \d\s*$/);
      await rows(f, '^\\s*Screen \\d\\s*$', 3);
      await f.page.locator('.object-page').getByText('limit ok').first().waitFor();
      await openGroup(f, /^\s*Related\b/, /^\s*Ref \d\d\s*$/);
      // The first page of related has 25 edges, and the next page gives the other 5.
      await rows(f, '^\\s*Ref \\d\\d\\s*$', 25);
      await f.page.locator('.object-page button', {hasText: /more/i}).first().click();
      await rows(f, '^\\s*Ref \\d\\d\\s*$', 30);
      assert.equal(await f.page.locator('.object-page button', {hasText: /more/i}).count(), 0,
        width + ' px: the last page still offers more');
      // Each group reads its own pages of 25.
      const relations = reads.map(url => url.searchParams.get('relation'));
      assert.ok(relations.includes('related') && relations.includes('swap'), width + ' px: ' + reads.join(' '));
      assert.ok(reads.every(url => url.searchParams.get('limit') === '25'), width + ' px: ' + reads.join(' '));
      assert.equal(reads.filter(url => url.searchParams.get('relation') === 'related').length, 2);
      const overflow = await f.page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      assert.ok(overflow <= 0, width + ' px: the page scrolls sideways by ' + overflow + ' px');
      f.page.off('request', log);
    }
  } finally { await f.close(); }
});
