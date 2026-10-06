const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {readFileSync}=require('node:fs');
const assert=require('node:assert/strict');
const path=require('node:path');
const root=path.join(__dirname,'..');
const html=readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',readFileSync(path.join(root,'src/board.css'),'utf8'))
  .replace('/* BOARD_SCRIPT */',readFileSync(path.join(root,'src/board.js'),'utf8'));
const revision={number:1,title:'Account access',body:'Customers can sign in and keep access across devices. The session remains active after switching accounts, refreshing the page, and returning later. Every change should preserve the access contract for paying customers without requiring another purchase.',owner:'owner',reason:'Scope',consumers:['buyer'],acs:[{id:'a',text:'Sign-in works',description:'',depends_on:[],owner:null,before_launch:false,assessment:null}],confirmed:false,tasks:{},runs:{},issues:{},acceptance:null,sources:{}};
revision.acs.push({id:'b',text:'The customer remains signed in after a page refresh',description:'',depends_on:[],owner:null,before_launch:false,assessment:null});
revision.acs.push({id:'c',text:'A returning customer can use an existing purchase without paying twice',description:'',depends_on:[],owner:null,before_launch:false,assessment:null});
const responses={
  '/api/me':{actor:'owner',email:'owner@example.test'},
  '/api/workspaces':[{id:'w',title:'Demo'}],
  '/api/workspaces/w/board':{workspace:'w',position:4,lazy:true,state:{title:'Demo',members:{owner:'human'},domains:{d:{title:'Accounts',owner:'owner',description:'',confirmed:true,archived:false}},tree_order:{domain:['d']},capabilities:{},requirements:{},consumers:{},intake:{},notes:{},projects:{}},signals:{domains:{d:{tone:'grey',label:'No capabilities'}},capabilities:{},requirements:{}},summary:{},history:[],launch:{configured:false,ready:false,blockers:[]},system_ids:{domain:{},capability:{},requirement:{},ac:{},consumer:{}},member_labels:{},object_types:{domain:{title:'Domains',role:'group',root:true,color:'#17685f'},consumer:{title:'Consumers',role:'document',root:true,color:'#247eab'},note:{title:'Inputs',role:'document',root:true,color:'#7664aa'}},counts:{consumers:1}},
  '/api/workspaces/w/board?level=domain&id=d':{capabilities:{c:{title:'Sign in',domain:'d',owner:'owner',description:'',confirmed:true},c2:{title:'Manage account',domain:'d',owner:'owner',description:'',confirmed:true},c3:{title:'Recover access',domain:'d',owner:'owner',description:'',confirmed:true}},signals:{c:{tone:'grey',label:'No requirements'},c2:{tone:'grey',label:'No requirements'},c3:{tone:'grey',label:'No requirements'}},order:['c','c2','c3'],system_ids:{}},
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
    await page.route('http://joppa.test/**',async route=>{
      const pathname=new URL(route.request().url()).pathname+new URL(route.request().url()).search;
      if(pathname==='/')return route.fulfill({contentType:'text/html',body:html});
      if(pathname==='/favicon.svg')return route.fulfill({contentType:'image/svg+xml',body:'<svg xmlns="http://www.w3.org/2000/svg"/>'});
      seen.push(pathname);
      assert.equal(route.request().method(),'GET','the board reads data without changing objects');
      if(pathname.startsWith('/api/workspaces/w/objects?')){
        const query=new URL(route.request().url()).searchParams;
        const value=query.has('address')?{address:'consumer:buyer',kind:'consumer',id:'buyer',current_revision:1,
          document:{revision:1,data:{title:'Buyer',owner:'owner',description:'Pays for access'}},links:[],incoming:[],annotations:{}}:{items:query.get('kind')==='consumer'?[{address:'consumer:buyer',kind:'consumer',title:'Buyer',owner:'owner',work_ready:true}]:[],next_cursor:null};
        return route.fulfill({json:value});
      }
      if(pathname.includes('level=domain'))await new Promise(resolve=>setTimeout(resolve,180));
      return route.fulfill({status:responses[pathname]===undefined?404:200,json:responses[pathname]||{error:'Unknown path'}});
    });
    await page.goto('http://joppa.test/');
    const domain=page.locator('[data-domain="d"]');await domain.waitFor();
    await page.evaluate(()=>window.initialIntro=document.querySelector('.tree-intro'));
    const style=await page.locator('#workspaces').evaluate(e=>{const s=getComputedStyle(e);return [s.height,s.fontSize,s.fontWeight,s.borderRadius];});
    for(const trigger of await page.locator('.filter-select').all())assert.deepEqual(await trigger.evaluate(e=>{const s=getComputedStyle(e);return [s.height,s.fontSize,s.fontWeight,s.borderRadius];}),style,'filters use the workspace selection style');
    assert.equal(await page.locator('[data-capability]').count(),0);
    assert.equal(await page.locator('[data-requirement]').count(),0);
    assert.deepEqual(seen,['/api/me','/api/workspaces','/api/workspaces/w/board']);
    assert.equal(await page.evaluate(()=>statusCircle({tone:'green',label:'Complete',stale_after:'2020-01-01T00:00:00Z'}).dataset.tone),'orange');
    await domain.locator(':scope > summary').click();
    const cap=page.locator('[data-capability="c"]');await cap.waitFor();
    assert.equal(await page.evaluate(()=>document.querySelector('.tree-intro')===window.initialIntro),true);
    assert.equal(await domain.locator('.domain-body').evaluate(e=>getComputedStyle(e).animationName),'none');
    const loadedStart=await domain.evaluate(e=>e.getBoundingClientRect().height);
    await page.waitForTimeout(90);
    const loadedMiddle=await domain.evaluate(e=>e.getBoundingClientRect().height);
    assert(loadedMiddle>loadedStart,`Lazy content should grow smoothly: ${loadedStart} < ${loadedMiddle}`);
    await page.waitForTimeout(400);
    await domain.locator(':scope > summary').click();
    await page.waitForTimeout(400);
    await domain.locator(':scope > summary').click();
    const openingHeight=await domain.evaluate(e=>e.getBoundingClientRect().height);
    await page.waitForTimeout(120);
    const middleHeight=await domain.evaluate(e=>e.getBoundingClientRect().height);
    await page.waitForTimeout(400);
    const finalHeight=await domain.evaluate(e=>e.getBoundingClientRect().height);
    assert(openingHeight<middleHeight&&middleHeight<finalHeight,`Expected smooth expansion: ${openingHeight} < ${middleHeight} < ${finalHeight}`);
    await domain.locator(':scope > summary').click();
    await page.waitForTimeout(70);
    await domain.locator(':scope > summary').click();
    await page.waitForTimeout(400);
    assert.equal(await domain.evaluate(e=>e.open&&!e.style.height),true);
    await page.emulateMedia({reducedMotion:'reduce'});
    await domain.locator(':scope > summary').click();
    assert.equal(await domain.evaluate(e=>e.open),false);
    await domain.locator(':scope > summary').click();
    await page.emulateMedia({reducedMotion:'no-preference'});
    await cap.locator(':scope > summary').click();
    const req=page.locator('[data-requirement="r"]');await req.waitFor();
    await page.waitForTimeout(400);
    assert.equal(await page.evaluate(()=>document.querySelector('.tree-intro')===window.initialIntro),true);
    assert.equal(await req.locator('.ac-item').count(),0);
    const bodyPreview=req.locator(':scope > summary > .requirement-preview');
    const bodyTop=await bodyPreview.evaluate(e=>e.getBoundingClientRect().top+scrollY);
    await req.locator(':scope > summary').click();
    assert.equal(await bodyPreview.evaluate((e,top)=>Math.abs(e.getBoundingClientRect().top+scrollY-top)<2,bodyTop),true);
    await req.getByText('Sign-in works').waitFor();
    assert.equal(await req.evaluate(e=>getComputedStyle(e).filter),'none');
    assert.equal(await req.locator('.ac-item').count(),3);
    assert.deepEqual(await page.evaluate(()=>{
      const before=data.summary.r.pending_run;
      data.summary.r.pending_run='newer-run';
      const wasLazy=data.lazy;data.lazy=false;
      const result={requirement:requirementSignal('r').tone,ac:acSignal(data.summary.r.acs.a,data.state.requirements.r.revisions.at(-1),'a','r').tone};
      data.summary.r.acs.a.verification='fail';
      result.failedLabel=requirementSignal('r').label;
      data.summary.r.acs.a.verification='n/a';
      data.lazy=wasLazy;
      if(before===undefined)delete data.summary.r.pending_run;else data.summary.r.pending_run=before;
      return result;
    }),{requirement:'yellow',ac:'yellow',failedLabel:'Checks failed · newer run in progress'});
    assert.equal(await req.locator('.requirement-body').count(),0);
    const consumers=page.locator('.consumer-section');
    assert.equal(await consumers.evaluate(e=>e.style.getPropertyValue('--object-color')),'#247eab');
    await page.getByRole('tab',{name:'Consumers',exact:true}).click();
    await consumers.getByText('Buyer').waitFor();
    const buyer=consumers.locator('[data-object-address="consumer:buyer"]');
    assert.equal(await buyer.evaluate(e=>e.open),false,'a consumer starts closed like a domain');
    assert(!seen.some(url=>url.includes('address=consumer')),'the collection reads only summaries');
    await buyer.locator(':scope > summary').click();
    await buyer.getByText('Pays for access').waitFor();
    await page.waitForFunction(()=>document.getAnimations().length===0);
    await buyer.locator(':scope > summary').click();
    await page.waitForFunction(()=>document.getAnimations().length===0);
    await buyer.locator(':scope > summary').click();
    assert.equal(seen.filter(url=>url.includes('address=consumer')).length,1,'a consumer reuses its loaded details');
    const related=buyer.locator('.note-section');
    await related.locator(':scope > summary').click();
    await page.waitForFunction(()=>document.querySelector('[data-object-address="consumer:buyer"] .note-section').classList.contains('empty-notes'));
    assert.equal(await related.evaluate(e=>e.open),false,'an empty related Notes section stays closed');
    await related.locator(':scope > summary').click();
    assert.equal(await related.evaluate(e=>e.open),false);
    const inputs=page.locator('#tree > .note-section');
    await page.getByRole('tab',{name:'Inputs',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('#tree > .note-section').classList.contains('empty-notes'));
    assert.equal(await inputs.evaluate(e=>e.open),false,'an empty Notes queue stays closed');
    await inputs.getByLabel('All notes').check();
    await page.waitForFunction(()=>document.querySelector('#tree > .note-section').classList.contains('empty-notes')&&!document.querySelector('#tree > .note-section .object-loading'));
    assert.equal(await inputs.evaluate(e=>e.open),false);
    await page.getByRole('tab',{name:'Domains',exact:true}).click();
    assert.equal(await page.evaluate(()=>document.querySelector('.tree-intro')===window.initialIntro),true);
    assert.equal(await page.locator('.intake-section').count(),0);
    assert(!seen.some(path=>path.includes('level=intake')));
    if(process.env.JOPPA_SCREENSHOT){await page.evaluate(()=>scrollTo(0,0));await page.waitForTimeout(260);await page.screenshot({path:process.env.JOPPA_SCREENSHOT,fullPage:true});}
    assert(seen.some(url=>url.includes('/objects?')&&new URL('http://joppa.test'+url).searchParams.get('kind')==='consumer'));
    assert(!seen.some(path=>path.includes('level=history')));
    await req.locator(':scope > summary .history-button').click();
    await page.locator('.history-dialog').waitFor();
    assert(seen.includes('/api/workspaces/w/board?level=history&id=r'));
    await page.locator('.history-close').click();
    await page.getByRole('combobox',{name:'Filter by status'}).click();
    await page.getByRole('option',{name:'Waiting for decision'}).click();
    await page.locator('[data-requirement="r"]').waitFor();
    assert(seen.includes('/api/workspaces/w/board?level=filter'));
    await page.setViewportSize({width:390,height:844});
    const mobileReq=page.locator('[data-requirement="r"]');
    if(await mobileReq.evaluate(e=>e.open)){
      await mobileReq.locator(':scope > summary > .node-title').click();
      await page.waitForTimeout(350);
    }
    const mobileBody=mobileReq.locator(':scope > summary > .requirement-preview');
    const mobileTop=await mobileBody.evaluate(e=>e.getBoundingClientRect().top+scrollY);
    await mobileReq.locator(':scope > summary > .node-title').click();
    assert.equal(await mobileBody.evaluate((e,top)=>Math.abs(e.getBoundingClientRect().top+scrollY-top)<2,mobileTop),true);
    assert.equal(await mobileReq.evaluate(e=>e.open),true);
    await mobileReq.getByText('Sign-in works').waitFor();
    if(process.env.JOPPA_MOBILE_SCREENSHOT){await page.evaluate(()=>scrollTo(0,0));await page.waitForTimeout(260);await page.screenshot({path:process.env.JOPPA_MOBILE_SCREENSHOT,fullPage:true});}
    assert.deepEqual(errors,[]);
    await page.evaluate(()=>sessionStorage.clear());
    await page.reload();
    const quickDomain=page.locator('[data-domain="d"]');await quickDomain.waitFor();
    await quickDomain.locator(':scope > summary > .node-title').click();
    await page.waitForTimeout(30);
    await quickDomain.locator(':scope > summary > .node-title').click();
    await page.waitForTimeout(300);
    assert.equal(await quickDomain.evaluate(e=>e.open||e.classList.contains('branch-closing')),false);
  }finally{await browser.close();}
})().catch(error=>{console.error(error);process.exitCode=1;});
