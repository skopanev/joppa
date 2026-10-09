#!/usr/bin/env bun
// Test lists on criteria and JUnit reports, end to end over HTTP on an isolated test DB.
//   JOPPA_TEST_DATABASE_URL=... bun tests/reports_smoke.ts
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const ROOT = join(import.meta.dir, "..");
const BIN = process.env.JOPPA_BIN ?? join(ROOT, "target/debug/joppa");
const url = process.env.JOPPA_TEST_DATABASE_URL!;
if (!new URL(url).pathname.split("/").pop()!.startsWith("joppa_test")) throw new Error("test DB only");
const env = { ...process.env, JOPPA_DATABASE_URL: url, JOPPA_ACTOR: "owner" } as Record<string, string>;
for (const name of ["JOPPA_GOOGLE_CLIENT_FILE", "JOPPA_LOGIN_USERS_FILE", "DEEPINFRA_API_KEY"]) delete env[name];
const run = (args: string[], stdin?: string) => {
  const r = Bun.spawnSync([BIN, ...args], { env, stdin: stdin ? new TextEncoder().encode(stdin) : undefined });
  if (r.exitCode !== 0) throw new Error(`${args.join(" ")}: ${r.stderr.toString()}`);
};
const check = (ok: unknown, what: unknown) => { if (!ok) throw new Error(JSON.stringify(what)); };

run(["migrate"]);
const token = () => crypto.randomUUID().replaceAll("-", "");
const [owner, agent, outsider] = [token(), token(), token()];
const ws = `reports-${crypto.randomUUID()}`;
run(["command", ws, "server-create"], JSON.stringify({ op: "workspace", title: "Reports" }));
const temp = mkdtempSync(join(tmpdir(), "joppa-reports-"));
const tokens = join(temp, "tokens.json");
writeFileSync(tokens, JSON.stringify({ [owner]: "owner", [agent]: "agent", [outsider]: "outsider" }));
chmodSync(tokens, 0o600);
env.JOPPA_TOKENS_FILE = tokens;
const port = 20000 + Math.floor(Math.random() * 20000);
const base = `http://127.0.0.1:${port}/api/workspaces/${ws}`;
const server = Bun.spawn([BIN, "serve", String(port)], { env, stdout: "ignore", stderr: "ignore" });

async function call(path: string, body?: unknown, auth: string | null = owner) {
  const headers: Record<string, string> = { "Content-Type": "application/json" };
  if (auth) headers.Authorization = `Bearer ${auth}`;
  const reply = await fetch(base + path, body === undefined ? { headers } : { method: "POST", headers, body: JSON.stringify(body) });
  const text = await reply.text();
  let data: any = text;
  try { data = JSON.parse(text); } catch {}
  return [reply.status, data] as const;
}
async function ok(command: unknown, auth = owner) {
  const [status, data] = await call("/commands", { request_id: crypto.randomUUID(), command }, auth);
  check(status === 200, [status, data]);
}
const junit = (cases: [string, string][]) =>
  `<testsuites><testsuite name="tests/auth/login.test.ts"><testsuite name="login">${cases
    .map(([name, s]) => `<testcase name="${name}">${s === "fail" ? '<failure message="boom"/>' : s === "skipped" ? "<skipped/>" : ""}</testcase>`)
    .join("")}</testsuite></testsuite></testsuites>`;
const upload = (commit: string, order: number, cases: [string, string][], o: { full?: boolean; auth?: string; default_branch?: boolean; dirty?: boolean } = {}) =>
  call("/reports", { stamp: { commit, order, branch: "master", default_branch: o.default_branch ?? true, dirty: o.dirty ?? false, full: o.full ?? true }, junit: junit(cases) }, o.auth ?? agent);
async function status() {
  const [code, data] = await call("/tests?ac=" + encodeURIComponent("ac:key-rights"));
  check(code === 200, [code, data]);
  return data;
}
const A = "tests/auth/login.test.ts > login > opens";
const B = "tests/auth/login.test.ts > login > refuses";
const revise = (expected: number, text: string) => ok({ op: "revise", req: "key", expected_revision: expected, capability: "restrict",
  title: "Key rights", body: "Scope", owner: "owner", consumers: ["customer"], reason: "Agreed", acs: [{ id: "key-rights", text }] });

