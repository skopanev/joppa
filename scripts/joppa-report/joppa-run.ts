#!/usr/bin/env bun
// Joppa's own full run: cargo test (with the database tests) and the bun tests of this kit,
// as one report for the pre-push hook.
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
process.exit(cargo.exitCode || bun.exitCode ? 1 : 0);
