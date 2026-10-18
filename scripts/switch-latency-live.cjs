// [ac:714c82e6-c4e5-4330-ab59-635505bd8166] The live workspace switch measurement on the host.
// A visible Chrome opens the board. The owner signs in with Google in that window. Then the same
// window makes 20 timed selections of the target workspace and one fast selection check.
// The run sends only GET reads to the board and the two sign-in calls, and it lists any other
// request. It saves no cookie, no storage state and no header. Closing the browser ends the session.
//
// JOPPA_LIVE_BUILD=f85839e JOPPA_SWITCH_OUT=<file> PLAYWRIGHT_MODULE=<playwright> node scripts/switch-latency-live.cjs
const assert = require('node:assert/strict');
const {writeFileSync} = require('node:fs');
const os = require('node:os');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const {installProbe, timedRun, summary, measureRuns} = require('../tests/switch_probe.cjs');

const root = process.env.JOPPA_LIVE_ROOT || 'https://joppa.otion.us';
const target = {ws: process.env.JOPPA_LIVE_WORKSPACE || 'joppa',
  rows: (process.env.JOPPA_LIVE_ROWS || 'domain:agents,domain:evidence,domain:inbox,domain:owner,domain:specification').split(',')};
const expectedBuild = process.env.JOPPA_LIVE_BUILD, out = process.env.JOPPA_SWITCH_OUT;
const RUNS = 20, SIGN_IN_MS = 10 * 60 * 1000;
const allowed = new Set(['/auth/login', '/auth/poll']);