try {
  for (let i = 0; i < 100; i++) {
    try { if ((await fetch(`http://127.0.0.1:${port}/health`)).ok) break; } catch {}
    await Bun.sleep(100);
  }
  await ok({ op: "member", id: "agent", role: "agent" });
  await ok({ op: "consumer", id: "customer", title: "Customer", owner: "owner" });
  await ok({ op: "domain", id: "security", title: "Security", authority: "owner" });
  await ok({ op: "capability", id: "restrict", title: "Restrict access", domain: "security" });
  await revise(0, "Only read and verify before launch");

  check((await status()).reason === "no tests", "no tests");
  await ok({ op: "set_ac_tests", target: "ac:key-rights", tests: [A, B], reason: "Prove the rights" }, agent);
  let s = await status();
  check(s.status === "grey" && s.tests[0].status === "not_run", s);

  // Upload rules: token first, members only, replays are idempotent.
  check((await call("/reports", { stamp: {}, junit: "" }, null))[0] === 401, "no token");
  check((await upload("a1a1a1a", 1, [["opens", "pass"]], { auth: outsider }))[0] === 404, "outsider");
  const [code, first] = await upload("a1a1a1a", 1, [["opens", "pass"], ["refuses", "pass"]]);
  check(code === 200 && first.tests === 2 && first.uploader === "agent", first);
  const [, again] = await upload("a1a1a1a", 1, [["opens", "pass"], ["refuses", "pass"]]);
  check(again.replayed && again.report === first.report, again);
  s = await status();
  check(s.status === "green" && s.commit === "a1a1a1a" && s.commits_behind === 0, s);

  // Dirty runs and other branches never change the status.
  await upload("b2b2b2b", 2, [["refuses", "fail"]], { dirty: true });
  await upload("c3c3c3c", 2, [["refuses", "fail"]], { default_branch: false });
  check((await status()).status === "green", "filters");

  // A partial run updates only its tests; a full run without a listed test makes it missing.
  await upload("d4d4d4d", 3, [["opens", "pass"], ["refuses", "skipped"]], { full: false });
  s = await status();
  check(s.status === "green" && s.commit === "a1a1a1a" && s.commits_behind === 2, s);
  await upload("e5e5e5e", 4, [["opens", "pass"]]);
  s = await status();
  check(s.status === "red" && s.tests.map((t: any) => t.status).join() === "pass,missing", s);
  await upload("f6f6f6f", 5, [["opens", "pass"], ["refuses", "fail"]]);
  s = await status();
  check(s.status === "red" && s.tests[1].message === "boom", s);
  await upload("a7a7a7a", 6, [["opens", "pass"], ["refuses", "pass"]]);
  check((await status()).status === "green", "recovered");
  const [, overview] = await call("/tests");
  const row = overview.items.find((i: any) => i.address === "ac:key-rights");
  check(row?.status === "green" && row.ancestors.includes("capability:restrict") && row.ancestors.includes("domain:security"), overview);

  // A text change makes the list a draft until it is saved again.
  await revise(1, "Only read and verify until launch");
  s = await status();
  check(s.status === "grey" && s.reason === "tests_draft" && s.tests_draft, s);
  await ok({ op: "set_ac_tests", target: "ac:key-rights", tests: [A, B], reason: "Still proves it" }, agent);
  s = await status();
  check(s.status === "green" && s.tests_revision === 2, s);

  const [bad, body] = await call("/commands", { request_id: crypto.randomUUID(), command: { op: "set_ac_tests", target: "ac:key-rights", tests: ["login ok"], reason: "Bad" } }, agent);
  check(bad === 409 && JSON.stringify(body).includes("file > describe > name"), body);
  console.log("reports smoke: ok");
} finally {
  server.kill();
  rmSync(temp, { recursive: true, force: true });
}
