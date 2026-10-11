// Board behaviour that is not about the tree shape: read-only access, sign in,
// escaping, the verification inspector, attachments, mobile and the offline
// snapshot. Each check names the criterion it proves where one exists.
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {readFileSync}=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const root=path.join(__dirname,'..');
const source=readFileSync(path.join(root,'src/board.js'),'utf8');
const style=readFileSync(path.join(root,'src/board.css'),'utf8');
const page_html=snapshot=>readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',style)
  .replace('/* BRAND_STYLE */',readFileSync(path.join(root,'src/brand.css'),'utf8'))
  .replace('/* BOARD_SCRIPT */',source)
  .replace('/* SNAPSHOT */',snapshot?'window.SNAPSHOT='+JSON.stringify(snapshot).replaceAll('<','\\u003c').replaceAll('>','\\u003e').replaceAll('&','\\u0026')+';':'');

const ATTACK='</script><script>window.attacked=true</script>';
const SUBJECT_ATTACK='<img src=x onerror=alert(1)>';
const types={
  area:{title:'Area',index:10,role:'group',root:true,parents:[]},
  spec:{title:'Spec',index:20,role:'requirement',root:false,parents:['area']},
  measure:{title:'Measure',index:30,role:'ac',root:false,parents:['spec']},
};
const schema={workspace:'w',current_revision:2,schema:{revision:2,actor:'owner',
  at:'2026-10-06T09:00:00Z',reason:'Configured tree',types}};

const object=(address,extra={})=>({address,kind:address.split(':')[0],id:address.split(':')[1],
  title:extra.title||'Object',revision:1,present:true,owner:'owner',at:'2026-10-06T09:00:00Z',
  work_ready:true,review_required:false,item_id:'w-O-ABCDEFGHIJ',links:[],
  parent:extra.parent??null,...extra});
const rows=[
  object('area:core',{title:'Core area'}),
  object('spec:access',{title:'Access spec',parent:'area:core',process:{req:'access',revision:3}}),
];
const old=new Date(Date.now()-14*24*60*60*1000).toISOString();
const record={revision:3,confirmed:true,
  verification_scope:{scope:'One build of the web client'},
  criteria:[
    {id:'m1',address:'measure:m1',title:'Sign in works',signal:{tone:'green',label:'Checks passed',stale_after:old}},
    {id:'m2',address:'measure:m2',title:'Session survives a refresh',signal:{tone:'red',label:'Checks failed'}},
    {id:'m3',address:'measure:m3',title:'Nothing linked yet',signal:{tone:'grey',label:'Not checked'}},
  ],
  runs:{r1:{scope:'One build of the web client',subject:{code:'code-sha-v1',environment:'pg16-image-v1',fixture:SUBJECT_ATTACK},plan:[
    {id:'t-pass[m1]',ac:'m1',method:'automated',expected:['a session cookie']},
    {id:'t-stale[m1]',ac:'m1',method:'automated',expected:['an older pass']},
    {id:'t-fail[m2]',ac:'m2',method:'automated',expected:['a kept session']},
    {id:'t-skip[m2]',ac:'m2',method:'manual',expected:['a manual look']},
  ],results:{
    't-pass[m1]':{outcome:'pass',observed:['a session cookie'],note:'Recorded by the suite'},
    't-stale[m1]':{outcome:'pass',observed:['an older pass'],note:'Recorded two weeks ago'},
    't-fail[m2]':{outcome:'fail',observed:['the session was dropped'],note:'Reproduced twice'},
    't-skip[m2]':{outcome:'skip',observed:[],note:''},
  }}},
  result_dates:{r1:{'t-pass[m1]':new Date().toISOString(),'t-stale[m1]':old}}};

const detail=address=>({workspace:'w',address,kind:address.split(':')[0],id:address.split(':')[1],
  item_id:'w-O-ABCDEFGHIJ',current_revision:1,revision_count:1,work_ready:true,review_required:false,
  process:rows.find(r=>r.address===address)?.process||null,
  document:{revision:1,parent:rows.find(r=>r.address===address)?.parent||null,present:true,links:[],
    data:{title:rows.find(r=>r.address===address)?.title||'Object',owner:'owner',body:ATTACK},
    actor:'owner',at:'2026-10-06T09:00:00Z',reason:'Create'},
  links:[],incoming:[],
  annotations:{comments:{},attachments:{f1:{name:'evidence.png',content_type:'image/png',
    size:2048,hash:'abc',actor:'owner',at:'2026-10-06T09:00:00Z',removed:false}}}});

