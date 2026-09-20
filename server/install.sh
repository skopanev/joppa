#!/usr/bin/env bash
# Invoked by deploy.sh as root; arguments contain no credentials.
set -euo pipefail
umask 077
[[ $EUID == 0 && $# == 2 ]] || { echo 'Run via server/deploy.sh as root' >&2; exit 2; }
domain=$1 stage=$2
[[ "$domain" =~ ^[a-z0-9]([a-z0-9.-]*[a-z0-9])?$ && "$domain" == *.* ]] || exit 2
source_dir="$stage/source"
exec 9>/run/lock/joppa-deploy.lock
flock -n 9 || { echo 'Another deployment is running' >&2; exit 1; }
# Reject invalid private inputs before changing services; never print their content.
python3 - "$stage/private" "$domain" <<'PY'
import json, pathlib, sys
p = pathlib.Path(sys.argv[1])
try:
    tokens = json.loads((p / 'tokens.json').read_text())
    users = json.loads((p / 'login-users.json').read_text())
    assert isinstance(tokens, dict) and tokens
    assert all(isinstance(k, str) and len(k) >= 24 and isinstance(v, str) and v for k, v in tokens.items())
    assert isinstance(users, dict) and users
    assert all(isinstance(k, str) and '@' in k and v in tokens.values() for k, v in users.items())
    google = p / 'google-client.json'
    if google.exists():
        client = json.loads(google.read_text())
        assert client['client_id'] and client['client_secret']
        assert client['redirect_uri'] == f'https://{sys.argv[2]}/auth/google/callback'
except Exception:
    sys.exit('Invalid private login configuration or Google callback URL')
PY
if [[ ! -x /root/.cargo/bin/cargo || ! -f /usr/share/postgresql/16/extension/vector.control || ! -d /etc/joppa ]] || ! command -v caddy >/dev/null; then
    bash "$source_dir/server/provision.sh"
fi
rsync -a --delete --exclude target/ "$source_dir/" /opt/joppa/src/
cd /opt/joppa/src
/root/.cargo/bin/cargo build --release --locked -j 1
install -m 644 server/postgres-2gb.conf /etc/postgresql/16/main/conf.d/joppa.conf
systemctl enable --now postgresql
systemctl reload postgresql
if [[ $(runuser -u postgres -- psql -X -Atqc 'SELECT count(*) FROM pg_settings WHERE pending_restart') != 0 ]]; then
    systemctl restart postgresql
fi
db_exists=$(runuser -u postgres -- psql -X -Atqc "SELECT 1 FROM pg_database WHERE datname='joppa'")
if [[ -f "$stage/initial.dump" && "$db_exists" == 1 ]]; then
    echo 'Refusing to restore over an existing database. Omit INITIAL_DUMP for updates.' >&2
    exit 1
fi
if [[ "$db_exists" != 1 ]]; then
    runuser -u postgres -- createdb joppa
    if [[ -f "$stage/initial.dump" ]]; then
        runuser -u postgres -- pg_restore --exit-on-error --single-transaction --no-owner --no-acl -d joppa < "$stage/initial.dump"
        rm -f "$stage/initial.dump"
    fi
fi
runuser -u postgres -- psql -X -v ON_ERROR_STOP=1 -d joppa < migrations/001_journal.sql
runuser -u postgres -- psql -X -v ON_ERROR_STOP=1 -d joppa -c 'CREATE EXTENSION IF NOT EXISTS vector'
# Generate a database password only once and keep it out of argv, logs and Git.
python3 - <<'PY'
import os, pathlib, secrets, subprocess
config = pathlib.Path('/etc/joppa/runtime.env')
if not config.exists():
    password = secrets.token_hex(32)
    sql = "CREATE ROLE joppa_app LOGIN PASSWORD '" + password + "' NOSUPERUSER NOCREATEDB NOCREATEROLE;"
    result = subprocess.run(['runuser','-u','postgres','--','psql','-X','-v','ON_ERROR_STOP=1','-d','joppa'], input=sql, text=True, capture_output=True)
    if result.returncode:
        raise SystemExit('Database role setup failed; runtime configuration was not changed.')
    with os.fdopen(os.open(config, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o640), 'w') as f:
        f.write('JOPPA_DATABASE_URL=postgresql://joppa_app:'+password+'@127.0.0.1:5432/joppa\nJOPPA_TOKENS_FILE=/etc/joppa/tokens.json\nJOPPA_LOGIN_USERS_FILE=/etc/joppa/login-users.json\n')
PY
runuser -u postgres -- psql -X -v ON_ERROR_STOP=1 -d joppa <<'SQL'
GRANT CONNECT ON DATABASE joppa TO joppa_app;
GRANT USAGE ON SCHEMA joppa TO joppa_app;
GRANT SELECT, INSERT ON ALL TABLES IN SCHEMA joppa TO joppa_app;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA joppa TO joppa_app;
SQL
for name in tokens.json login-users.json google-client.json; do
    [[ ! -f "$stage/private/$name" ]] || install -m 640 -o root -g joppa "$stage/private/$name" "/etc/joppa/$name"
done
for name in backup.env storage.env; do
    [[ ! -f "$stage/private/$name" ]] || install -m 600 -o root -g root "$stage/private/$name" "/etc/joppa/$name"
done
if [[ -f /etc/joppa/google-client.json ]] && ! grep -q '^JOPPA_GOOGLE_CLIENT_FILE=' /etc/joppa/runtime.env; then
    printf 'JOPPA_GOOGLE_CLIENT_FILE=/etc/joppa/google-client.json\n' >> /etc/joppa/runtime.env
fi
chown root:joppa /etc/joppa/runtime.env
chmod 640 /etc/joppa/runtime.env
install -m 644 server/joppa.service /etc/systemd/system/joppa.service
install -m 755 server/backup.sh /opt/joppa/server/backup.sh
install -m 644 server/joppa-backup.service server/joppa-backup.timer /etc/systemd/system/
if [[ -f /opt/joppa/bin/joppa ]]; then
    cp -p /opt/joppa/bin/joppa /opt/joppa/bin/joppa.previous
fi
install -m 755 target/release/joppa /opt/joppa/bin/joppa.next
mv /opt/joppa/bin/joppa.next /opt/joppa/bin/joppa
systemctl daemon-reload
systemctl enable joppa
systemctl restart joppa
healthy=false
for attempt in {1..15}; do
    if curl --fail --silent --max-time 4 http://127.0.0.1:6212/health >/dev/null; then
        healthy=true; break
    fi
    sleep 2
done
if [[ "$healthy" != true ]]; then
    if [[ -f /opt/joppa/bin/joppa.previous ]]; then
        mv /opt/joppa/bin/joppa.previous /opt/joppa/bin/joppa
        systemctl restart joppa
    fi
    echo 'Health check failed; previous binary restored if available. Inspect journalctl -u joppa.' >&2
    exit 1
fi
export JOPPA_DOMAIN="$domain"
caddy validate --config server/Caddyfile --adapter caddyfile
install -d -m 755 /etc/systemd/system/caddy.service.d
printf '[Service]\nEnvironment=JOPPA_DOMAIN=%s\n' "$domain" > /etc/systemd/system/caddy.service.d/joppa.conf
install -m 644 server/Caddyfile /etc/caddy/Caddyfile
systemctl daemon-reload
systemctl enable caddy
systemctl restart caddy
if [[ -f /etc/joppa/backup.env ]]; then
    systemctl enable --now joppa-backup.timer
fi
if [[ ! -f /etc/joppa/google-client.json ]]; then
    echo 'Site is running; Google sign-in needs private google-client.json. Add it and rerun deploy.'
fi
echo 'Application health passed; HTTPS proxy and configured backup timer started.'
