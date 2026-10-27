const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {readFileSync}=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const root=path.join(__dirname,'..');
const html=readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',readFileSync(path.join(root,'src/board.css'),'utf8'))
  .replace('/* BRAND_STYLE */',readFileSync(path.join(root,'src/brand.css'),'utf8')).replace('/* BOARD_IMPORTS */',require('./board_script.cjs'));
const types={folder:{title:'Folder',role:'group', status:'children',root:true,parents:[],badge:'FOLD',title_size:21},
  incoming:{title:'Incoming information',role:'document', status:'none',root:true,parents:['*'],inbox:true},
  measure:{title:'Measure',role:'ac', status:'tests',root:false,parents:['folder'],badge:'TEST',title_preview_length:24}};
const records={
  'folder:alpha':{title:'Alpha',body:'Current source context',links:[{target:'incoming:beta',relation:'related'}]},
  'incoming:beta':{title:'Beta',body:'Current incoming information',links:[{target:'folder:alpha',relation:'related'},{target:'measure:r/1',revision:1,relation:'depends_on'}]},
  'measure:r/1':{title:'A complete acceptance condition with all important context preserved in the saved document',body:'Current criterion context',parent:'folder:alpha',links:[]},
  'incoming:archived':{title:'Archived source',body:'The original archived source remains readable',present:false,links:[]},
};
const row=(address)=>({address,kind:address.split(':')[0],id:address.split(':')[1],title:records[address].title,
  parent:records[address].parent||null,present:records[address].present!==false,revision:2,
  item_id:'W-O-'+address.replaceAll(':','-'),work_ready:true,signal:{tone:'grey',label:'Current document'}});
