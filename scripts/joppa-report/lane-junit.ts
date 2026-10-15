#!/usr/bin/env bun
// Turn the worker logs of a full lane run into .joppa/junit.xml and a full stamp.
//   bun scripts/joppa-report/lane-junit.ts worker-0.log worker-1.log
// The pre-push hook uploads it with the pushed commit, so the gate is the report.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { junit, parse } from "./cargo-junit";
import { writeStamp } from "./stamp-lib";

const output = Bun.argv.slice(2).map((file) => readFileSync(file, "utf8")).join("\n");
const suites = parse(output);
if (!suites.size) throw new Error("lane-junit: the worker logs have no test results");
mkdirSync(".joppa", { recursive: true });
writeFileSync(".joppa/junit.xml", junit(suites));
writeStamp(true);
