"""The server platform as the repository defines it: the host stack, PostgreSQL, Caddy,
systemd, the backup, the install checks and the build. No network and no server."""
import json
import pathlib
import re
import subprocess
import sys
import tempfile
import unittest

SOURCE = pathlib.Path(__file__).resolve().parents[1]
SERVER = SOURCE / "server"


def read(name):
    return (SERVER / name).read_text()


def unit(name):
    """The key=value lines of a systemd unit. A repeated key keeps every value."""
    values = {}
    for line in read(name).splitlines():
        if "=" in line and not line.startswith(("#", "[")):
            key, value = line.split("=", 1)
            values.setdefault(key.strip(), []).append(value.strip())
    return values


def validator():
    """The Python check of the private files from server/install.sh."""
    text = read("install.sh")
    match = re.search(r"python3 - \"\$stage/private\" \"\$domain\" <<'PY'\n(.*?)\nPY\n", text, re.S)
    assert match, "install.sh has no private file check"
    return match.group(1)


class PlatformConfig(unittest.TestCase):
    def test_provision_installs_ubuntu_24_04_postgresql_16_pgvector_0_8_6_and_caddy(self):
        text = read("provision.sh")
        self.assertIn("$ID == ubuntu && $VERSION_ID == 24.04", text)
        self.assertIn("postgresql-16 postgresql-server-dev-16", text)
        self.assertIn("pgvector/pgvector/archive/refs/tags/v0.8.6.tar.gz", text)
        self.assertIn("PG_CONFIG=/usr/lib/postgresql/16/bin/pg_config", text)
        self.assertIn("https://dl.cloudsmith.io/public/caddy/stable/", text)
        self.assertIn("apt-get install -y -qq caddy", text)
        for rule in ("ufw allow OpenSSH", "ufw allow 80/tcp", "ufw allow 443/tcp", "ufw --force enable"):
            self.assertIn(rule, text)
        self.assertIn("fallocate -l 2G /swapfile-joppa", text)
        self.assertIn("sh /tmp/joppa-rustup.sh -y --profile minimal", text)

    def test_postgres_uses_the_2_gb_budget_on_loopback(self):
        settings = {}
        for line in read("postgres-2gb.conf").splitlines():
            if line.strip() and not line.startswith("#"):
                key, value = (part.strip() for part in line.split("=", 1))
                settings[key] = value
        self.assertEqual(settings, {
            "listen_addresses": "'127.0.0.1'",
            "max_connections": "30",
            "shared_buffers": "'256MB'",
            "effective_cache_size": "'768MB'",
            "work_mem": "'4MB'",
            "maintenance_work_mem": "'64MB'",
            "autovacuum_work_mem": "'32MB'",
            "max_parallel_workers_per_gather": "0",
            "max_parallel_maintenance_workers": "0",
        })

    def test_caddy_serves_https_and_forwards_to_port_6212(self):
        lines = [line.strip() for line in read("Caddyfile").splitlines()]
        self.assertEqual(lines[0], "{$JOPPA_DOMAIN} {")
        for line in (
            "encode zstd gzip",
            "X-Content-Type-Options nosniff",
            "Referrer-Policy strict-origin-when-cross-origin",
            "X-Frame-Options DENY",
            'Strict-Transport-Security "max-age=31536000"',
            "-Server",
            "reverse_proxy 127.0.0.1:6212",
        ):
            self.assertIn(line, lines)

    def test_service_runs_serve_on_loopback_with_memory_and_task_limits(self):
        values = unit("joppa.service")
        expected = {
            "ExecStart": ["/opt/joppa/bin/joppa serve 6212 127.0.0.1"],
            "User": ["joppa"],
            "Group": ["joppa"],
            "WorkingDirectory": ["/var/lib/joppa"],
            "EnvironmentFile": ["/etc/joppa/runtime.env", "-/etc/joppa/storage.env", "-/etc/joppa/search.env"],
            "Restart": ["on-failure"],
            "RestartSec": ["3"],
            "MemoryHigh": ["384M"],
            "MemoryMax": ["512M"],
            "TasksMax": ["128"],
            "LimitNOFILE": ["8192"],
            "NoNewPrivileges": ["true"],
            "ProtectSystem": ["strict"],
            "ReadWritePaths": ["/var/lib/joppa"],
            "Requires": ["postgresql.service"],
        }
        for key, value in expected.items():
            self.assertEqual(values.get(key), value, key)

    def test_backup_timer_runs_daily_at_03_30_utc(self):
        timer = unit("joppa-backup.timer")
        self.assertEqual(timer["OnCalendar"], ["*-*-* 03:30:00 UTC"])
        self.assertEqual(timer["RandomizedDelaySec"], ["900"])
        self.assertEqual(timer["Persistent"], ["true"])
        service = unit("joppa-backup.service")
        self.assertEqual(service["User"], ["postgres"])
        self.assertEqual(service["Type"], ["oneshot"])
        self.assertEqual(service["EnvironmentFile"], ["/etc/joppa/backup.env"])
        self.assertEqual(service["ExecStart"], ["/opt/joppa/server/backup.sh"])

    def test_backup_uploads_a_verified_custom_dump_with_sigv4(self):
        text = read("backup.sh")
        self.assertIn("pg_dump --format=custom --no-owner --no-acl", text)
        self.assertIn('pg_restore --list "$temporary"', text)
        self.assertIn('--aws-sigv4 "aws:amz:${SPACES_REGION}:s3"', text)
        self.assertIn('put "postgres/joppa-$stamp.dump"', text)
        self.assertIn('put "postgres/latest.dump"', text)
        self.assertIn('[[ "$SPACES_ENDPOINT" == https://* ]]', text)
        self.assertIn("flock -n 9", text)
        self.assertIn("stamp=$(date -u +%Y%m%dT%H%M%SZ)", text)
        # The steps go in this order: dump, check, then the two uploads.
        self.assertLess(text.index("pg_dump"), text.index("pg_restore --list"))
        self.assertLess(text.index("pg_restore --list"), text.index('put "postgres/'))

    def test_install_validates_private_config_files(self):
        script = validator()
        domain = "app.example.test"
        token = "t" * 24
        valid = {
            "tokens.json": {token: "owner"},
            "login-users.json": {"owner@example.test": "owner"},
            "google-client.json": {"client_id": "id", "client_secret": "secret",
                                   "redirect_uri": f"https://{domain}/auth/google/callback"},
            "access-rules.json": {"domains": {"example.test": ["product"]}, "emails": {}},
        }
        changes = {
            "short token": ("tokens.json", {"t" * 23: "owner"}),
            "unknown actor": ("login-users.json", {"owner@example.test": "nobody"}),
            "wrong callback": ("google-client.json", dict(valid["google-client.json"], redirect_uri="https://other.test/auth/google/callback")),
            "unknown rule key": ("access-rules.json", {"domains": {}, "teams": {}}),
        }

        def run(files):
            with tempfile.TemporaryDirectory(prefix="joppa-private-") as folder:
                for name, value in files.items():
                    (pathlib.Path(folder) / name).write_text(json.dumps(value))
                return subprocess.run([sys.executable, "-", folder, domain], input=script,
                                      text=True, capture_output=True).returncode

        self.assertEqual(run(valid), 0)
        for case, (name, value) in changes.items():
            with self.subTest(case):
                self.assertNotEqual(run(dict(valid, **{name: value})), 0)

    def test_install_keeps_the_previous_binary_and_checks_health(self):
        text = read("install.sh")
        self.assertIn("cp -p /opt/joppa/bin/joppa /opt/joppa/bin/joppa.previous", text)
        self.assertIn("cargo build --release --locked -j 1", text)
        self.assertIn("/opt/joppa/bin/joppa migrate", text)
        self.assertIn("/opt/joppa/bin/joppa backfill-board", text)
        self.assertIn("for attempt in {1..15}; do", text)
        self.assertIn("curl --fail --silent --max-time 4 http://127.0.0.1:6212/health", text)
        self.assertIn("sleep 2", text)
        self.assertIn("restore_previous || true", text)
        self.assertIn("Refusing to restore over an existing database", text)
        # The new binary replaces the old one only after the service stops, and the
        # projections migrate before the service starts again.
        self.assertLess(text.index("systemctl stop joppa"), text.index("mv /opt/joppa/bin/joppa.next"))
        self.assertLess(text.index("/opt/joppa/bin/joppa migrate"), text.index("systemctl restart joppa\nhealthy"))

    def test_the_binary_is_rust_2024_axum_0_8_with_embedded_markdown_libraries(self):
        cargo = (SOURCE / "Cargo.toml").read_text()
        for line in ('edition = "2024"', 'rust-version = "1.90"', 'axum = "0.8"', 'deadpool-postgres = "0.14"',
                     'tokio-postgres = "0.7"', 'reqwest = { version = "0.13"'):
            self.assertIn(line, cargo)
        self.assertRegex(cargo, r'\ntokio = \{ version = "1"')
        vendor = SOURCE / "src" / "vendor"
        self.assertIn("marked v18.1.2", (vendor / "marked.umd.js").read_text()[:200])
        self.assertIn("DOMPurify 3.4.17", (vendor / "purify.min.js").read_text()[:200])
        routes = (SOURCE / "src" / "web_server.rs").read_text()
        self.assertIn('include_str!("vendor/marked.umd.js")', routes)
        self.assertIn('include_str!("vendor/purify.min.js")', routes)


if __name__ == "__main__":
    unittest.main()
