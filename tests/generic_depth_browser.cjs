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
  .replace('/* BOARD_SCRIPT */',readFileSync(path.join(root,'src/board.js'),'utf8'));

const chain=[['portfolio','Portfolio','group'],['stream','Stream','group'],['epic','Epic','group'],
  ['outcome','Outcome','requirement'],['measure','Measure','ac'],['action','Action','task']];
const types={};
chain.forEach(([kind,title,role],i)=>{types[kind]={title,role,index:i*10,root:i===0,
  parents:i===0?[]:[chain[i-1][0]]};});
types.intake={title:'Intake',role:'document',index:70,root:true,parents:[],inbox:true,
  description:'Customer input to review.'};
const schema={workspace:'w',current_revision:3,schema:{revision:3,actor:'owner',
  at:'2026-10-06T09:00:00Z',reason:'Six configured levels',types}};

const address=i=>`${chain[i][0]}:${chain[i][0]}-1`;
const row=(i,links=[])=>({address:address(i),kind:chain[i][0],id:`${chain[i][0]}-1`,
  title:`${chain[i][1]} one`,revision:1,present:true,owner:'owner',at:'2026-10-06T09:00:00Z',
  work_ready:true,review_required:false,item_id:`W-${chain[i][0].toUpperCase()}-1`,
  parent:i===0?null:address(i-1),links});
// The deepest action points at a root-level intake: a link across two roots.
const rows=[...chain.map((_,i)=>row(i)),
  {...row(0),address:'intake:in-1',kind:'intake',id:'in-1',title:'Customer input',
   parent:null,item_id:'W-O-9',links:[{target:address(5),relation:'related'}]}];
const find=a=>rows.find(r=>r.address===a);
const document=a=>({workspace:'w',address:a,kind:find(a).kind,id:find(a).id,item_id:find(a).item_id,
  current_revision:1,revision_count:1,work_ready:true,review_required:false,
  document:{revision:1,parent:find(a).parent,present:true,links:find(a).links,
    data:{title:find(a).title,owner:'owner'},actor:'owner',at:'2026-10-06T09:00:00Z',reason:'Create'},
  links:find(a).links.map(l=>({link:l,title:find(l.target).title,kind:find(l.target).kind,
    current_revision:1,present:true})),incoming:[],annotations:{}});

(async()=>{
const browser=await chromium.launch({headless:true});
try{
  const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  let clipboardWorks=true;
  await page.addInitScript(()=>{window.__copied=null;});
  await page.route('http://joppa.test/**',async route=>{
    const url=new URL(route.request().url());
    if(url.pathname==='/')return route.fulfill({contentType:'text/html',body:html});
    if(url.pathname==='/favicon.svg')return route.fulfill({contentType:'image/svg+xml',body:'<svg/>'});
    let value;
    if(url.pathname==='/api/me')value={actor:'owner',email:'owner@example.test'};
    else if(url.pathname==='/api/workspaces')value=[{id:'w',title:'Example product'}];
    else if(url.pathname.endsWith('/board'))value={workspace:'w',custom_tree:false,lazy:true,
      object_types:types,state:{title:'Example product',members:{owner:'human'},domains:{},
      capabilities:{},requirements:{},consumers:{},notes:{},projects:{}},summary:{},signals:{},
      launch:{configured:false,ready:false,blockers:[]},counts:{},system_ids:{}};
    else if(url.pathname.endsWith('/tree'))value=schema;
    else if(url.pathname.endsWith('/objects')){
      const a=url.searchParams.get('address');
      value=a?document(a):{items:rows.filter(r=>{
        const parent=url.searchParams.get('parent');
        const kind=url.searchParams.get('kind');
        return (kind?r.kind===kind:true)&&(parent==='*'?true:(parent||null)===r.parent);
      }),next_cursor:null};
    }
    return route.fulfill({status:value?200:404,json:value||{error:'Unknown route'}});
  });

  await page.goto('http://joppa.test/');

  // 1. The preset flag is gone: a schema with no built-in names still renders.
  await page.locator(`[data-object-address="${address(0)}"]`).waitFor();
  assert.equal(await page.locator('.tree-domain').count(),0,
    'no native renderer leaks into a configured schema');

  // 2. Six configured levels open one inside the other.
  for(let i=0;i<chain.length;i++){
    const card=page.locator(`[data-object-address="${address(i)}"]`);
    await card.waitFor();
    assert.match(await card.locator(':scope > summary').innerText(),new RegExp(chain[i][1],'i'),
      `level ${i+1} shows its schema title`);
    if(i<chain.length-1)await card.locator(':scope > summary').click();
  }

  // 3. A link crosses two roots and keeps its own title.
  await page.locator('#tab-intake').click();
  const intake=page.locator('[data-object-address="intake:in-1"]');
  await intake.waitFor();
  await intake.locator(':scope > summary').click();
  await intake.getByText('Action one',{exact:false}).waitFor();

  // 4. One copy action: same control on every card, success only after the
  //    clipboard resolves, and one toast that leaves on its own.
  await page.locator('#tab-portfolio').click();
  await page.locator(`[data-object-address="${address(5)}"]`).waitFor();
  // navigator.clipboard is undefined on plain http, so define it outright.
  await page.evaluate(()=>{Object.defineProperty(navigator,'clipboard',{configurable:true,
    value:{writeText:t=>{window.__copied=t;return Promise.resolve();}}});});
  const deep=page.locator(`[data-object-address="${address(5)}"] .copy`).first();
  const before=await deep.boundingBox();
  await deep.click();
  assert.equal(await deep.innerText(),'Copied','the label changes only after success');
  assert.equal(await page.evaluate(()=>window.__copied),'W-ACTION-1','the common item_id is copied');
  const after=await deep.boundingBox();
  assert.equal(Math.round(before.width),Math.round(after.width),'Copied must not resize the card');
  const toast=page.locator('#toast');
  assert.equal(await toast.getAttribute('role'),'status');
  assert.equal(await toast.getAttribute('aria-live'),'polite');
  await toast.waitFor({state:'visible'});

  // 5. A clipboard failure says so instead of claiming success.
  await page.evaluate(()=>{Object.defineProperty(navigator,'clipboard',{configurable:true,
    value:{writeText:()=>Promise.reject(Error('denied'))}});document.execCommand=()=>false;});
  const other=page.locator(`[data-object-address="${address(0)}"] .copy`).first();
  await other.click();
  assert.match(await toast.innerText(),/Could not copy/,'a failed copy is visible');
  assert.notEqual(await other.innerText(),'Copied','a failed copy never claims success');

  assert.deepEqual(errors,[],'no page errors');
  console.log('generic_depth_browser: ok');
}finally{await browser.close();}
})();
