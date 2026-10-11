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
const globalRoot = Bun.spawnSync(["npm", "root", "-g"]).stdout.toString().trim();
const browser = Bun.spawnSync(
  ["node", "--test", "--test-reporter=junit", "--test-reporter-destination=.joppa/browser.junit.xml", "tests/grey_ac_browser.cjs"],
  { stdout: "inherit", stderr: "inherit", env: { ...process.env, NODE_PATH: process.env.NODE_PATH ?? globalRoot } },
);
writeFileSync(".joppa/browser.stamp.json", JSON.stringify({ full: true }) + "\n");
process.exit(cargo.exitCode || bun.exitCode || browser.exitCode ? 1 : 0);
