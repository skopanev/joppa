#!/usr/bin/env python3
"""HTTP/CLI/MCP smoke against an isolated test DB. Optional real browser pass.
JOPPA_TEST_DATABASE_URL=... python3 tests/http_smoke.py [--browser]
Browser needs Playwright (PLAYWRIGHT_MODULE may specify an installed module).
"""
import json
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
from auth_provider import Provider

ROOT = Path(__file__).resolve().parents[1]
BIN = str(ROOT / 'target/debug/joppa')
url = os.environ['JOPPA_TEST_DATABASE_URL']
assert urllib.parse.urlparse(url).path.rsplit('/', 1)[-1].startswith('joppa_test'), 'test DB only'
env = {**os.environ, 'JOPPA_DATABASE_URL': url, 'JOPPA_ACTOR': 'owner'}
subprocess.run([BIN, 'migrate'], env=env, check=True, stdout=subprocess.DEVNULL)
owner_token, agent_token = secrets.token_urlsafe(32), secrets.token_urlsafe(32)
ws = 'http-' + str(uuid.uuid4())
with socket.socket() as s:
    s.bind(('127.0.0.1', 0))
    port = s.getsockname()[1]
base = f'http://127.0.0.1:{port}'

def request(path, body=None, token=owner_token):
    headers = {'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'}
    req = urllib.request.Request(base + path, data=None if body is None else json.dumps(body).encode(), headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=5) as r:
            return r.status, json.load(r)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()

def command(value, token=owner_token, request_id=None):
    return request('/api/workspaces/' + ws + '/commands', {'request_id': request_id or str(uuid.uuid4()), 'command': value}, token)

def ok(value):
    status, data = command(value)
    assert status == 200, (status, data)
    return data

