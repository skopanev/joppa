// Chromium check of the collapse of the Flow card (ac:sap7j0yx): the button "Collapse the card"
// at the start of .flow-card-head hides every part of the card below the head, the card stays
// collapsed across picks, the same button expands it again, and × hides the card and clears
// the marks.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');
const {types, declare, build, open, snap, marked, waitMarks, NONE, card, waitCard, click, closeCard} = require('./flow_support.cjs');

// The title of B: 120 characters.
const LONG = ('Check the limit ' + 'of the daily transfer amount and the account rule '.repeat(3)).slice(0, 119) + 'z';

// The nodes of ac:board-graph-ac1: the swap edges A→B (when "limit ok"), A→C, B→D and C→D, and
// B with depends_on A. B has the long title.
const NODES = [
  {id: 'd', title: 'Zebra receipt'},
  {id: 'c', title: 'Ask for a document', relations: {swap: {edges: [{to: 'step:d'}]}}},
  {id: 'b', title: LONG, parent: 'step:c', relations: {swap: {edges: [{to: 'step:d'}]}, depends_on: {edges: [{to: 'step:a'}]}}},
  {id: 'a', title: 'Enter the amount', relations: {swap: {edges: [{to: 'step:b', when: 'limit ok'}, {to: 'step:c'}]}}},
];

// The order of the children of .flow-card-head.
const ORDER = ['toggle', 'circle', 'title', 'copy', 'open', 'close'];

// In the page: the state of the card. parts gives the visibility of each child of .flow-card
// other than the head. children gives the name, the text and the top of each child of the head.
// line is the line height of the title in px.
function cardState() {
  const box = document.querySelector('.flow-card'), head = box?.querySelector('.flow-card-head');
  if (!box || !head) return null;
  const name = c => {
    const label = c.getAttribute('aria-label'), cls = String(c.className), tag = c.tagName.toLowerCase();
    if (tag === 'button' && (label === 'Collapse the card' || label === 'Expand the card')) return 'toggle';
    if (/\bstatus-circle\b/.test(cls)) return 'circle';
    if (/\bflow-card-title\b/.test(cls)) return 'title';
    if (label === 'Copy ID for agent') return 'copy';
    if (tag === 'a' && /^Open\b/.test(c.textContent.trim())) return 'open';
    if (c.textContent.trim() === '×') return 'close';
    return tag + '.' + cls;
  };
  const title = head.querySelector('.flow-card-title'), style = title && getComputedStyle(title);
  const line = style ? (style.lineHeight === 'normal' ? Number.parseFloat(style.fontSize) * 1.2 : Number.parseFloat(style.lineHeight)) : null;
  const toggle = head.querySelector('button[aria-expanded]');
  return {collapsed: box.classList.contains('flow-card-collapsed'), scroll: [box.scrollWidth, box.clientWidth],
    parts: [...box.children].filter(c => c !== head).map(c => ({cls: c.className, shown: c.checkVisibility() && c.getBoundingClientRect().height > 0})),
    children: [...head.children].map(c => { const r = c.getBoundingClientRect(); return {name: name(c), text: c.textContent.trim(), mid: r.top + r.height / 2, shown: c.checkVisibility()}; }),
    height: head.getBoundingClientRect().height, line,
    title: title ? {scroll: title.scrollWidth, client: title.clientWidth, overflow: style.textOverflow} : null,
    toggle: toggle ? {label: toggle.getAttribute('aria-label'), expanded: toggle.getAttribute('aria-expanded')} : null};
}
const state = f => f.page.evaluate(cardState);

// Wait until the card state satisfies `ok`, or fail with the last state.
async function waitState(f, ok, step) {
  let last;
  for (const end = Date.now() + 10000; Date.now() < end; await f.page.waitForTimeout(100)) {
    last = await state(f);
    if (last && ok(last)) return last;
  }
  assert.fail(step + ': ' + JSON.stringify(last));
}

// The head has the toggle, the status circle, the title, the copy button, Open and × in order.
const assertOrder = (s, step) => assert.deepEqual(s.children.map(c => c.name), ORDER, step + ': the head children: ' + JSON.stringify(s.children));