const document=(address,revision)=>{
  const current=row(address),r=Number(revision)||2,historical=r!==2;
  return {...current,current_revision:2,revision_count:2,historical,
    ...(!historical&&address==='folder:alpha'?{verification_summary:{total:1,passed:0}}:{}),
    document:{revision:r,parent:current.parent,present:current.present,
      data:{title:current.title,owner:'owner',body:historical?'Original criterion context':records[address].body},
      links:records[address].links,at:'2026-10-08T09:00:00Z'},
    links:records[address].links.map(link=>({link,title:records[link.target].title,current_revision:2,present:true})),
    incoming:[],annotations:historical?{}:{comments:{one:{actor:'owner',text:'Current discussion',at:'2026-10-08T09:00:00Z'}}},
    process:address==='measure:r/1'&&!historical?{req:'scope'}:null};
};
const url=(address,revision,workspace='w')=>{
  const target=new URL('http://joppa.test/');target.searchParams.set('workspace',workspace);
  if(address)target.searchParams.set('object',address);
  if(revision!==undefined)target.searchParams.set('revision',revision);
  return target.href;
};
(async()=>{
  const browser=await chromium.launch({headless:true});
  try{
    const context=await browser.newContext(),page=await context.newPage(),requests=[],errors=[];
    let authenticated=true;
    page.on('pageerror',e=>errors.push(e.message));
    await context.addInitScript(()=>sessionStorage.setItem('joppa-workspace','other'));
    await context.route('http://joppa.test/**',async route=>{
      const target=new URL(route.request().url());
      if(target.pathname==='/')return route.fulfill({contentType:'text/html',body:html});
      if(target.pathname==='/auth/mock')return route.fulfill({body:'Sign-in fixture'});
      if(target.pathname==='/favicon.svg')return route.fulfill({contentType:'image/svg+xml',body:'<svg/>'});
      requests.push(target.pathname+target.search);
      let value,status=200;
      const identity={actor:'owner',email:'owner@example.test'};
      if(target.pathname==='/api/me'){value=authenticated?identity:{error:'Sign in required'};status=authenticated?200:401;}
      else if(target.pathname==='/auth/login')value={verification_url:'http://joppa.test/auth/mock',expires_in:30};
      else if(target.pathname==='/auth/poll'){authenticated=true;value={...identity,status:'ready'};}
      else if(!authenticated){status=401;value={error:'Sign in required'};}
      else if(target.pathname==='/api/workspaces')value=[{id:'w',title:'Workspace one'},{id:'other',title:'Workspace two'}];
      else if(target.pathname.endsWith('/board'))value={workspace:target.pathname.split('/')[3],lazy:true,object_types:types,
        state:{title:'Workspace one',members:{owner:'human'},requirements:{}},launch:{configured:false},system_ids:{}};
      else if(target.pathname.endsWith('/tree'))value={schema:{revision:1,types}};
      // No criterion of this fixture lists tests.
      else if(target.pathname.endsWith('/tests'))value={items:[],tests:[]};
      else if(target.pathname.endsWith('/objects')){
        const address=target.searchParams.get('address'),workspace=target.pathname.split('/')[3];
        if(address){
          if(workspace!=='w'||!records[address]){status=workspace==='w'?409:404;value={error:'unknown object'};}
          else value=document(address,target.searchParams.get('revision'));
        }else{
          const parent=target.searchParams.get('parent'),kind=target.searchParams.get('kind');
          value={items:Object.keys(records).filter(a=>records[a].present!==false&&(!kind||a.startsWith(kind+':'))
            &&(parent==='*'||(records[a].parent||null)===(parent||null))).map(row),next_cursor:null};
        }
      }else{status=404;value={error:'Not found'};}
      return route.fulfill({status,json:value});
    });
    const selected=address=>page.locator('.object-page > .common-object[data-object-address="'+address+'"]');
    // The body is ready once its secondary part exists. The properties in it stay hidden until the row button opens them.
    const ready=async address=>{await selected(address).locator(':scope > .object-body > .object-secondary').waitFor({state:'attached'});};
    const follow=async address=>{
      const links=page.locator('.object-page > .common-object > .object-body > .object-secondary > .object-relations');
      await links.locator(':scope > summary').click();
      await links.locator('a[data-link-address="'+address+'"]').click();await ready(address);
    };
    await page.goto(url('folder:alpha'));await ready('folder:alpha');
    assert.equal(await page.getByLabel('Workspace',{exact:true}).inputValue(),'w','The URL takes priority over the saved workspace');
    await selected('folder:alpha').locator('[data-object-address="measure:r/1"]').waitFor();
    assert.equal(requests.filter(r=>r.includes('address=folder%3Aalpha')).length,1,'The selected document is read once');
    assert.equal(requests.some(r=>r.endsWith('/objects?limit=50')||r.includes('parent=*')||r.includes('parent=%2A')),false,'An item URL does not load root collections');
    assert.equal(requests.some(r=>r.includes('address=measure')),false,'Child bodies are not read before expansion');
    assert.equal(await page.getByRole('tab').count(),0,'The item page omits root navigation and filters');
    assert.equal(await selected('folder:alpha').locator('.type-badge').first().innerText(),'FOLD','The badge comes from the schema');
    assert.equal(await selected('folder:alpha').locator(':scope > summary .verification-chip').innerText(),'1 Measure','The direct page uses the current object count');

    await follow('incoming:beta');
    assert.equal(new URL(page.url()).searchParams.get('object'),'incoming:beta');
    assert.equal(await page.getByRole('dialog').count(),0);
    await follow('folder:alpha');
    assert.equal(await page.locator('.object-page > .common-object').count(),1,'A cycle replaces the page instead of nesting another object');
    assert.equal(await page.getByRole('dialog').count(),0);
    await page.goBack();await ready('incoming:beta');
    await page.goForward();await ready('folder:alpha');
    await page.reload();await ready('folder:alpha');
    assert.equal(await selected('folder:alpha').getByText('Current source context').isVisible(),true,'Reload preserves the selected item');

    await page.goto(url('incoming:beta'));await ready('incoming:beta');
    await follow('measure:r/1');
    assert.equal(new URL(page.url()).searchParams.get('revision'),'1','A dependency link keeps its pinned revision');
    assert.equal(await page.getByText('Revision 1 · Saved document',{exact:true}).isVisible(),true);
    assert.equal(await selected('measure:r/1').getByText('Original criterion context').isVisible(),true);
    assert.equal(await selected('measure:r/1').locator('.object-checks,.object-comments,.object-children > *').count(),0,'Historical URLs omit current context');
    assert.equal(await selected('measure:r/1').locator(':scope > summary .status-circle').getAttribute('data-tone'),'grey');
    await page.getByRole('link',{name:'Parent',exact:true}).click();await ready('folder:alpha');

    await page.goto(url('measure:r/1'));await ready('measure:r/1');
    const measure=selected('measure:r/1');
    const title=await measure.locator(':scope > summary .node-title').innerText();
    assert.equal(Array.from(title).length,24,'The title preview length comes from the schema');
    assert.ok(title.endsWith('…'));
    assert.equal(await measure.locator('.object-full-title').isVisible(),false,'The full title does not repeat in the main body');
    await measure.locator(':scope > summary .fields-button').click();
    await page.waitForFunction(()=>document.getAnimations().length===0);
    assert.equal(await measure.locator('.object-full-title').isVisible(),true);
    assert.equal(await measure.locator('.object-full-title p').innerText(),records['measure:r/1'].title,'Details preserves all title text');
    await page.setViewportSize({width:390,height:844});
    assert.equal(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),true);
    await page.screenshot({path:'/tmp/joppa-object-page-mobile.png',fullPage:true});
    await page.setViewportSize({width:1280,height:900});

    await page.goto(url('incoming:archived'));await ready('incoming:archived');
    assert.equal(await selected('incoming:archived').getByText('The original archived source remains readable').isVisible(),true);
    assert.equal(await selected('incoming:archived').getByText('Historical object. This object is absent from the current tree.').isVisible(),true);
    await page.goto(url('foreign:private'));await page.getByText('Object not found.',{exact:true}).waitFor();
    assert.equal(await page.locator('.object-page .common-object').count(),0);
    requests.length=0;
    await page.goto(url('folder:alpha',undefined,'private'));await page.getByText('This workspace is not available to your account.').waitFor();
    assert.equal(requests.some(r=>r.includes('/workspaces/private/')),false,'An unavailable workspace does not fall back to another product');
    await page.getByRole('link',{name:'Back to board',exact:true}).click();await page.getByRole('tab',{name:'Folder',exact:true}).waitFor();

    authenticated=false;
    await page.goto(url('measure:r/1',1));await page.getByRole('button',{name:'Continue with Google'}).waitFor();
    await page.getByRole('button',{name:'Continue with Google'}).click();await ready('measure:r/1');
    assert.equal(new URL(page.url()).searchParams.get('object'),'measure:r/1','Sign-in preserves the object URL');
    assert.equal(new URL(page.url()).searchParams.get('revision'),'1','Sign-in preserves the saved revision');
    await page.getByLabel('Workspace',{exact:true}).selectOption('other');
    await page.getByRole('tab',{name:'Folder',exact:true}).waitFor();
    assert.equal(new URL(page.url()).searchParams.get('workspace'),'other');
    assert.equal(new URL(page.url()).searchParams.has('object'),false,'A workspace change drops the previous item');
    assert.equal(new URL(page.url()).searchParams.has('revision'),false);
    assert.deepEqual(errors,[]);
    console.log('Object URL browser checks passed');
  }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exitCode=1;});
