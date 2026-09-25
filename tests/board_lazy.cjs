const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {readFileSync}=require('node:fs');
const assert=require('node:assert/strict');
const path=require('node:path');
const root=path.join(__dirname,'..');
const html=readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',readFileSync(path.join(root,'src/board.css'),'utf8'))
  .replace('/* BOARD_SCRIPT */',readFileSync(path.join(root,'src/board.js'),'utf8'));
const revision={number:1,title:'Account access',body:'Customers can sign in',owner:'owner',reason:'Scope',consumers:['buyer'],acs:[{id:'a',text:'Sign-in works',description:'',depends_on:[],owner:null,before_launch:false,assessment:null}],confirmed:false,tasks:{},runs:{},issues:{},acceptance:null,sources:{}};
const responses={
  '/api/me':{actor:'owner',email:'owner@example.test'},
  '/api/workspaces':[{id:'w',title:'Demo'}],
  '/api/workspaces/w/board':{workspace:'w',position:4,lazy:true,state:{title:'Demo',members:{owner:'human'},domains:{d:{title:'Accounts',owner:'owner',description:'',confirmed:true,archived:false}},tree_order:{domain:['d']},capabilities:{},requirements:{},consumers:{},intake:{},notes:{},projects:{}},signals:{domains:{d:{tone:'grey',label:'No capabilities'}},capabilities:{},requirements:{}},summary:{},history:[],launch:{configured:false,ready:false,blockers:[]},system_ids:{domain:{},capability:{},requirement:{},ac:{},consumer:{}},member_labels:{},counts:{consumers:1,intake:0}},
  '/api/workspaces/w/board?level=domain&id=d':{capabilities:{c:{title:'Sign in',domain:'d',owner:'owner',description:'',confirmed:true}},signals:{c:{tone:'grey',label:'No requirements'}},order:['c'],system_ids:{}},
  '/api/workspaces/w/board?level=capability&id=c':{requirements:{r:{capability:'c',revisions:[{number:1,title:revision.title,body:revision.body}],revision_count:1}},signals:{r:{tone:'yellow',label:'Waiting for decision',status:'waiting'}},order:['r'],system_ids:{}},
  '/api/workspaces/w/board?level=requirement&id=r':{requirement:{capability:'c',revisions:[revision],comments:{}},summary:{acs:{a:{ready:false,verification:'n/a'}},ready_for_acceptance:false,accepted:false},consumers:{buyer:{title:'Buyer',description:'Pays for access',owner:'owner'}},notes:{},projects:{},status_events:[{position:4,actor:'owner',recorded_at:'2026-09-25 12:00:00+00',command:{op:'revise',req:'r',expected_revision:0,acs:[{id:'a',text:'Sign-in works'}]}}],system_ids:{ac:{}}},
  '/api/workspaces/w/board?level=consumers':{consumers:{buyer:{title:'Buyer',description:'Pays for access',owner:'owner'}},system_ids:{}},
  '/api/workspaces/w/board?level=history&id=r':{history:[{position:4,actor:'owner',recorded_at:'2026-09-25 12:00:00+00',command:{op:'revise',req:'r',expected_revision:0,title:revision.title,body:revision.body,owner:'owner',consumers:['buyer'],capability:'c',acs:revision.acs}}],requirement:{capability:'c',revisions:[revision],comments:{}}},
  '/api/workspaces/w/board?level=filter':{capabilities:{c:{title:'Sign in',domain:'d',owner:'owner',description:'',confirmed:true}},requirements:{r:{capability:'c',revisions:[revision],revision_count:1}},summary:{r:{acs:{a:{ready:false,verification:'n/a'}},ready_for_acceptance:false,accepted:false}},signals:{domains:{d:{tone:'yellow',label:'Current capabilities: 1 pending'}},capabilities:{c:{tone:'yellow',label:'Current requirements: 1 pending'}},requirements:{r:{tone:'yellow',label:'Waiting for decision',status:'waiting'}}},tree_order:{domain:['d'],'capability:d':['c'],'requirement:c':['r']},system_ids:{capability:{},requirement:{}}},
};
(async()=>{
  const browser=await chromium.launch({headless:true,...(process.env.PLAYWRIGHT_CHROMIUM?{executablePath:process.env.PLAYWRIGHT_CHROMIUM}:{})});
  try{
    const page=await browser.newPage();
    const seen=[],errors=[];
    page.on('pageerror',error=>errors.push(error.message));
    await page.route('http://joppa.test/**',route=>{
      const pathname=new URL(route.request().url()).pathname+new URL(route.request().url()).search;
      if(pathname==='/')return route.fulfill({contentType:'text/html',body:html});
      seen.push(pathname);
      return route.fulfill({status:responses[pathname]===undefined?404:200,json:responses[pathname]||{error:'Unknown path'}});
    });
    await page.goto('http://joppa.test/');
    const domain=page.locator('[data-domain="d"]');await domain.waitFor();
    assert.equal(await page.locator('[data-capability]').count(),0);
    assert.equal(await page.locator('[data-requirement]').count(),0);
    assert.deepEqual(seen,['/api/me','/api/workspaces','/api/workspaces/w/board']);
    assert.equal(await page.evaluate(()=>statusCircle({tone:'green',label:'Complete',stale_after:'2020-01-01T00:00:00Z'}).dataset.tone),'orange');
    await domain.locator(':scope > summary').click();
    const cap=page.locator('[data-capability="c"]');await cap.waitFor();
    await cap.locator(':scope > summary').click();
    const req=page.locator('[data-requirement="r"]');await req.waitFor();
    assert.equal(await req.locator('.ac-item').count(),0);
    await req.locator(':scope > summary').click();
    await req.getByText('Sign-in works').waitFor();
    assert.equal(await req.locator('.ac-item').count(),1);
    const consumers=page.locator('.consumer-section');
    assert.equal(await consumers.evaluate(e=>getComputedStyle(e.querySelector('summary')).backgroundColor),'rgb(220, 239, 250)');
    await consumers.locator('summary').click();
    await consumers.getByText('Buyer').waitFor();
    if(process.env.JOPPA_SCREENSHOT){await page.waitForTimeout(260);await page.screenshot({path:process.env.JOPPA_SCREENSHOT,fullPage:true});}
    assert(seen.includes('/api/workspaces/w/board?level=consumers'));
    assert(!seen.some(path=>path.includes('level=history')));
    await req.locator(':scope > summary .history-button').click();
    await page.locator('.history-dialog').waitFor();
    assert(seen.includes('/api/workspaces/w/board?level=history&id=r'));
    await page.locator('.history-close').click();
    await page.getByRole('combobox',{name:'Filter by status'}).click();
    await page.getByRole('option',{name:'Waiting for decision'}).click();
    await page.locator('[data-requirement="r"]').waitFor();
    assert(seen.includes('/api/workspaces/w/board?level=filter'));
    assert.deepEqual(errors,[]);
  }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
