// Chromium check of the title confirmation in the Settings deletion: Delete for everyone
// waits for the exact title of the selected workspace, and the page sends no POST /delete
// while the button is disabled.
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {fixture} = require('./board_fixture.cjs');

test('jfz3m79d: Delete for everyone waits for the exact title of the workspace', async () => {
  const f = await fixture({note: {title: 'Notes', role: 'document', status: 'none', root: true, parents: ['*']}});
  try {
    const request = f.page.context().request;
    // Alex owns W and V. The titles are different and have letters in two cases.
    const make = async prefix => {
      const created = await request.post(f.live.root + '/api/workspaces', {headers: {Origin: f.live.root}, data: {title: prefix + ' ' + crypto.randomUUID()}});
      assert.equal(created.status(), 200);
      return created.json();
    };
    const w = await make('Walnut'), v = await make('Violet');
    assert.notEqual(w.title, v.title);
    // The request log keeps every POST /delete that the page sends.
    const deletes = [];
    f.page.on('request', r => { if (r.method() === 'POST' && new URL(r.url()).pathname.endsWith('/delete')) deletes.push(r.url()); });

    assert.equal((await f.page.goto(f.live.root + '/settings')).status(), 200);
    const select = f.page.getByLabel('Workspace', {exact: true});
    for (const id of [w.id, v.id]) await select.locator('option[value="' + id + '"]').waitFor({state: 'attached'});
    await select.selectOption(w.id);

    const box = f.page.locator('#delete-confirm');
    const field = f.page.locator('#delete-title');
    const button = f.page.locator('#delete-confirm-button');
    const keyboard = f.page.keyboard;
    const open = async () => {
      await f.page.getByRole('button', {name: 'Delete workspace'}).click();
      await box.waitFor({state: 'visible'});
    };
    // Each opening starts with an empty field and a disabled button.
    const fresh = async why => {
      await open();
      assert.equal(await field.inputValue(), '', why);
      assert.equal(await button.isDisabled(), true, why);
    };
    // Enter in the field and a click on the disabled button send no POST /delete.
    const quiet = async (count, why) => {
      await field.focus();
      await keyboard.press('Enter');
      if (await button.isDisabled()) await button.click({force: true});
      await f.page.waitForTimeout(300);
      assert.equal(deletes.length, count, why);
    };
    // Replace the value with one paste: one input event.
    const paste = async value => {
      await field.fill('');
      await field.focus();
      if (value) await keyboard.insertText(value);
      assert.equal(await field.inputValue(), value);
    };

    await fresh('first opening');
    // The confirmation shows the title of W as text outside the field, and not the title of V.
    const shown = await box.innerText();
    assert.ok(shown.includes(w.title), shown);
    assert.ok(!shown.includes(v.title), shown);
    assert.equal(await box.evaluate((node, title) => [node, ...node.querySelectorAll('*')]
      .some(e => e.tagName !== 'INPUT' && e.tagName !== 'TEXTAREA' && [...e.childNodes]
        .some(c => c.nodeType === 3 && c.textContent.includes(title))), w.title), true);
    // The field has the label "Type the workspace title to confirm".
    const labelled = f.page.getByLabel('Type the workspace title to confirm', {exact: true});
    assert.equal(await labelled.evaluate(e => e.id), 'delete-title');
    assert.equal(await button.innerText(), 'Delete for everyone');

    // keyboard.type: after each input event, the button is enabled only for the whole title.
    await field.focus();
    const letters = [...w.title];
    for (const [i, letter] of letters.entries()) {
      await keyboard.type(letter);
      assert.equal(await button.isDisabled(), i < letters.length - 1, 'after letter ' + i + ' of ' + w.title);
    }
    assert.equal(await field.inputValue(), w.title);
    // A trailing space disables the button. Backspace gives the exact title again.
    await keyboard.type(' ');
    assert.equal(await button.isDisabled(), true, 'trailing space typed');
    await quiet(0, 'Enter with a trailing space');
    await field.press('End');
    await keyboard.press('Backspace');
    assert.equal(await field.inputValue(), w.title);
    assert.equal(await button.isDisabled(), false, 'exact title after Backspace');

    // keyboard.insertText: values that are not exactly the title of W keep the button disabled.
    for (const [why, value] of [['empty value', ''], ['upper case', w.title.toUpperCase()],
      ['lower case', w.title.toLowerCase()], ['trailing space', w.title + ' '], ['title of V', v.title]]) {
      await paste(value);
      assert.equal(await button.isDisabled(), true, why);
      await quiet(0, 'Enter with ' + why);
    }
    // One paste of the whole title enables the button.
    await paste(w.title);
    assert.equal(await button.isDisabled(), false, 'one paste of the title');
    assert.equal(deletes.length, 0);

    // Cancel: the next opening starts empty.
    await f.page.getByRole('button', {name: 'Cancel'}).click();
    await box.waitFor({state: 'hidden'});
    await fresh('after Cancel');

    // A workspace change: the confirmation of V starts empty and wants the title of V.
    await paste(w.title);
    assert.equal(await button.isDisabled(), false);
    await select.selectOption(v.id);
    await fresh('after a change to V');
    const onV = await box.innerText();
    assert.ok(onV.includes(v.title) && !onV.includes(w.title), onV);
    await paste(w.title);
    assert.equal(await button.isDisabled(), true, 'title of W on V');
    await quiet(0, 'Enter with the title of W on V');
    await select.selectOption(w.id);
    await fresh('after a change back to W');

    // A failed delete: the button stays disabled while the request runs and the value stays.
    let release;
    const held = new Promise(resolve => { release = resolve; });
    await f.page.route('**/delete', async route => {
      await held;
      await route.fulfill({status: 500, contentType: 'application/json', body: JSON.stringify({error: 'delete unavailable'})});
    }, {times: 1});
    await paste(w.title);
    const sent = f.page.waitForRequest(r => r.method() === 'POST' && new URL(r.url()).pathname.endsWith('/delete'));
    await button.click();
    await sent;
    assert.equal(deletes.length, 1);
    assert.equal(await button.isDisabled(), true, 'while the delete request runs');
    // An input event and Enter while the request runs send no second request.
    await field.focus();
    await keyboard.type('x');
    await keyboard.press('Backspace');
    assert.equal(await button.isDisabled(), true, 'input while the delete request runs');
    await keyboard.press('Enter');
    await f.page.waitForTimeout(300);
    assert.equal(deletes.length, 1);
    release();
    await f.page.waitForFunction(() => document.getElementById('delete-error').textContent.trim() !== '');
    assert.equal(await field.inputValue(), w.title, 'a failed delete keeps the value');
    await f.page.waitForFunction(() => !document.getElementById('delete-confirm-button').disabled);
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + w.id)).status(), 200);
    assert.equal(deletes.length, 1);

    // A deletion: delete W for real, then open the confirmation of V.
    const deleted = f.page.waitForResponse(r => r.request().method() === 'POST' && r.url().endsWith('/delete'));
    await button.click();
    assert.equal((await deleted).status(), 200);
    assert.equal(deletes.length, 2);
    await select.locator('option[value="' + w.id + '"]').waitFor({state: 'detached'});
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + w.id)).status(), 404);
    await select.selectOption(v.id);
    await fresh('after a deletion');
    assert.ok((await box.innerText()).includes(v.title));
    await quiet(2, 'Enter with an empty field after a deletion');
    await paste(v.title);
    assert.equal(await button.isDisabled(), false, 'title of V on V');
    assert.equal(deletes.length, 2);
    assert.equal((await request.get(f.live.root + '/api/workspaces/' + v.id)).status(), 200);
  } finally { await f.close(); }
});