// The collapsed card: every part below the head is hidden, and the head keeps its children
// in one line.
function assertCollapsed(s, title, step) {
  assert.equal(s.collapsed, true, step + ': .flow-card has the class flow-card-collapsed');
  assert.ok(s.parts.length > 0 && s.parts.every(p => !p.shown), step + ': every part below the head is hidden: ' + JSON.stringify(s.parts));
  assertOrder(s, step);
  for (const c of s.children) assert.ok(c.shown, step + ': the head shows ' + c.name);
  assert.equal(s.children[ORDER.indexOf('title')].text, title, step + ': the head title');
  // The head centres its children, so one row means one vertical centre.
  const mids = s.children.map(c => c.mid);
  assert.ok(Math.max(...mids) - Math.min(...mids) <= 4, step + ': the head children share one row: ' + JSON.stringify(mids));
  assert.ok(s.line > 0 && s.height <= 1.5 * s.line, step + ': the head height ' + s.height + ' is at most 1.5 x the line height ' + s.line);
  assert.deepEqual(s.toggle, {label: 'Expand the card', expanded: 'false'}, step + ': the toggle');
}

// ac:sap7j0yx
test('sap7j0yx: The Flow card collapses into its head and stays collapsed across picks', async () => {
  assert.equal(LONG.length, 120, 'the title of B has 120 characters');
  const f = await fixture(types);
  try {
    await declare(f, {swap: {title: 'Swap', description: 'Client screens'}});
    await build(f, NODES);
    await open(f, '&relation=swap', 4);
    await click(f, 'step:b');
    await waitCard(f, LONG);
    await waitMarks(f, {lit: {nodes: ['step:a', 'step:b', 'step:d']}}, 'B');
    const expanded = await state(f);
    assertOrder(expanded, 'before the collapse');
    assert.deepEqual(expanded.toggle, {label: 'Collapse the card', expanded: 'true'}, 'the toggle before the collapse');
    assert.equal(expanded.collapsed, false, 'the card is not collapsed before the click');

    // The collapse keeps only the head in one line, the long title ends with an ellipsis, the
    // card has no scroll bar, and A, B and D stay lit.
    await f.page.locator('.flow-card-head button[aria-label="Collapse the card"]').click();
    const collapsed = await waitState(f, s => s.collapsed && s.toggle?.expanded === 'false', 'the collapse');
    assertCollapsed(collapsed, LONG, 'after the collapse');
    assert.equal(collapsed.scroll[0], collapsed.scroll[1], 'the card has no scroll bar: scrollWidth, clientWidth ' + collapsed.scroll);
    const t = collapsed.title;
    assert.ok(t.scroll > t.client, 'the title of B is cut: ' + JSON.stringify(t));
    assert.equal(t.overflow, 'ellipsis', 'the title of B ends with an ellipsis');
    assert.deepEqual(marked(await snap(f), 'flow-lit').nodes, ['step:a', 'step:b', 'step:d'], 'A, B and D stay .flow-lit');

    // A click on D shows D in the card, and the card stays collapsed.
    await click(f, 'step:d');
    await waitCard(f, 'Zebra receipt');
    assertCollapsed(await waitState(f, s => s.collapsed, 'the pick of D'), 'Zebra receipt', 'after the click on D');
    assert.equal((await card(f)).open, 'step:d', 'the card shows D');

    // The same button expands the card: the facts of D show again.
    await f.page.locator('.flow-card-head button[aria-label="Expand the card"]').click();
    const again = await waitState(f, s => !s.collapsed && s.toggle?.expanded === 'true', 'the expand');
    assert.deepEqual(again.toggle, {label: 'Collapse the card', expanded: 'true'}, 'the toggle after the expand');
    assertOrder(again, 'after the expand');
    assert.ok(again.parts.length > 0 && again.parts.every(p => p.shown), 'the parts below the head show again: ' + JSON.stringify(again.parts));
    const d = await card(f);
    assert.equal(d.facts.Object, 'Zebra receipt', 'the facts of D show again: ' + JSON.stringify(d));
    assert.ok(d.facts.Previous && d.facts.Next, 'the card gives Previous and Next of D: ' + JSON.stringify(d.facts));

    // × hides the card and clears every mark.
    await closeCard(f);
    await f.page.waitForFunction(() => !document.querySelector('.flow-card')?.checkVisibility())
      .catch(async () => assert.fail('the card stays: ' + JSON.stringify(await state(f))));
    await waitMarks(f, {lit: NONE, faint: NONE}, '×');
  } finally { await f.close(); }
});
