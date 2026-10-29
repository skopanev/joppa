"""Part 2 of tests/http_smoke.py. The script runs it in its own global names, in order."""
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
assert re.match(r'^mcp-authoring-checks(-[0-9]+)?-D-', request('/api/workspaces/'+mcp_ws+'/objects?address=domain:'+draft_id)[1]['item_id'])
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
public_id = request('/api/workspaces/'+mcp_ws+'/objects?address=requirement:'+task_req)[1]['item_id']
resolved = mcp('joppa_search',{'workspace':mcp_ws,'query':public_id})['items'][0]
assert resolved['exact_id'] is True and resolved['kind'] == 'requirement' and resolved['id'] == task_req
task_id = 'task:mcp-open-task'
mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-open-task','command':{
    'op':'save_object','kind':'task','id':'mcp-open-task','revision':0,'parent':'ac:'+task_ac,
    'data':{'title':'Restore access','owner':'owner','projects':[],'purpose':'implementation',
        'contract':'Implement the access recovery flow and attach merged build evidence'},'reason':'Plan the work'}})
assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'waiting'})['items'][0]['address'] == task_id
criterion_read = mcp('joppa_objects',{'workspace':mcp_ws,'address':'ac:'+task_ac})
criterion_process = criterion_read['process']
mcp('joppa_command', {'workspace':mcp_ws,'request_id':'mcp-task-ready','command':{
    'op':'assess','req':criterion_read['document']['parent'],'revision':criterion_process['assessment']['revision'],'ac':'ac:'+task_ac,
    'assessment':{'ready':True,'reason':'Technical contract is clear'}}})
assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'available'})['items'][0]['address'] == task_id
claimed = mcp('joppa_command',{'workspace':mcp_ws,'request_id':'mcp-claim-next',
    'command':{'op':'claim_next_task','priorities':['domain:'+draft_id]}})
assert claimed['command']['op'] == 'claim_task' and claimed['command']['id'] == task_id
assert mcp('joppa_tasks',{'workspace':mcp_ws,'view':'mine'})['items'][0]['status'] == 'in_progress'
client_env={**env,'XDG_CACHE_HOME':str(Path(temp)/'client-cache'),'JOPPA_MCP_URL':base+'/mcp','JOPPA_TOKEN':agent_token,'JOPPA_WORKSPACE':mcp_ws}
def client(*arguments):
    result=subprocess.run([sys.executable,str(ROOT/'client/joppa'),'task',*arguments],
                          env=client_env,capture_output=True,text=True,check=True)
    return json.loads(result.stdout)
assert client('list','--view','mine')['items'][0]['address'] == task_id
client('release','mcp-open-task','--reason','Handoff to another process')
preview = client('next','--dry-run','--priority','domain:'+draft_id)
assert preview['task']['address'] == task_id
assert client('list','--view','available')['items'][0]['address'] == task_id
assert client('next','--priority','requirement:'+task_req,
              '--request-id','cli-priority-next')['command']['id'] == task_id
client('release','mcp-open-task','--reason','Return to explicit claim')
assert client('claim','mcp-open-task','--request-id','cli-claim')['command']['id'] == task_id
assert client('claim','mcp-open-task','--request-id','cli-claim')['replayed'] is True
found = mcp('joppa_search', {'workspace':mcp_ws,'query':'recovery','kind':'requirement'})
assert found['mode'] == 'full_text' and found['semantic_status'] == 'unavailable'
assert found['items'][0]['id'] == task_req and 'system_id' not in found['items'][0] and 'req' not in found['items'][0]
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
status, refused = command({'op':'plan','req':'requirement:auto','revision':auto_revision,'run':'refused'})
assert status >= 400 and 'plan was removed; use joppa_record_results' in str(refused), (status, refused)
# Changes are made by an integration; the portal only reads the result.
ok({'op':'confirm','req':'requirement:key','revision':key_revision})
ok({'op':'assess','req':'requirement:key','revision':key_revision,'ac':'ac:key-rights',
    'assessment':{'ready':True,'reason':'Confirmed exact permission list'}})
# [launch-gate] Without an acceptance on a commit the gate refuses.
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
assert headers['current_revision']==3 and headers['total']==3 and [r['revision'] for r in headers['items']]==[1,2,3]
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
    if kind=='measure': custom_criterion_id=request('/api/workspaces/'+mcp_ws+'/objects?address=measure:'+ident)[1]['item_id']
def custom(value):
    return mcp('joppa_command', {'workspace':mcp_ws,'request_id':str(uuid.uuid4()),'command':value}, owner_token)
custom({'op':'confirm','req':'epic:same','revision':3})
for ac in ('measure:same','rule:same'):
    custom({'op':'assess','req':'epic:same','revision':3,'ac':ac,
            'assessment':{'ready':True,'reason':'The contract is clear'}})
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
print('HTTP auth, workspace isolation, idempotency and MCP smoke passed')
