// Board behaviour that is not about the tree shape: read-only access, sign in,
// escaping, attachments, saved revisions and mobile. Each check names the
// criterion it proves where one exists.
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const {readFileSync}=require('node:fs');
const path=require('node:path');
const assert=require('node:assert/strict');
const root=path.join(__dirname,'..');
const source=require('./board_script.cjs');
const style=readFileSync(path.join(root,'src/board.css'),'utf8');
const page_html=()=>readFileSync(path.join(root,'src/board.html'),'utf8')
  .replace('/* BOARD_STYLE */',style)
  .replace('/* BRAND_STYLE */',readFileSync(path.join(root,'src/brand.css'),'utf8'))
  .replace('/* BOARD_IMPORTS */',source);

const {ATTACK,types,schema,rows,detail}=require('./browser_fixture.cjs');

(async()=>{
const browser=await chromium.launch({headless:true});
try{
  const page=await browser.newPage();
  const errors=[];page.on('pageerror',e=>errors.push(e.message));
  let signedIn=false;const methods=new Set();
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
        value.process={req:'access',revision:3,verification:'pass'};
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

  // Attachments recorded on the object are listed in its Files section.
  await spec.locator('.object-files > summary').click();
  await spec.getByText('evidence.png',{exact:false}).waitFor();

  // Mobile: the same card fits a narrow screen without a horizontal page scrollbar.
  await page.setViewportSize({width:390,height:780});
  await page.waitForFunction(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1);

  // Saved revisions do not borrow the current verdict.
  await spec.locator(':scope > summary .history-button').click();
  const history=page.getByRole('dialog');
  await history.getByLabel('Object revision').selectOption('1');
  await history.getByText('Saved document revision.',{exact:false}).waitFor();
  assert.equal(await history.locator('[data-tone="green"]').count(),0,
    'a saved revision never borrows the current green status');
  assert.equal(await history.getByText('Verification: Passed',{exact:true}).count(),0);
  await history.getByRole('button',{name:'Close',exact:true}).click();

  assert.deepEqual(errors,[],'no page errors');
  if(process.env.JOPPA_TEST_URL)await verifyRealServer(browser);
  console.log('Browser: sign in, read-only access, escaping, attachments, saved revisions and mobile');
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
  // Comments start collapsed. The section opens from its summary.
  await req.locator(':scope > .object-body > .object-secondary > .object-comments > summary').click();
  await req.getByText('Agent note',{exact:true}).waitFor();
  assert(!requests.some(url=>url.searchParams.has('revision')),'expansion does not read history');
  await req.locator(':scope > summary .history-button').click();
  const history=page.getByRole('dialog',{name:'Ключ до запуска',exact:true});
  await history.getByRole('combobox',{name:'Object revision'}).selectOption('1');
  // The real portal renders the body as Markdown and the sanitizer drops the script element.
  await history.locator('.markdown.object-content').waitFor();
  assert.equal(await history.locator('script').count(),0,'a script in a saved body never reaches the page');
  assert.equal(await page.evaluate(()=>window.attacked),undefined);
  await history.getByRole('button',{name:'Close',exact:true}).click();
  const before=requests.filter(url=>url.searchParams.get('address')==='domain:security').length;
  await domain.locator(':scope > summary').click();
  await domain.locator(':scope > summary').click();
  assert.equal(requests.filter(url=>url.searchParams.get('address')==='domain:security').length,before,'reopening uses the loaded card');
  await page.setViewportSize({width:390,height:844});
  assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth+1),'the real cards fit the mobile screen');
  assert.deepEqual(errors,[]);
  await page.close();
}
