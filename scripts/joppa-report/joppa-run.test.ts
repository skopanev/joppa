import { expect, test } from "bun:test";
import { chmodSync, copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

function runBrowserPart(available: boolean) {
  const dir = mkdtempSync(join(tmpdir(), "joppa-browser-report-"));
  try {
    mkdirSync(join(dir, "bin"));
    mkdirSync(join(dir, "tests"));
    mkdirSync(join(dir, ".joppa"));
    copyFileSync(join(import.meta.dir, "joppa-run.ts"), join(dir, "joppa-run.ts"));
    // Replace Rust and Bun runners. The Node runner and report writer remain real.
    writeFileSync(join(dir, "bin/bun"), "#!/bin/sh\nexit 0\n");
    chmodSync(join(dir, "bin/bun"), 0o755);
    writeFileSync(join(dir, "tests/grey_ac_browser.cjs"), "const {test}=require('node:test'); test('a failure stays visible', () => {throw new Error('expected failure')});\n");
    writeFileSync(join(dir, ".joppa/browser.junit.xml"), "stale passing report");
    const module = join(dir, "playwright.cjs");
    if (available) writeFileSync(module, "module.exports={};\n");
    const result = Bun.spawnSync([process.execPath, "joppa-run.ts"], {
      cwd: dir,
      env: { ...process.env, PATH: `${join(dir, "bin")}:${process.env.PATH}`, PLAYWRIGHT_MODULE: module },
      stdout: "pipe",
      stderr: "pipe",
    });
    return {
      code: result.exitCode,
      stamp: JSON.parse(readFileSync(join(dir, ".joppa/browser.stamp.json"), "utf8")),
      junit: readFileSync(join(dir, ".joppa/browser.junit.xml"), "utf8"),
      stderr: result.stderr.toString(),
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("an absent browser runner replaces old results and stamps the run partial", () => {
  const result = runBrowserPart(false);
  expect(result.code).toBe(0);
  expect(result.stamp.full).toBe(false);
  expect(result.junit).toBe("<testsuites></testsuites>\n");
  expect(result.stderr).toContain("this run is partial");
});

test("an available browser runner keeps real failures and their JUnit results", () => {
  const result = runBrowserPart(true);
  expect(result.code).toBe(1);
  expect(result.stamp.full).toBe(true);
  expect(result.junit).toContain("<failure");
  expect(result.junit).toContain("a failure stays visible");
});
