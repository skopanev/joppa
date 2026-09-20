const {
  chromium
} = require(process.env.PLAYWRIGHT_MODULE || 'playwright');
const assert = require('node:assert/strict');
(async () => {
  const browser = await chromium.launch({
    headless: true,
    ...(process.env.PLAYWRIGHT_CHROMIUM ? {
      executablePath: process.env.PLAYWRIGHT_CHROMIUM
    } : {})
  });
  try {
    const page = await browser.newPage({
      viewport: {
        width: 1280,
        height: 900
      }
    });
    // UI fixture only: Google protocol and real server sessions are tested in Rust.
    // The fixture supplies the existing test API identity after the mocked popup.
    let signedIn=false;
    await page.route('**/api/**',async route=>{
      const request=route.request();
      assert.equal(request.method(),'GET','portal API calls must be read-only');
      if(!signedIn)return route.fulfill({status:401,json:{error:'Please sign in'}});
      await route.continue({headers:{...request.headers(),authorization:'Bearer '+process.env.JOPPA_TEST_TOKEN}});
    });
    await page.route('**/auth/login',route=>route.fulfill({json:{verification_url:process.env.JOPPA_TEST_URL+'/guide',expires_in:300}}));
    await page.route('**/auth/poll',route=>{signedIn=true;return route.fulfill({json:{status:'ready',actor:'owner'}});});
    await page.route('**/auth/logout',route=>{signedIn=false;return route.fulfill({json:{ok:true}});});
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.goto(process.env.JOPPA_TEST_URL);
    await page.getByRole('heading',{name:'Authorization.'}).waitFor();
    assert.equal(await page.locator('.app-header').isVisible(),false);
    assert.equal(await page.locator('.layout').isVisible(),false);
    assert.equal(await page.locator('input[type=password]').count(),0);
    await page.getByRole('button', {
      name: 'Continue with Google',
      exact: true
    }).click();
    await page.locator('#workspaces option').first().waitFor({
      state: 'attached'
    });
    await page.locator('#workspaces').selectOption(process.env.JOPPA_TEST_WORKSPACE);
    await page.locator('#tree .tree-domain').first().waitFor();
    assert.equal(await page.locator('#card').isVisible(),false);
    assert((await page.locator('#sidebar').boundingBox()).width>900);
    assert.equal(await page.getByRole('button',{name:/New requirement|Add requirement/}).count(),0);
    await page.screenshot({path:'/tmp/joppa-tree-desktop.png',fullPage:true});
    await page.setViewportSize({width:390,height:844});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.screenshot({path:'/tmp/joppa-tree-mobile.png',fullPage:true});
    await page.setViewportSize({width:1280,height:900});
    await page.context().grantPermissions(['clipboard-read','clipboard-write']);
    async function copied(){
      await page.getByRole('button',{name:'Copy for agent',exact:true}).click();
      await page.getByText('Context copied. Paste it into your agent chat.',{exact:true}).waitFor();
      const text=await page.evaluate(()=>navigator.clipboard.readText());
      return JSON.parse(text.slice(text.indexOf('{')));
    }
    function noForms(){return page.locator('form').count().then(n=>assert.equal(n,0));}
    await noForms();
    assert(await page.locator('.brand-mark').isVisible());
    assert.equal(await page.locator('link[rel=icon]').count(),1);
    assert.equal(await page.locator('#workspaces').evaluate(e=>getComputedStyle(e).fontSize),'16px');
    await page.locator('#tree').getByRole('button',{name:'Доступ к данным',exact:true}).click();
    assert.equal(await page.getByText('What is a domain?',{exact:true}).count(),0);
    const domain=await copied();
    assert.deepEqual(domain.selection,{kind:'domain',id:'security'});
    assert.equal(domain.workspace.id,process.env.JOPPA_TEST_WORKSPACE);
    assert.deepEqual(Object.keys(domain.capabilities),['restrict']);
    assert.deepEqual(Object.keys(domain.requirements).sort(),['auto','key']);
    await noForms();
    await page.getByRole('button',{name:'← Product tree',exact:true}).click();
    await page.locator('#tree').getByRole('button',{name:'Ограничивать доступ',exact:true}).click();
    assert.equal(await page.getByText('What is a capability?',{exact:true}).count(),0);
    assert.deepEqual((await copied()).selection,{kind:'capability',id:'restrict'});
    await noForms();
    await page.getByRole('button',{name:'← Product tree',exact:true}).click();
    await page.locator('#tree').getByRole('button',{name:/Ключ до запуска/}).click();
    await page.getByRole('heading',{name:'Ключ до запуска',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.attacked),undefined);
    const req=await copied();
    assert.deepEqual(req.selection,{kind:'requirement',id:'key',revision:2});
    assert.deepEqual(Object.keys(req.requirements),['key']);
    assert.equal(req.requirements.key.revisions.at(-1).acs[0].id,'key-rights');
    assert(req.history.some(e=>e.command.op==='accept'));
    assert(req.history.every(e=>!e.command.req || e.command.req==='key'));
    assert.equal(req.latest_revision_summary.key.accepted,false);
    await page.locator('#card > select').selectOption('1');
    assert.deepEqual((await copied()).selection,{kind:'requirement',id:'key',revision:1});
    await page.getByText('Checks · 1',{exact:true}).click();
    await page.getByText('manual1 · Finished',{exact:true}).click();
    await page.getByText('Expected: read, verify',{exact:true}).waitFor();
    await page.getByText(/Observed: read, verify/).waitFor();
    await noForms();
    assert.equal(await page.getByRole('button',{name:/Edit |Confirm |Accept |Save |Create |Post reply|Finish run|Record check/}).count(),0);
    await page.screenshot({path:'/tmp/joppa-browser-check.png',fullPage:true});
    // Denied clipboard access exposes a selectable read-only fallback, not a false success.
    await page.evaluate(()=>Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:()=>Promise.reject(Error('denied'))}}));
    await page.getByRole('button',{name:'Copy for agent',exact:true}).click();
    const fallback=page.getByRole('dialog',{name:'Copy context for your agent'});
    await fallback.waitFor();
    assert(await fallback.getByRole('textbox').evaluate(e=>e.readOnly));
    assert((await fallback.getByRole('textbox').inputValue()).includes('key-rights'));
    await fallback.getByRole('button',{name:'Close',exact:true}).click();
    await page.goto('file://' + process.env.JOPPA_TEST_SNAPSHOT);
    await page.locator('#tree').getByRole('button',{name:/Ключ до запуска/}).click();
    await page.getByRole('heading', {
      name: 'Ключ до запуска',
      exact: true
    }).waitFor();
    assert.equal(await page.evaluate(() => window.attacked), undefined);
    await page.goto(process.env.JOPPA_TEST_URL+'/guide');
    await page.getByRole('heading',{name:'Four levels of the tree'}).waitFor();
    // Cookie login survives refresh; obsolete token fragments are erased.
    await page.goto(process.env.JOPPA_TEST_URL+'/#token='+encodeURIComponent(process.env.JOPPA_TEST_TOKEN));
    await page.locator('#identity').filter({hasText:'Account'}).waitFor();
    assert.equal(await page.evaluate(()=>location.hash),'');
    await page.locator('#workspaces').selectOption(process.env.JOPPA_TEST_WORKSPACE);
    await page.locator('#tree').getByRole('button',{name:'Доступ к данным',exact:true}).waitFor();
    await page.reload();
    await page.locator('#identity').filter({hasText:'Account'}).waitFor();
    await page.locator('#tree .tree-domain').first().waitFor();
    await page.setViewportSize({width:390,height:844});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    assert.equal(await page.locator('#sidebar').isVisible(),true);
    assert.equal(await page.getByRole('button',{name:/New requirement|Add requirement/}).count(),0);
    await page.locator('#tree').getByRole('button',{name:'Доступ к данным',exact:true}).click();
    assert.equal(await page.locator('#sidebar').isVisible(),false);
    await page.getByRole('button',{name:'← Product tree',exact:true}).click();
    assert.equal(await page.locator('#sidebar').isVisible(),true);
    await page.locator('#logout').click();
    await page.getByRole('heading',{name:'Authorization.'}).waitFor();
    assert.equal(await page.locator('.layout').isVisible(),false);
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.screenshot({path:'/tmp/joppa-login-mobile.png'});
    // A closed Google window leaves a clear retry action, without exposing the app.
    const popupPromise=page.waitForEvent('popup');
    await page.locator('#login-button').click();
    const popup=await popupPromise;
    await popup.close();
    await page.locator('#login-cancel').click();
    assert.equal(await page.locator('#login-button').isEnabled(),true);
    assert.deepEqual(errors, []);
    console.log('Browser: read-only tree, scoped clipboard, historical revision, fallback, mobile and XSS-safe snapshot passed');
  } finally {
    await browser.close();
  }
})().catch(e => {
  console.error(e);
  process.exitCode = 1;
});