async function main() {
  assert.ok(expectedBuild && out, 'set JOPPA_LIVE_BUILD and JOPPA_SWITCH_OUT');
  const browser = await chromium.launch({channel: 'chrome', headless: false});
  const report = {label: 'production', root, workspace: target.ws, expected_rows: target.rows, expected_build: expectedBuild,
    browser: 'Google Chrome ' + browser.version(), host: os.hostname(), writes: []};
  try {
    const context = await browser.newContext({viewport: {width: 1440, height: 1000}});
    // The run itself sends no write. Every other board request than a read or a sign-in call is
    // listed in the report. A route would block it, but a route also slows every timed read.
    context.on('request', request => {
      const url = new URL(request.url());
      if (url.origin === new URL(root).origin && !['GET', 'HEAD'].includes(request.method()) && !allowed.has(url.pathname))
        report.writes.push(request.method() + ' ' + url.pathname);
    });
    const page = await context.newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(installProbe);
    // The build stamp of the served page, before its script turns the stamp into a label.
    const response = await page.goto(root + '/?workspace=' + encodeURIComponent(target.ws));
    report.build = (await response.text()).match(/data-build="([^"]*)"/)?.[1];
    assert.ok(report.build?.endsWith(' ' + expectedBuild), 'the page build ' + report.build + ' is not ' + expectedBuild);
    console.log('Sign in with Google in the open Chrome window. The run waits up to 10 minutes.');
    await page.waitForFunction(() => document.body.classList.contains('authenticated'), null, {timeout: SIGN_IN_MS});
    console.log('Signed in. The measurement starts. Do not use the window until it closes.');

    const shown = async (wanted, timeout = 30000) => page.waitForFunction(wanted => wanted.rows.every(address =>
      document.querySelector('#tree .common-object[data-object-address="' + address + '"]')?.checkVisibility()), wanted, {timeout});
    // Another workspace is shown when its selection is current, no target row is left and its
    // board has drawn rows or an empty collection.
    const foreign = new Set();
    const awayShown = async away => {
      await page.waitForFunction(({away, rows}) => document.getElementById('workspaces').value === away.ws
        && !rows.some(address => document.querySelector('#tree [data-object-address="' + address + '"]'))
        && document.querySelector('#tree .common-object, #tree .collection-empty, #tree .empty-state'), {away, rows: target.rows}, {timeout: 30000});
      await page.waitForTimeout(300);
      for (const address of await page.evaluate(() => [...document.querySelectorAll('#tree .common-object')].map(row => row.dataset.objectAddress)))
        if (!target.rows.includes(address)) foreign.add(address);
    };
    await shown(target);
    const options = await page.locator('#workspaces option').evaluateAll(list => list.map(option => option.value));
    const aways = options.filter(id => id && id !== target.ws).slice(0, 2).map(ws => ({ws}));
    assert.ok(aways.length, 'the account needs a second workspace to switch from');
    report.away_workspaces = aways.length;
    for (const away of aways) { await page.locator('#workspaces').selectOption(away.ws); await awayShown(away); }

    // The network path: the connection of the first page and five uncached health reads.
    report.network = await page.evaluate(async () => {
      const [navigation] = performance.getEntriesByType('navigation'), health = [];
      for (let i = 0; i < 5; i++) { const start = performance.now(); await fetch('/health', {cache: 'no-store'}); health.push(+(performance.now() - start).toFixed(1)); }
      return {protocol: navigation.nextHopProtocol, connect_ms: +(navigation.connectEnd - navigation.connectStart).toFixed(1),
        tls_ms: navigation.secureConnectionStart ? +(navigation.connectEnd - navigation.secureConnectionStart).toFixed(1) : null,
        health_ms: health};
    });

    report.runs = [];
    await measureRuns(RUNS, async run => {
      const away = aways[(run - 1) % aways.length];
      const wanted = {ws: target.ws, rows: target.rows, foreign: [...foreign]};
      const result = await timedRun(page, away, target, wanted, awayShown);
      return {ms: +result.ms.toFixed(1), resources: result.resources};
    }, report.runs);

    // Fast selections: an older read of the target ends after a newer selection, then four
    // selections without a wait end on the target.
    const fast = {};
    try {
      const away = aways[0], pattern = root + '/api/workspaces/' + encodeURIComponent(target.ws) + '/board';
      await page.route(pattern, async route => { await new Promise(resolve => setTimeout(resolve, 1500)); await route.continue(); });
      await page.locator('#workspaces').selectOption(target.ws);
      await page.locator('#workspaces').selectOption(away.ws);
      await page.waitForTimeout(2500);
      await page.unroute(pattern);
      await awayShown(away);
      fast.late_read = 'the newer selection stayed after the older read ended';
    } catch (error) { fast.late_read = 'FAILED: ' + error.message.split('\n')[0]; }
    try {
      for (const id of [...aways.map(away => away.ws), target.ws, aways[0].ws, target.ws]) await page.locator('#workspaces').selectOption(id);
      await shown(target);
      await page.waitForTimeout(1500);
      await shown(target, 1000);
      const url = new URL(page.url()).searchParams.get('workspace'), value = await page.locator('#workspaces').inputValue();
      fast.last_selection = url === target.ws && value === target.ws ? 'the board, the address and the selector show the last selection'
        : 'FAILED: address ' + url + ', selector ' + value;
    } catch (error) { fast.last_selection = 'FAILED: ' + error.message.split('\n')[0]; }
    report.fast_switch = fast;
    report.page_errors = errors;
    await context.close();
  } finally {
    await browser.close();
    if (report.runs) Object.assign(report, summary(report.runs));
    report.failures = failures(report);
    writeFileSync(out, JSON.stringify(report, null, 1) + '\n');
  }
  console.log(JSON.stringify({build: report.build, sorted_ms: report.sorted_ms, p95_ms: report.p95_ms,
    failed_runs: report.failed_runs, fast_switch: report.fast_switch, writes: report.writes, failures: report.failures}));
  if (report.failures.length) process.exitCode = 1;
}

/// The criteria of the run. Each failed criterion gives one line. The report is saved first.
function failures(report) {
  const list = [];
  if ((report.runs || []).length !== RUNS) list.push('runs: ' + (report.runs || []).length + ' of ' + RUNS);
  if (report.failed_runs) list.push('failed runs: ' + report.failed_runs);
  if (report.p95_ms === null || report.p95_ms === undefined || report.p95_ms > 1000) list.push('p95 ' + report.p95_ms + ' ms is not at most 1000 ms');
  for (const [check, result] of Object.entries(report.fast_switch || {})) if (result.startsWith('FAILED')) list.push(check + ': ' + result);
  if (!report.fast_switch) list.push('fast selections: not run');
  if (!report.page_errors || report.page_errors.length) list.push('page errors: ' + JSON.stringify(report.page_errors ?? 'not read'));
  if (report.writes.length) list.push('board requests other than reads: ' + report.writes.join(', '));
  return list;
}

main().catch(error => { console.error(error); process.exitCode = 1; });