(async()=>{
const browser=await chromium.launch({headless:true});
try{
  const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  let signedIn=false,revised=false;const methods=new Set();
  await page.route('http://joppa.test/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname.startsWith('/api/workspaces/'))methods.add(route.request().method());
    if(url.pathname==='/')return route.fulfill({contentType:'text/html',body:page_html()});
    if(url.pathname==='/favicon.svg')return route.fulfill({contentType:'image/svg+xml',body:'<svg/>'});
    if(url.pathname==='/auth/login')return route.fulfill({json:{verification_url:'http://joppa.test/guide',expires_in:300}});
    if(url.pathname==='/auth/poll')return route.fulfill({json:{status:'pending'}});
    if(url.pathname==='/guide')return route.fulfill({contentType:'text/html',body:'<p>guide</p>'});
    if(!signedIn&&url.pathname.startsWith('/api/'))return route.fulfill({status:401,json:{error:'Please sign in'}});
    let value;
    if(url.pathname==='/api/me')value={actor:'owner',email:'owner@example.test'};
    else if(url.pathname==='/api/workspaces')value=[{id:'w',title:'Example product'}];
    else if(url.pathname.endsWith('/board')&&url.searchParams.get('level')==='process')value=revised?
      {...record,revision:4,confirmed:false,runs:{},result_dates:{},criteria:record.criteria.map(c=>({...c,signal:{tone:'grey',label:'Not checked'}}))}:record;
    else if(url.pathname.endsWith('/board'))value={workspace:'w',custom_tree:true,lazy:true,
      object_types:types,state:{title:'Example product',members:{owner:'human'},domains:{},
      capabilities:{},requirements:{},consumers:{},notes:{},projects:{}},summary:{},signals:{},
      launch:{configured:false,ready:false,blockers:[]},counts:{},system_ids:{}};
    else if(url.pathname.endsWith('/tree'))value=schema;
    else if(url.pathname.endsWith('/objects')){
      const a=url.searchParams.get('address');
      value=a?detail(a):{items:rows.filter(r=>{
        const parent=url.searchParams.get('parent'),kind=url.searchParams.get('kind');
        if(url.searchParams.get('related_to'))return false;
        if(kind&&r.kind!==kind)return false;
        return parent==='*'?true:(parent||null)===r.parent;
      }),next_cursor:null};
      if(a==='spec:access'){
        value.current_revision=2;value.revision_count=2;value.document.revision=2;
        value.process={req:'access',revision:revised?4:3,verification:revised?'unverified':'pass'};
        if(url.searchParams.get('revision')==='1'){
          value.document.revision=1;value.historical=true;value.process=null;value.signal=null;
        }
      }
    }
    return route.fulfill({status:value?200:404,json:value||{error:'Unknown route'}});
  });

  // Signing in: the gate holds until the popup flow completes.
  await page.goto('http://joppa.test/');
  await page.locator('#login-button').waitFor();
  const popup=page.waitForEvent('popup');
  await page.locator('#login-button').click();
  await (await popup).close();
  await page.locator('#login-cancel').click();
  assert.ok(await page.locator('#login-button').isEnabled(),
    'cancelling the sign-in flow leaves the button usable');

  signedIn=true;
  await page.goto('http://joppa.test/');
  await page.locator('[data-object-address="area:core"]').waitFor();
  assert.equal(await page.locator('link[rel=icon]').getAttribute('href'),'/favicon.svg',
    'the board declares its icon');
  assert.deepEqual([...methods],['GET'],
    '[owner-governs-workspace-ac2] the board only reads, it never writes');

  // Escaping: a document body that looks like a script stays text.
  const spec=page.locator('[data-object-address="spec:access"]');
  await page.locator('[data-object-address="area:core"] > summary').click();
  await spec.waitFor();
  await spec.locator(':scope > summary').click();
  await spec.getByText(ATTACK,{exact:false}).waitFor();
  assert.equal(await page.evaluate(()=>window.attacked),undefined,
    'a script inside a document body never runs');

  // Attachments recorded on the object are listed with it.
  await spec.getByText('evidence.png',{exact:false}).waitFor();

  // The verification inspector: one record, one section per criterion, and a
  // tone for every recorded outcome.
  await spec.getByRole('button',{name:'Checks'}).click();
  const inspector=spec.locator('.verification-record');
  await inspector.waitFor();
  assert.equal(await inspector.locator('.ac-checks').count(),3,
    'every criterion of the revision gets its own section');
  const criterion=i=>inspector.locator('.ac-checks').nth(i);
  assert.equal(await criterion(0).locator('.ac-check').count(),2,
    'a criterion lists each check planned against it');
  const tone=locator=>locator.locator('.ac-check-heading .status-circle').getAttribute('data-tone');
  assert.equal(await tone(criterion(0).locator('.ac-check').nth(0)),'green','a pass reads green');
  assert.equal(await tone(criterion(0).locator('.ac-check').nth(1)),'green',
    '[green-needs-all-checks-ac4] elapsed time alone does not expire a pass');
  assert.equal(await tone(criterion(1).locator('.ac-check').nth(0)),'red','a failure reads red');
  assert.match(await criterion(0).locator('.ac-check').nth(1).locator('.status-circle').getAttribute('aria-label'),
    /^Status: Passed/,'the old pass keeps its outcome label');
  assert.equal(await criterion(0).locator(':scope > .ac-check-heading .status-circle').getAttribute('data-tone'),
    'green','the server verdict stays green when a legacy stale_after date has elapsed');
  await criterion(1).locator('.ac-check').nth(1).getByText('Skipped',{exact:false}).waitFor();
  await criterion(2).getByText('No tests linked yet.',{exact:true}).waitFor();
  assert.match(await criterion(0).locator(':scope > .ac-check-heading .status-circle').getAttribute('aria-label'),
    /^Status: Checks passed/,'a criterion keeps its own status label');
  await criterion(0).getByText('Recorded by the suite',{exact:false}).waitFor();
  await criterion(0).locator('.ac-check').first().getByText('code: code-sha-v1 · environment: pg16-image-v1 · fixture: '+SUBJECT_ATTACK,{exact:true}).waitFor();
  assert.equal(await inspector.locator('img').count(),0,'subject values render as text, never HTML');

  // Mobile: the same card and inspector fit a narrow screen without a
  // horizontal page scrollbar.
  await page.setViewportSize({width:390,height:780});
  await page.waitForFunction(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1);
  await inspector.waitFor();

  // Saved revisions do not borrow the current verdict. A new context has no old runs.
  await spec.locator(':scope > summary .history-button').click();
  const history=page.getByRole('dialog');
  await history.getByLabel('Object revision').selectOption('1');
  await history.getByText('Saved document revision.',{exact:false}).waitFor();
  assert.equal(await history.getByRole('button',{name:'Checks',exact:true}).count(),0,
    '[green-needs-all-checks-ac4] a saved document does not expose current checks');
  assert.equal(await history.locator('[data-tone="green"]').count(),0,
    'a saved revision never borrows the current green status');
  assert.equal(await history.getByText('Verification: Passed',{exact:true}).count(),0);
  await history.getByRole('button',{name:'Close',exact:true}).click();
  revised=true;
  await page.reload();
  await page.locator('[data-object-address="area:core"] > summary').click();
  await spec.locator(':scope > summary').click();
  await spec.getByRole('button',{name:'Checks',exact:true}).click();
  await spec.locator('.verification-record').getByText('Revision 4',{exact:false}).waitFor();
  assert.equal(await spec.locator('.verification-record .ac-check').count(),0,
    'the current context does not reuse checks from the earlier revision');
  assert.equal(await spec.locator('.verification-record [data-tone="green"]').count(),0);
  assert.equal(await spec.locator('.verification-record [data-tone="grey"]').count(),3);

  // The offline snapshot renders the same card with no network at all.
  const snapshot={workspace:'w',lazy:false,custom_tree:true,object_types:types,
    snapshot_schema:schema.schema,
    state:{title:'Example product',members:{owner:'human'},domains:{},capabilities:{},
      requirements:{},consumers:{},notes:{},projects:{}},
    summary:{},signals:{},launch:{configured:false,ready:false,blockers:[]},counts:{},system_ids:{},
    snapshot_objects:{'area:core':{...object('area:core',{title:'Core area'}),current_revision:1,
      revisions:[{revision:1,parent:null,present:true,links:[],
        data:{title:'Core area',owner:'owner',body:ATTACK},actor:'owner',at:'2026-10-06T09:00:00Z',
        reason:'Create'}],annotations:{},process:null}}};
  const offline=await browser.newPage();
  const offlineErrors=[];offline.on('pageerror',e=>offlineErrors.push(e.message));
  let offlineRequests=0;
  await offline.route('**/api/**',route=>{offlineRequests++;return route.abort();});
  await offline.route('http://joppa.test/',route=>route.fulfill({contentType:'text/html',body:page_html(snapshot)}));
  await offline.goto('http://joppa.test/');
  const area=offline.locator('[data-object-address="area:core"]');
  await area.waitFor();
  await area.locator(':scope > summary').click();
  await area.getByText(ATTACK,{exact:false}).waitFor();
  assert.equal(await offline.evaluate(()=>window.attacked),undefined,
    'the snapshot escapes a document body as well');
  assert.equal(offlineRequests,0,'the snapshot reads nothing from the network');
  assert.deepEqual(offlineErrors,[],'the snapshot renders without errors');

  assert.deepEqual(errors,[],'no page errors');
  if(process.env.JOPPA_TEST_URL)await verifyRealServer(browser);
  console.log('Browser: sign in, read-only access, escaping, verification tones, attachments, mobile and snapshot');
}finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});

