// The report kit end to end on a fixture git repository: a real `bun test` with the
// stamp preload, then a real `git push` through the pre-push hook to a fake Joppa.
import { afterAll, beforeAll, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const KIT = import.meta.dir;
// The body that the real endpoint accepts (tests/reports_http.rs posts the same fixture).
const FIXTURE = JSON.parse(readFileSync(join(KIT, "../../tests/fixtures/report-upload.json"), "utf8"));
const shape = (v: any): any =>
  v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, shape(v[k])])) : typeof v;
let root = "";
let repo = "";
const uploads: any[] = [];
let server: ReturnType<typeof Bun.serve>;

function run(cmd: string[], env: Record<string, string> = {}) {
  const r = Bun.spawnSync(cmd, { cwd: repo || root, env: { ...process.env, ...env }, stdout: "pipe", stderr: "pipe" });
  return { code: r.exitCode, out: r.stdout.toString() + r.stderr.toString() };
}
const git = (...args: string[]) => run(["git", ...args]);
const stamp = () => JSON.parse(readFileSync(join(repo, ".joppa/stamp.json"), "utf8"));
const head = () => git("rev-parse", "HEAD").out.trim();
const bunTest = (env: Record<string, string> = {}, args: string[] = []) => run(["bun", "test", ...args], env);
// Async: the fake Joppa answers the hook from this same process.
async function push(...args: string[]) {
  const p = Bun.spawn(["git", "push", ...args, "origin", "master"], {
    cwd: repo, env: { ...process.env, JOPPA_REPORT_TOKEN: "fixture-token" }, stdout: "pipe", stderr: "pipe",
  });
  const out = (await new Response(p.stdout).text()) + (await new Response(p.stderr).text());
  return { code: await p.exited, out };
}

beforeAll(() => {
  server = Bun.serve({
    port: 0,
    async fetch(request) {
      const body = await request.json();
      // Reject any body that the real endpoint would not take, so hook and server cannot drift.
      if (JSON.stringify(shape(body)) !== JSON.stringify(shape(FIXTURE)))
        return Response.json({ error: "body does not match the upload fixture" }, { status: 422 });
      uploads.push({ auth: request.headers.get("authorization"), path: new URL(request.url).pathname, body });
      return Response.json({ tests: 2, report: uploads.length });
    },
  });
  root = mkdtempSync(join(tmpdir(), "joppa-kit-"));
  run(["git", "init", "-q", "--bare", join(root, "remote.git")]);
  run(["git", "init", "-q", "-b", "master", join(root, "repo")]);
  repo = join(root, "repo");
  git("config", "user.email", "kit@example.test");
  git("config", "user.name", "Kit");
  writeFileSync(join(repo, "bunfig.toml"), `[test]\npreload = ["${KIT}/stamp.ts"]\n[test.reporter]\njunit = ".joppa/junit.xml"\n`);
  writeFileSync(join(repo, ".gitignore"), ".joppa/junit.xml\n.joppa/stamp.json\n");
  writeFileSync(
    join(repo, "login.test.ts"),
    `import { describe, expect, test } from "bun:test";\ndescribe("login", () => {\n  test("opens", () => expect(1).toBe(1));\n  test("refuses", () => expect(2).toBe(2));\n  test("remembers", () => expect(3).toBe(3));\n});\n`,
  );
  Bun.spawnSync(["mkdir", "-p", join(repo, ".joppa")]);
  writeFileSync(join(repo, ".joppa/config.json"), JSON.stringify({ url: `http://127.0.0.1:${server.port}`, workspace: "fixture", default_branch: "master" }));
  git("add", ".");
  git("commit", "-q", "-m", "Fixture");
  git("remote", "add", "origin", join(root, "remote.git"));
  symlinkSync(join(KIT, "pre-push"), join(repo, ".git/hooks/pre-push"));
});
afterAll(() => {
  server.stop(true);
  rmSync(root, { recursive: true, force: true });
});

