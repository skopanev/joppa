#!/usr/bin/env python3
"""Test lists on criteria and JUnit reports, end to end over HTTP on an isolated test DB.
JOPPA_TEST_DATABASE_URL=... python3 tests/reports_smoke.py
"""
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
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
owner, agent, outsider = (secrets.token_urlsafe(32) for _ in range(3))
ws = 'reports-' + str(uuid.uuid4())
subprocess.run([BIN, 'command', ws, 'server-create'], env=env, input=json.dumps({'op': 'workspace', 'title': 'Reports'}),
               text=True, check=True, stdout=subprocess.DEVNULL)
with socket.socket() as s:
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
base = f'http://127.0.0.1:{port}/api/workspaces/{ws}'

def call(path, body=None, token=owner):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            return r.status, json.load(r)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()

def ok(command, token=owner):
    status, data = call('/commands', {'request_id': str(uuid.uuid4()), 'command': command}, token)
    assert status == 200, (status, data)
    return data

def junit(cases, file='tests/auth/login.test.ts'):
    rows = ''.join(
        f'<testcase name="{name}">' + ('<failure message="boom"/>' if status == 'fail' else '<skipped/>' if status == 'skipped' else '') + '</testcase>'
        for name, status in cases)
    return f'<testsuites><testsuite name="{file}"><testsuite name="login">{rows}</testsuite></testsuite></testsuites>'

def upload(commit, order, cases, full=True, token=agent, default_branch=True, dirty=False):
    stamp = {'commit': commit, 'order': order, 'branch': 'master', 'default_branch': default_branch, 'dirty': dirty, 'full': full}
    return call('/reports', {'stamp': stamp, 'junit': junit(cases)}, token)

def status(ac='ac:key-rights'):
    code, data = call('/tests?ac=' + urllib.parse.quote(ac))
    assert code == 200, (code, data)
    return data

A = 'tests/auth/login.test.ts > login > opens'
B = 'tests/auth/login.test.ts > login > refuses'
with tempfile.TemporaryDirectory(prefix='joppa-reports-') as temp:
    tokens = Path(temp) / 'tokens.json'
    tokens.write_text(json.dumps({owner: 'owner', agent: 'agent', outsider: 'outsider'}))
    tokens.chmod(0o600)
    env['JOPPA_TOKENS_FILE'] = str(tokens)
    for name in ('JOPPA_GOOGLE_CLIENT_FILE', 'JOPPA_LOGIN_USERS_FILE', 'DEEPINFRA_API_KEY'):
        env.pop(name, None)
    server = subprocess.Popen([BIN, 'serve', str(port)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        for _ in range(100):
            try:
                urllib.request.urlopen(f'http://127.0.0.1:{port}/health', timeout=1)
                break
            except Exception:
                time.sleep(0.1)
        ok({'op': 'member', 'id': 'agent', 'role': 'agent'})
        ok({'op': 'consumer', 'id': 'customer', 'title': 'Customer', 'owner': 'owner'})
        ok({'op': 'domain', 'id': 'security', 'title': 'Security', 'authority': 'owner'})
        ok({'op': 'capability', 'id': 'restrict', 'title': 'Restrict access', 'domain': 'security'})
        ok({'op': 'revise', 'req': 'key', 'expected_revision': 0, 'capability': 'restrict', 'title': 'Key rights',
            'body': 'Scope', 'owner': 'owner', 'consumers': ['customer'], 'reason': 'Agreed',
            'acs': [{'id': 'key-rights', 'text': 'Only read and verify before launch'}]})

        assert status()['reason'] == 'no tests'
        ok({'op': 'set_ac_tests', 'target': 'ac:key-rights', 'tests': [A, B], 'reason': 'Prove the rights'}, agent)
        assert status()['tests'][0]['status'] == 'not_run' and status()['status'] == 'grey'

        # Upload rules: a token is required, a browser-free member token works, repeats are idempotent.
        assert call('/reports', {'stamp': {}, 'junit': ''}, None)[0] == 401
        assert upload('a1a1a1a', 1, [('opens', 'pass')], token=outsider)[0] == 404
        code, first = upload('a1a1a1a', 1, [('opens', 'pass'), ('refuses', 'pass')])
        assert code == 200 and first['tests'] == 2 and first['uploader'] == 'agent', first
        code, again = upload('a1a1a1a', 1, [('opens', 'pass'), ('refuses', 'pass')])
        assert again['replayed'] and again['report'] == first['report']
        s = status()
        assert (s['status'], s['commit'], s['commits_behind']) == ('green', 'a1a1a1a', 0), s

        # Dirty runs and other branches never change the status.
        upload('b2b2b2b', 2, [('refuses', 'fail')], dirty=True)
        upload('c3c3c3c', 2, [('refuses', 'fail')], default_branch=False)
        assert status()['status'] == 'green'

        # A partial run updates only its tests; a full run without a listed test makes it missing.
        upload('d4d4d4d', 3, [('opens', 'pass')], full=False)
        s = status()
        assert s['status'] == 'green' and s['commit'] == 'a1a1a1a' and s['commits_behind'] == 2, s
        upload('e5e5e5e', 4, [('opens', 'pass')], full=True)
        s = status()
        assert s['status'] == 'red' and [t['status'] for t in s['tests']] == ['pass', 'missing'], s
        upload('f6f6f6f', 5, [('opens', 'pass'), ('refuses', 'fail')])
        s = status()
        assert s['status'] == 'red' and s['tests'][1]['message'] == 'boom', s
        upload('a7a7a7a', 6, [('opens', 'pass'), ('refuses', 'pass')])
        assert status()['status'] == 'green'

        # A text change makes the list a draft until it is saved again.
        ok({'op': 'revise', 'req': 'key', 'expected_revision': 1, 'capability': 'restrict', 'title': 'Key rights',
            'body': 'Scope', 'owner': 'owner', 'consumers': ['customer'], 'reason': 'Sharper wording',
            'acs': [{'id': 'key-rights', 'text': 'Only read and verify until launch'}]})
        s = status()
        assert (s['status'], s['reason'], s['tests_draft']) == ('grey', 'tests_draft', True), s
        ok({'op': 'set_ac_tests', 'target': 'ac:key-rights', 'tests': [A, B], 'reason': 'Still proves it'}, agent)
        s = status()
        assert s['status'] == 'green' and s['tests_revision'] == 2, s

        code, body = call('/commands', {'request_id': str(uuid.uuid4()), 'command': {
            'op': 'set_ac_tests', 'target': 'ac:key-rights', 'tests': ['login ok'], 'reason': 'Bad'}}, agent)
        assert code == 409 and 'file > describe > name' in body, body
    finally:
        server.terminate()
        server.wait(timeout=10)
print('reports smoke: ok')
