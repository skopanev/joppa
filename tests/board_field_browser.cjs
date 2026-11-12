// Chromium checks of the board key of a typed field: the card of ac:a shows the field stage,
// which has board: true, and does not show the field note, which has no board key.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture, testType} = require('./board_fixture.cjs');

const types = {
  requirement: {title: 'Requirements', role: 'requirement', status: 'children', root: true, parents: []},
  ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['requirement'],
    fields: {stage: {type: 'enum', values: ['plan', 'done'], board: true}, note: {type: 'text'}}},
  test: testType(['ac']),
};
const row = address => '#tree .common-object[data-object-address="' + address + '"]';

// The visible texts of a card: its row and its body, without the cards of its children.
const shown = (page, address) => page.evaluate(address => {
  const card = document.querySelector('#tree .common-object[data-object-address="' + address + '"]');
  const walker = document.createTreeWalker(card, NodeFilter.SHOW_TEXT);
  const out = [];
  for (let node = walker.nextNode(); node; node = walker.nextNode()) {
    const element = node.parentElement;
    if (element.closest('.common-object') !== card || !element.checkVisibility()) continue;
    const text = node.textContent.trim();
    if (text) out.push(text);
  }
  return out;
}, address);

// The card shows stage plan and does not show note or its value x.
const onlyStage = (texts, step) => {
  assert.ok(texts.some(text => text.includes('plan')), step + ': no plan in ' + JSON.stringify(texts));
  assert.ok(texts.some(text => /stage/i.test(text)), step + ': no stage in ' + JSON.stringify(texts));
  assert.ok(!texts.some(text => text === 'x' || /^note\b/i.test(text)), step + ': note shows in ' + JSON.stringify(texts));
};

// ac:sigd1mxp
test('a_typed_field_shows_on_the_board_card_only_with_board_true', async () => {
  const f = await fixture(types);
  try {
    await f.save('requirement:r', 'Paid access');
    await f.command({op: 'save_object', kind: 'ac', id: 'a', revision: 0, parent: 'requirement:r',
      data: {title: 'Access stays after sign-in', owner: 'owner', body: 'Fixture text', stage: 'plan', note: 'x'},
      reason: 'Plan A'});
    await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws));
    await f.page.locator(row('requirement:r') + ' > summary').click();
    const card = f.page.locator(row('ac:a'));
    await card.waitFor();
    // The member opens the card of ac:a.
    await card.locator(':scope > summary').click();
    await f.page.waitForFunction(selector => document.querySelector(selector)?.open, row('ac:a'));
    // The body is loaded: it has children and no loading line.
    await f.page.waitForFunction(selector => {
      const body = document.querySelector(selector + ' > .object-body');
      return body && body.children.length > 0 && !body.querySelector(':scope > .object-loading');
    }, row('ac:a'));
    // The properties of the card show too.
    const properties = card.locator(':scope > summary .fields-button');
    if (await properties.getAttribute('aria-expanded') !== 'true') await properties.click();
    assert.equal(await properties.getAttribute('aria-expanded'), 'true');
    // The properties may show after a short branch motion: read the card until plan shows.
    let texts = [];
    for (let tries = 0; tries < 50 && !texts.some(text => text.includes('plan')); tries++) {
      if (tries) await new Promise(resolve => setTimeout(resolve, 100));
      texts = await shown(f.page, 'ac:a');
    }
    onlyStage(texts, 'open card');
  } finally { await f.close(); }
});
