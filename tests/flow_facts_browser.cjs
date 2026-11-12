// Chromium check of the facts in Flow view (ac:cjeugcm8): the dot of a step shows the worst tone
// of its own tone and the tones of its facts, and the Flow card lists the facts in a section
// Facts after Next.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, toneColour, waitCard, click} = require('./flow_support.cjs');

// The facts of S in the stored order, with the title and the own tone of each fact.
const FACTS = [
  {address: 'ac:f1', title: 'First fact fails', tone: 'red'},
  {address: 'step:f2', title: 'Second fact', tone: 'blue'},
  {address: 'step:f3', title: 'Third fact', tone: 'purple'},
  {address: 'ac:u', title: 'Last step passes', tone: 'green'},
];

// The swap edges S→T and T→U, and S with the facts [F1, F2, F3, U]. F1 is a criterion whose
// only test failed (red). F1 holds swap with no edges. F2 is a step with no status (blue). F3 is a step that is archived after
// the save of S. U is a criterion whose only test passed (green).
async function factsWorld(f) {
  await declare(f, {swap: {title: 'Swap', description: 'Client screens'}});
  await f.save('requirement:r', 'Rules');
  await f.command({op: 'save_object', kind: 'ac', id: 'f1', revision: 0, parent: 'requirement:r',
    data: {title: 'First fact fails', owner: 'owner', body: 'Fixture text'}, reason: 'Define the browser fixture',
    relations: {swap: {edges: []}}});
  await f.addTest('ac:f1', 'tests/facts.rs > f1');
  await f.save('ac:u', 'Last step passes', 'requirement:r');
  await f.addTest('ac:u', 'tests/facts.rs > u');
  await build(f, [
    {id: 'f2', title: 'Second fact'},
    {id: 'f3', title: 'Third fact'},
    {id: 't', title: 'Middle step', relations: {swap: {edges: [{to: 'ac:u'}]}}},
    {id: 's', title: 'Start step', relations: {swap: {edges: [{to: 'step:t'}], facts: FACTS.map(x => x.address)}}},
  ]);
  await f.command({op: 'archive_object', target: 'step:f3', revision: 1, reason: 'Remove the fact'});
  await f.mcp('joppa_record_results', {commit: 'c0ffee5', results: [{test: 'tests/facts.rs > u', status: 'pass'}, {test: 'tests/facts.rs > f1', status: 'fail'}]});
  const tone = async address => (await f.api('/objects?address=' + encodeURIComponent(address))).signal?.tone;
  assert.equal(await tone('ac:f1'), 'red', 'the fixture: F1 is red');
  assert.equal(await tone('ac:u'), 'green', 'the fixture: U is green');
}

// In the page: the address, the data-tone and the computed fill of each dot.
const dotPaint = () => [...document.querySelectorAll('.flow-node')].map(n => {
  const dot = n.matches('circle,ellipse,rect') ? n : n.querySelector('circle,ellipse,rect') || n;
  return {address: n.dataset.address, tone: n.dataset.tone ?? dot.dataset.tone, fill: getComputedStyle(dot).fill};
});

// In the page: the Facts section of the open card. A row is the widest element around the
// link to object=<address> of one fact that holds no link of another fact. A tone is the
// data-tone of a .status-circle, or else its tone class.
function readFacts(addresses) {
  const card = document.querySelector('.flow-card-head')?.parentElement;
  if (!card) return null;
  const tones = ['red', 'purple', 'yellow', 'orange', 'blue', 'green'];
  const tone = c => c ? c.dataset.tone || [...c.classList].find(n => tones.includes(n)) || null : null;
  const leaf = text => [...card.querySelectorAll('*')].find(e => e.children.length === 0 && e.textContent.trim() === text) || null;
  const after = (a, b) => Boolean(a && b && (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING));
  const label = leaf('Facts'), next = leaf('Next');
  const target = a => new URLSearchParams((a.getAttribute('href') || '').split('?')[1] || '').get('object');
  const links = [...card.querySelectorAll('a')].filter(a => !a.closest('.flow-card-head') && addresses.includes(target(a)));
  const rows = links.map(a => {
    let row = a;
    while (row.parentElement && row.parentElement !== card && links.filter(l => row.parentElement.contains(l)).length === 1) row = row.parentElement;
    return {address: target(a), tone: tone(row.querySelector('.status-circle')), text: row.textContent.trim(),
      copies: row.querySelectorAll('button[aria-label="Copy ID for agent"]').length, afterLabel: after(label, a)};
  });
  return {head: tone(card.querySelector('.flow-card-head .status-circle')), facts: Boolean(label), afterNext: after(next, label), rows};
}
const facts = f => f.page.evaluate(readFacts, FACTS.map(x => x.address));

// ac:cjeugcm8
test('cjeugcm8: A step shows the worst tone of its facts and lists them in the card', async () => {
  const f = await fixture(types);
  try {
    await factsWorld(f);
    await open(f, '&relation=swap', 3);
    // The dots show after every read ends: S takes the tone red of F1.
    await f.page.waitForFunction(() => document.querySelector('.flow-node[data-address="step:s"]')?.dataset.tone === 'red')
      .catch(async () => assert.fail('the dot of S: ' + JSON.stringify(await f.page.evaluate(dotPaint))));
    const dots = Object.fromEntries((await f.page.evaluate(dotPaint)).map(d => [d.address, d]));
    assert.deepEqual(Object.keys(dots).sort(), ['ac:u', 'step:s', 'step:t'], 'F1, F2 and F3 have no dot, U keeps its dot');
    assert.equal(dots['step:s'].tone, 'red', 'S has data-tone red');
    assert.equal(dots['step:s'].fill, await toneColour(f, 'red'), 'S has the fill of var(--dot-red)');
    assert.equal(dots['ac:u'].tone, 'green', 'U keeps data-tone green');
    assert.equal(dots['step:t'].tone, 'blue', 'T keeps data-tone blue');

    // The card of S has the section Facts after Next, with one row for each fact in order.
    await click(f, 'step:s');
    await waitCard(f, 'Start step');
    const s = await facts(f);
    assert.ok(s, 'the card of S is open');
    assert.ok(s.facts, 'the card of S has the section Facts: ' + JSON.stringify(s));
    assert.ok(s.afterNext, 'the section Facts comes after Next');
    assert.equal(s.head, 'red', 'the card head circle of S is red');
    assert.deepEqual(s.rows.map(r => r.address), FACTS.map(x => x.address), 'the fact rows in the stored order: ' + JSON.stringify(s.rows));
    FACTS.forEach((fact, i) => {
      const row = s.rows[i];
      assert.equal(row.tone, fact.tone, fact.address + ': the status circle has its own tone ' + fact.tone);
      assert.ok(row.text.includes(fact.title), fact.address + ': the row gives the title: ' + row.text);
      assert.equal(row.copies, 1, fact.address + ': the row has one copy button "Copy ID for agent"');
      assert.ok(row.afterLabel, fact.address + ': the row is in the section Facts');
    });

    // The card of T has no section Facts.
    await click(f, 'step:t');
    await waitCard(f, 'Middle step');
    const t = await facts(f);
    assert.ok(t, 'the card of T is open');
    assert.equal(t.facts, false, 'the card of T has no section Facts: ' + JSON.stringify(t));
  } finally { await f.close(); }
});
