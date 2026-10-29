#!/usr/bin/env python3
"""HTTP/CLI/MCP smoke against an isolated test DB. Optional real browser pass.
JOPPA_TEST_DATABASE_URL=... python3 tests/http_smoke.py [--browser]
Browser needs Playwright (PLAYWRIGHT_MODULE may specify an installed module).
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

def internal(value, workspace=ws, actor='owner'):
    """Check runs are internal now: the server CLI writes them, and HTTP refuses them."""
    subprocess.run([BIN, 'command', workspace, str(uuid.uuid4())], env={**env, 'JOPPA_ACTOR': actor},
                   input=json.dumps(value), text=True, check=True, stdout=subprocess.DEVNULL)

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
        class Preview(HTMLParser):
            def __init__(self):
                super().__init__()
                self.meta = {}
            def handle_starttag(self, tag, attrs):
                values = dict(attrs)
                if tag == 'meta':
                    self.meta[values.get('property', values.get('name'))] = values.get('content')
        with urllib.request.urlopen(urllib.request.Request(base+'/', headers={'User-Agent':'TelegramBot'})) as response:
            preview = Preview()
            public_html = response.read().decode()
            preview.feed(public_html)
            assert response.status == 200
        # [ac:public-link-preview-ac1] The preview contains absolute URLs and product metadata.
        assert preview.meta['og:title'].startswith('Joppa')
        assert urllib.parse.urlparse(preview.meta['og:url']).scheme in ('http','https')
        assert preview.meta['og:description'] == 'AI first product specification. A versioned product tree with connected tasks.'
        # [ac:public-link-preview-ac3] Public HTML omits private product data.
        assert 'Проверка карточки' not in public_html and ws not in public_html
        assert preview.meta['twitter:card'] == 'summary_large_image'
        assert urllib.parse.urlparse(preview.meta['og:image']).scheme in ('https', 'http')
        image_path = urllib.parse.urlparse(preview.meta['og:image']).path
        with urllib.request.urlopen(urllib.request.Request(base+image_path, headers={'User-Agent':'TelegramBot'})) as response:
            image = response.read()
            assert response.status == 200 and response.headers['Content-Type'] == 'image/png'
            assert image[:8] == b'\x89PNG\r\n\x1a\n'
            # [ac:public-link-preview-ac2] The public PNG has the declared dimensions.
            assert int.from_bytes(image[16:20], 'big') == 1200
            assert int.from_bytes(image[20:24], 'big') == 630
        with urllib.request.urlopen(urllib.request.Request(base+'/api/me',headers={'Authorization':'Bearer '+owner_token})) as response:
            assert response.headers.get('Cache-Control') == 'no-store'
        assert request('/api/workspaces', token='invalid')[0] == 401
        assert request('/api/me')[1] == {'actor':'owner','email':None,'name':None}
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
        # The member command takes only people; an agent enrolls through its token.
        assert command({'op': 'member', 'id': 'agent', 'role': 'agent'})[0] == 409
        assert request(f'/api/workspaces/{ws}/agent-tokens', {'agent': 'agent'})[0] == 200
        ok({'op':'member','id':'legal','role':'human'})
        ok({'op':'member','id':'reviewer','role':'human'})
        def save(kind, ident, parent, data, reason='Setup'):
            return ok({'op':'save_object','kind':kind,'id':ident,'revision':0,'parent':parent,'data':data,'reason':reason})
        def criterion(title, before_launch=False):
            return {'title':title,'owner':'owner','consumer':'customer','before_launch':before_launch,
                    'decision_required':False,'depends_on':[]}
        save('consumer','customer',None,{'title':'Customer','owner':'owner'})
        save('domain','security',None,{'title':'Доступ к данным','owner':'owner'})
        save('capability','restrict','domain:security',{'title':'Ограничивать доступ','owner':'owner'})
        save('capability','audit','domain:security',{'title':'Audit access','owner':'owner'})
        save('domain','operations',None,{'title':'Operations','owner':'owner'})
        save('capability','observe','domain:operations',{'title':'Observe service health','owner':'owner'})
        save('requirement','key','capability:restrict',{'title':'Ключ до запуска',
            'body':'</script><script>window.attacked=true</script>','owner':'owner'},'Проверка полного цикла')
        save('ac','key-rights','requirement:key',criterion('До запуска у ключа только read и verify',True))
        # The criterion save gives requirement:key its second revision.
        key_revision = 2
        status, _ = command({'op': 'confirm', 'req':'requirement:key', 'revision': key_revision}, token=agent_token)
        assert status == 409, 'agent cannot confirm owner wording'
        status, body = command({'op': 'confirm', 'req':'requirement:key', 'revision': key_revision, 'actor': 'owner'}, token=agent_token)
        assert status == 422 and 'actor comes from the identity' in str(body), 'actor injection must be refused'
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
        c = {'op': 'comment', 'req':'requirement:key', 'revision': key_revision, 'id': 'hello', 'text': 'Hello', 'kind': 'comment'}
        assert command(c, request_id=retry)[0] == 200
        assert command(c, request_id=retry)[1]['replayed'] is True
        # Remote MCP notifications return 202 without a JSON-RPC response.
        assert request('/mcp', {'jsonrpc':'2.0','method':'notifications/initialized'})[0] == 202
        reply=request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/list'})[1]
        assert {t['name'] for t in reply['result']['tools']} == {'joppa_list','joppa_tasks','joppa_command','joppa_batch','joppa_help','joppa_search','joppa_tree','joppa_tests','joppa_objects','joppa_timeline','joppa_record_results','joppa_history','joppa_history_mark'}
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
        operations = mcp('joppa_help', {})['operations']
        assert 'archive_object' in operations and 'restore_object' in operations
        assert mcp('joppa_tasks', {'workspace':ws})['total'] == 0
        # migrate_test_rules adds the sixth revision: the test rules and the status rule of each type.
        # migrate_note_parents adds the seventh: an inbox type has no parent.
        assert mcp('joppa_tree', {'workspace':ws})['schema']['revision'] == 7
        common = {'op':'save_object','kind':'note','id':'source','revision':0,'parent':None,
                  'data':{'title':'Source note','owner':'owner','body':'Original text'},
                  'links':[{'target':'domain:security','relation':'related'}],'reason':'Record new information'}
        created = mcp('joppa_command', {'workspace':ws,'request_id':'common-note','command':common})
        common.update(revision=1, data={**common['data'],'body':'Updated text'})
        mcp('joppa_command', {'workspace':ws,'request_id':'common-note-edit','command':common})
        old = mcp('joppa_objects', {'workspace':ws,'address':'note:source','revision':1})
        assert old['document']['data']['body'] == 'Original text'
        assert old['historical'] and old.get('process') is None and not old.get('link_groups') and not old.get('annotations')
        assert request('/api/workspaces/'+ws+'/objects?address=note:source')[1]['document']['revision'] == 2
        assert request('/api/workspaces/'+ws+'/tree')[1]['current_revision'] == 7
        empty = mcp('joppa_command', {'workspace':ws,'request_id':'empty-group',
            'command':{'op':'save_object','kind':'domain','id':'unused','revision':0,'parent':None,
                'data':{'title':'Unused group','owner':'owner'},'links':[], 'reason':'Create an empty group'}})
        archive = {'op':'archive_object','target':'domain:unused','revision':1,'reason':'Archive an empty group'}
        mcp('joppa_command', {'workspace':ws,'request_id':'archive-group','command':archive}, owner_token)
        assert mcp('joppa_objects', {'workspace':ws,'address':'domain:unused'})['document']['present'] is False
        assert all(item['address']!='domain:unused' for item in mcp('joppa_objects', {'workspace':ws,'kind':'domain','parent':'*'})['items'])
        mcp('joppa_command', {'workspace':ws,'request_id':'restore-group',
            'command':{'op':'restore_object','target':'domain:unused','revision':2,'reason':'Restore the group'}}, owner_token)
        current_group = mcp('joppa_objects', {'workspace':ws,'address':'domain:unused'})
        assert current_group['document']['present'] is True and current_group['current_revision']==3
        assert 'item_id' not in current_group
        assert request('/api/workspaces/'+ws+'/objects?address=domain:unused')[1]['item_id']==empty['system_ids']['domain']['unused']
        assert mcp('joppa_objects', {'workspace':ws,'address':'domain:unused','revision':2})['document']['present'] is False
        # [ac:mcp-schema-objects-ac2] Process reads retain workspace authorization.
        assert request('/api/workspaces/'+other_ws+'/board?level=process&id=requirement:key', token=agent_token)[0] == 404
        assert request('/api/workspaces/'+other_ws+'/objects',token=agent_token)[0] == 404
        with urllib.request.urlopen(base+'/tree?workspace='+ws) as response:
            assert response.status == 200 and 'Schema revision' in response.read().decode()

        # Agents can still add both object notes and revision discussion through MCP.
        for request_id, value in [
            ('agent-note', {'op':'note','target':'requirement:key','id':'agent-note','text':'Agent note'}),
            ('agent-comment', {'op':'comment','req':'requirement:key','revision':key_revision,'id':'agent-comment','text':'Agent discussion','kind':'comment'}),
        ]:
            mcp('joppa_command', {'workspace':ws,'request_id':request_id,'command':value})
        readback = mcp('joppa_objects', {'workspace':ws,'address':'requirement:key'})
        assert any(c['id'] == 'agent-note' and c['text'] == 'Agent note' for c in readback['comments']['items'])
        upload = urllib.request.Request(base+'/api/workspaces/'+ws+'/blobs', data=b'Attachment evidence',
            headers={'Authorization':'Bearer '+agent_token,'Content-Type':'application/octet-stream'})
        with urllib.request.urlopen(upload) as response:
            blob = json.load(response)
        targets=['domain:security','capability:restrict','requirement:key','ac:key-rights',
                 'note:source']
        for index, target in enumerate(targets):
            mcp('joppa_command', {'workspace':ws,'request_id':'attachment-'+str(index),
                'command':{'op':'attach','target':target,'id':'attachment-'+str(index),
                    'name':'evidence.txt','content_type':'text/plain','size':blob['size'],'hash':blob['hash']}})


        for name, arguments in [
            ('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-create-denied',
                'command':{'op':'workspace','title':'Forbidden remote create'}}),
            ('joppa_batch', {'workspace':mcp_ws,'request_id':'mcp-batch-create-denied',
                'operations':[{'command':{'op':'workspace','title':'Forbidden remote create'}}]}),
            ('joppa_batch', {'workspace':mcp_ws,'request_id':'mcp-late-create-denied',
                'operations':[{'command':{'op':'member','id':'should-not-exist','role':'human'}},
                              {'command':{'op':'workspace','title':'Forbidden remote create'}}]}),
        ]:
            denied = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
                'params':{'name':name,'arguments':arguments}}, owner_token)[1]['result']
            assert denied['isError'] and 'server CLI' in denied['content'][0]['text']
        assert 'should-not-exist' not in request('/api/workspaces/'+mcp_ws)[1]['index']['members']
        assert request(f'/api/workspaces/{mcp_ws}/agent-tokens', {'agent': 'agent'})[0] == 200
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'consumer',
            'command':{'op':'save_object','kind':'consumer','id':'customer','revision':0,'parent':None,
                'data':{'title':'Customer','owner':'owner'},'reason':'Setup'}},owner_token)
        draft = mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-generated',
            'command':{'op':'save_object','kind':'domain','revision':0,'parent':None,
                'data':{'title':'Draft area','owner':'owner'},'reason':'Draft'}})
        draft_id = draft['command']['id']
        uuid.UUID(draft_id)
        # Item IDs start with a slug of the workspace title, MCP authoring checks.
        assert re.match(r'^mcp-authoring-checks(-[0-9]+)?-D-', draft['system_ids']['domain'][draft_id])
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-draft-edit',
            'command':{'op':'save_object','kind':'domain','id':draft_id,'revision':1,'parent':None,
                'data':{'title':'Clear area','owner':'owner'},'reason':'Clarification'}})
        # A batch names an object that it saves, with a generated ID, as {"$ref":"label/address"}.
        batch = {'workspace':mcp_ws,'request_id':'mcp-batch','operations':[
            {'ref':'cap','command':{'op':'save_object','kind':'capability','revision':0,'parent':'domain:'+draft_id,
                'data':{'title':'Recover access','owner':'owner'},'reason':'Agreed scope'}},
            {'ref':'req','command':{'op':'save_object','kind':'requirement','revision':0,'parent':{'$ref':'cap/address'},
                'data':{'title':'Recovery preserves access','body':'Only iOS','owner':'owner'},'reason':'Agreed scope'}},
            {'command':{'op':'save_object','kind':'ac','revision':0,'parent':{'$ref':'req/address'},
                'data':{'title':'On iOS paid access is preserved','owner':'owner','consumer':'customer',
                    'before_launch':False,'decision_required':False,'depends_on':[]},'reason':'Agreed scope'}},
        ]}
        created = mcp('joppa_batch',batch)
        replay = mcp('joppa_batch',batch)
        assert replay['replayed'] and created['results'] == replay['results']
        assert created['results'][1]['command']['parent'] == 'capability:'+created['results'][0]['command']['id']
        task_req = created['results'][1]['command']['id']
        task_ac = created['results'][2]['command']['id']
        assert created['results'][2]['command']['parent'] == 'requirement:'+task_req
        public_id = created['results'][1]['system_ids']['requirement'][task_req]
        resolved = mcp('joppa_search',{'workspace':mcp_ws,'query':public_id})['items'][0]
        assert resolved['exact_id'] is True and resolved['kind'] == 'requirement' and resolved['id'] == task_req
        task_id = 'task:mcp-open-task'
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-open-task','command':{
            'op':'save_object','kind':'task','id':'mcp-open-task','revision':0,'parent':'ac:'+task_ac,
            'data':{'title':'Restore access','owner':'owner','projects':[],'purpose':'implementation',
                'contract':'Implement the access recovery flow and attach merged build evidence'},'reason':'Plan the work'}})
        assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'waiting'})['tasks'][0]['id'] == task_id
        criterion_process = mcp('joppa_objects',{'workspace':mcp_ws,'address':'ac:'+task_ac})['process']
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-task-ready','command':{
            'op':'assess','req':criterion_process['req'],'revision':criterion_process['revision'],'ac':'ac:'+task_ac,
            'assessment':{'ready':True,'reason':'Technical contract is clear'}}})
        assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'available'})['tasks'][0]['id'] == task_id
        claimed = mcp('joppa_command',{'workspace':mcp_ws,'request_id':'mcp-claim-next',
            'command':{'op':'claim_next_task','priorities':['domain:'+draft_id]}})
        assert claimed['command']['op'] == 'claim_task' and claimed['command']['id'] == task_id
        assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'mine'})['tasks'][0]['status'] == 'in_progress'
        client_env={**env,'XDG_CACHE_HOME':str(Path(temp)/'client-cache'),'JOPPA_MCP_URL':base+'/mcp','JOPPA_TOKEN':agent_token,'JOPPA_WORKSPACE':mcp_ws}
        def client(*arguments):
            result=subprocess.run([sys.executable,str(ROOT/'client/joppa'),'task',*arguments],
                                  env=client_env,capture_output=True,text=True,check=True)
            return json.loads(result.stdout)
        assert client('list','--view','mine')['tasks'][0]['id'] == task_id
        client('release','mcp-open-task','--reason','Handoff to another process')
        preview = client('next','--dry-run','--priority','domain:'+draft_id)
        assert preview['task']['id'] == task_id
        assert client('list','--view','available')['tasks'][0]['id'] == task_id
        assert client('next','--priority','requirement:'+task_req,
                      '--request-id','cli-priority-next')['command']['id'] == task_id
        client('release','mcp-open-task','--reason','Return to explicit claim')
        assert client('claim','mcp-open-task','--request-id','cli-claim')['command']['id'] == task_id
        assert client('claim','mcp-open-task','--request-id','cli-claim')['replayed'] is True
        found = mcp('joppa_search', {'workspace':mcp_ws,'query':'recovery','kind':'requirement'})
        assert found['mode'] == 'full_text' and found['semantic_status'] == 'unavailable'
        assert found['items'][0]['system_id'] == public_id
        denied_search = request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
            'params':{'name':'joppa_search','arguments':{'workspace':mcp_ws,'query':'recovery'}}},other_token)[1]
        assert denied_search['result']['isError']
        assert denied_search['result']['content'][0]['text'] == 'Workspace not found'
        foreign_read={'jsonrpc':'2.0','id':1,'method':'tools/call','params':{
            'name':'joppa_tree','arguments':{'workspace':ws}}}
        reply=request('/mcp',foreign_read,other_token)[1]
        assert reply['result']['isError'] is True
        assert reply['result']['content'][0]['text'] == 'Workspace not found'
        save('requirement','auto','capability:restrict',{'title':'Automated example','body':'','owner':'owner'},'Adapter test')
        save('ac','auto-ac','requirement:auto',criterion('Command exits successfully'))
        auto_revision = 2
        ok({'op': 'confirm', 'req':'requirement:auto', 'revision': auto_revision})
        ok({'op': 'assess', 'req':'requirement:auto', 'revision': auto_revision, 'ac':'ac:auto-ac',
            'assessment': {'ready': True, 'reason': 'Known contract'}})
        internal({'op':'define_verification_scope','req':'requirement:auto','revision':auto_revision,
            'subject_keys':['code'],'scope':'One declared command','reason':'Owner-approved test boundary'})
        status, refused = command({'op':'plan','req':'requirement:auto','revision':auto_revision,'run':'refused'})
        assert status >= 400 and 'unknown operation' in str(refused), (status, refused)
        # Changes are made by an integration; the portal only reads the result.
        ok({'op':'confirm','req':'requirement:key','revision':key_revision})
        ok({'op':'assess','req':'requirement:key','revision':key_revision,'ac':'ac:key-rights',
            'assessment':{'ready':True,'reason':'Confirmed exact permission list'}})
        internal({'op':'define_verification_scope','req':'requirement:key','revision':key_revision,
            'subject_keys':['code','environment'],'scope':'Key permissions before launch',
            'reason':'Owner-approved provider configuration boundary'})
        internal({'op':'plan','req':'requirement:key','revision':key_revision,'run':'manual1',
            'subject':{'code':'code-sha-v1','environment':'config-v1'},'scope':'Key permissions before launch',
            'discovery_complete':True,'checks':[{'id':'manual-key-rights','ac':'ac:key-rights',
                'method':'manual','expected':['read','verify']}]})
        internal({'op':'result','req':'requirement:key','revision':key_revision,'run':'manual1',
            'evidence':{'check':'manual-key-rights','outcome':'pass','observed':['read','verify'],
                'note':'Compared the provider permission list'}})
        internal({'op':'finish','req':'requirement:key','revision':key_revision,'run':'manual1'})
        internal({'op':'accept','req':'requirement:key','revision':key_revision,'run':'manual1'})
        # [ac:mcp-schema-objects-ac3] Read only the selected current verification record.
        process = request('/api/workspaces/'+ws+'/board?level=process&id=ac:key-rights')[1]
        assert process['summary']['accepted'] is True
        assert process['req'] in ('key','requirement:key') and process['revision'] == key_revision
        assert set(process['runs']) == {'manual1'}
        assert process['criteria'][0]['address'] == 'ac:key-rights'
        assert process['result_dates']['manual1']['manual-key-rights']
        assert 'state' not in process and 'history' not in process
        # [launch-gate] A check-run acceptance names no commit, so the gate refuses it.
        gate = subprocess.run([BIN, 'gate', ws, 'code-sha-v1'], env=env, text=True, capture_output=True)
        assert gate.returncode != 0 and 'no acceptance on a commit' in gate.stderr, gate.stderr
        save('task','agent-task','ac:auto-ac',{'title':'Implement command','owner':'owner','projects':[],
            'purpose':'implementation','contract':'Implement command and attach merged build evidence'},'Plan the work')
        # The portal keeps older revisions after a wording change.
        ok({'op':'save_object','kind':'requirement','id':'key','revision':key_revision,'parent':'capability:restrict',
            'data':{'title':'Ключ до запуска','body':'Updated wording','owner':'owner'},'reason':'Clarify scope'})
        refs=system_ids()
        # MCP lists the revision headers and reads exactly one chosen revision.
        latest=mcp('joppa_objects',{'workspace':ws,'address':'requirement:key'})
        headers=mcp('joppa_objects',{'workspace':ws,'address':'requirement:key','history':True})
        assert latest['current_revision']==3 and 'revisions' not in latest
        assert headers['current_revision']==3 and [r['revision'] for r in headers['revisions']]==[1,2,3]
        assert latest['document']['data']['body']=='Updated wording' and 'item_id' not in latest
        previous=mcp('joppa_objects',{'workspace':ws,'address':'requirement:key','revision':2})
        assert previous['document']['revision']==2
        assert previous['document']['data']['body']=='</script><script>window.attacked=true</script>'
        def rejected(arguments, token=agent_token):
            response=request('/mcp', {'jsonrpc':'2.0','id':1,'method':'tools/call',
                'params':{'name':'joppa_objects','arguments':arguments}}, token)[1]['result']
            assert response.get('isError'), response
            return response['content'][0]['text']
        for bad in (0,-1,1.5,'1',4294967296):
            assert 'revision' in rejected({'workspace':ws,'address':'requirement:key','revision':bad})
        assert 'Workspace not found'==rejected({'workspace':ws,'address':'requirement:key','revision':1},other_token)
        assert all('document' not in item and 'data' not in item for item in request('/api/workspaces/'+ws)[1]['index']['objects'])
        assert mcp('joppa_objects',{'workspace':ws,'address':'domain:security','revision':1})['document']['revision'] == 1
        all_refs=[ref for items in refs.values() for ref in items.values()]
        assert len(all_refs)==len(set(all_refs))
        for kind,items in refs.items():
            tag={'domain':'D','capability':'C','consumer':'U','requirement':'R','ac':'A','task':'T','note':'N'}.get(kind,'O')
            for local_id,ref in items.items():
                assert ref.startswith(ws+'-'+tag+'-') and len(ref.rsplit('-',1)[1])==10
                # A copied ID resolves through search: the first result is the exact match.
                found=mcp('joppa_search',{'workspace':ws,'query':ref})['items'][0]
                assert found['exact_id'] is True and found['kind']==kind and found['id']==local_id, (ref, found)
        denied=request('/mcp',{'jsonrpc':'2.0','id':9,'method':'tools/call','params':{'name':'joppa_search',
            'arguments':{'workspace':ws,'query':refs['requirement']['key']}}},other_token)[1]
        assert denied['result']['isError'] and denied['result']['content'][0]['text']=='Workspace not found'
        # Custom names run the same workflow through the actual MCP endpoint.
        configured = mcp('joppa_tree', {'workspace': mcp_ws})
        types = configured['schema']['types']
        types.update({
            'spec': {'title':'Spec','role':'requirement','root':True,'parents':['*'],'status':'children'},
            'epic': {'title':'Epic','role':'requirement','root':True,'parents':['*'],'status':'children'},
            'section': {'title':'Section','role':'group','root':True,'parents':['*'],'status':'children'},
            'measure': {'title':'Measure','role':'ac','root':False,'parents':['*'],'status':'tests'},
            'rule': {'title':'Rule','role':'ac','root':False,'parents':['*'],'status':'tests'},
        })
        mcp('joppa_command', {'workspace':mcp_ws,'request_id':'custom-schema','command':
            {'op':'set_tree_schema','revision':configured['current_revision'],'types':types,'reason':'Configure workflow types'}}, owner_token)
        for kind, ident, parent, text in [
            ('spec','same',None,'Outer specification'),
            ('epic','same','spec:same','Inner specification'),
            ('section','middle','epic:same','Intermediate group'),
            ('measure','same','section:middle','The first result'),
            ('rule','same','section:middle','The second result'),
        ]:
            result = mcp('joppa_command', {'workspace':mcp_ws,'request_id':'custom-'+kind,'command':
                {'op':'save_object','kind':kind,'id':ident,'revision':0,'parent':parent,
                 'data':{'title':text,'body':text,'owner':'owner'},'reason':'Define the contract'}})
            if kind=='measure': custom_criterion_id=result['system_ids'][kind][ident]
        def custom(value):
            return mcp('joppa_command', {'workspace':mcp_ws,'request_id':str(uuid.uuid4()),'command':value}, owner_token)
        def internal_custom(value):
            internal(value, mcp_ws)
        custom({'op':'confirm','req':'epic:same','revision':3})
        internal_custom({'op':'define_verification_scope','req':'epic:same','revision':3,'subject_keys':['build'],
                'scope':'The exact custom build','reason':'Agree on the build'})
        for ac in ('measure:same','rule:same'):
            custom({'op':'assess','req':'epic:same','revision':3,'ac':ac,
                    'assessment':{'ready':True,'reason':'The contract is clear'}})
        internal_custom({'op':'plan','req':'epic:same','revision':3,'run':'custom-green','subject':{'build':'sha-one'},
                'scope':'The exact custom build','discovery_complete':True,
                'checks':[{'id':'['+ac+'] check','ac':ac,'method':'automated','expected':['exact behavior']}
                          for ac in ('measure:same','rule:same')]})
        for ac in ('measure:same','rule:same'):
            internal_custom({'op':'result','req':'epic:same','revision':3,'run':'custom-green',
                    'evidence':{'check':'['+ac+'] check','outcome':'pass','observed':['exact behavior'],'note':'Executed the exact test'}})
        internal_custom({'op':'finish','req':'epic:same','revision':3,'run':'custom-green'})
        internal_custom({'op':'accept','req':'epic:same','revision':3,'run':'custom-green'})
        inspected = request('/api/workspaces/'+mcp_ws+'/board?level=process&id=measure:same')[1]
        assert inspected['req']=='epic:same' and inspected['summary']['accepted'] is True
        assert {c['id'] for c in inspected['criteria']} == {'measure:same','rule:same'}
        assert len(inspected['result_dates']['custom-green'])==2
        copied=mcp('joppa_search',{'workspace':mcp_ws,'query':custom_criterion_id})['items'][0]
        assert copied['exact_id'] is True and copied['kind']=='measure' and copied['id']=='same'

        subprocess.run([BIN,'migrate'],env=env,check=True,stdout=subprocess.DEVNULL)
        assert system_ids()==refs
        before=request('/api/workspaces/'+ws)[1]
        ok({'op':'rename_workspace','title':'Review workspace','reason':'Name clarified'})
        after=request('/api/workspaces/'+ws)[1]
        assert after['workspace']==before['workspace']
        # The rename and the prefix that the server derives from the new title: two events.
        assert after['position']==before['position']+2
        # The index keeps its content; each item ID keeps its suffix under the new prefix.
        def suffixes(value):
            if isinstance(value, dict):
                return {k: suffixes(v) for k, v in value.items()}
            if isinstance(value, list):
                return [suffixes(v) for v in value]
            if isinstance(value, str) and re.match(r'^[a-z0-9._-]+-[A-Z]-[A-Z0-9]{10}$', value):
                return value[-13:]
            return value
        assert suffixes(after['index'])==suffixes(before['index'])
        assert 'state' not in after and 'history' not in after
        assert request('/api/workspaces/'+ws+'/board')[1]['state']['title']=='Review workspace'
        if '--browser' in sys.argv:
            browser_env = {**env, 'JOPPA_TEST_URL': base, 'JOPPA_TEST_TOKEN': owner_token,
                           'JOPPA_TEST_WORKSPACE': ws}
            subprocess.run(['node', str(ROOT / 'tests/browser.cjs')], env=browser_env, check=True)
            assert request('/api/workspaces/'+ws)[1] == after, 'browsing must never mutate product data'
        print('HTTP auth, workspace isolation, idempotency and MCP smoke passed')
    finally:
        server.terminate()
        server.wait(timeout=5)
