"""Deployment packaging and failure cleanup, without a server or real secrets."""
import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

SOURCE = pathlib.Path(__file__).resolve().parents[1]


class Deployment(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="joppa-deploy-test-")
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.repo = self.root / "repo"
        (self.repo / "server").mkdir(parents=True)
        shutil.copy(SOURCE / "server/deploy.sh", self.repo / "server/deploy.sh")
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=Test", "-c", "user.email=test@example.test", "commit", "-qm", "fixture")
        self.config = self.root / "private config"
        self.config.mkdir()
        for name in ("tokens.json", "login-users.json", "google-client.json", "access-rules.json", "backup.env", "storage.env", "search.env", "unrelated-secret"):
            (self.config / name).write_text("test-only-value")
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "ssh.log"
        ssh = self.bin / "ssh"
        ssh.write_text('''#!/usr/bin/env python3
import io, json, os, pathlib, subprocess, sys, tarfile
cmd = sys.argv[-1]
with open(os.environ['TEST_LOG'], 'a') as f: f.write(json.dumps(cmd)+'\\n')
if 'mktemp -d' in cmd:
    print('/root/joppa-deploy.TEST1234')
elif cmd.startswith('tar -xzf -'):
    with tarfile.open(fileobj=sys.stdin.buffer, mode='r|gz') as archive:
        names = []
        for entry in archive:
            names.append(entry.name)
            if entry.name == './source.tar' and os.environ.get('TEST_MOVE_HEAD'):
                with tarfile.open(fileobj=io.BytesIO(archive.extractfile(entry).read())) as source:
                    pathlib.Path(os.environ['TEST_PACKED_MARKER']).write_bytes(source.extractfile('commit-marker').read())
    pathlib.Path(os.environ['TEST_CONTENTS']).write_text(json.dumps(names))
elif 'install.sh' in cmd:
    if os.environ.get('TEST_MOVE_HEAD'):
        subprocess.run(['git', '-C', os.environ['TEST_REPO'], 'checkout', '--detach', os.environ['TEST_MOVE_HEAD']], check=True, capture_output=True)
    sys.exit(int(os.environ.get('TEST_INSTALL_EXIT', '0')))
''')
        ssh.chmod(0o755)
        curl = self.bin / "curl"
        curl.write_text('#!/bin/sh\nprintf \'{"ok":true}\\n\'\n')
        curl.chmod(0o755)
        self.env = {**os.environ, "PATH": str(self.bin) + ":" + os.environ["PATH"],
                    "TEST_LOG": str(self.log), "TEST_CONTENTS": str(self.root / "contents.json")}

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], check=True, capture_output=True)

    def deploy(self, target="root@server.example", **extra):
        return subprocess.run(["bash", str(self.repo / "server/deploy.sh"), target,
                               "app.example.test", str(self.config)], env={**self.env, **extra},
                              text=True, capture_output=True)

    def test_only_whitelisted_private_files_are_transferred(self):
        result = self.deploy()
        self.assertEqual(result.returncode, 0, result.stderr)
        names = json.loads((self.root / "contents.json").read_text())
        self.assertNotIn("./private/unrelated-secret", names)
        self.assertIn("./private/backup.env", names)
        self.assertIn("./private/search.env", names)
        self.assertIn("./private/access-rules.json", names)
        self.assertIn("./source.tar", names)
        self.assertIn("rm -rf -- '/root/joppa-deploy.TEST1234'", self.log.read_text())
        self.assertNotIn("test-only-value", result.stdout + result.stderr)

    def test_remote_failure_is_reported_and_staging_removed(self):
        result = self.deploy(TEST_INSTALL_EXIT="23")
        self.assertEqual(result.returncode, 23)
        self.assertNotIn("Deployed", result.stdout)
        self.assertIn("rm -rf -- '/root/joppa-deploy.TEST1234'", self.log.read_text())

    def test_dirty_checkout_is_rejected_before_connecting(self):
        """[release-commit-integrity-ac1] Reject a dirty release checkout."""
        (self.repo / "private-data").write_text("must not ship")
        result = self.deploy()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())

    def test_head_change_cannot_change_the_packaged_or_reported_commit(self):
        """[release-commit-integrity-ac3] Keep the selected commit during concurrent work."""
        marker = self.repo / 'commit-marker'
        marker.write_text('selected commit')
        self.git('add', '.')
        self.git('-c', 'user.name=Test', '-c', 'user.email=test@example.test', 'commit', '-qm', 'selected')
        selected = self.git('rev-parse', 'HEAD').stdout.decode().strip()
        short = self.git('rev-parse', '--short', selected).stdout.decode().strip()
        marker.write_text('another commit')
        self.git('add', '.')
        self.git('-c', 'user.name=Test', '-c', 'user.email=test@example.test', 'commit', '-qm', 'another')
        changed = self.git('rev-parse', 'HEAD').stdout.decode().strip()
        self.git('checkout', '--detach', selected)
        packed = self.root / 'packed-marker'
        result = self.deploy(TEST_MOVE_HEAD=changed, TEST_REPO=str(self.repo), TEST_PACKED_MARKER=str(packed))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(packed.read_text(), 'selected commit')
        self.assertIn('Deployed ' + short + ' at ', result.stdout)
        self.assertEqual(self.git('rev-parse', 'HEAD').stdout.decode().strip(), changed)

    def test_ssh_option_injection_is_rejected(self):
        result = self.deploy(target="-oProxyCommand=unexpected")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())

    def test_environment_configuration(self):
        env = {**self.env, "JOPPA_DEPLOY_HOST": "root@server.example",
               "JOPPA_DOMAIN": "app.example.test", "JOPPA_CONFIG_DIR": str(self.config)}
        result = subprocess.run(["bash", str(self.repo / "server/deploy.sh")],
                                env=env, text=True, capture_output=True)
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
