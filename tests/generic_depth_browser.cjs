// Every schema renders through the one generic board: arbitrary type names,
// six levels of nesting, links that cross roots, and one copy action.
// custom_tree is false here on purpose: the preset flag must not pick a renderer.
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {readFileSync}=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const root=path.join(__dirname,'..');
const html=readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',readFileSync(path.join(root,'src/board.css'),'utf8'))
  .replace('/* BRAND_STYLE */',readFileSync(path.join(root,'src/brand.css'),'utf8')).replace('/* BOARD_SCRIPT */',readFileSync(path.join(root,'src/board.js'),'utf8'));

const chain=[['portfolio','Portfolio','group'],['stream','Stream','group'],['epic','Epic','group'],
  ['outcome','Outcome','requirement'],['measure','Measure','ac'],['action','Action','task']];
const types={};
chain.forEach(([kind,title,role],i)=>{types[kind]={title,role,index:i*10,root:i===0,
  parents:i===0?[]:[chain[i-1][0]],title_size:[21,18,16,17,16,15][i]};});
types.intake={title:'Intake',role:'document',index:70,root:true,parents:[],inbox:true,
  description:'Customer input to review.'};
const schema={workspace:'w',current_revision:3,schema:{revision:3,actor:'owner',
  at:'2026-10-06T09:00:00Z',reason:'Six configured levels',types}};

const address=i=>`${chain[i][0]}:${chain[i][0]}-1`;
const row=(i,links=[])=>({address:address(i),kind:chain[i][0],id:`${chain[i][0]}-1`,
  title:`${chain[i][1]} one`+(i===4?' — A long acceptance condition stays readable on a narrow screen':''),revision:1,present:true,owner:'owner',at:'2026-10-06T09:00:00Z',
  work_ready:true,review_required:false,item_id:`W-${chain[i][0].toUpperCase()}-1`,
  parent:i===0?null:address(i-1),links});
// The deepest action points at a root-level intake: a link across two roots.
const rows=[...chain.map((_,i)=>row(i)),
  {...row(0),address:'intake:in-1',kind:'intake',id:'in-1',title:'Customer input',
   parent:null,item_id:'W-O-9',links:[{target:address(5),relation:'related'},{target:'intake:in-2',relation:'related'}]},
  {...row(0),address:'intake:in-2',kind:'intake',id:'in-2',title:'Second input',
   parent:null,item_id:'W-O-10',links:[{target:'intake:in-1',relation:'related'}]}];
rows[0].verification_summary={total:2,passed:1};
rows[1].verification_summary={total:2,passed:0};
rows[4].signal={tone:'green',label:'Checks passed'};
rows.push({...row(4),address:'measure:measure-2',id:'measure-2',title:'Failed condition',item_id:'W-MEASURE-2',signal:{tone:'red',label:'Checks failed'}});
const find=a=>rows.find(r=>r.address===a);
const document=a=>({workspace:'w',address:a,kind:find(a).kind,id:find(a).id,item_id:find(a).item_id,
  current_revision:1,revision_count:1,work_ready:true,review_required:false,
  document:{revision:1,parent:find(a).parent,present:true,links:find(a).links,
    data:{title:find(a).title,owner:a===address(1)?'rita':a===address(5)?'build-runner':'owner',body:'Read the current contract before work. Keep the source and its saved revisions.'},actor:'owner',at:'2026-10-06T09:00:00Z',reason:'Create'},
  links:find(a).links.map(l=>({link:l,title:find(l.target).title,kind:find(l.target).kind,
    current_revision:1,present:true})),incoming:a==='intake:in-1'?[{source:'intake:in-2',title:'Second input',relation:'related'}]:[],annotations:{},process:a===address(3)?{req:'context',verification:'pass'}:undefined});