test("a normal test run writes one testcase per test", () => {
  // result-has-output-ac1
  const r = bunTest();
  expect(r.code, r.out).toBe(0);
  const report = join(repo, ".joppa/junit.xml");
  if (!existsSync(report)) throw new Error(`no .joppa/junit.xml after bun test:\n${r.out}`);
  expect(readFileSync(report, "utf8").match(/<testcase /g)?.length ?? 0, r.out).toBe(3);
});

test("a clean run stamps its commit as clean and full", () => {
  // result-has-output-ac3
  expect(bunTest().code).toBe(0);
  expect(stamp()).toMatchObject({ commit: head(), dirty: false, full: true, branch: "master" });
});

test("a run on a dirty tree is stamped dirty", () => {
  // result-has-output-ac4
  writeFileSync(join(repo, "login.test.ts"), readFileSync(join(repo, "login.test.ts"), "utf8") + "// edit\n");
  bunTest();
  expect(stamp().dirty).toBe(true);
  git("checkout", "--", "login.test.ts");
});

test("a filtered run is stamped partial", () => {
  // result-has-output-ac5
  expect(bunTest({ JOPPA_FULL_RUN: "1" }, ["-t", "opens"]).code).toBe(0);
  expect(stamp().full).toBe(false);
});

test("a run of one test file is stamped partial", () => {
  expect(bunTest({}, ["./login.test.ts"]).code).toBe(0);
  expect(stamp().full).toBe(false);
});

test("a run of a file filter after -- is stamped partial", () => {
  // The filter starts with "-", so only the "--" separator shows that it is not a flag.
  writeFileSync(join(repo, "-extra.test.ts"), `import { test } from "bun:test";\ntest("extra", () => {});\n`);
  const r = bunTest({}, ["--", "-extra"]);
  rmSync(join(repo, "-extra.test.ts"));
  expect(r.code).toBe(0);
  expect(stamp().full).toBe(false);
});

test("a run with --only is stamped partial", () => {
  expect(bunTest({}, ["--only"]).code).toBe(0);
  expect(stamp().full).toBe(false);
});

test("a push uploads the report of the pushed commit", async () => {
  // runner-executed-ac1
  bunTest();
  uploads.length = 0;
  expect((await push()).code).toBe(0);
  expect(uploads.length).toBe(1);
  expect(uploads[0].path).toBe("/api/workspaces/fixture/reports");
  expect(uploads[0].auth).toBe("Bearer fixture-token");
  expect(uploads[0].body.stamp).toMatchObject({ commit: head(), dirty: false, full: true, default_branch: true });
  expect(uploads[0].body.junit).toContain('name="opens"');
});

test("a push with an old report uploads nothing", async () => {
  // runner-executed-ac2
  bunTest();
  git("commit", "-q", "--allow-empty", "-m", "New commit without a test run");
  uploads.length = 0;
  const pushed = await push();
  expect(pushed.code).toBe(0);
  expect(uploads.length).toBe(0);
  expect(pushed.out).toContain("not for the pushed default-branch commit");
});

test("a push with --no-verify uploads nothing", async () => {
  // runner-executed-ac3
  git("commit", "-q", "--allow-empty", "-m", "Another commit");
  bunTest();
  uploads.length = 0;
  expect((await push("--no-verify")).code).toBe(0);
  expect(uploads.length).toBe(0);
});

test("extra reports of the same run join the upload", async () => {
  // The Joppa kit runs cargo and bun tests; the hook sends both in one report.
  bunTest();
  writeFileSync(join(repo, ".joppa/extra.junit.xml"), '<testsuites><testsuite name="tests/extra.rs"><testcase name="extra_case" /></testsuite></testsuites>');
  writeFileSync(join(repo, ".joppa/extra.stamp.json"), JSON.stringify({ full: true }));
  git("commit", "-q", "--allow-empty", "-m", "Merge check");
  bunTest();
  uploads.length = 0;
  expect((await push()).code).toBe(0);
  expect(uploads.length).toBe(1);
  expect(uploads[0].body.junit).toContain('name="opens"');
  expect(uploads[0].body.junit).toContain('name="extra_case"');
  expect(uploads[0].body.stamp.full).toBe(true);
});

