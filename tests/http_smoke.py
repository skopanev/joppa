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

        status, body = command({'op': 'workspace', 'title': 'Forbidden remote create'})
        assert status == 409 and 'server CLI' in body
        ok({'op': 'member', 'id': 'agent', 'role': 'agent'})
        ok({'op':'member','id':'legal','role':'human'})
        ok({'op':'member','id':'reviewer','role':'human'})
        ok({'op':'consumer','id':'customer','title':'Customer','owner':'owner'})
        ok({'op': 'domain', 'id': 'security', 'title': 'Доступ к данным', 'authority': 'owner'})
        ok({'op': 'capability', 'id': 'restrict', 'title': 'Ограничивать доступ', 'domain': 'security'})
        ok({'op':'capability','id':'audit','title':'Audit access','domain':'security'})
        ok({'op':'domain','id':'operations','title':'Operations','authority':'owner'})
        ok({'op':'capability','id':'observe','title':'Observe service health','domain':'operations'})
        ok({'op': 'revise', 'req': 'key', 'expected_revision': 0, 'capability': 'restrict',
            'title': 'Ключ до запуска', 'body': '</script><script>window.attacked=true</script>',
            'owner': 'owner', 'consumers':['customer'], 'reason': 'Проверка полного цикла',
            'acs': [{'id': 'key-rights', 'text': 'До запуска у ключа только read и verify', 'before_launch': True}]})
        status, _ = command({'op': 'confirm', 'req': 'key', 'revision': 1}, token=agent_token)
        assert status == 409, 'agent cannot confirm owner wording'
        status, _ = command({'op': 'confirm', 'req': 'key', 'revision': 1, 'actor': 'owner'}, token=agent_token)
        assert status == 422, 'actor injection must fail deserialization'
        assert request('/api/workspaces/missing')[0] == 404
        status, body = request('/api/workspaces/' + other_ws + '/commands',
                       {'request_id':'create','command':{'op':'workspace','title':'Forbidden remote create'}}, other_token)
        assert status == 409 and 'server CLI' in body
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
        assert {t['name'] for t in reply['result']['tools']} == {'joppa_list','joppa_read','joppa_tasks','joppa_command','joppa_batch','joppa_help','joppa_model_docs','joppa_search','joppa_tree','joppa_objects'}
        tools = {tool['name']: tool for tool in reply['result']['tools']}
        writing = (ROOT/'docs/writing.md').read_text()
        initialized = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'initialize'})[1]['result']
        assert writing in initialized['instructions']
        for schema in (tools['joppa_command']['inputSchema']['properties']['command'],
                       tools['joppa_batch']['inputSchema']['properties']['operations']['items']['properties']['command']):
            assert 'ASD-STE100' in schema['properties']['contract']['description']
            assert 'said' not in schema['properties']
        def mcp(name, arguments, token=agent_token):
            status, response = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
                'params':{'name':name,'arguments':arguments}}, token)
            assert status == 200, (status, response)
            result = response['result']
            assert not result.get('isError'), result
            return json.loads(result['content'][0]['text'])
        assert mcp('joppa_help', {})['search'] == (ROOT/'docs/search.md').read_text()
        assert mcp('joppa_help', {})['writing'] == writing
        assert mcp('joppa_tasks', {'workspace':ws})['total'] == 0
        assert mcp('joppa_tree', {'workspace':ws})['schema']['revision'] == 2
        common = {'op':'save_object','kind':'note','id':'source','revision':0,'parent':None,
                  'data':{'title':'Source note','owner':'owner','body':'Original text'},
                  'links':[{'target':'domain:security','relation':'related'}],'reason':'Record new information'}
        created = mcp('joppa_command', {'workspace':ws,'request_id':'common-note','command':common})
        common.update(revision=1, data={**common['data'],'body':'Updated text'})
        mcp('joppa_command', {'workspace':ws,'request_id':'common-note-edit','command':common})
        old = mcp('joppa_objects', {'workspace':ws,'address':'note:source','revision':1})
        assert old['document']['data']['body'] == 'Original text'
        assert old['historical'] and old['process'] is None and not old['incoming'] and not old['annotations']
        assert request('/api/workspaces/'+ws+'/objects?address=note:source')[1]['document']['revision'] == 2
        assert request('/api/workspaces/'+ws+'/tree')[1]['current_revision'] == 2
        assert request('/api/workspaces/'+other_ws+'/objects',token=agent_token)[0] == 404
        with urllib.request.urlopen(base+'/tree?workspace='+ws) as response:
            assert response.status == 200 and 'Schema revision' in response.read().decode()

        # Agents can still add both object notes and revision discussion through MCP.
        for request_id, value in [
            ('agent-note', {'op':'note','target':'requirement:key','id':'agent-note','text':'Agent note'}),
            ('agent-comment', {'op':'comment','req':'key','revision':1,'id':'agent-comment','text':'Agent discussion','kind':'comment'}),
        ]:
            mcp('joppa_command', {'workspace':ws,'request_id':request_id,'command':value})
        readback = request('/api/workspaces/'+ws)[1]['state']
        assert readback['notes']['requirement:key']['comments']['agent-note']['text'] == 'Agent note'
        assert readback['requirements']['key']['comments']['agent-comment']['text'] == 'Agent discussion'
        upload = urllib.request.Request(base+'/api/workspaces/'+ws+'/blobs', data=b'Attachment evidence',
            headers={'Authorization':'Bearer '+agent_token,'Content-Type':'application/octet-stream'})
        with urllib.request.urlopen(upload) as response:
            blob = json.load(response)
        targets=['domain:security','capability:restrict','requirement:key','ac:key/key-rights',
                 'note:source']
        for index, target in enumerate(targets):
            mcp('joppa_command', {'workspace':ws,'request_id':'attachment-'+str(index),
                'command':{'op':'attach','target':target,'id':'attachment-'+str(index),
                    'name':'evidence.txt','content_type':'text/plain','size':blob['size'],'hash':blob['hash']}})


        docs = mcp('joppa_model_docs', {})
        assert set(docs) == {'model','process','green','writing','objects'}
        assert docs['writing'] == writing
        assert mcp('joppa_model_docs', {'section':'writing'}) == {'writing':writing}
        assert docs['model'] == (ROOT/'docs/model.md').read_text()
        assert docs['process'] == (ROOT/'docs/process.md').read_text()
        assert docs['green'] == (ROOT/'docs/green.md').read_text()
        assert set(mcp('joppa_model_docs', {'section':'green'})) == {'green'}
        for name, arguments in [
            ('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-create-denied',
                'command':{'op':'workspace','title':'Forbidden remote create'}}),
            ('joppa_batch', {'workspace':mcp_ws,'request_id':'mcp-batch-create-denied',
                'operations':[{'command':{'op':'workspace','title':'Forbidden remote create'}}]}),
            ('joppa_batch', {'workspace':mcp_ws,'request_id':'mcp-late-create-denied',
                'operations':[{'command':{'op':'member','id':'should-not-exist','role':'agent'}},
                              {'command':{'op':'workspace','title':'Forbidden remote create'}}]}),
        ]:
            denied = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
                'params':{'name':name,'arguments':arguments}}, owner_token)[1]['result']
            assert denied['isError'] and 'server CLI' in denied['content'][0]['text']
        assert 'should-not-exist' not in mcp('joppa_read', {'workspace':mcp_ws}, owner_token)['index']['members']
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'member',
            'command':{'op':'member','id':'agent','role':'agent'}},owner_token)
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'consumer',
            'command':{'op':'consumer','id':'customer','title':'Customer','owner':'owner'}},owner_token)
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
                'title':'Recovery preserves access','body':'Only iOS','owner':'owner','consumers':['customer'],'reason':'Agreed scope',
                'acs':[{'text':'On iOS paid access is preserved'}]}}
        ]}
        created = mcp('joppa_batch',batch)
        replay = mcp('joppa_batch',batch)
        assert replay['replayed'] and created['results'] == replay['results']
        assert created['results'][1]['command']['capability'] == created['results'][0]['command']['id']
        public_id = next(iter(created['results'][1]['system_ids']['requirement'].values()))
        resolved = mcp('joppa_read',{'item_id':public_id})
        assert resolved['selection']['id'] == created['results'][1]['command']['req']
        task_req = created['results'][1]['command']['req']
        task_ac = created['results'][1]['command']['acs'][0]['id']
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-open-task','command':{
            'op':'task','req':task_req,'revision':1,'id':'mcp-open-task','title':'Restore access',
            'acs':[task_ac],'projects':[],'purpose':'implementation',
            'contract':'Implement the access recovery flow and attach merged build evidence'}})
        assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'waiting'})['tasks'][0]['id'] == 'task:'+task_req+'/mcp-open-task'
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-task-ready','command':{
            'op':'assess','req':task_req,'revision':1,'ac':task_ac,
            'assessment':{'ready':True,'reason':'Technical contract is clear'}}})
        assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'available'})['tasks'][0]['id'] == 'task:'+task_req+'/mcp-open-task'
        claimed = mcp('joppa_command',{'workspace':mcp_ws,'request_id':'mcp-claim-next',
            'command':{'op':'claim_next_task','priorities':['domain:'+draft_id]}})
        assert claimed['command']['op'] == 'claim_task' and claimed['command']['id'] == 'task:'+task_req+'/mcp-open-task'
        assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'mine'})['tasks'][0]['status'] == 'in_progress'
        client_env={**env,'XDG_CACHE_HOME':str(Path(temp)/'client-cache'),'JOPPA_MCP_URL':base+'/mcp','JOPPA_TOKEN':agent_token,'JOPPA_WORKSPACE':mcp_ws}
        def client(*arguments):
            result=subprocess.run([sys.executable,str(ROOT/'client/joppa'),'task',*arguments],
                                  env=client_env,capture_output=True,text=True,check=True)
            return json.loads(result.stdout)
        assert client('list','--view','mine')['tasks'][0]['id'] == 'task:'+task_req+'/mcp-open-task'
        client('release','mcp-open-task','--reason','Handoff to another process')
        preview = client('next','--dry-run','--priority','domain:'+draft_id)
        assert preview['task']['id'] == 'task:'+task_req+'/mcp-open-task'
        assert client('list','--view','available')['tasks'][0]['id'] == 'task:'+task_req+'/mcp-open-task'
        assert client('next','--priority','requirement:'+task_req,
                      '--request-id','cli-priority-next')['command']['id'] == 'task:'+task_req+'/mcp-open-task'
        client('release','mcp-open-task','--reason','Return to explicit claim')
        assert client('claim','mcp-open-task','--request-id','cli-claim')['command']['id'] == 'task:'+task_req+'/mcp-open-task'
        assert client('claim','mcp-open-task','--request-id','cli-claim')['replayed'] is True
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
            'title': 'Automated example', 'body': '', 'owner': 'owner', 'consumers':['customer'], 'reason': 'Adapter test',
            'acs': [{'id': 'auto-ac', 'text': 'Command exits successfully'}]})
        ok({'op': 'confirm', 'req': 'auto', 'revision': 1})
        ok({'op': 'assess', 'req': 'auto', 'revision': 1, 'ac': 'auto-ac',
            'assessment': {'ready': True, 'reason': 'Known contract'}})
        ok({'op':'define_verification_scope','req':'auto','revision':1,
            'subject_keys':['code'],'scope':'One declared command','reason':'Owner-approved test boundary'})
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
        ok({'op':'define_verification_scope','req':'key','revision':1,
            'subject_keys':['environment'],'scope':'Key permissions before launch',
            'reason':'Owner-approved provider configuration boundary'})
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
        ok({'op':'task','req':'auto','revision':1,'id':'agent-task','title':'Implement command','acs':['auto-ac'],'projects':[],'purpose':'implementation','contract':'Implement command and attach merged build evidence'})
        # The portal retains older revisions after a wording change.
        ok({'op':'revise','req':'key','expected_revision':1,'capability':'restrict',
            'title':'Ключ до запуска','body':'Updated wording','owner':'owner','consumers':['customer'],'reason':'Clarify scope',
            'acs':[{'id':'key-rights','text':'Before launch the key has exactly read and verify','before_launch':True,'decider':'legal'}]})
        refs=request('/api/workspaces/'+ws)[1]['system_ids']
        # MCP discovers revision counts, then reads exactly one chosen revision.
        tools_reply=request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/list'})[1]
        schema=next(t for t in tools_reply['result']['tools'] if t['name']=='joppa_read')['inputSchema']['properties']
        assert schema['revision']['minimum']==1 and 'cursor' not in schema and 'limit' not in schema
        def rejected_read(arguments, token=agent_token):
            response=request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
                'params':{'name':'joppa_read','arguments':arguments}}, token)[1]['result']
            assert response.get('isError'), response
            return response['content'][0]['text']
        for selector in ({'workspace':ws,'req':'key'}, {'item_id':refs['requirement']['key']}, {'item_id':refs['ac']['key-rights']}):
            latest=mcp('joppa_read',selector)
            assert latest['requirement']['revision_count']==2
            assert latest['requirement']['current_revision']==2
            assert [r['number'] for r in latest['requirement']['revisions']]==[2]
            assert latest['requirement']['comments']=={}
            assert latest['requirement']['revisions'][0]['runs']=={}
            assert latest['summary']['accepted'] is False
            assert 'history' not in latest and 'state' not in latest
            history=mcp('joppa_read',{**selector,'history':True})
            assert history['requirement']=={'id':'key','capability':'restrict','revision_count':2,'current_revision':2}
            for key in ('history','notes','summary','state','nextCursor'):
                assert key not in history, history
            for flags in ({'revision':1}, {'history':True,'revision':1}):
                previous=mcp('joppa_read',{**selector,**flags})
                assert [r['number'] for r in previous['requirement']['revisions']]==[1]
                assert set(previous['requirement']['comments'])=={'hello','agent-comment'}
                assert set(previous['requirement']['revisions'][0]['runs'])=={'manual1'}
                assert previous['summary']['accepted'] is True
                assert 'history' not in previous and 'state' not in previous
            for bad in (0,-1,1.5,'1',None,4294967296,999):
                assert 'revision' in rejected_read({**selector,'revision':bad})
            assert 'Workspace not found'==rejected_read({**selector,'revision':999},other_token)
            assert 'revision' in rejected_read({**selector,'history':True,'cursor':'old-cursor'})
        for history in (False,True):
            index=mcp('joppa_read',{'workspace':ws,'history':history})
            row=next(r for r in index['index']['requirements'] if r['id']=='key')
            assert row['revision_count']==2 and row['revision']==2
            assert 'revisions' not in row and 'history' not in index and 'state' not in index
        assert 'revision requires' in rejected_read({'workspace':ws,'revision':1})
        assert 'revision requires' in rejected_read({'item_id':refs['domain']['security'],'revision':1})
        # A copied AC removed from current wording must not silently return the old revision.
        generated_req=created['results'][1]['command']['req']
        generated_ac=created['results'][1]['command']['acs'][0]['id']
        generated_ac_ref=created['results'][1]['system_ids']['ac'][generated_ac]
        mcp('joppa_batch',{'workspace':mcp_ws,'request_id':'replace-ac','operations':[
            {'command':{'op':'note','target':'ac:'+generated_req+'/'+generated_ac,'id':'old-ac-note','text':'Old AC note'}},
            {'command':{'op':'revise','req':generated_req,'expected_revision':1,
                'capability':created['results'][0]['command']['id'],'title':'Recovery preserves access',
                'body':'Updated scope','owner':'owner','consumers':['customer'],'reason':'Replaced criterion',
                'acs':[{'text':'New criterion'}]}}
        ]})
        removed=mcp('joppa_read',{'item_id':generated_ac_ref})
        assert removed['selection']['present'] is False and removed['selection']['revision']==2
        assert [r['number'] for r in removed['requirement']['revisions']]==[2]
        assert 'Old AC note' not in json.dumps(removed)
        restored=mcp('joppa_read',{'item_id':generated_ac_ref,'revision':1})
        assert restored['selection']['present'] is True and restored['selection']['revision']==1
        assert [r['number'] for r in restored['requirement']['revisions']]==[1]
        all_refs=[ref for items in refs.values() for ref in items.values()]
        assert len(all_refs)==len(set(all_refs))
        for kind,items in refs.items():
            tag={'domain':'D','capability':'C','consumer':'U','requirement':'R','ac':'A','task':'T','note':'N'}.get(kind,'O')
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