with tempfile.TemporaryDirectory(prefix='joppa-http-') as temp:
    token_file = Path(temp) / 'tokens.json'
    token_file.write_text(json.dumps({owner_token: 'owner', agent_token: 'agent'}))
    token_file.chmod(0o600)
    env['JOPPA_TOKENS_FILE'] = str(token_file)
    identities = Path(temp) / 'identities.json'
    identities.write_text(json.dumps({'ntk-owner': 'owner'}))
    provider = Provider()
    env['JOPPA_LOGIN_USERS_FILE'] = str(identities)
    env['JOPPA_NTK_URL'] = provider.url

    server = subprocess.Popen([BIN, 'serve', str(port)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    try:
        for attempt in range(50):
            try:
                if request('/api/me')[0] == 200:
                    break
            except OSError:
                time.sleep(0.1)
        else:
            raise AssertionError('HTTP server did not become ready')
        assert request('/api/workspaces', token='invalid')[0] == 401
        assert request('/auth/login', body={})[0] == 403  # Origin is mandatory.
        def auth_request(path, cookie='', origin=base):
            req = urllib.request.Request(base + path, data=b'', headers={'Origin': origin, 'Cookie': cookie})
            return urllib.request.urlopen(req, timeout=5)
        def begin():
            with auth_request('/auth/login') as response:
                data = json.load(response)
                assert 'device_secret' not in data
                cookie = response.headers['Set-Cookie'].split(';')[0]
                return cookie, data['verification_url']
        attempt, verification_url = begin()
        with auth_request('/auth/poll', attempt) as response:
            assert json.load(response)['status'] == 'pending'
        try:
            auth_request('/auth/poll')
            raise AssertionError('another browser cannot collect this sign-in')
        except urllib.error.HTTPError as e:
            assert e.code == 403
        provider.user = 'outsider'
        urllib.request.urlopen(verification_url).close()
        try:
            auth_request('/auth/poll', attempt)
            raise AssertionError('unapproved provider identity must fail')
        except urllib.error.HTTPError as e:
            assert e.code == 403
        provider.user = 'ntk-owner'
        attempt, verification_url = begin()
        urllib.request.urlopen(verification_url).close()
        with auth_request('/auth/poll', attempt) as response:
            cookie = response.headers['Set-Cookie']
            assert 'HttpOnly' in cookie and 'SameSite=Strict' in cookie
            data = json.load(response)
            assert data['actor'] == 'owner' and 'key' not in data
            assert owner_token not in cookie
        cookie = cookie.split(';')[0]
        with urllib.request.urlopen(urllib.request.Request(base+'/api/me', headers={'Cookie': cookie})) as me:
            assert json.load(me)['actor'] == 'owner'
        try:
            auth_request('/auth/poll', attempt)
            raise AssertionError('completed attempt cannot be replayed')
        except urllib.error.HTTPError as e:
            assert e.code == 403
        try:
            urllib.request.urlopen(urllib.request.Request(base+'/api/me', headers={'Cookie': cookie, 'Authorization': 'Bearer invalid'}))
            raise AssertionError('invalid explicit credentials cannot fall back to a cookie')
        except urllib.error.HTTPError as e:
            assert e.code == 401
        forged = urllib.request.Request(base+'/api/workspaces/'+ws+'/commands', data=json.dumps({'request_id':'csrf','command':{'op':'workspace','title':'forged'}}).encode(), headers={'Content-Type':'application/json','Cookie':cookie,'Origin':'https://unrelated.example'})
        try:
            urllib.request.urlopen(forged)
            raise AssertionError('cross-origin cookie write must fail')
        except urllib.error.HTTPError as e:
            assert e.code == 403
        auth_request('/auth/logout', cookie).close()
        try:
            urllib.request.urlopen(urllib.request.Request(base+'/api/me', headers={'Cookie': cookie}))
            raise AssertionError('logout must invalidate the server session')
        except urllib.error.HTTPError as e:
            assert e.code == 401

        ok({'op': 'workspace', 'title': 'Проверка карточки'})
        ok({'op': 'member', 'id': 'agent', 'role': 'agent'})
        ok({'op': 'domain', 'id': 'security', 'title': 'Доступ к данным', 'authority': 'owner'})
        ok({'op': 'capability', 'id': 'restrict', 'title': 'Ограничивать доступ', 'domain': 'security'})
        ok({'op': 'revise', 'req': 'key', 'expected_revision': 0, 'capability': 'restrict',
            'title': 'Ключ до запуска', 'body': '</script><script>window.attacked=true</script>',
            'owner': 'owner', 'reason': 'Проверка полного цикла',
            'acs': [{'id': 'key-rights', 'text': 'До запуска у ключа только read и verify', 'before_launch': True}]})
        status, _ = command({'op': 'confirm', 'req': 'key', 'revision': 1}, token=agent_token)
        assert status == 409, 'agent cannot confirm owner wording'
        status, _ = command({'op': 'confirm', 'req': 'key', 'revision': 1, 'actor': 'owner'}, token=agent_token)
        assert status == 422, 'actor injection must fail deserialization'
        assert request('/api/workspaces/missing')[0] == 409
        retry = str(uuid.uuid4())
        c = {'op': 'comment', 'req': 'key', 'revision': 1, 'id': 'hello', 'text': 'Hello', 'kind': 'comment'}
        assert command(c, request_id=retry)[0] == 200
        assert command(c, request_id=retry)[1]['replayed'] is True
        # Stdio notification produces no response; names and help are accessible.
        messages = [
            {'jsonrpc': '2.0', 'method': 'notifications/initialized'},
            {'jsonrpc': '2.0', 'id': 1, 'method': 'tools/list'},
            {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/call', 'params': {'name': 'joppa_read', 'arguments': {'workspace': ws}}},
        ]
        output = subprocess.check_output([BIN], env=env, input='\n'.join(map(json.dumps, messages)) + '\n', text=True)
        replies = [json.loads(line) for line in output.splitlines()]
        assert len(replies) == 2 and replies[1]['id'] == 2
        assert len(replies[0]['result']['tools']) == 4
        snapshot = Path(temp) / 'snapshot.html'
        subprocess.run([BIN, 'board', ws, str(snapshot)], env=env, check=True, stdout=subprocess.DEVNULL)
        assert '\\u003c/script' in snapshot.read_text()
        # The generic automated runner really seals before execution and reports failures.
        ok({'op': 'revise', 'req': 'auto', 'expected_revision': 0, 'capability': 'restrict',
            'title': 'Automated example', 'body': '', 'owner': 'owner', 'reason': 'Adapter test',
            'acs': [{'id': 'auto-ac', 'text': 'Command exits successfully'}]})
        ok({'op': 'confirm', 'req': 'auto', 'revision': 1})
        ok({'op': 'assess', 'req': 'auto', 'revision': 1, 'ac': 'auto-ac',
            'assessment': {'ready': True, 'reason': 'Known contract'}})
        manifest = {'req': 'auto', 'revision': 1, 'subject': {'code': 'test-v1'},
                    'scope': 'One declared command', 'discovery_complete': True,
                    'checks': [{'id': '[auto-ac] succeeds', 'ac': 'auto-ac', 'method': 'automated',
                                'expected': ['exit code 0'], 'command': [sys.executable, '-c', 'print("passed")']}]}
        manifest_path = Path(temp) / 'manifest.json'
        for code in (0, 1):
            manifest['checks'][0]['command'] = [sys.executable, '-c', f'raise SystemExit({code})']
            manifest_path.write_text(json.dumps(manifest))
            result = subprocess.run([sys.executable, str(ROOT / 'scripts/run_checks.py'), ws,
                                     str(manifest_path), '--joppa', BIN], env=env, capture_output=True, text=True)
            assert result.returncode == code, result.stderr
            card = request('/api/workspaces/' + ws)[1]
            assert card['summary']['auto']['ready_for_acceptance'] is (code == 0)
        if '--browser' in sys.argv:
            browser_env = {**env, 'JOPPA_TEST_URL': base, 'JOPPA_TEST_TOKEN': owner_token,
                           'JOPPA_TEST_WORKSPACE': ws, 'JOPPA_TEST_SNAPSHOT': str(snapshot)}
            subprocess.run(['node', str(ROOT / 'tests/browser.cjs')], env=browser_env, check=True)
            card = request('/api/workspaces/' + ws)[1]
            assert card['summary']['key']['accepted'] is True
            # Actual deployment gate rejects a different target.
            gate = subprocess.run([BIN, 'gate', ws], env=env, input='{"environment":"config-v1"}', text=True, capture_output=True)
            assert gate.returncode == 0, gate.stderr
            gate = subprocess.run([BIN, 'gate', ws], env=env, input='{"environment":"config-v2"}', text=True, capture_output=True)
            assert gate.returncode != 0
        print('HTTP auth, workspace isolation, idempotency, CLI snapshot and MCP smoke passed')
    finally:
        server.terminate()
        server.wait(timeout=5)
        provider.close()
