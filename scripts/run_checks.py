#!/usr/bin/env python3
"""Seal the manifest BEFORE running commands; send every failure and skip.
The manifest is the full declared check scope, reviewed like the test code.
JOPPA_ACTOR is the runner identity assigned by the trusted local launcher.
"""
import argparse
import json
import os
import subprocess
import sys
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("workspace")
    parser.add_argument("manifest", help="JSON: req, revision, subject, scope, discovery_complete, checks")
    parser.add_argument("--joppa", default=os.environ.get("JOPPA_BIN", "joppa"))
    args = parser.parse_args()
    with open(args.manifest, encoding="utf-8") as source:
        manifest = json.load(source)
    run = "run-" + str(uuid.uuid4())
    base = {"req": manifest["req"], "revision": manifest["revision"], "run": run}
    checks = manifest["checks"]
    for check in checks:
        if check["method"] != "automated" or not isinstance(check.get("command"), list) or not check["command"]:
            parser.error("this adapter runs automated checks; manual/mixed plans use the API")
    def send(op, **fields):
        command = {"op": op, **base, **fields}
        subprocess.run([args.joppa, "command", args.workspace, str(uuid.uuid4())],
                       input=json.dumps(command), text=True, check=True, stdout=subprocess.DEVNULL)
    send("plan", subject=manifest["subject"], scope=manifest["scope"],
         discovery_complete=manifest["discovery_complete"],
         checks=[{**{k: c[k] for k in ("id", "ac", "method", "expected")}, **({"tasks":c["tasks"]} if c.get("tasks") else {})} for c in checks])
    failed = not manifest["discovery_complete"]
    for check in checks:
        try:
            # No shell interpolation. Timeouts and launch errors are failures too.
            result = subprocess.run(check["command"], cwd=check.get("cwd"), capture_output=True,
                                    text=True, timeout=check.get("timeout_seconds", 300))
            passed = result.returncode == 0
            # Reports retain a bounded result; full logs may be attached separately.
            observed = ["exit code " + str(result.returncode)]
            note = (result.stdout + result.stderr)[-12000:] or "Process completed without output"
        except (OSError, subprocess.TimeoutExpired) as error:
            passed, observed, note = False, [type(error).__name__], str(error)
        failed |= not passed
        send("result", evidence={"check": check["id"], "outcome": "pass" if passed else "fail",
                                "observed": observed, "note": note, "link": check.get("evidence_url")})
    send("finish")
    print(json.dumps({"run": run, "passed": not failed}))
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
