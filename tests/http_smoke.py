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

ROOT = Path(__file__).resolve().parents[1]
BIN = os.environ.get('JOPPA_BIN', str(ROOT / 'target/debug/joppa'))
url = os.environ['JOPPA_TEST_DATABASE_URL']
assert urllib.parse.urlparse(url).path.rsplit('/', 1)[-1].startswith('joppa_test'), 'test DB only'
env = {**os.environ, 'JOPPA_DATABASE_URL': url, 'JOPPA_ACTOR': 'owner'}
subprocess.run([BIN, 'migrate'], env=env, check=True, stdout=subprocess.DEVNULL)
owner_token, agent_token, other_token = (secrets.token_urlsafe(32) for _ in range(3))
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
            return r.status, json.load(r) if r.status != 202 else None
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
    token_file.write_text(json.dumps({owner_token: 'owner', agent_token: 'agent', other_token: 'other-owner'}))
    token_file.chmod(0o600)
    env['JOPPA_TOKENS_FILE'] = str(token_file)
    # Standalone browser login needs Joppa's own Google client. Unit tests cover
    # the OAuth exchange and session binding with a local Google fixture.
    env.pop('JOPPA_GOOGLE_CLIENT_FILE', None)
    env.pop('JOPPA_LOGIN_USERS_FILE', None)
    env.pop('DEEPINFRA_API_KEY', None)

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
        assert request('/health', token='')[1] == {'ok': True}
        with urllib.request.urlopen(urllib.request.Request(base+'/api/me',headers={'Authorization':'Bearer '+owner_token})) as response:
            assert response.headers.get('Cache-Control') == 'no-store'
        assert request('/api/workspaces', token='invalid')[0] == 401
        assert request('/api/me')[1] == {'actor':'owner','email':None}
        # Cookie/no-credential commands are rejected before authentication or DB access.
        payload=json.dumps({'request_id':'portal-write','command':{'op':'workspace','title':'Forbidden'}}).encode()
        for cookie in ('', 'joppa_session=browser-session'):
            req=urllib.request.Request(base+'/api/workspaces/portal-write/commands',data=payload,
                headers={'Content-Type':'application/json','Origin':base,'Cookie':cookie})
            try:
                urllib.request.urlopen(req)
                raise AssertionError('portal write must be forbidden')
            except urllib.error.HTTPError as e:
                assert e.code == 403 and 'read-only' in e.read().decode()

        assert request('/auth/login', body={})[0] == 403  # Origin is mandatory.
        try:
            urllib.request.urlopen(urllib.request.Request(base+'/auth/login', data=b'', headers={'Origin':base}))
            raise AssertionError('unconfigured Google login must fail closed')
        except urllib.error.HTTPError as e:
            assert e.code == 403 and 'configured for Joppa' in e.read().decode()
        with urllib.request.urlopen(base+'/auth/google/callback?state=forged&error=%3Cscript%3Ealert(1)%3C/script%3E') as response:
            html=response.read().decode()
            assert '<script>' not in html and 'another browser' in html

        ok({'op': 'workspace', 'title': 'Проверка карточки'})
        ok({'op': 'member', 'id': 'agent', 'role': 'agent'})
        ok({'op':'member','id':'legal','role':'human'})
        ok({'op':'member','id':'reviewer','role':'human'})
        ok({'op': 'domain', 'id': 'security', 'title': 'Доступ к данным', 'authority': 'owner'})
        ok({'op': 'capability', 'id': 'restrict', 'title': 'Ограничивать доступ', 'domain': 'security'})
        ok({'op':'capability','id':'audit','title':'Audit access','domain':'security'})
        ok({'op':'domain','id':'operations','title':'Operations','authority':'owner'})
        ok({'op':'capability','id':'observe','title':'Observe service health','domain':'operations'})
        ok({'op': 'revise', 'req': 'key', 'expected_revision': 0, 'capability': 'restrict',
            'title': 'Ключ до запуска', 'body': '</script><script>window.attacked=true</script>',
            'owner': 'owner', 'reason': 'Проверка полного цикла',
            'acs': [{'id': 'key-rights', 'text': 'До запуска у ключа только read и verify', 'before_launch': True}]})
        status, _ = command({'op': 'confirm', 'req': 'key', 'revision': 1}, token=agent_token)
        assert status == 409, 'agent cannot confirm owner wording'
        status, _ = command({'op': 'confirm', 'req': 'key', 'revision': 1, 'actor': 'owner'}, token=agent_token)
        assert status == 422, 'actor injection must fail deserialization'
        assert request('/api/workspaces/missing')[0] == 404
        other_ws = ws + '-other'
        assert request('/api/workspaces/' + other_ws + '/commands',
                       {'request_id':'create','command':{'op':'workspace','title':'Private second workspace'}}, other_token)[0] == 200
        assert all(w['id'] != other_ws for w in request('/api/workspaces')[1])
        assert all(w['id'] != ws for w in request('/api/workspaces',token=other_token)[1])
        missing = request('/api/workspaces/missing',token=other_token)
        assert request('/api/workspaces/' + ws,token=other_token) == missing
        assert request('/api/workspaces/' + other_ws) == missing
        for request_id in ('create','guessed-request'):
            status, body = command({'op':'member','id':'other-owner','role':'human'},token=other_token,request_id=request_id)
            assert status == 404 and body == missing[1]
        assert command({'op':'enroll_member','id':'other-owner'})[0] == 409
        retry = str(uuid.uuid4())
        c = {'op': 'comment', 'req': 'key', 'revision': 1, 'id': 'hello', 'text': 'Hello', 'kind': 'comment'}
        assert command(c, request_id=retry)[0] == 200
        assert command(c, request_id=retry)[1]['replayed'] is True
        # Remote MCP notifications return 202 without a JSON-RPC response.
        assert request('/mcp', {'jsonrpc':'2.0','method':'notifications/initialized'})[0] == 202
        reply=request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/list'})[1]
        assert {t['name'] for t in reply['result']['tools']} == {'joppa_list','joppa_read','joppa_command','joppa_batch','joppa_help','joppa_model_docs','joppa_search'}
        def mcp(name, arguments, token=agent_token):
            status, response = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
                'params':{'name':name,'arguments':arguments}}, token)
            assert status == 200, (status, response)
            result = response['result']
            assert not result.get('isError'), result
            return json.loads(result['content'][0]['text'])
        assert mcp('joppa_help', {})['search'] == (ROOT/'docs/search.md').read_text()
        # Agents can still add both object notes and revision discussion through MCP.
        for request_id, value in [
            ('agent-note', {'op':'note','target':'requirement:key','id':'agent-note','text':'Agent note'}),
            ('agent-comment', {'op':'comment','req':'key','revision':1,'id':'agent-comment','text':'Agent discussion','kind':'comment'}),
        ]:
            mcp('joppa_command', {'workspace':ws,'request_id':request_id,'command':value})
        readback = request('/api/workspaces/'+ws)[1]['state']
        assert readback['notes']['requirement:key']['comments']['agent-note']['text'] == 'Agent note'
        assert readback['requirements']['key']['comments']['agent-comment']['text'] == 'Agent discussion'

        docs = mcp('joppa_model_docs', {})
        assert set(docs) == {'model','process','green'}
        assert docs['model'] == (ROOT/'docs/model.md').read_text()
        assert docs['process'] == (ROOT/'docs/process.md').read_text()
        assert docs['green'] == (ROOT/'docs/green.md').read_text()
        assert set(mcp('joppa_model_docs', {'section':'green'})) == {'green'}
        mcp_ws = ws + '-authoring'
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'create',
            'command':{'op':'workspace','title':'MCP authoring checks'}},owner_token)
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'member',
            'command':{'op':'member','id':'agent','role':'agent'}},owner_token)
        draft = mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-generated',
            'command':{'op':'domain','title':'Draft area','authority':'owner'}})
        draft_id = draft['command']['id']
        uuid.UUID(draft_id)
        assert draft['system_ids']['domain'][draft_id].startswith(mcp_ws+'-D-')
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-draft-edit',
            'command':{'op':'edit_domain','id':draft_id,'title':'Clear area','authority':'owner','reason':'Clarification'}})
        batch = {'workspace':mcp_ws,'request_id':'mcp-batch','operations':[
            {'ref':'cap','command':{'op':'capability','title':'Recover access','domain':draft_id}},
            {'command':{'op':'revise','expected_revision':0,'capability':{'$ref':'cap/id'},
                'title':'Recovery preserves access','body':'Only iOS','owner':'owner','reason':'Agreed scope',
                'acs':[{'text':'On iOS paid access is preserved'}]}}
        ]}
        created = mcp('joppa_batch',batch)
        replay = mcp('joppa_batch',batch)
        assert replay['replayed'] and created['results'] == replay['results']
        assert created['results'][1]['command']['capability'] == created['results'][0]['command']['id']
        public_id = next(iter(created['results'][1]['system_ids']['requirement'].values()))
        resolved = mcp('joppa_read',{'item_id':public_id})
        assert resolved['selection']['id'] == created['results'][1]['command']['req']
        found = mcp('joppa_search', {'workspace':mcp_ws,'query':'recovery','kind':'requirement'})
        assert found['mode'] == 'full_text' and found['semantic_status'] == 'not_configured'
        assert found['items'][0]['system_id'] == public_id
        denied_search = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
            'params':{'name':'joppa_search','arguments':{'workspace':mcp_ws,'query':'recovery'}}},other_token)[1]
        assert denied_search['result']['isError']
        assert denied_search['result']['content'][0]['text'] == 'Workspace not found'
        foreign_read={'jsonrpc':'2.0','id':1,'method':'tools/call','params':{
            'name':'joppa_read','arguments':{'workspace':ws,'actor':'owner'}}}
        reply=request('/mcp',foreign_read,other_token)[1]
        assert reply['result']['isError'] is True
        assert reply['result']['content'][0]['text'] == 'Workspace not found'
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
        # Changes are made by an integration; the portal only reads the result.
        ok({'op':'confirm','req':'key','revision':1})
        ok({'op':'assess','req':'key','revision':1,'ac':'key-rights',
            'assessment':{'ready':True,'reason':'Confirmed exact permission list'}})
        ok({'op':'plan','req':'key','revision':1,'run':'manual1',
            'subject':{'environment':'config-v1'},'scope':'Key permissions before launch',
            'discovery_complete':True,'checks':[{'id':'manual-key-rights','ac':'key-rights',
                'method':'manual','expected':['read','verify']}]})
        ok({'op':'result','req':'key','revision':1,'run':'manual1',
            'evidence':{'check':'manual-key-rights','outcome':'pass','observed':['read','verify'],
                'note':'Compared the provider permission list'}})
        ok({'op':'finish','req':'key','revision':1,'run':'manual1'})
        ok({'op':'accept','req':'key','revision':1,'run':'manual1'})
        assert request('/api/workspaces/'+ws)[1]['summary']['key']['accepted'] is True
        gate = subprocess.run([BIN, 'gate', ws], env=env, input='{"environment":"config-v1"}', text=True, capture_output=True)
        assert gate.returncode == 0, gate.stderr
        gate = subprocess.run([BIN, 'gate', ws], env=env, input='{"environment":"config-v2"}', text=True, capture_output=True)
        assert gate.returncode != 0
        ok({'op':'task','req':'auto','revision':1,'id':'agent-task','title':'Implement command','acs':['auto-ac'],'projects':[],'purpose':'implementation','assignee':'reviewer'})
        # The portal retains older revisions after a wording change.
        ok({'op':'revise','req':'key','expected_revision':1,'capability':'restrict',
            'title':'Ключ до запуска','body':'Updated wording','owner':'owner','reason':'Clarify scope',
            'acs':[{'id':'key-rights','text':'Before launch the key has exactly read and verify','before_launch':True,'decider':'legal'}]})
        refs=request('/api/workspaces/'+ws)[1]['system_ids']
        all_refs=[ref for items in refs.values() for ref in items.values()]
        assert len(all_refs)==len(set(all_refs))
        for kind,items in refs.items():
            tag={'domain':'D','capability':'C','requirement':'R','ac':'A'}[kind]
            for local_id,ref in items.items():
                assert ref.startswith(ws+'-'+tag+'-') and len(ref.rsplit('-',1)[1])==10
                msg={'jsonrpc':'2.0','id':9,'method':'tools/call','params':{'name':'joppa_read','arguments':{'item_id':ref}}}
                found=request('/mcp',msg)[1]
                selection=json.loads(found['result']['content'][0]['text'])['selection']
                assert selection['kind']==kind and selection['id']==local_id
                if kind=='ac': assert selection['req'] in ('key','auto')
        msg['params']['arguments']['item_id']=refs['requirement']['key']
        denied=request('/mcp',msg,other_token)[1]
        assert denied['result']['isError'] and denied['result']['content'][0]['text']=='Workspace not found'
        subprocess.run([BIN,'migrate'],env=env,check=True,stdout=subprocess.DEVNULL)
        assert request('/api/workspaces/'+ws)[1]['system_ids']==refs
        before=request('/api/workspaces/'+ws)[1]
        ok({'op':'rename_workspace','title':'Review workspace','reason':'Name clarified'})
        after=request('/api/workspaces/'+ws)[1]
        assert after['workspace']==before['workspace']
        assert after['history'][:-1]==before['history']
        assert after['state']=={**before['state'],'title':'Review workspace'}
        if '--browser' in sys.argv:
            browser_env = {**env, 'JOPPA_TEST_URL': base, 'JOPPA_TEST_TOKEN': owner_token,
                           'JOPPA_TEST_WORKSPACE': ws, 'JOPPA_TEST_SNAPSHOT': str(snapshot)}
            subprocess.run(['node', str(ROOT / 'tests/browser.cjs')], env=browser_env, check=True)
            assert request('/api/workspaces/'+ws)[1] == after, 'browsing must never mutate product data'
        print('HTTP auth, workspace isolation, idempotency, CLI snapshot and MCP smoke passed')
    finally:
        server.terminate()
        server.wait(timeout=5)
