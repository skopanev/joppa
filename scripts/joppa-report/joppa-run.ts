#!/usr/bin/env bun
// Joppa's own full run: cargo test (with the database tests), the bun tests of this kit
// and the Chromium criteria checks, as one report for the pre-push hook.
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
// The same browser checks as CI. Playwright comes from PLAYWRIGHT_MODULE or the global npm root.
// A box that cannot start Chromium marks this part as not run, so the report is partial, not red.
const globalRoot = Bun.spawnSync(["npm", "root", "-g"]).stdout.toString().trim();
const env = { ...process.env, NODE_PATH: process.env.NODE_PATH ?? globalRoot };
const probe = Bun.spawnSync(
  ["node", "-e", "const fail=e=>{console.error(e.message.split('\\n')[0]);process.exit(1)};try{require(process.env.PLAYWRIGHT_MODULE||'playwright').chromium.launch().then(b=>b.close(),fail)}catch(e){fail(e)}"],
  { env },
);
let browserFailed = false;
if (probe.exitCode === 0) {
  const browser = Bun.spawnSync(
    ["node", "--test", "--test-reporter=junit", "--test-reporter-destination=.joppa/browser.junit.xml", "tests/grey_ac_browser.cjs"],
    { stdout: "inherit", stderr: "inherit", env },
  );
  browserFailed = browser.exitCode !== 0;
  writeFileSync(".joppa/browser.stamp.json", JSON.stringify({ full: true }) + "\n");
} else {
  const reason = probe.stderr.toString().trim().split("\n")[0] || "Chromium did not start";
  console.log(`browser part not run: ${reason}`);
  writeFileSync(".joppa/browser.junit.xml", '<?xml version="1.0" encoding="UTF-8"?>\n<testsuites></testsuites>\n');
  writeFileSync(".joppa/browser.stamp.json", JSON.stringify({ full: false }) + "\n");
}
process.exit(cargo.exitCode || bun.exitCode || browserFailed ? 1 : 0);
