#!/usr/bin/env bun
// Joppa's full run includes Rust, report kit and browser tests.
// The pre-push hook sends them as one report.
//   JOPPA_TEST_DATABASE_URL=... bun scripts/joppa-report/joppa-run.ts
import { writeFileSync } from "node:fs";

const cargo = Bun.spawnSync(
  ["bun", "scripts/joppa-report/cargo-junit.ts", "--no-fail-fast", "--", "--include-ignored", "--test-threads=1"],
  { stdout: "inherit", stderr: "inherit" },
);
const bun = Bun.spawnSync(
  ["bun", "test", "scripts/joppa-report", "--reporter=junit", "--reporter-outfile=.joppa/bun.junit.xml"],
  { stdout: "inherit", stderr: "inherit" },
);
writeFileSync(".joppa/bun.stamp.json", JSON.stringify({ full: true }) + "\n");
let available = false;
try {
  available = Bun.spawnSync(
    ["node", "-e", "require(process.env.PLAYWRIGHT_MODULE || 'playwright')"],
    { stdout: "ignore", stderr: "ignore" },
  ).exitCode === 0;
} catch {
  // Node is optional when the browser runner is absent.
}
let browserExit = 0;
if (available) {
  const browser = Bun.spawnSync(
    ["node", "--test", "--test-reporter=junit", "tests/grey_ac_browser.cjs"],
    { stdout: "pipe", stderr: "inherit" },
  );
  writeFileSync(".joppa/browser.junit.xml", browser.stdout);
  browserExit = browser.exitCode;
} else {
  writeFileSync(".joppa/browser.junit.xml", "<testsuites></testsuites>\n");
  console.error("joppa report: browser tests skipped because Playwright is absent; this run is partial");
}
writeFileSync(".joppa/browser.stamp.json", JSON.stringify({ full: available }) + "\n");
process.exit(cargo.exitCode || bun.exitCode || browserExit ? 1 : 0);
