#!/usr/bin/env bun
// Run `cargo test` as usual and leave .joppa/junit.xml and .joppa/stamp.json behind.
//   bun scripts/joppa-report/cargo-junit.ts [cargo test arguments]
// The exit code is cargo's. A test address is the source file and the test name:
// "tests/title_length.rs > new_titles_fit_the_type_preview_length".
// The run is full only without a test filter or a target selection.
import { mkdirSync, writeFileSync } from "node:fs";
import { writeStamp } from "./stamp-lib";

const SELECTION = new Set(["--test", "--lib", "--bin", "--bins", "--tests", "--doc", "--example",
  "--examples", "--bench", "--benches", "-p", "--package", "--exclude", "--skip", "--exact"]);
const WITH_VALUE = new Set(["--features", "-F", "--target", "--target-dir", "--manifest-path", "-j",
  "--jobs", "--profile", "--test-threads", "--color", "--format", "-Z", "--config"]);

export function isFull(args: string[]): boolean {
  let skipValue = false;
  for (const arg of args) {
    if (skipValue) { skipValue = false; continue; }
    if (arg === "--") continue;
    const name = arg.split("=", 1)[0];
    if (SELECTION.has(name)) return false;
    if (WITH_VALUE.has(name) && !arg.includes("=")) skipValue = true;
    else if (!arg.startsWith("-")) return false;
  }
  return true;
}

type Case = { name: string; status: "pass" | "fail" | "skipped"; message?: string };

export function parse(output: string): Map<string, Case[]> {
  const suites = new Map<string, Case[]>();
  const failures = new Map<string, string[]>();
  let suite = "unknown";
  let failing: string | null = null;
  for (const line of output.split("\n")) {
    const running = line.match(/^\s+Running (?:unittests )?(\S+) \(/);
    if (running) { suite = running[1]; continue; }
    if (/^\s+Doc-tests /.test(line)) { suite = "doc"; continue; }
    const test = line.match(/^test (.+?) \.\.\. (ok|FAILED|ignored)/);
    if (test) {
      const status = test[2] === "ok" ? "pass" : test[2] === "FAILED" ? "fail" : "skipped";
      if (!suites.has(suite)) suites.set(suite, []);
      suites.get(suite)!.push({ name: test[1], status });
      continue;
    }
    const header = line.match(/^---- (.+?) stdout ----/);
    if (header) { failing = `${suite}\u0000${header[1]}`; failures.set(failing, []); }
    else if (failing && line.trim() && !/^(failures:|test result)/.test(line)) failures.get(failing)!.push(line);
  }
  for (const [key, lines] of failures) {
    const [name_suite, name] = key.split("\u0000");
    for (const c of suites.get(name_suite) ?? [])
      if (c.name === name && c.status === "fail") c.message = lines.join("\n").slice(0, 4000);
  }
  return suites;
}

const attr = (value: string) =>
  `"${value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/"/g, "&quot;").replace(/\n/g, "&#10;")}"`;

export function junit(suites: Map<string, Case[]>): string {
  const out = ['<?xml version="1.0" encoding="UTF-8"?>', '<testsuites name="cargo test">'];
  for (const [suite, cases] of suites) {
    out.push(`  <testsuite name=${attr(suite)}>`);
    for (const c of cases) {
      if (c.status === "pass") out.push(`    <testcase name=${attr(c.name)} />`);
      else if (c.status === "fail") out.push(`    <testcase name=${attr(c.name)}><failure message=${attr(c.message ?? "failed")} /></testcase>`);
      else out.push(`    <testcase name=${attr(c.name)}><skipped /></testcase>`);
    }
    out.push("  </testsuite>");
  }
  out.push("</testsuites>");
  return out.join("\n") + "\n";
}

if (import.meta.main) {
  const args = Bun.argv.slice(2);
  // cargo prints "Running <file>" to stderr and results to stdout: merge them in order.
  const run = Bun.spawnSync(["sh", "-c", 'exec bun run test:rust "$@" 2>&1', "cargo-test", ...args], { stdout: "pipe" });
  const output = run.stdout.toString();
  process.stdout.write(output);
  mkdirSync(".joppa", { recursive: true });
  writeFileSync(".joppa/junit.xml", junit(parse(output)));
  writeStamp(isFull(args));
  process.exit(run.exitCode ?? 1);
}
