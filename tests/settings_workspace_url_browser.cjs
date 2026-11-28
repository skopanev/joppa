// Chromium checks of ac:1i9zv5lm: /settings selects the workspace from ?workspace= and from the
// board #settings-link, updates sessionStorage and ?workspace= on switch, and falls back cleanly.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const {fixture} = require('./board_fixture.cjs');

/// Wait until #workspace-select is enabled and lists both `w1` and `w2`.
async function waitSettingsReady(page, w1, w2) {
  await page.waitForFunction(([a, b]) => {
    const s = document.getElementById('workspace-select');
    if (!s || s.disabled) return false;
    const ids = [...s.options].map(o => o.value);
    return ids.includes(a) && ids.includes(b);
  }, [w1, w2], {timeout: 15000});
}

/// Verify #hide-workspace starts checked, uncheck it, and confirm POST /hide targets `expectedId`.
async function uncheckHide(page, expectedId) {
  const box = page.locator('#hide-workspace');
  await box.waitFor({state: 'visible', timeout: 15000});
  await page.waitForFunction(() => {
    const el = document.getElementById('hide-workspace');
    return Boolean(el && !el.disabled && el.checked === true);
  }, null, {timeout: 5000}).catch(() => {});
  assert.equal(await box.isChecked(), true, '#hide-workspace must reflect the hidden workspace');
  const saved = page.waitForResponse(r => r.url().endsWith('/hide') && r.request().method() === 'POST', {timeout: 15000});
  await box.uncheck();
  const res = await saved;
  assert.equal(res.status(), 200);
  assert.ok(new URL(res.url()).pathname.endsWith('/api/workspaces/' + encodeURIComponent(expectedId) + '/hide'), res.url());
  assert.equal(await box.isChecked(), false);
}

/// Create an isolated member context belonging only to hidden workspaces `w1` and `w2` in list order.
async function setup() {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: []}});
  const member = await f.page.context().browser().newContext({
    reducedMotion: 'reduce', colorScheme: 'light', viewport: {width: 1280, height: 900},
  });
  try {
    const root = f.live.root, request = f.page.context().request, origin = {Origin: root};
    const create = async title => {
      const res = await request.post(root + '/api/workspaces', {headers: origin, data: {title}});
      assert.equal(res.status(), 200, await res.text());
      return (await res.json()).id;
    };
    const suffix = crypto.randomUUID().slice(0, 8);
    const a = await create('Shared plans ' + suffix), b = await create('Side plans ' + suffix);
    for (const id of [a, b]) {
      const added = await fetch(root + '/api/workspaces/' + encodeURIComponent(id) + '/commands', {
        method: 'POST',
        headers: {'Authorization': 'Bearer ' + f.live.alex, 'Content-Type': 'application/json'},
        body: JSON.stringify({request_id: crypto.randomUUID(), command: {op: 'member', id: f.live.maria, role: 'human'}}),
      });
      assert.equal(added.status, 200, await added.text());
      const granted = await request.post(root + '/api/email-grants', {
        headers: origin, data: {workspace: id, email: f.live.maria_email},
      });
      assert.equal(granted.status(), 200, await granted.text());
    }
    await member.addCookies([{name: 'joppa_session', value: f.live.maria_cookie, url: root}]);

    const readIds = async suffixPath => {
      const res = await member.request.get(root + suffixPath);
      assert.equal(res.status(), 200);
      const body = await res.json();
      return (body.items || body).map(item => item.id);
    };
    const boardList = () => readIds('/api/workspaces');
    const [w1, w2] = await readIds('/api/workspaces?all=true');
    assert.ok(w1 && w2 && w1 !== w2, 'expected two distinct listed workspaces');

    // Hide both W1 and W2 in a temporary tab so the test tab starts with empty sessionStorage.
    const initPage = await member.newPage();
    assert.equal((await initPage.goto(root + '/settings')).status(), 200);
    await waitSettingsReady(initPage, w1, w2);
    for (const id of [w1, w2]) {
      await initPage.locator('#workspace-select').selectOption(id);
      await initPage.waitForFunction(() => {
        const el = document.getElementById('hide-workspace');
        return Boolean(el && !el.disabled && el.checked === false);
      }, null, {timeout: 10000});
      const hid = initPage.waitForResponse(r => r.url().endsWith('/hide') && r.request().method() === 'POST', {timeout: 15000});
      await initPage.locator('#hide-workspace').check();
      assert.equal((await hid).status(), 200);
    }
    await initPage.close();

    assert.deepEqual(await readIds('/api/workspaces?all=true'), [w1, w2]);
    assert.deepEqual(await boardList(), []);

    const page = await member.newPage(), pageErrors = [];
    page.on('pageerror', error => pageErrors.push(error.message));
    const close = async () => {
      try { await member.close(); }
      finally { await f.close(); assert.deepEqual(pageErrors, []); }
    };
    return {page, root, w1, w2, boardList, close};
  } catch (error) {
    await member.close();
    await f.close();
    throw error;
  }
}

