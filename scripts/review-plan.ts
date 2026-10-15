#!/usr/bin/env bun
// Print the focused test plan for the change BASE..HEAD as JSON lines:
//   {"kind":"rust","target":"lib|bin|<tests file stem>","filter":"<test path>"}
//   {"kind":"bun","file":"scripts/.../x.test.ts","name":"<test name>"}
//   {"kind":"browser","file":"tests/x.cjs","name":"<test name>"}
// A test is in the plan when the change adds or edits a line in its block, from the
// attributes and comments above it to its last line. Deleted tests are not in the plan.
// An edit outside every test block of a tests/*.rs file (a helper, an import) can change
// any test of that file, so the plan runs the whole file: {"kind":"rust","target":..,"filter":""}.
//   bun scripts/review-plan.ts BASE
import { existsSync, readFileSync } from "node:fs";

const base = process.argv[2];
if (!base) {
  console.error("usage: bun scripts/review-plan.ts BASE");
  process.exit(2);
}
function git(...args: string[]): string {
  const r = Bun.spawnSync(["git", ...args]);
  if (r.exitCode !== 0) {
    console.error(`review-plan: git ${args.join(" ")}: ${r.stderr.toString().trim()}`);
    process.exit(2);
  }
  return r.stdout.toString();
}
git("merge-base", "--is-ancestor", base, "HEAD");

// Lines that the change added or edited, per file, on the new side.
const added = new Map<string, Set<number>>();
let file = "";
for (const line of git("diff", "-U0", `${base}..HEAD`, "--", "tests", "src", "scripts").split("\n")) {
  if (line.startsWith("+++ ")) file = line.startsWith("+++ b/") ? line.slice(6) : "";
  const hunk = line.match(/^@@ .*\+(\d+)(?:,(\d+))? @@/);
  if (!hunk || !file) continue;
  const start = Number(hunk[1]);
  const count = hunk[2] === undefined ? 1 : Number(hunk[2]);
  const set = added.get(file) ?? new Set<number>();
  for (let n = start; n < start + count; n++) set.add(n);
  added.set(file, set);
}
const touched = (path: string, from: number, to: number) => {
  const set = added.get(path);
  for (let n = from; n <= to; n++) if (set?.has(n)) return true;
  return false;
};

// Unit tests in src/ belong to the binary when main.rs declares their top module.
const binMods = new Set(
  [...readFileSync("src/main.rs", "utf8").matchAll(/^\s*(?:pub\s+)?mod\s+([a-z0-9_]+)\s*;/gm)].map((m) => m[1]),
);
function rustTarget(path: string): string {
  if (path.startsWith("tests/")) return path.slice(6, -3);
  if (path === "src/main.rs") return "bin";
  if (path === "src/lib.rs") return "lib";
  const top = path.slice(4, -3).replace(/\/mod$/, "").split("/")[0];
  return binMods.has(top) ? "bin" : "lib";
}

const plan: Record<string, string>[] = [];
for (const path of [...added.keys()].sort()) {
  if (!existsSync(path)) continue;
  const text = readFileSync(path, "utf8").split("\n");
  if (path.endsWith(".rs") && (path.startsWith("tests/") || path.startsWith("src/"))) {
    if (path.startsWith("tests/") && path.split("/").length !== 2) continue;
    const inTests = new Set<number>();
    const found: Record<string, string>[] = [];
    for (let i = 0; i < text.length; i++) {
      const m = text[i].match(/^\s*(?:pub\s+)?(?:async\s+)?fn\s+([a-z0-9_]+)\s*\(/);
      if (!m || !/#\[(tokio::)?test/.test(text.slice(Math.max(0, i - 4), i).join("\n"))) continue;
      let start = i;
      while (start > 0 && /^\s*(#\[|\/\/)/.test(text[start - 1])) start--;
      let depth = 0;
      let end = i;
      for (; end < text.length; end++) {
        depth += (text[end].match(/{/g) ?? []).length - (text[end].match(/}/g) ?? []).length;
        if (depth <= 0 && text.slice(i, end + 1).join("").includes("{")) break;
      }
      for (let n = start + 1; n <= end + 1; n++) inTests.add(n);
      if (touched(path, start + 1, end + 1)) found.push({ kind: "rust", target: rustTarget(path), filter: m[1] });
      i = end;
    }
    const outside = [...(added.get(path) ?? [])].some((n) => !inTests.has(n) && (text[n - 1] ?? "").trim() !== "");
    if (path.startsWith("tests/") && outside) plan.push({ kind: "rust", target: rustTarget(path), filter: "" });
    else plan.push(...found);
  } else if (path.startsWith("tests/") && path.endsWith(".cjs")) {
    const starts = text.flatMap((line, n) => {
      const m = line.match(/^\s*test\((['"`])(.+?)\1/);
      return m ? [[n, m[2]] as const] : [];
    });
    starts.forEach(([n, name], k) => {
      const end = k + 1 < starts.length ? starts[k + 1][0] : text.length;
      if (touched(path, n + 1, end)) plan.push({ kind: "browser", file: path, name });
    });
  } else if (path.startsWith("scripts/") && path.endsWith(".test.ts")) {
    const starts = text.flatMap((line, n) => {
      const m = line.match(/^\s*test\((['"`])(.+?)\1/);
      return m ? [[n, m[2]] as const] : [];
    });
    starts.forEach(([n, name], k) => {
      const end = k + 1 < starts.length ? starts[k + 1][0] : text.length;
      if (touched(path, n + 1, end)) plan.push({ kind: "bun", file: path, name });
    });
  }
}
for (const item of plan) console.log(JSON.stringify(item));
