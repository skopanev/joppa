// Chromium check of the copy button in the Flow card head (requirement:board-graph): the button
// sits between the title and the Copy link button, and a click puts the public item ID of the
// picked node on the clipboard while the card and the marks stay.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, swapWorld, open, snap, marked, card, waitCard, click} = require('./flow_support.cjs');

// In the page: the children of .flow-card-head in their order. A child gives its tag, its
// classes, its text and its aria-label.
function headChildren() {
  const head = document.querySelector('.flow-card-head');
  if (!head) return null;
  return [...head.children].map(c => ({tag: c.tagName.toLowerCase(), cls: c.className, text: c.textContent.trim(),
    label: c.getAttribute('aria-label'), state: c.dataset.copyState ?? null}));
}

// ac:xwzeqnds
test('xwzeqnds: The Flow card copies the item ID of the picked node', async () => {
  const f = await fixture(types);
  try {
    await f.page.context().grantPermissions(['clipboard-read', 'clipboard-write'], {origin: f.live.root});
    await swapWorld(f);
    // P is the public item ID of B, the ID that each copy button of the board copies.
    const P = (await f.api('/objects?address=step:b')).item_id;
    assert.ok(typeof P === 'string' && P.length > 0, 'B has a public item ID: ' + JSON.stringify(P));
    await open(f, '&relation=swap', 4);
    await click(f, 'step:b');
    await waitCard(f, 'Check the limit');

    // The head has one copy button, between the title and the Copy link button.
    const children = await f.page.evaluate(headChildren);
    assert.ok(children, 'the card has .flow-card-head');
    const copies = children.map((c, i) => c.label === 'Copy ID for agent' ? i : -1).filter(i => i >= 0);
    assert.equal(copies.length, 1, 'one copy button in the head: ' + JSON.stringify(children));
    const copyAt = copies[0], copy = children[copyAt];
    assert.equal(copy.tag, 'button', 'the copy control is a button: ' + JSON.stringify(copy));
    const titleAt = children.findIndex(c => /\bflow-card-title\b/.test(c.cls));
    const linkAt = children.findIndex(c => c.tag === 'button' && c.label === 'Copy link to node');
    const openAt = children.findIndex(c => c.tag === 'a' && /^Open\b/.test(c.text));
    assert.ok(titleAt >= 0 && linkAt >= 0 && openAt >= 0, 'the head has the title, Copy link and Open: ' + JSON.stringify(children));
    assert.equal(children[titleAt].text, 'Check the limit');
    assert.deepEqual([titleAt + 1, copyAt + 1, linkAt + 1], [copyAt, linkAt, openAt], 'title, Copy ID, Copy link, Open: ' + JSON.stringify(children));
    assert.equal(await f.page.locator('.flow-card-head button[aria-label="Copy ID for agent"]').count(), 1);

    await f.page.locator('.flow-card-head button[aria-label="Copy ID for agent"]').click();

    // The button gets data-copy-state "copied", and the toast says "Copied P".
    const button = f.page.locator('.flow-card-head > :nth-child(' + (copyAt + 1) + ')');
    await f.page.waitForFunction(at => document.querySelector('.flow-card-head')?.children[at]?.dataset.copyState === 'copied', copyAt)
      .catch(async () => assert.fail('copy state: ' + JSON.stringify(await f.page.evaluate(headChildren))));
    assert.equal(await button.getAttribute('data-copy-state'), 'copied');
    await f.page.waitForFunction(text => document.querySelector('#toast')?.textContent.trim() === text, 'Copied ' + P)
      .catch(async () => assert.fail('toast: ' + JSON.stringify(await f.page.evaluate(() => document.querySelector('#toast')?.textContent))));

    // The click puts P on the clipboard.
    assert.equal(await f.page.evaluate(() => navigator.clipboard.readText()), P, 'the clipboard text');

    // The card stays open with B, and B stays lit.
    const picked = await card(f);
    assert.ok(picked, 'the card stays open');
    assert.equal(picked.head, 'Check the limit');
    assert.equal(picked.open, 'step:b', 'the card address');
    assert.ok(picked.text.includes('step:b'), 'the card gives the address: ' + picked.text);
    assert.ok(marked(await snap(f), 'flow-lit').nodes.includes('step:b'), 'B stays .flow-lit');
  } finally { await f.close(); }
});
