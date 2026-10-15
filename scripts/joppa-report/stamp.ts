// Bun preload: writes .joppa/stamp.json next to the JUnit report of `bun test`.
// A run is full only when it has no test name pattern, no path or file argument
// and no --only. Bun does not show its arguments to a preload in process.argv, so
// the preload reads the command line from the operating system.
//   bunfig.toml:
//     [test]
//     preload = ["./scripts/joppa-report/stamp.ts"]
//     [test.reporter]
//     junit = ".joppa/junit.xml"
import { readFileSync } from "node:fs";
import { writeStamp } from "./stamp-lib";

// The arguments after `test`, or undefined when they cannot be read.
function testArgs(): string[] | undefined {
  if (process.argv[1] === "test") return process.argv.slice(2);
  let cmd: string[] = [];
  try {
    cmd = readFileSync("/proc/self/cmdline", "utf8").split("\0").slice(0, -1);
  } catch {
    try {
      // No /proc (macOS): ps joins the arguments with spaces, which can only add arguments.
      const ps = Bun.spawnSync(["ps", "-ww", "-o", "args=", "-p", String(process.pid)]);
      if (ps.exitCode === 0) cmd = ps.stdout.toString().trim().split(/\s+/);
    } catch {}
  }
  const at = cmd.indexOf("test");
  return at < 1 ? undefined : cmd.slice(at + 1);
}

// Any argument that is not a flag is a path, a file or a flag value; a flag value
// only makes the run partial, never full. Every argument after `--` is a file filter.
const isFull = (args: string[] | undefined) =>
  args !== undefined &&
  args.every((a, i) => a.startsWith("-") && !/^(-t|--test-name-pattern)/.test(a) && a !== "--only" && args[i - 1] !== "--");

writeStamp(isFull(testArgs()));
