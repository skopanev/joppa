// The stamp: what a test run knows about the code it ran on.
import { mkdirSync, writeFileSync } from "node:fs";

const git = (...args: string[]) =>
  Bun.spawnSync(["git", ...args]).stdout.toString().trim();

export function writeStamp(full: boolean) {
  const stamp = {
    commit: git("rev-parse", "HEAD"),
    commit_time: git("show", "-s", "--format=%cI", "HEAD"),
    order: Number(git("rev-list", "--count", "--first-parent", "HEAD")),
    branch: git("rev-parse", "--abbrev-ref", "HEAD"),
    dirty: git("status", "--porcelain", "--untracked-files=no") !== "",
    full,
  };
  mkdirSync(".joppa", { recursive: true });
  writeFileSync(".joppa/stamp.json", JSON.stringify(stamp, null, 1) + "\n");
  return stamp;
}
