// A workspace selection binds every response and error to that selection. An older read never
// changes the board of a newer selection, and the board of the old selection is not usable.
const {test, before, after} = require('node:test');
const assert = require('node:assert/strict');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const {server, domainTree, workspaces, shows} = require('./switch_support.cjs');

let live, browser, setup;
before(async () => {
  assert.ok(process.env.JOPPA_TEST_DATABASE_URL, 'isolated test database required');
  live = await server();
  browser = await chromium.launch({headless: true});
  // 5 domains with one capability each, so a domain row can open.
  setup = await workspaces(live, browser, domainTree([1]));
});
after(async () => { await browser?.close(); await live?.stop(); });

/// A new page on `start`, with a log of the API reads and the page errors.
async function open(start) {
  const page = await setup.context.newPage(), reads = [], errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('request', request => { if (request.url().includes('/api/')) reads.push(new URL(request.url())); });
  await page.goto(live.root + '/' + (start ? '?workspace=' + encodeURIComponent(start.ws) : ''));
  if (start) await shows(page, setup.expect(start));
  return {page, reads, errors};
}

/// Hold the board read of `target` until the returned function runs. Its argument replaces the response.
async function hold(page, target) {
  let release;
  const released = new Promise(resolve => { release = resolve; });
  const pattern = live.root + '/api/workspaces/' + encodeURIComponent(target.ws) + '/board';
  await page.route(pattern, async route => {
    const reply = await released;
    await (reply ? route.fulfill(reply) : route.continue());
  });
  return async reply => { release(reply); await page.waitForTimeout(500); await page.unroute(pattern); };
}

const select = (page, target) => page.locator('#workspaces').selectOption(target.ws);

test('a late 404 of an older selection keeps the board of the newer selection', async () => {
  const {big, other, third, expect} = setup;
  const {page, errors} = await open(other);
  const finish = await hold(page, third);
  await select(page, third);
  await select(page, big);
  await shows(page, expect(big));
  await finish({status: 404, json: {error: 'workspace not found'}});
  await shows(page, expect(big));
  assert.equal(await page.getByText('This workspace is no longer available').count(), 0, 'the old 404 shows no message on the new board');
  assert.equal(await page.locator('#workspaces').inputValue(), big.ws);
  assert.deepEqual(errors, []);
  await page.close();
});

test('the rows of the old selection are not usable while the new board loads', async () => {
  const {big, other, expect} = setup;
  const {page, reads, errors} = await open(big);
  const finish = await hold(page, other);
  await select(page, other);
  await page.waitForTimeout(300);
  const usable = await page.evaluate(() => [...document.querySelectorAll('#tree .common-object[data-object-address^="domain:d"]')]
    .filter(row => row.checkVisibility() && !row.closest('[inert]')).map(row => row.dataset.objectAddress));
  assert.deepEqual(usable, [], 'no row of the old selection is visible and usable');
  // A click on an old row, when one is still there, must not read its children under the new selection.
  const old = page.locator('#tree .common-object[data-object-address="domain:d1"] > summary');
  if (await old.count()) await old.click({force: true, timeout: 2000}).catch(() => {});
  await finish();
  await shows(page, expect(other));
  const mixed = reads.filter(url => url.pathname.includes('/' + other.ws + '/') && /domain:d\d/.test(url.search));
  assert.deepEqual(mixed.map(String), [], 'no read of the new workspace asks for a node of the old workspace');
  assert.deepEqual(errors, []);
  await page.close();
});

test('Back after the first entry without a workspace returns to the first selection', async () => {
  const {big, other, third, expect} = setup;
  const {page, errors} = await open();
  await page.locator('#workspaces option').nth(2).waitFor({state: 'attached'});
  const chosen = await page.locator('#workspaces').inputValue();
  const start = [big, other, third].find(target => target.ws === chosen);
  assert.ok(start, 'the list selects one fixture workspace');
  await shows(page, expect(start));
  const next = [big, other, third].find(target => target !== start);
  await select(page, next);
  await shows(page, expect(next));
  await page.goBack();
  await shows(page, expect(start));
  assert.equal(await page.locator('#workspaces').inputValue(), start.ws, 'the selector shows the first selection again');
  assert.deepEqual(errors, []);
  await page.close();
});

test('fast selections end on the last selection, also when an older read ends later', async () => {
  const {big, other, third, expect} = setup;
  const {page, errors} = await open(other);
  for (const [slow, last] of [[big, third], [third, big]]) {
    const finish = await hold(page, slow);
    await select(page, slow);
    await select(page, last);
    await shows(page, expect(last));
    await finish();
    await shows(page, expect(last));
  }
  for (const target of [other, big, third, big]) await select(page, target);
  await page.waitForTimeout(1000);
  await shows(page, expect(big));
  assert.equal(new URL(page.url()).searchParams.get('workspace'), big.ws, 'the address names the last selection');
  assert.equal(await page.locator('#workspaces').inputValue(), big.ws, 'the selector shows the last selection');
  assert.deepEqual(errors, []);
  await page.close();
});