(async()=>{
const browser=await chromium.launch({headless:true});
try{
  const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  let clipboardWorks=true,processReads=0;
  let holdInputReload=false,releaseInputReload,inputRequestStarted;
  const inputRequest=new Promise(resolve=>{inputRequestStarted=resolve;});
  await page.addInitScript(()=>{window.__copied=null;});
  await page.route('http://joppa.test/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname==='/')return route.fulfill({contentType:'text/html',body:html});
    if(url.pathname==='/favicon.svg')return route.fulfill({contentType:'image/svg+xml',body:'<svg/>'});
    let value;
    if(url.pathname==='/api/me')value={actor:'owner',email:'sergey.kopanev@example.test'};
    else if(url.pathname==='/api/workspaces')value=[{id:'w',title:'Example product'}];
    else if(url.pathname.endsWith('/board')&&url.searchParams.get('level')==='process'){processReads++;value={revision:1,confirmed:true,criteria:[],runs:{}};}
    else if(url.pathname.endsWith('/board'))value={workspace:'w',custom_tree:false,lazy:true,
      object_types:types,member_labels:{owner:'sergey.kopanev',rita:'Rita Petrov'},state:{title:'Example product',members:{owner:'human',rita:'human','build-runner':'agent'},domains:{},
      capabilities:{},requirements:{},consumers:{},notes:{},projects:{}},summary:{},signals:{},
      launch:{configured:false,ready:false,blockers:[]},counts:{},system_ids:{}};
    else if(url.pathname.endsWith('/tree'))value=schema;
    else if(url.pathname.endsWith('/objects')){
      const a=url.searchParams.get('address');
      const related=url.searchParams.get('related_to');
      if(holdInputReload&&url.searchParams.get('kind')==='intake'&&!related&&!url.searchParams.has('note_status')){
        holdInputReload=false;
        await new Promise(resolve=>{releaseInputReload=resolve;inputRequestStarted();});
      }
      value=a?document(a):{items:rows.filter(r=>{
        const parent=url.searchParams.get('parent');
        const kind=url.searchParams.get('kind');
        if(kind&&r.kind!==kind)return false;
        // A related list holds only the objects actually linked to that address.
        if(related)return r.links.some(l=>l.target===related)
          ||(find(related)?.links||[]).some(l=>l.target===r.address);
        return parent==='*'?true:(parent||null)===r.parent;
      }),next_cursor:null};
    }
    return route.fulfill({status:value?200:404,json:value||{error:'Unknown route'}});
  });

  await page.goto('http://joppa.test/');

  // 1. The preset flag is gone: a schema with no built-in names still renders.
  await page.locator(`[data-object-address="${address(0)}"]`).waitFor();
  assert.equal(await page.locator('.tree-domain').count(),0,
    '[ac:uniform-object-cards-ac1] one card component serves a schema with no built-in type name');

  assert.equal(await page.getByText('Open an object to read its details and children.').count(),0);

  // 2. Six configured levels open one inside the other.
  for(let i=0;i<chain.length;i++){
    const card=page.locator(`[data-object-address="${address(i)}"]`);
    await card.waitFor();
    assert.equal(await card.locator(':scope > summary > .node-title').evaluate(e=>getComputedStyle(e).fontSize),types[chain[i][0]].title_size+'px','Every configured type supplies its own size');
    assert.match(await card.locator(':scope > summary').innerText(),new RegExp(chain[i][1],'i'),
      `[ac:uniform-object-cards-ac1] level ${i+1} takes its title from the schema`);
    await card.locator(':scope > summary').click();
    await card.locator(':scope > .object-body > .object-secondary').waitFor();
  }

  assert.equal(await page.locator('#identity').innerText(),'Sergey Kopanev');
  assert.match(await page.locator(`[data-object-address="${address(0)}"] > .object-body > .object-secondary > .object-details > .object-meta`).textContent(),/Owner: Sergey Kopanev/);
  assert.match(await page.locator(`[data-object-address="${address(1)}"] > .object-body > .object-secondary > .object-details > .object-meta`).textContent(),/Owner: Rita Petrov/);
  assert.match(await page.locator(`[data-object-address="${address(5)}"] > .object-body > .object-secondary > .object-details > .object-meta`).textContent(),/Owner: build-runner/);
  for(let i=0;i<chain.length;i++){
    const card=page.locator(`[data-object-address="${address(i)}"]`);
    assert.equal(await card.locator(':scope > .object-body > .object-secondary .object-details').getAttribute('open'),null,'Details start collapsed');
    assert.ok(await card.locator(':scope > .object-body').evaluate(body=>{
      const nodes=[...body.children];return nodes.indexOf(body.querySelector(':scope > .object-children'))<nodes.indexOf(body.querySelector(':scope > .object-secondary'));
    }),'Structural children precede secondary sections');
  }
  const failed=page.locator('[data-object-address="measure:measure-2"] > summary .status-circle');
  assert.equal(await failed.isVisible(),true,'A failed criterion keeps a visible status before expansion');
  assert.equal(await failed.getAttribute('data-tone'),'red');
  assert.match(await failed.getAttribute('aria-label'),/^Status: Checks failed/);
  assert.equal(await page.locator(`[data-object-address="${address(1)}"] > summary .verification-chip`).innerText(),'2','Zero passing criteria show only the total');
  const rootSummary=page.locator(`[data-object-address="${address(0)}"] > summary`);
  assert.equal(await rootSummary.locator('.verification-chip').innerText(),'1/2');
  assert.equal(await rootSummary.locator('.status-circle').getAttribute('data-tone'),'grey',
    'Partial progress does not verify the whole group');
  assert.equal(await page.locator(`[data-object-address="${address(4)}"] > summary .verification-chip`).innerText(),'Passed',
    'A custom criterion type shows a visible passing label');
  const checks=page.locator(`[data-object-address="${address(3)}"] > .object-body > .object-secondary > .object-checks`);
  assert.equal(await checks.getAttribute('open'),null,'Checks start collapsed');
  assert.equal(processReads,0,'Opening cards does not load check history');
  await checks.locator(':scope > summary').click();
  await checks.locator('.verification-record h3').waitFor();
  assert.equal(processReads,1,'Opening Checks reads only its verification context');
  await checks.locator(':scope > summary').click();
  await page.waitForFunction(()=>document.getAnimations().length===0);
  await checks.locator(':scope > summary').click();
  await page.waitForFunction(()=>document.getAnimations().length===0);
  assert.equal(processReads,1,'Reopening Checks reuses loaded evidence');
  await checks.locator(':scope > summary').click();
  await rootSummary.focus();
  assert.equal(await rootSummary.evaluate(e=>getComputedStyle(e).outlineStyle),'none','Card focus does not outline the whole block');
  assert.equal(await page.getByRole('tabpanel').getAttribute('tabindex'),'-1','Collection panels do not add an invisible keyboard stop');
  assert.equal(await page.locator(`[data-object-address="${address(5)}"] > .object-body > .object-children`).innerText(),'','A leaf shows no empty child section');
  for(const width of [390,320]){
    await page.setViewportSize({width,height:844});
    await page.waitForFunction(()=>document.getAnimations().length===0);
    await page.screenshot({path:'/tmp/joppa-depth-mobile-'+width+'.png',fullPage:true});
    const full=page.locator(`[data-object-address="${address(4)}"] > .object-body > .object-secondary > .object-details .object-full-title`);
    assert.equal(await full.isVisible(),false,'The full title stays in closed Details');
    const details=page.locator(`[data-object-address="${address(4)}"] > .object-body > .object-secondary > .object-details`);
    await details.locator(':scope > summary').click();
    assert.equal(await full.locator('p').innerText(),find(address(4)).title,'Details preserves the complete long title');
    await details.locator(':scope > summary').click();
    await page.waitForFunction(()=>document.getAnimations().length===0);
    assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'The six-level tree fits a mobile viewport');
    for(let i=0;i<chain.length;i++){
      const card=page.locator(`[data-object-address="${address(i)}"]`);
      assert.equal(await card.locator(':scope > summary > .node-title').evaluate(e=>getComputedStyle(e).fontSize),types[chain[i][0]].title_size+'px','Mobile titles retain the configured size');
      const bounds=await card.boundingBox();
      assert.ok(bounds.width>220,`Deep cards keep space for text: viewport=${width}, level=${i+1}, width=${bounds.width}`);
      const title=await card.locator(':scope > summary > .node-title').boundingBox();
      const actions=await card.locator(':scope > summary > .item-actions').boundingBox();
      assert.ok(title.y+title.height<=actions.y+1,'Mobile actions stay below the title');
    }
  }
  await page.screenshot({path:'/tmp/joppa-depth-mobile.png',fullPage:true});
  await page.setViewportSize({width:1280,height:900});
  await page.screenshot({path:'/tmp/joppa-depth-desktop.png',fullPage:true});

  // A long title, the close control and the revision selector never overlap.
  for(const width of [1280,390,320]){
    await page.setViewportSize({width,height:844});
    await page.locator(`[data-object-address="${address(4)}"] > summary .history-button`).click();
    const dialog=page.getByRole('dialog');
    await dialog.getByLabel('Object revision').waitFor();
    const title=await dialog.locator('h2').boundingBox();
    const close=await dialog.getByRole('button',{name:'Close',exact:true}).boundingBox();
    const revision=await dialog.getByLabel('Object revision').boundingBox();
    assert.ok(title.x+title.width<=close.x-8,'The title keeps space before Close');
    assert.ok(revision.y>=Math.max(title.y+title.height,close.y+close.height)+8,'The selector stays below the title and Close');
    assert.ok(await dialog.evaluate(e=>e.scrollWidth<=e.clientWidth),'The dialog has no horizontal overflow');
    assert.ok(await dialog.getByRole('button',{name:'Close',exact:true}).evaluate(e=>e.scrollWidth<=e.clientWidth),'The Close label fits');
    await page.screenshot({path:`/tmp/joppa-revisions-${width}.png`});
    await dialog.getByRole('button',{name:'Close',exact:true}).click();
  }
  await page.setViewportSize({width:1280,height:900});

  // 3. A link crosses two roots and keeps its own title.
  // Root collections come from the schema, in its order.
  assert.deepEqual(await page.getByRole('tab').allTextContents(),['Portfolio','Intake'],
    '[ac:uniform-object-cards-ac2] root collection tabs follow the schema order and titles');
  await page.locator('#tab-intake').click();
  // A collection header is flat: only the cards inside it expand.
  assert.equal(await page.locator('#collection-intake > summary').count(),0,
    '[ac:uniform-object-cards-ac3] the collection header adds no expansion control');
  assert.equal(await page.locator('#collection-intake > header').count(),1,
    '[ac:uniform-object-cards-ac3] the collection keeps a plain header');
  // An input list is read only: the All switch is its only control.
  await page.locator('#collection-intake [data-object-address="intake:in-1"]').waitFor();
  assert.equal(await page.locator('#collection-intake textarea, #collection-intake [contenteditable="true"]').count(),0,
    '[ac:uniform-object-cards-ac4] the input list offers no editing control');
  assert.equal(await page.locator('#collection-intake input:not([type="checkbox"])').count(),0,
    '[ac:uniform-object-cards-ac4] the input list takes no typed input');
  const intake=page.locator('#collection-intake [data-object-address="intake:in-1"]').first();
  await intake.waitFor();
  await intake.locator(':scope > summary').click();
  const links=intake.locator(':scope > .object-body .object-relations');
  await links.waitFor();
  assert.equal(await links.getAttribute('open'),null,'Links start collapsed');
  assert.equal(await links.locator(':scope > summary').innerText(),'Links 2','Incoming and outgoing links to the same object share one row');
  assert.equal(await intake.locator('.note-items').count(),0,'Related inputs do not repeat linked objects as full cards');
  await links.locator(':scope > summary').click();
  assert.equal(await links.locator('[data-link-address="intake:in-2"]').count(),1,'A linked input appears once');
  const target=links.locator('[data-link-address="intake:in-2"]');
  assert.equal(await target.evaluate(e=>e.tagName),'A','Object links use native browser navigation');
  const destination=new URL(await target.getAttribute('href'));
  assert.equal(destination.searchParams.get('workspace'),'w');
  assert.equal(destination.searchParams.get('object'),'intake:in-2');
  // The URL cycle and Back/Forward cases have a separate browser fixture.

  holdInputReload=true;
  await page.getByLabel('All intake',{exact:true}).check();
  await inputRequest;
  await page.locator('#tab-portfolio').click();
  releaseInputReload();
  await page.waitForFunction(()=>!document.querySelector('#collection-intake .object-loading'));
  assert.equal(await page.locator('#collection-intake').isVisible(),false,'A late input response keeps its inactive tab hidden');
  assert.equal(await page.getByRole('tabpanel').count(),1);
  await page.locator('#tab-intake').click();
  await page.setViewportSize({width:320,height:844});
  assert.ok(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'The input queue switch fits a narrow viewport');
  await page.setViewportSize({width:1280,height:900});

  // 4. One copy action: same control on every card, success only after the
  //    clipboard resolves, and one toast that leaves on its own.
  await page.locator('#tab-portfolio').click();
  await page.locator(`[data-object-address="${address(5)}"]`).waitFor();
  // navigator.clipboard is undefined on plain http, so define it outright.
  await page.evaluate(()=>{Object.defineProperty(navigator,'clipboard',{configurable:true,
    value:{writeText:t=>{window.__copied=t;return Promise.resolve();}}});});
  const deep=page.locator(`[data-object-address="${address(5)}"] .copy`).first();
  assert.equal(await deep.locator('.agent-face').count(),1);
  const before=await deep.boundingBox();
  await deep.click();
  assert.equal(await deep.getAttribute('data-copy-state'),'copied',
    '[ac:copy-feedback-ac2] Copied appears only after the clipboard resolved');
  assert.equal(await page.evaluate(()=>window.__copied),'W-ACTION-1',
    '[ac:copy-feedback-ac1] the shared action copies the public identifier');
  const after=await deep.boundingBox();
  assert.equal(Math.round(before.width),Math.round(after.width),
    '[ac:copy-feedback-ac2] Copied does not move the card layout');
  const toast=page.locator('#toast');
  await toast.waitFor({state:'visible'});
  assert.equal(await toast.getAttribute('role'),'status',
    '[ac:copy-feedback-ac3] the toast announces itself once as a status');
  assert.equal(await toast.getAttribute('aria-live'),'polite',
    '[ac:copy-feedback-ac3] the toast announcement stays polite');
  // Lower right: the box starts past the middle on both axes.
  const view=page.viewportSize(),box=await toast.boundingBox();
  assert.ok(box.x>view.width/2&&box.y>view.height/2,
    '[ac:copy-feedback-ac3] the toast sits in the lower right corner');
  assert.equal(await toast.evaluate(e=>getComputedStyle(e).transform==='none'||getComputedStyle(e).transform.includes('matrix')),true,
    '[ac:copy-feedback-ac3] the toast arrives by moving, not by jumping');
  // Three seconds of reading time, then it leaves on its own.
  await new Promise(done=>setTimeout(done,2000));
  assert.equal(await toast.evaluate(e=>e.classList.contains('toast-visible')),true,
    '[ac:copy-feedback-ac3] the toast is still readable two seconds in');
  await page.waitForFunction(()=>!document.querySelector('#toast')?.classList.contains('toast-visible'),null,{timeout:4000});
  assert.equal(await toast.evaluate(e=>e.classList.contains('toast-visible')),false,
    '[ac:copy-feedback-ac3] the toast leaves after about three seconds');

  // 5. A clipboard failure says so instead of claiming success.
  await page.evaluate(()=>{Object.defineProperty(navigator,'clipboard',{configurable:true,
    value:{writeText:()=>Promise.reject(Error('denied'))}});document.execCommand=()=>false;});
  const other=page.locator(`[data-object-address="${address(0)}"] .copy`).first();
  await other.click();
  assert.match(await toast.innerText(),/Could not copy/,
    '[ac:copy-feedback-ac4] a refused clipboard reports the failure');
  assert.notEqual(await other.getAttribute('data-copy-state'),'copied',
    '[ac:copy-feedback-ac4] a refused clipboard never claims success');
  // The criterion asks for the identifier to stay available by hand, so it has
  // to be readable somewhere after the failure, not only inside the clipboard.
  assert.equal(await page.getByRole('dialog',{name:'Copy ID',exact:true}).getByLabel('ID',{exact:true}).inputValue(),find(address(0)).item_id,
    '[ac:copy-feedback-ac4] a refused clipboard leaves the identifier readable for manual copying');

  assert.deepEqual(errors,[],'no page errors');
  console.log('generic_depth_browser: ok');
}finally{await browser.close();}
})();
