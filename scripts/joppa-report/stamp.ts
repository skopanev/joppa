// Bun preload: writes .joppa/stamp.json next to the JUnit report of `bun test`.
// Bun does not show its arguments to a preload, so a run is full only when the
// project's full test script sets JOPPA_FULL_RUN=1.
//   bunfig.toml:
//     [test]
//     preload = ["./scripts/joppa-report/stamp.ts"]
//     [test.reporter]
//     junit = ".joppa/junit.xml"
import { writeStamp } from "./stamp-lib";

writeStamp(process.env.JOPPA_FULL_RUN === "1");
