#!/usr/bin/env python3
"""HTTP/CLI/MCP smoke against an isolated test DB.
JOPPA_TEST_DATABASE_URL=... python3 tests/http_smoke.py
"""
import json
import re
from html.parser import HTMLParser
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid

ROOT = Path(__file__).resolve().parents[1]
BIN = os.environ.get('JOPPA_BIN', str(ROOT / 'target/debug/joppa'))
url = os.environ['JOPPA_TEST_DATABASE_URL']
assert urllib.parse.urlparse(url).path.rsplit('/', 1)[-1].startswith('joppa_test'), 'test DB only'
env = {**os.environ, 'JOPPA_DATABASE_URL': url, 'JOPPA_ACTOR': 'owner'}
subprocess.run([BIN, 'migrate'], env=env, check=True, stdout=subprocess.DEVNULL)
owner_token, agent_token, other_token = (secrets.token_urlsafe(32) for _ in range(3))
ws = 'http-' + str(uuid.uuid4())
other_ws = ws + '-other'
mcp_ws = ws + '-authoring'
for workspace, actor, title in [
    (ws, 'owner', 'Проверка карточки'),
    (other_ws, 'other-owner', 'Private second workspace'),
    (mcp_ws, 'owner', 'MCP authoring checks'),
]:
    subprocess.run(
        [BIN, 'command', workspace, 'server-create'],
        env={**env, 'JOPPA_ACTOR': actor},
        input=json.dumps({'op': 'workspace', 'title': title}),
        text=True,
        check=True,
        stdout=subprocess.DEVNULL,
    )
with socket.socket() as s:
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
base = f'http://127.0.0.1:{port}'

def request(path, body=None, token=owner_token):
    headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
    req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.load(r) if r.status != 202 else None
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()

def system_ids(workspace=ws):
    """The item ID of every current object, by kind and local ID, from the HTTP object pages."""
    ids, cursor = {}, None
    while True:
        query = '/api/workspaces/'+workspace+'/objects?parent=*&limit=100'
        if cursor: query += '&cursor='+urllib.parse.quote(cursor)
        status, page = request(query)
        assert status == 200, (status, page)
        for item in page['items']:
            ids.setdefault(item['kind'], {})[item['id']] = item['item_id']
        cursor = page.get('next_cursor')
        if not cursor: return ids

def command(value, token=owner_token, request_id=None):
    return request('/api/workspaces/' + ws + '/commands', {'request_id': request_id or str(uuid.uuid4()), 'command': value}, token)

def ok(value):
    status, data = command(value)
    assert status == 200, (status, data)
    return data

with tempfile.TemporaryDirectory(prefix='joppa-http-') as temp:
    token_file = Path(temp) / 'tokens.json'
    token_file.write_text(json.dumps({owner_token: 'owner', agent_token: 'agent', other_token: 'other-owner'}))
    token_file.chmod(0o600)
    env['JOPPA_TOKENS_FILE'] = str(token_file)
    # Standalone browser login needs Joppa's own Google client. Unit tests cover
    # the OAuth exchange and session binding with a local Google fixture.
    env.pop('JOPPA_GOOGLE_CLIENT_FILE', None)
    env.pop('JOPPA_LOGIN_USERS_FILE', None)
    # Search is always hybrid: a test key and a closed local port stand in for the provider.
    env['DEEPINFRA_API_KEY'] = 'test-only-key'
    env['JOPPA_EMBEDDING_URL'] = 'http://127.0.0.1:9/embeddings'
    # Attachments live in Spaces: a local stand-in takes the presigned requests.
    source = ROOT / 'tests' / 'http_smoke' / 'fake_spaces.py'
    exec(compile(source.read_text(), str(source), 'exec'), globals())

    server = subprocess.Popen([BIN, 'serve', str(port)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        # Each part runs in the global names of this script, in order, like one body.
        for part in ('part1.py', 'part2.py'):
            source = ROOT / 'tests' / 'http_smoke' / part
            exec(compile(source.read_text(), str(source), 'exec'), globals())
    finally:
        server.terminate()
        server.wait(timeout=5)
