"""Part 1 of tests/http_smoke.py. The script runs it in its own global names, in order."""
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
# [ac:public-link-preview-ac1] A shared tree, guide or settings link shows the same card with its own URL.
card = {k: v for k, v in preview.meta.items() if k and k.startswith(('og:', 'twitter:')) and k != 'og:url'}
for path in ('/tree?workspace='+ws, '/guide', '/settings'):
    with urllib.request.urlopen(urllib.request.Request(base+path, headers={'User-Agent':'TelegramBot'})) as response:
        page = Preview()
        page.feed(response.read().decode())
    page_url = urllib.parse.urlparse(page.meta.pop('og:url'))
    assert page_url.scheme in ('http','https') and page_url.path == path.split('?')[0], (path, page_url)
    assert len(card) == 13 and all(page.meta.get(k) == v for k, v in card.items()), path
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
assert {t['name'] for t in reply['result']['tools']} == {'joppa_list','joppa_tasks','joppa_command','joppa_batch','joppa_help','joppa_search','joppa_tree','joppa_tests','joppa_objects','joppa_timeline','joppa_record_results','joppa_history','joppa_history_mark','joppa_attach','joppa_attachments'}
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
assert mcp('joppa_objects', {'workspace':ws,'address':'domain:unused'})['present'] is False
assert all(item['address']!='domain:unused' for item in mcp('joppa_objects', {'workspace':ws,'kind':'domain','parent':'*'})['items'])
mcp('joppa_command', {'workspace':ws,'request_id':'restore-group',
    'command':{'op':'restore_object','target':'domain:unused','revision':2,'reason':'Restore the group'}}, owner_token)
current_group = mcp('joppa_objects', {'workspace':ws,'address':'domain:unused'})
assert 'present' not in current_group and current_group['revision']==3
assert 'item_id' not in current_group
assert 'system_ids' not in empty and request('/api/workspaces/'+ws+'/objects?address=domain:unused')[1]['item_id']
assert mcp('joppa_objects', {'workspace':ws,'address':'domain:unused','revision':2})['document']['present'] is False
assert request('/api/workspaces/'+other_ws+'/objects',token=agent_token)[0] == 404
with urllib.request.urlopen(base+'/tree?workspace='+ws) as response:
    assert response.status == 200 and 'Schema revision' in response.read().decode()

