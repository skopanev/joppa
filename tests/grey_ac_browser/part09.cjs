// Part 9 of tests/grey_ac_browser.cjs: tests that its describe block registers, so their names stay unchanged.
/* eslint-disable no-unused-vars */
const {test} = require('node:test');
const assert = require('node:assert/strict');
const {spawn, spawnSync} = require('node:child_process');
const {createHash} = require('node:crypto');
const {writeFileSync} = require('node:fs');
const {once} = require('node:events');
const http = require('node:http');
const path = require('node:path');

module.exports = shared => {
  const {server, fixture, criterionRecords, types, root, titlePoint, markdownTypes, markdownBody} = shared;

test('compact-open-object-ac5: rows are separated without dashed lines', async (t) => {
  const f = await fixture({'domain:X': {title: 'Domain X'}, 'capability:Y': {title: 'Capability Y', parent: 'domain:X'},
    'requirement:R1': {title: 'Requirement one', parent: 'capability:Y'}, 'requirement:R2': {title: 'Requirement two', parent: 'capability:Y'},
    'requirement:R3': {title: 'Requirement three', parent: 'capability:Y'}});
  const row = card => card.locator(':scope > summary');
  const ready = card => card.locator(':scope > .object-body > .object-secondary').waitFor({state: 'attached'});
  const rules = {light: 'rgb(217, 211, 199)', dark: 'rgb(68, 73, 81)'};
  try {
    await f.page.goto(f.live.root + '/?workspace=' + f.live.ws);
    const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
    await X.waitFor();
    await row(X).click({position: {x: 4, y: 4}}); await ready(X);
    const Y = X.locator(':scope > .object-body > .object-children > .common-object[data-object-address="capability:Y"]');
    await Y.waitFor();
    await row(Y).click({position: {x: 4, y: 4}}); await ready(Y);
    const requirements = Y.locator(':scope > .object-body > .object-children > .common-object[data-object-address^="requirement:"]');
    await requirements.nth(2).waitFor();
    await Promise.all(f.requests.map(entry => entry.body));
    await f.page.evaluate(() => {
      const name = node => node.tagName.toLowerCase() + (node.id ? '#' + node.id : '') + [...node.classList].map(value => '.' + value).join('') +
        (node.dataset?.objectAddress ? '[' + node.dataset.objectAddress + ']' : '');
      const lines = [...['Top', 'Right', 'Bottom', 'Left'].map(side => ['border-' + side.toLowerCase() + '-style', 'border' + side + 'Style', 'border' + side + 'Width']),
        ['outline-style', 'outlineStyle', 'outlineWidth'], ['text-decoration-style', 'textDecorationStyle', 'textDecorationThickness']];
      // A pseudo-element without content draws nothing, so its outline cannot show focus.
      const drawn = (style, pseudo) => !pseudo || !['none', 'normal'].includes(style.content);
      const indicator = (node, pseudo) => {
        const style = getComputedStyle(node, pseudo || null), label = name(node) + pseudo;
        if (!drawn(style, pseudo)) return null;
        if (style.outlineStyle === 'solid' && parseFloat(style.outlineWidth) >= 1) return label + ' solid outline';
        if (style.textDecorationLine.includes('underline') && style.textDecorationStyle === 'solid') return label + ' solid underline';
        return null;
      };
      window.ac5 = {visited: new Set(),
        // Every element and pseudo-element on the page in its current focus state.
        scan: where => {
          const found = []; let checked = 0;
          for (const node of document.querySelectorAll('*')) for (const pseudo of ['', '::before', '::after', '::marker']) {
            const style = getComputedStyle(node, pseudo || null); checked++;
            for (const [property, key, width] of lines) if (style[key] === 'dashed') found.push(where + ': ' + name(node) + pseudo + ' ' + property + ' dashed ' + style[width]);
          }
          return {found, checked};
        },
        focus: () => {
          const node = document.activeElement;
          if (!node || node === document.body) return null;
          const repeat = ac5.visited.has(node); ac5.visited.add(node);
          const candidates = [[node, ''], [node, '::before'], [node, '::after'], ...[...node.querySelectorAll(':scope > .node-title')].map(title => [title, ''])];
          return {name: name(node), repeat, indicator: candidates.map(([element, pseudo]) => indicator(element, pseudo)).find(Boolean) || null};
        },
        missing: () => [...document.querySelectorAll('a[href],button,input,select,textarea,summary,[tabindex]')]
          .filter(node => node.tabIndex >= 0 && !node.disabled && !node.closest('[inert]') && node.checkVisibility() && !ac5.visited.has(node)).map(name),
        separators: () => {
          const probe = document.createElement('div'); probe.style.color = 'var(--rule)'; document.body.append(probe);
          const rule = getComputedStyle(probe).color; probe.remove();
          return {rule, rows: [...document.querySelectorAll('.common-object[data-object-address^="requirement:"]')].map(node => {
            const style = getComputedStyle(node);
            return {name: name(node), open: node.open, style: style.borderBottomStyle, width: parseFloat(style.borderBottomWidth), color: style.borderBottomColor};
          })};
        }};
    });
    const failures = [];
    let scans = 0, steps = 0;
    for (const [width, height] of [[1280, 720], [390, 844]]) {
      await f.page.setViewportSize({width, height});
      for (const [scheme, background] of [['light', 'rgb(255, 252, 245)'], ['dark', 'rgb(25, 28, 32)']]) {
        const where = 'width ' + width + ', ' + scheme;
        await f.page.emulateMedia({colorScheme: scheme});
        await f.page.mouse.move(0, 0);
        assert.equal(await f.page.evaluate(() => getComputedStyle(document.body).backgroundColor), background, where);
        assert.equal(await X.getAttribute('open'), '', 'X is open, ' + where); assert.equal(await Y.getAttribute('open'), '', 'Y is open, ' + where);
        const separators = await f.page.evaluate(() => ac5.separators());
        assert.equal(separators.rule, rules[scheme], 'the --rule colour of the ' + where + ' theme');
        assert.equal(separators.rows.length, 3, where);
        for (const separator of separators.rows) {
          const text = where + ': ' + separator.name + ' bottom border ' + separator.style + ' ' + separator.width + 'px ' + separator.color;
          t.diagnostic(text);
          if (separator.open || !(separator.style === 'none' ||
            (separator.style === 'solid' && separator.width <= 1 && separator.color === separators.rule))) failures.push(text);
        }
        const idle = await f.page.evaluate(where => ac5.scan(where), where);
        failures.push(...idle.found); scans += idle.checked;
        // Tab from the current position until the focus returns to a visited element.
        await f.page.evaluate(() => { ac5.visited.clear(); });
        let focused = 0;
        for (let press = 0; press < 400; press++) {
          await f.page.keyboard.press('Tab');
          const state = await f.page.evaluate(() => ac5.focus());
          if (!state) continue;
          if (state.repeat) break;
          focused++; steps++;
          const text = where + ', focus ' + state.name;
          if (state.indicator) t.diagnostic(text + ': ' + state.indicator);
          const scan = await f.page.evaluate(where => ac5.scan(where), text);
          failures.push(...scan.found); scans += scan.checked;
        }
        assert.ok(focused > 0, 'Tab reaches the board, ' + where);
        assert.deepEqual(await f.page.evaluate(() => ac5.missing()), [], 'Tab visits every focusable board element, ' + where);
        await f.page.evaluate(() => document.activeElement?.blur());
      }
    }
    t.diagnostic(failures.length + ' dashed or invalid styles in ' + scans + ' element and pseudo-element states over ' + steps + ' focus steps');
    assert.deepEqual(failures, [], 'no board element or pseudo-element shows a dashed line');
  } finally { await f.close(); }
});

test('no-dashed-focus-ac2: keyboard focus draws no outline frame on the board and the tree', async (t) => {
  const f = await fixture({'domain:X': {title: 'Domain X'}, 'capability:Y': {title: 'Capability Y', parent: 'domain:X'}});
  const themes = {light: {background: 'rgb(255, 252, 245)', link: 'rgb(0, 92, 239)'}, dark: {background: 'rgb(25, 28, 32)', link: 'rgb(140, 181, 255)'}};
  const X = f.page.locator('#tree .common-object[data-object-address="domain:X"]');
  const pages = {
    board: {path: '/?workspace=', mouse: X.locator(':scope > summary .copy'), ready: async () => {
      await X.waitFor();
      if (await X.getAttribute('open') === null) await X.locator(':scope > summary').click({position: {x: 4, y: 4}});
      await X.locator(':scope > .object-body > .object-children > .common-object[data-object-address="capability:Y"]').waitFor();
    }},
    tree: {path: '/tree?workspace=', mouse: f.page.locator('#types .type[data-kind="capability"]'),
      ready: () => f.page.locator('#types .type[data-kind="ac"]').waitFor()},
  };
  const install = () => {
    const name = node => node.tagName.toLowerCase() + (node.id ? '#' + node.id : '') + [...node.classList].map(value => '.' + value).join('') +
      (node.dataset?.objectAddress ? '[' + node.dataset.objectAddress + ']' : '') + (node.dataset?.kind ? '[' + node.dataset.kind + ']' : '');
    const colour = value => {
      const probe = document.createElement('div'); probe.style.color = value; document.body.append(probe);
      const resolved = getComputedStyle(probe).color; probe.remove(); return resolved;
    };
    // A pseudo-element without content draws nothing, so it cannot show focus.
    const parts = node => ['', '::before', '::after'].map(pseudo => [pseudo, getComputedStyle(node, pseudo || null)])
      .filter(([pseudo, style]) => !pseudo || !['none', 'normal'].includes(style.content));
    const measure = style => ({outline: style.outlineStyle + ' ' + style.outlineWidth + ' ' + style.outlineColor, boxShadow: style.boxShadow,
      border: ['Top', 'Right', 'Bottom', 'Left'].map(side => style['border' + side + 'Style']).join(' '),
      textDecoration: style.textDecorationLine + ' ' + style.textDecorationStyle + ' ' + style.textDecorationColor});
    const shadow = (style, link) => style.boxShadow !== 'none' && style.boxShadow.split(/,(?![^(]*\))/).some(part =>
      part.includes(link) && (part.replace(link, '').match(/-?[\d.]+px/g) || []).some(length => parseFloat(length) !== 0));
    const mark = (node, link) => parts(node).map(([pseudo, style]) =>
      style.outlineStyle === 'solid' && parseFloat(style.outlineWidth) >= 1 && style.outlineColor === link ? pseudo + ' solid outline' :
      shadow(style, link) ? pseudo + ' solid box-shadow' : null).find(Boolean) || null;
    window.ac2 = {visited: [],
      link: () => colour('var(--link)'),
      focus: () => {
        const node = document.activeElement;
        if (!node || node === document.body) return null;
        const repeat = ac2.visited.includes(node), first = ac2.visited[0] === node;
        if (!repeat) ac2.visited.push(node);
        const link = ac2.link(), measured = parts(node).map(([pseudo, style]) => ({pseudo, ...measure(style)}));
        const dashed = parts(node).flatMap(([pseudo, style]) => [['outline', style.outlineStyle], ['text-decoration', style.textDecorationStyle],
          ...['Top', 'Right', 'Bottom', 'Left'].map(side => ['border-' + side.toLowerCase(), style['border' + side + 'Style']])]
          .filter(([, value]) => value === 'dashed').map(([property]) => pseudo + ' ' + property + ' dashed'));
        const frame = parts(node).some(([, style]) => style.outlineStyle !== 'none');
        return {name: name(node), repeat, first, link, mark: mark(node, link), frame, measured, dashed};
      },
      visits: selector => [...document.querySelectorAll(selector)].map(node => ({name: name(node), visited: ac2.visited.includes(node)})),
      missing: () => [...document.querySelectorAll('a[href],button,input,select,textarea,summary,[tabindex]')]
        .filter(node => node.tabIndex >= 0 && !node.disabled && !node.closest('[inert]') && node.checkVisibility() && !ac2.visited.includes(node)).map(name),
      pointer: () => {
        const node = document.activeElement, link = ac2.link();
        return {name: name(node), visible: node.matches(':focus-visible'), mark: mark(node, link),
          outline: parts(node).some(([, style]) => style.outlineStyle !== 'none' || style.boxShadow !== 'none'), measured: measure(getComputedStyle(node))};
      }};
  };
  try {
    const failures = [];
    let steps = 0;
    for (const [page, setup] of Object.entries(pages)) {
      const response = await f.page.goto(f.live.root + setup.path + f.live.ws);
      assert.equal(response.status(), 200, page);
      await setup.ready();
      await Promise.all(f.requests.map(entry => entry.body));
      await f.page.evaluate(install);
      for (const [scheme, theme] of Object.entries(themes)) {
        const where = page + ', ' + scheme;
        await f.page.evaluate(scheme => { document.documentElement.dataset.theme = scheme; document.activeElement?.blur(); ac2.visited = []; }, scheme);
        await f.page.mouse.move(0, 0);
        assert.equal(await f.page.evaluate(() => getComputedStyle(document.body).backgroundColor), theme.background, 'the ' + where + ' theme is active');
        assert.equal(await f.page.evaluate(() => ac2.link()), theme.link, 'the --link colour of the ' + where + ' theme');
        // Tab from the page start until the focus returns to the first control.
        let focused = 0, returned = false;
        for (let press = 0; press < 400; press++) {
          await f.page.keyboard.press('Tab');
          const state = await f.page.evaluate(() => ac2.focus());
          if (!state) continue;
          if (state.repeat) { returned = state.first; if (!returned) failures.push(where + ': the focus returns to ' + state.name + ', not the first control'); break; }
          focused++; steps++;
          const text = where + ', focus ' + state.name + ': ' + JSON.stringify(state.measured);
          t.diagnostic(text + (state.mark ? ', ' + state.mark : ''));
          if (state.frame) failures.push(text + ': focus draws an outline frame');
          if (state.dashed.length) failures.push(text + ': ' + state.dashed.join(', '));
        }
        assert.ok(focused > 0, 'Tab reaches a control, ' + where);
        assert.ok(returned, 'the focus returns to the first control, ' + where);
        assert.deepEqual(await f.page.evaluate(() => ac2.missing()), [], 'Tab visits every focusable control, ' + where);
        if (page === 'tree') {
          const visits = await f.page.evaluate(() => ac2.visits('.schema-scroll, #types .type'));
          assert.ok(visits.length >= 6, 'the tree shows the diagram and its type cards, ' + where);
          assert.deepEqual(visits.filter(visit => !visit.visited), [], 'Tab visits the diagram and every type card, ' + where);
        }
        // Mouse focus shows no mark.
        await setup.mouse.click();
        const pointer = await f.page.evaluate(() => ac2.pointer());
        t.diagnostic(where + ', mouse focus ' + JSON.stringify(pointer));
        if (pointer.visible || pointer.mark || pointer.outline) failures.push(where + ': mouse focus on ' + pointer.name + ' shows a mark ' + JSON.stringify(pointer.measured));
      }
    }
    t.diagnostic(failures.length + ' focus defects over ' + steps + ' focus steps');
    assert.deepEqual(failures, [], 'every keyboard focus shows a solid mark in the link colour and no dashed line');
  } finally { await f.close(); }
});
};
