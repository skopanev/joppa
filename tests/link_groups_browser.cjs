// Chromium checks of object links on a card: an incoming link group shows its count and reads
// its links in pages only when the reader opens it, and a Markdown link [Title](kind:id)
// opens that object.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []},
  note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: [], inbox: true},
};

function save(kind, id, data, links = []) {
  return {op: 'save_object', kind, id, revision: 0, parent: null, data: {owner: 'owner', ...data}, links, reason: 'Define the browser fixture'};
}

async function openObject(f, address) {
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&object=' + encodeURIComponent(address));
  await f.page.locator('.object-page .common-object[data-object-address="' + address + '"]').waitFor();
}

test('grouped-links-board: the card loads a link group when the reader opens it', async () => {
  const f = await fixture(types);
  try {
    await f.command(save('requirement', 'r', {title: 'Paid access', body: 'Fixture text'}));
    for (let i = 0; i < 120; i++) {
      await f.command(save('note', 'n' + String(i).padStart(3, '0'), {title: 'Input ' + i, body: 'Source'}, [{target: 'requirement:r', relation: 'related'}]));
    }
    const reads = [];
    f.page.on('request', request => { if (request.url().includes('links_of=')) reads.push(request.url()); });
    await openObject(f, 'requirement:r');
    const group = f.page.locator('details.link-group[data-relation="related"][data-kind="note"]');
    await group.waitFor();
    assert.match(await group.locator('summary').textContent(), /Linked from · related · note 120/);
    assert.equal(reads.length, 0, 'a link was read before the group opened');
    const rows = group.locator('.object-links > a');
    await group.locator('summary').click();
    await f.page.waitForFunction(() => document.querySelectorAll('details.link-group .object-links > a').length === 50);
    for (const count of [100, 120]) {
      await group.locator('button.load-more').click();
      await f.page.waitForFunction(n => document.querySelectorAll('details.link-group .object-links > a').length === n, count);
    }
    assert.equal(await rows.count(), 120);
    assert.equal(await group.locator('button.load-more').count(), 0, 'the last page still offers Load more');
    assert.equal(reads.length, 3);
  } finally { await f.close(); }
});

test('markdown-object-link: a Markdown link [Title](kind:id) opens that object', async () => {
  const f = await fixture(types);
  try {
    await f.command(save('requirement', 'r', {title: 'Paid access', body: 'Fixture text'}));
    await f.command(save('requirement', 'm', {title: 'Refunds', body: 'Refunds keep [Paid access](requirement:r) until the end of the period.'}));
    await openObject(f, 'requirement:m');
    const link = f.page.locator('.object-page .markdown a', {hasText: 'Paid access'});
    await link.waitFor();
    const href = new URL(await link.getAttribute('href'), f.live.root);
    assert.equal(href.origin, new URL(f.live.root).origin);
    assert.equal(href.searchParams.get('object'), 'requirement:r');
    assert.equal(await link.getAttribute('target'), null, 'an object link opens a new tab');
    await link.click();
    await f.page.locator('.object-page .common-object[data-object-address="requirement:r"]').waitFor();
  } finally { await f.close(); }
});