// ac:1i9zv5lm
// test:yzlcfyps
test('settings-workspace-url: /settings?workspace=W2 selects hidden W2, unhides W2 only, and updates URL and sessionStorage on switch', async () => {
  const s = await setup();
  try {
    assert.equal((await s.page.goto(s.root + '/settings?workspace=' + encodeURIComponent(s.w2))).status(), 200);
    await waitSettingsReady(s.page, s.w1, s.w2);
    const options = await s.page.locator('#workspace-select option').evaluateAll(list => list.map(o => o.value));
    assert.deepEqual(options, [s.w1, s.w2]);
    assert.equal(await s.page.locator('#workspace-select').inputValue(), s.w2);

    await uncheckHide(s.page, s.w2);
    assert.deepEqual(await s.boardList(), [s.w2]);

    await s.page.locator('#workspace-select').selectOption(s.w1);
    await s.page.waitForFunction(id => sessionStorage.getItem('joppa-workspace') === id
      && new URL(location.href).searchParams.get('workspace') === id, s.w1, {timeout: 5000}).catch(() => {});
    assert.equal(await s.page.evaluate(() => sessionStorage.getItem('joppa-workspace')), s.w1);
    assert.equal(await s.page.evaluate(() => new URL(location.href).searchParams.get('workspace')), s.w1);
    await s.page.waitForFunction(() => document.getElementById('hide-workspace')?.checked === true, null, {timeout: 5000}).catch(() => {});
    assert.equal(await s.page.locator('#hide-workspace').isChecked(), true);
  } finally { await s.close(); }
});

// ac:1i9zv5lm
// test:85s6r7pq
test('settings-workspace-url: board /?workspace=W2 stores W2 and links #settings-link to /settings?workspace=W2 to unhide W2', async () => {
  const s = await setup();
  try {
    assert.deepEqual(await s.boardList(), []);
    assert.equal((await s.page.goto(s.root + '/?workspace=' + encodeURIComponent(s.w2))).status(), 200);
    await s.page.waitForFunction(id => document.getElementById('workspaces')?.value === id, s.w2, {timeout: 15000});

    await s.page.waitForFunction(id => sessionStorage.getItem('joppa-workspace') === id, s.w2, {timeout: 5000}).catch(() => {});
    assert.equal(await s.page.evaluate(() => sessionStorage.getItem('joppa-workspace')), s.w2);

    const expectedHrefEnd = '/settings?workspace=' + encodeURIComponent(s.w2);
    await s.page.waitForFunction(end => document.getElementById('settings-link')?.href?.endsWith(end),
      expectedHrefEnd, {timeout: 5000}).catch(() => {});
    const href = await s.page.locator('#settings-link').evaluate(a => a.href);
    assert.ok(href.endsWith(expectedHrefEnd), 'expected #settings-link href to end with ' + expectedHrefEnd + ', got ' + href);

    const link = s.page.locator('#settings-link');
    if (!(await link.isVisible())) {
      const menuTrigger = s.page.locator('#account-button, [aria-controls="account-menu"], details:has(#settings-link) > summary, .app-header button[aria-expanded="false"]').first();
      if (await menuTrigger.count()) await menuTrigger.click();
    }
    await link.waitFor({state: 'visible', timeout: 10000});
    await link.click();

    await s.page.waitForURL(url => url.pathname === '/settings', {timeout: 15000});
    assert.equal(new URL(s.page.url()).searchParams.get('workspace'), s.w2);
    await waitSettingsReady(s.page, s.w1, s.w2);
    assert.equal(await s.page.locator('#workspace-select').inputValue(), s.w2);

    await uncheckHide(s.page, s.w2);
    assert.deepEqual(await s.boardList(), [s.w2]);

    const listed = s.page.waitForResponse(r => new URL(r.url()).pathname === '/api/workspaces' && r.request().method() === 'GET', {timeout: 15000});
    assert.equal((await s.page.goto(s.root + '/')).status(), 200);
    await listed;
    await s.page.waitForFunction(id => [...(document.getElementById('workspaces')?.options || [])].some(o => o.value === id), s.w2, {timeout: 15000});
    assert.deepEqual(await s.page.locator('#workspaces option').evaluateAll(list => list.map(o => o.value)), [s.w2]);
  } finally { await s.close(); }
});

// ac:1i9zv5lm
// test:fz6o8ssp
test('settings-workspace-url: unlisted or absent ?workspace= falls back to sessionStorage or the first listed workspace', async () => {
  const s = await setup();
  try {
    assert.equal((await s.page.goto(s.root + '/api/me')).status(), 200);
    await s.page.evaluate(id => sessionStorage.setItem('joppa-workspace', id), s.w2);
    assert.equal((await s.page.goto(s.root + '/settings?workspace=no-such-workspace')).status(), 200);
    await waitSettingsReady(s.page, s.w1, s.w2);
    assert.equal(await s.page.locator('#workspace-select').inputValue(), s.w2);

    assert.equal((await s.page.goto(s.root + '/api/me')).status(), 200);
    await s.page.evaluate(id => sessionStorage.setItem('joppa-workspace', id), s.w2);
    assert.equal((await s.page.goto(s.root + '/settings')).status(), 200);
    await waitSettingsReady(s.page, s.w1, s.w2);
    assert.equal(await s.page.locator('#workspace-select').inputValue(), s.w2);

    assert.equal((await s.page.goto(s.root + '/api/me')).status(), 200);
    await s.page.evaluate(() => sessionStorage.removeItem('joppa-workspace'));
    assert.equal(await s.page.evaluate(() => sessionStorage.getItem('joppa-workspace')), null);
    assert.equal((await s.page.goto(s.root + '/settings?workspace=no-such-workspace')).status(), 200);
    await waitSettingsReady(s.page, s.w1, s.w2);
    assert.deepEqual(await s.page.locator('#workspace-select option').evaluateAll(list => list.map(o => o.value)), [s.w1, s.w2]);
    assert.equal(await s.page.locator('#workspace-select').inputValue(), s.w1);

    assert.equal((await s.page.goto(s.root + '/api/me')).status(), 200);
    await s.page.evaluate(() => sessionStorage.setItem('joppa-workspace', 'other-unlisted'));
    assert.equal((await s.page.goto(s.root + '/settings')).status(), 200);
    await waitSettingsReady(s.page, s.w1, s.w2);
    assert.equal(await s.page.locator('#workspace-select').inputValue(), s.w1);
  } finally { await s.close(); }
});