test("a merged report is full only when every part is full", async () => {
  writeFileSync(join(repo, ".joppa/extra.stamp.json"), JSON.stringify({ full: false }));
  git("commit", "-q", "--allow-empty", "-m", "Partial extra");
  bunTest();
  uploads.length = 0;
  expect((await push()).code).toBe(0);
  expect(uploads.length).toBe(1);
  expect(uploads[0].body.stamp.full).toBe(false);
  rmSync(join(repo, ".joppa/extra.junit.xml"));
  rmSync(join(repo, ".joppa/extra.stamp.json"));
});

test("an extra part from another commit makes the merged report partial", async () => {
  git("commit", "-q", "--allow-empty", "-m", "Stale extra");
  bunTest({ JOPPA_FULL_RUN: "1" });
  writeFileSync(join(repo, ".joppa/extra.junit.xml"), '<testsuites><testsuite name="tests/extra.rs"><testcase name="extra_case" /></testsuite></testsuites>');
  writeFileSync(join(repo, ".joppa/extra.stamp.json"), JSON.stringify({ full: true, commit: "0".repeat(40), dirty: false }));
  uploads.length = 0;
  expect((await push()).code).toBe(0);
  expect(uploads.length).toBe(1);
  expect(uploads[0].body.stamp.full).toBe(false);
  writeFileSync(join(repo, ".joppa/extra.stamp.json"), JSON.stringify({ full: true, commit: head(), dirty: false }));
  git("commit", "-q", "--allow-empty", "-m", "Matching extra");
  bunTest({ JOPPA_FULL_RUN: "1" });
  writeFileSync(join(repo, ".joppa/extra.stamp.json"), JSON.stringify({ full: true, commit: head(), dirty: false }));
  uploads.length = 0;
  expect((await push()).code).toBe(0);
  expect(uploads[0].body.stamp.full).toBe(true);
  rmSync(join(repo, ".joppa/extra.junit.xml"));
  rmSync(join(repo, ".joppa/extra.stamp.json"));
});

test("a push to another branch uploads nothing", async () => {
  // runner-executed: only the default branch counts, even with a fresh full report.
  git("checkout", "-q", "-b", "feature/x");
  git("commit", "-q", "--allow-empty", "-m", "Feature work");
  bunTest();
  uploads.length = 0;
  const p = Bun.spawn(["git", "push", "origin", "feature/x"], {
    cwd: repo, env: { ...process.env, JOPPA_REPORT_TOKEN: "fixture-token" }, stdout: "pipe", stderr: "pipe",
  });
  const out = await new Response(p.stderr).text();
  expect(await p.exited).toBe(0);
  expect(uploads.length).toBe(0);
  expect(out).toContain("not for the pushed default-branch commit");
  git("checkout", "-q", "master");
});

// A lane that uploads after its push needs an exit code that proves the upload.
async function upload(env: Record<string, string>) {
  const p = Bun.spawn(["bun", join(KIT, "pre-push"), "origin", "x"], {
    cwd: repo, env: { ...process.env, JOPPA_REPORT_TOKEN: "", ...env }, stdin: "pipe", stdout: "pipe", stderr: "pipe",
  });
  p.stdin.write(`refs/heads/master ${head()} refs/heads/master ${"0".repeat(40)}\n`);
  p.stdin.end();
  return await p.exited;
}

test("strict upload exits 0 only when the report is stored", async () => {
  bunTest({ JOPPA_FULL_RUN: "1" });
  uploads.length = 0;
  expect(await upload({ JOPPA_REPORT_STRICT: "1", JOPPA_REPORT_TOKEN: "fixture-token" })).toBe(0);
  expect(uploads.length).toBe(1);
  expect(await upload({ JOPPA_REPORT_STRICT: "1" })).toBe(1);
  expect(await upload({})).toBe(0);
  expect(uploads.length).toBe(1);
});