async function verifyRealServer(browser){
  const base=process.env.JOPPA_TEST_URL, workspace=process.env.JOPPA_TEST_WORKSPACE;
  const page=await browser.newPage({viewport:{width:1280,height:900}});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  const requests=[];
  await page.route('**/api/**',async route=>{
    assert.equal(route.request().method(),'GET','the real portal only reads product data');
    requests.push(new URL(route.request().url()));
    await route.continue({headers:{...route.request().headers(),authorization:'Bearer '+process.env.JOPPA_TEST_TOKEN}});
  });
  await page.goto(base+'/#token='+encodeURIComponent(process.env.JOPPA_TEST_TOKEN));
  await page.locator('#workspaces option[value="'+workspace+'"]').waitFor({state:'attached'});
  await page.locator('#workspaces').selectOption(workspace);
  await page.getByRole('tab',{name:'Domain',exact:true}).click();
  const card=address=>page.locator('[data-object-address="'+address+'"]');
  async function open(address){const item=card(address);await item.waitFor();await item.locator(':scope > summary').click();await item.locator(':scope > .object-body > .object-children').waitFor();return item;}
  const domain=await open('domain:security');
  await open('capability:restrict');
  const req=await open('requirement:key');
  await req.getByText('Updated wording',{exact:true}).waitFor();
  await req.getByText('Agent note',{exact:true}).waitFor();
  assert(!requests.some(url=>url.searchParams.has('revision')),'expansion does not read history');
  await req.getByRole('button',{name:'Checks',exact:true}).first().click();
  await req.locator(':scope > .object-body > .verification-record').waitFor();
  assert(requests.some(url=>url.searchParams.get('level')==='process'&&url.searchParams.get('id')==='requirement:key'));
  await req.locator(':scope > summary .history-button').click();
  const history=page.getByRole('dialog',{name:'Ключ до запуска',exact:true});
  await history.getByRole('combobox',{name:'Object revision'}).selectOption('1');
  await history.getByText(ATTACK,{exact:true}).waitFor();
  assert.equal(await page.evaluate(()=>window.attacked),undefined);
  await history.getByRole('button',{name:'Close',exact:true}).click();
  const before=requests.filter(url=>url.searchParams.get('address')==='domain:security').length;
  await domain.locator(':scope > summary').click();
  await domain.locator(':scope > summary').click();
  assert.equal(requests.filter(url=>url.searchParams.get('address')==='domain:security').length,before,'reopening uses the loaded card');
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'the real cards fit the mobile screen');
  const offline=await browser.newPage();
  const offlineErrors=[];offline.on('pageerror',error=>offlineErrors.push(error.message));
  let network=0;
  await offline.route('**/api/**',route=>{network++;return route.abort();});
  await offline.goto('file://'+process.env.JOPPA_TEST_SNAPSHOT);
  await offline.getByRole('tab',{name:'Domain',exact:true}).click();
  for(const address of ['domain:security','capability:restrict','requirement:key']){
    const item=offline.locator('[data-object-address="'+address+'"]');
    await item.waitFor();await item.locator(':scope > summary').click();
    await item.locator(':scope > .object-body > .object-children').waitFor();
  }
  await offline.getByText(ATTACK,{exact:true}).waitFor();
  assert.equal(await offline.evaluate(()=>window.attacked),undefined);
  assert.equal(network,0,'the real CLI snapshot uses no API requests');
  assert.deepEqual(errors,[]);assert.deepEqual(offlineErrors,[]);
  await offline.close();await page.close();
}
