"""Local full export packaging, Spaces SHA-256 verification and failure cleanup."""
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import io
import json
import os
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import threading
import unittest

SOURCE = pathlib.Path(__file__).resolve().parents[1]


class MockSpacesHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        server = self.server
        path_only, _, query = self.path.partition("?")
        server.requests.append((path_only, query))
        if "X-Amz-Signature=" not in query or "X-Amz-Algorithm=AWS4-HMAC-SHA256" not in query:
            self.send_response(403)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        payload = server.blobs.get(path_only)
        if payload is None:
            self.send_response(404)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _fmt, *_args):
        return


class ExportBackup(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="joppa-export-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.repo = self.root / "repo"
        (self.repo / "server").mkdir(parents=True)
        shutil.copy(SOURCE / "server/export.sh", self.repo / "server/export.sh")
        shutil.copy(SOURCE / "server/export_bundle.py", self.repo / "server/export_bundle.py")
        (self.repo / "server/export.sh").chmod(0o755)
        (self.repo / "server/export_bundle.py").chmod(0o755)

        self.blob_bytes = b"attachment content for export verification\n"
        self.blob_hash = hashlib.sha256(self.blob_bytes).hexdigest()
        self.swept_hash = "b" * 64

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), MockSpacesHandler)
        self.httpd.blobs = {f"/test-bucket/workspaces/alpha/{self.blob_hash}": self.blob_bytes}
        self.httpd.requests = []
        self.thread = threading.Thread(target=self.httpd.serve_forever, daemon=True)
        self.thread.start()
        self.addCleanup(self.httpd.server_close)
        self.addCleanup(self.httpd.shutdown)

        self.config = self.root / "private-config"
        self.config.mkdir()
        port = self.httpd.server_address[1]
        (self.config / "storage.env").write_text(
            f"SPACES_BUCKET=test-bucket\n"
            f"SPACES_ENDPOINT=http://127.0.0.1:{port}\n"
            f"SPACES_REGION=ams3\n"
            f"AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\n"
            f"AWS_SECRET_ACCESS_KEY=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\n"
        )

        self.raw_fixture = self.root / "raw-fixture"
        self.raw_fixture.mkdir()
        (self.raw_fixture / "joppa.dump").write_bytes(b"PGDMP-custom-fixture")
        (self.raw_fixture / "raw_workspaces.jsonl").write_text(
            json.dumps({"id": "alpha", "created_at": "2026-11-17T00:00:00Z", "aliases": ["alpha"]}) + "\n"
        )
        (self.raw_fixture / "raw_state_entries.jsonl").write_text(
            "\n".join(
                [
                    json.dumps(
                        {
                            "workspace": "alpha",
                            "section": "meta",
                            "key": "tree_schema",
                            "kind": None,
                            "value": {"revision": 1, "types": {"ac": {"role": "criterion"}}},
                            "compact": None,
                        }
                    ),
                    json.dumps(
                        {
                            "workspace": "alpha",
                            "section": "notes",
                            "key": "ac:one",
                            "kind": None,
                            "value": {
                                "comments": {"c1": {"actor": "alice", "text": "hello"}},
                                "attachments": {
                                    "f1": {
                                        "name": "spec.txt",
                                        "content_type": "text/plain",
                                        "size": len(self.blob_bytes),
                                        "hash": self.blob_hash,
                                    },
                                    "f2": {
                                        "name": "old.txt",
                                        "content_type": "text/plain",
                                        "size": 12,
                                        "hash": self.swept_hash,
                                        "removed": True,
                                    },
                                },
                            },
                            "compact": None,
                        }
                    ),
                ]
            )
            + "\n"
        )
        (self.raw_fixture / "raw_objects.jsonl").write_text(
            json.dumps(
                {
                    "workspace": "alpha",
                    "address": "ac:one",
                    "kind": "ac",
                    "local_id": "one",
                    "revision": 2,
                    "parent": None,
                    "present": True,
                    "title": "First criterion",
                    "document": {"data": {"title": "First criterion"}},
                }
            )
            + "\n"
        )
        (self.raw_fixture / "raw_revisions.jsonl").write_text(
            json.dumps(
                {
                    "workspace": "alpha",
                    "address": "ac:one",
                    "revision": 1,
                    "document": {"data": {"title": "Draft"}},
                }
            )
            + "\n"
        )
        (self.raw_fixture / "raw_test_reports.jsonl").write_text(
            json.dumps({"id": 7, "workspace": "alpha", "commit_sha": "abcdef1", "recorded": True, "junit": ""}) + "\n"
        )
        (self.raw_fixture / "raw_test_results.jsonl").write_text(
            json.dumps({"workspace": "alpha", "report_id": 7, "address": "test:t1", "status": "pass"}) + "\n"
        )
        (self.raw_fixture / "raw_history_marks.jsonl").write_text(
            json.dumps({"workspace": "alpha", "position": 1, "actor": "alice", "reason": "ok", "nodes": ["ac:one"]}) + "\n"
        )
        (self.raw_fixture / "raw_events.jsonl").write_text(
            json.dumps(
                {
                    "id": 1,
                    "workspace": "alpha",
                    "position": 1,
                    "request_id": "req-1",
                    "actor": "alice",
                    "alias": None,
                    "recorded_at": "2026-11-17T00:00:01Z",
                    "command": {"op": "save_object"},
                }
            )
            + "\n"
        )

        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "ssh.log"
        ssh = self.bin / "ssh"
        ssh.write_text(
            """#!/usr/bin/env python3
import io, json, os, pathlib, sys, tarfile
cmd = sys.argv[-1]
with open(os.environ['TEST_LOG'], 'a') as f:
    f.write(json.dumps(cmd) + '\\n')
if 'mktemp -d /var/backups/joppa/export.' in cmd:
    print('/var/backups/joppa/export.TEST1234')
elif 'pg_dump --format=custom' in cmd:
    sys.exit(int(os.environ.get('TEST_REMOTE_EXIT', '0')))
elif cmd.startswith('tar -czf -'):
    buf = io.BytesIO()
    raw = pathlib.Path(os.environ['TEST_RAW_FIXTURE'])
    with tarfile.open(fileobj=buf, mode='w:gz') as tf:
        for item in sorted(raw.iterdir()):
            tf.add(item, arcname=item.name)
    sys.stdout.buffer.write(buf.getvalue())
"""
        )
        ssh.chmod(0o755)
        pg_restore = self.bin / "pg_restore"
        pg_restore.write_text("#!/bin/sh\nexit 0\n")
        pg_restore.chmod(0o755)

        self.out = self.root / "exports"
        self.env = {
            **os.environ,
            "PATH": f"{self.bin}:{os.environ['PATH']}",
            "TEST_LOG": str(self.log),
            "TEST_RAW_FIXTURE": str(self.raw_fixture),
            "JOPPA_DEPLOY_HOST": "root@server.example",
            "JOPPA_CONFIG_DIR": str(self.config),
        }

    def run_export(self, *args, **extra_env):
        return subprocess.run(
            ["bash", str(self.repo / "server/export.sh"), *args],
            env={**self.env, **extra_env},
            text=True,
            capture_output=True,
        )

    def test_ssh_option_injection_is_rejected(self):
        result = self.run_export("-oProxyCommand=bad", str(self.config), str(self.out))
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())

    def test_remote_failure_cleans_up_remote_staging(self):
        result = self.run_export(str(self.out), TEST_REMOTE_EXIT="19")
        self.assertEqual(result.returncode, 19)
        self.assertIn("rm -rf -- '/var/backups/joppa/export.TEST1234'", self.log.read_text())

    def test_exports_complete_verified_archive_and_reuses_blob_cache(self):
        first = self.run_export(str(self.out), JOPPA_EXPORT_STAMP="20261117T080001Z")
        self.assertEqual(first.returncode, 0, first.stderr)
        summary1 = json.loads(first.stdout.strip())
        self.assertEqual(summary1["workspaces"], 1)
        self.assertEqual(summary1["objects"], 1)
        self.assertEqual(summary1["revisions"], 1)
        self.assertEqual(summary1["notes"], 1)
        self.assertEqual(summary1["events"], 1)
        self.assertEqual(summary1["files_exported"], 1)
        self.assertEqual(summary1["files_downloaded"], 1)
        self.assertEqual(summary1["files_cached"], 0)
        self.assertEqual(summary1["files_swept_detached"], 1)

        archive_path = pathlib.Path(summary1["archive"])
        self.assertEqual(archive_path.stat().st_mode & 0o777, 0o600)
        with tarfile.open(archive_path, mode="r:gz") as tf:
            root_prefix = archive_path.name[: -len(".tar.gz")]
            ws_json = json.loads(tf.extractfile(f"{root_prefix}/workspaces/alpha/workspace.json").read())
            self.assertEqual(ws_json["schema"]["revision"], 1)
            self.assertEqual(ws_json["counts"]["present_objects"], 1)
            blob = tf.extractfile(f"{root_prefix}/files/alpha/{self.blob_hash}").read()
            self.assertEqual(blob, self.blob_bytes)
            dump = tf.extractfile(f"{root_prefix}/postgres/joppa.dump").read()
            self.assertEqual(dump, b"PGDMP-custom-fixture")
            swept = json.loads(tf.extractfile(f"{root_prefix}/files/swept.json").read())
            self.assertEqual([row["hash"] for row in swept], [self.swept_hash])

        before_requests = len(self.httpd.requests)
        second = self.run_export(str(self.out), JOPPA_EXPORT_STAMP="20261117T080002Z")
        self.assertEqual(second.returncode, 0, second.stderr)
        summary2 = json.loads(second.stdout.strip())
        self.assertEqual(summary2["files_downloaded"], 0)
        self.assertEqual(summary2["files_cached"], 1)
        # Only the detached swept blob is probed again; the active blob came from .files-cache.
        self.assertEqual(len(self.httpd.requests) - before_requests, 1)

    def test_corrupted_spaces_blob_fails_closed(self):
        self.httpd.blobs[f"/test-bucket/workspaces/alpha/{self.blob_hash}"] = b"tampered"
        result = self.run_export(str(self.root / "corrupt-out"))
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        archives = list((self.root / "corrupt-out").glob("joppa-export-*.tar.gz"))
        self.assertEqual(archives, [])


if __name__ == "__main__":
    unittest.main()
