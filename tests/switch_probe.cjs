// The page probe of the workspace switch measurement. The local check and the live run use it.

/// Mark the change event, then find the first animation frame in which every expected top-level
/// row is laid out and visible and no row of another workspace is in the tree. The resource
/// entries keep only the path and the times of each read, no header and no body.
function installProbe() {
  window.switchProbe = {expect: null, result: null};
  document.addEventListener('change', event => {
    const probe = window.switchProbe;
    if (event.target.id !== 'workspaces' || !probe.expect || event.target.value !== probe.expect.ws) return;
    const start = performance.now(), expect = probe.expect;
    const seen = address => {
      const row = document.querySelector('#tree .common-object[data-object-address="' + address + '"]');
      return row && row.checkVisibility() && row.getBoundingClientRect().height > 0;
    };
    const frame = now => {
      if (probe.expect !== expect) return;
      const foreign = expect.foreign.some(address => document.querySelector('#tree [data-object-address="' + address + '"]'));
      if (!foreign && expect.rows.every(seen)) {
        probe.result = {ms: now - start, resources: performance.getEntriesByType('resource')
          .filter(entry => entry.startTime >= start).map(entry => ({url: new URL(entry.name).pathname + new URL(entry.name).search,
            start_ms: +(entry.startTime - start).toFixed(1), wait_ms: +(entry.responseStart - entry.requestStart).toFixed(1),
            duration_ms: +entry.duration.toFixed(1)}))};
        return;
      }
      requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }, true);
}

/// Select `target` from `away` once and return the probe result. `shown` waits for a workspace.
async function timedRun(page, away, target, expect, shown) {
  await page.evaluate(() => { window.switchProbe.expect = null; });
  await page.locator('#workspaces').selectOption(away.ws);
  await shown(away);
  await page.evaluate(() => performance.clearResourceTimings());
  await page.evaluate(wanted => { window.switchProbe.expect = wanted; window.switchProbe.result = null; }, expect);
  await page.locator('#workspaces').selectOption(target.ws);
  await page.waitForFunction(() => window.switchProbe.result, null, {timeout: 30000});
  return page.evaluate(() => window.switchProbe.result);
}

/// The sorted durations, the 19th value as p95 of 20 runs, and the median.
function summary(runs) {
  const sorted = runs.map(run => run.ms).sort((a, b) => a - b);
  return {sorted_ms: sorted, p95_ms: sorted[18] ?? null, median_ms: sorted.length ? (sorted[(sorted.length - 1) >> 1] + sorted[sorted.length >> 1]) / 2 : null};
}

module.exports = {installProbe, timedRun, summary};
