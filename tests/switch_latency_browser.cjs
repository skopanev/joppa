// [ac:714c82e6-c4e5-4330-ab59-635505bd8166] The time from a workspace selection to the visible
// top level, on the private browser_server fixture. This is a local measurement, not production.
// JOPPA_SWITCH_OUT names the JSON file for the report.
const assert = require('node:assert/strict');
const {writeFileSync} = require('node:fs');
const os = require('node:os');
const {chromium} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const {server, domainTree, workspaces, shows, build} = require('./switch_support.cjs');
const {installProbe, timedRun, summary, measureRuns} = require('./switch_probe.cjs');

const RUNS = 20;

async function main() {
  assert.ok(process.env.JOPPA_TEST_DATABASE_URL, 'isolated test database required');
  const live = await server();
  const browser = await chromium.launch({headless: true});
  const report = {label: 'local fixture, not production', browser: 'Chromium ' + browser.version(), host: os.hostname(),
    network: 'loopback ' + live.root, build: build(), server_profile: 'release'};
  try {
    // 5 domains, 15 capabilities, 150 requirements and 600 criteria.
    const records = domainTree([3, 10, 4]);
    const {context, big, other, third, expect} = await workspaces(live, browser, records);
    report.large = {workspace: big.ws, objects: records.length, domains: big.rows.length};
    const page = await context.newPage(), errors = [];
    page.on('pageerror', error => errors.push(error.message));
    await page.addInitScript(installProbe);
    await page.goto(live.root + '/?workspace=' + encodeURIComponent(other.ws));
    await shows(page, expect(other));
    await page.locator('#workspaces option[value="' + big.ws + '"]').waitFor({state: 'attached'});
    // 20 timed selections of the large workspace. Each run starts on another workspace, so the
    // board holds no data of the large workspace before the selection.
    report.runs = [];
    await measureRuns(RUNS, async run => {
      const away = run % 2 ? other : third;
      const result = await timedRun(page, away, big, expect(big), target => shows(page, expect(target)));
      return {from: away.ws, ms: +result.ms.toFixed(1), resources: result.resources};
    }, report.runs);
    report.page_errors = errors;
    assert.deepEqual(errors, []);
    await context.close();
  } finally {
    await browser.close();
    await live.stop();
    if (report.runs) Object.assign(report, summary(report.runs));
    if (process.env.JOPPA_SWITCH_OUT) writeFileSync(process.env.JOPPA_SWITCH_OUT, JSON.stringify(report, null, 1) + '\n');
  }
  console.log(JSON.stringify({label: report.label, sorted_ms: report.sorted_ms, p95_ms: report.p95_ms, failed_runs: report.failed_runs}));
  assert.equal(report.failed_runs, 0, 'every run shows the top level');
  assert.ok(report.p95_ms !== null && report.p95_ms <= 1000, 'p95 ' + report.p95_ms + ' ms is above 1000 ms');
}

require('node:test').test('714c82e6 switch-latency: the top level of a large workspace shows within 1 second', main);
