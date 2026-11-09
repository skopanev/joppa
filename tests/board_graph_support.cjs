// The fixture and the helpers of the board graph checks (board_graph_browser.cjs).
const assert = require('node:assert/strict');
const {testType} = require('./board_fixture.cjs');

// Domains pay and ship. Criterion c depends on b, and b depends on a. Criteria d and e have no
// link. Maria owns c, only the title of a has the word Zebra, a has the tag swap and d the tag
// label. The schema offers the filter field tags.
const world = {
  types: {
    domain: {title: 'Domains', role: 'group', status: 'children', root: true, parents: []},
    ac: {title: 'Criteria', role: 'ac', status: 'children', parents: ['domain']},
    test: testType(['ac']),
  },
  async build(f) {
    await f.command({op: 'member', id: 'maria', role: 'human'}, f.live.owner);
    const tree = await f.api('/tree');
    await f.command({op: 'set_tree_schema', revision: tree.current_revision, filters: ['tags'], reason: 'Offer the tags filter'}, f.live.owner);
    await f.save('domain:pay', 'Payments', null, 'alex');
    await f.save('domain:ship', 'Shipping', null, 'alex');
    const criterion = (id, title, parent, owner, target, tags) => f.command({op: 'save_object', kind: 'ac', id, revision: 0, parent,
      data: {title, owner, body: 'Fixture text', ...(tags ? {tags} : {})}, reason: 'Define the graph fixture',
      ...(target ? {relations: {depends_on: {edges: [{to: target}]}}} : {})});
    await criterion('a', 'Zebra card capture', 'domain:pay', 'alex', null, ['swap']);
    await criterion('b', 'Refund the capture', 'domain:pay', 'alex', 'ac:a');
    await criterion('c', 'Ship after the refund', 'domain:ship', 'maria', 'ac:b');
    await criterion('d', 'Print the label', 'domain:ship', 'alex', null, ['label']);
    await criterion('e', 'Show the receipt', 'domain:pay', 'alex');
    await f.addTest('ac:a', 'tests/graph.rs > a');
    await f.mcp('joppa_record_results', {commit: 'c0ffee5', results: [{test: 'tests/graph.rs > a', status: 'pass'}]});
  },
};

const ALL = ['ac:a', 'ac:b', 'ac:c', 'ac:d', 'ac:e', 'domain:pay', 'domain:ship', 'test:'];

// Open the graph with a query and wait for the layout to rest.
async function open(f, query) {
  await f.page.goto(f.live.root + '/?workspace=' + encodeURIComponent(f.live.ws) + '&view=graph' + query);
  await f.page.locator('.graph-canvas[data-state=rest]').waitFor();
}

// The addresses of the hidden, lit and faint dots, sorted. A test node counts as `test:`.
const read = () => {
  const name = d => d.dataset.address.startsWith('test:') ? 'test:' : d.dataset.address;
  const of = cls => [...document.querySelectorAll('.graph-node.' + cls)].map(name).sort();
  return {hidden: of('graph-hidden'), lit: of('graph-lit'), faint: of('graph-faint')};
};
const classes = f => f.page.evaluate(read);
async function waitClasses(f, wanted, step) {
  const sorted = Object.fromEntries(Object.entries(wanted).map(([k, v]) => [k, [...v].sort()]));
  await f.page.waitForFunction(([read, wanted]) => JSON.stringify(eval(read)()) === wanted, [String(read), JSON.stringify(sorted)])
    .catch(async () => assert.fail(step + ': ' + JSON.stringify(await classes(f))));
}

// True when the centre of each named dot is inside the view box of the canvas.
function inView(f, addresses) {
  return f.page.evaluate(addresses => {
    const [x, y, w, h] = document.querySelector('.graph-canvas').getAttribute('viewBox').split(' ').map(Number);
    return [...document.querySelectorAll('.graph-node')].filter(d => addresses.includes(d.dataset.address.startsWith('test:') ? 'test:' : d.dataset.address))
      .every(d => {const cx = Number(d.getAttribute('cx')), cy = Number(d.getAttribute('cy'));return cx >= x && cx <= x + w && cy >= y && cy <= y + h;});
  }, addresses);
}

module.exports = {world, ALL, open, classes, waitClasses, inView};
