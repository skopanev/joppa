import { expect, test } from "bun:test";
import { isFull, junit, parse } from "./cargo-junit";

const OUTPUT = `     Running unittests src/lib.rs (target/debug/deps/joppa-1)
test objects::tests::address ... ok
     Running tests/title_length.rs (target/debug/deps/title_length-2)
test new_titles_fit ... FAILED
test needs_db ... ignored, requires isolated JOPPA_TEST_DATABASE_URL

failures:

---- new_titles_fit stdout ----
thread 'new_titles_fit' panicked at tests/title_length.rs:13:5:
title is at most 64 characters

failures:
    new_titles_fit
test result: FAILED. 0 passed; 1 failed; 1 ignored`;

test("cargo output becomes file > name addresses with failures and skips", () => {
  const suites = parse(OUTPUT);
  expect([...suites.keys()]).toEqual(["src/lib.rs", "tests/title_length.rs"]);
  const cases = suites.get("tests/title_length.rs")!;
  expect(cases.map((c) => [c.name, c.status])).toEqual([["new_titles_fit", "fail"], ["needs_db", "skipped"]]);
  expect(cases[0].message).toContain("title is at most 64 characters");
  expect(junit(suites)).toContain('<testsuite name="tests/title_length.rs">');
});

test("a run is full only without a filter or a target selection", () => {
  expect(isFull([])).toBe(true);
  expect(isFull(["--", "--ignored", "--test-threads=1"])).toBe(true);
  expect(isFull(["--features", "x"])).toBe(true);
  expect(isFull(["--test", "title_length"])).toBe(false);
  expect(isFull(["titles"])).toBe(false);
  expect(isFull(["--", "titles"])).toBe(false);
});
