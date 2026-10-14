// Run with node tests/oauth_browser.cjs; uses the same Playwright env as browser.cjs.
const {chromium}=require(process.env.PLAYWRIGHT_MODULE||'playwright');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const path=require('node:path');
(async()=>{
 const browser=await chromium.launch({headless:true,...(process.env.PLAYWRIGHT_CHROMIUM?{executablePath:process.env.PLAYWRIGHT_CHROMIUM}:{})});
 try{
  for(const mode of ['link','code','redirect']){
   const page=await browser.newPage({viewport:{width:375,height:812}});
   const errors=[];page.on('pageerror',e=>errors.push(e.message));
   const callback='http://localhost:45678/callback?code=test-only&state=a%2Bb%26c';
   await page.clock.install();
   await page.route('https://joppa.example/**',route=>{
    const p=new URL(route.request().url()).pathname;
    if(p==='/oauth/authorize')return route.fulfill({contentType:'text/html',body:fs.readFileSync(path.join(__dirname,'../src/oauth.html'),'utf8').replace('/* BOARD_STYLE */',fs.readFileSync(path.join(__dirname,'../src/board.css'),'utf8')).replace('/* BRAND_STYLE */',fs.readFileSync(path.join(__dirname,'../src/brand.css'),'utf8'))});
    if(p==='/oauth/details')return route.fulfill({json:{client_name:'Test client',redirect_uri:callback,manual:mode==='code',copy_link:mode==='link'}});
    if(p==='/api/me')return route.fulfill({json:{actor:'test-owner'}});
    if(p==='/oauth/approve')return route.fulfill({json:mode==='code'?{code:'test-only-code',expires_in:300}:{redirect:mode==='link'?callback:'https://client.example/callback',copy_link:mode==='link',expires_in:300}});
    return route.fulfill({status:404,body:''});
   });
   await page.route('https://client.example/**',route=>route.fulfill({body:'Connected'}));
   await page.route('http://localhost:45678/**',route=>route.fulfill({body:'Authentication complete'}));
   await page.goto('https://joppa.example/oauth/authorize');
   await page.getByRole('button',{name:'Allow access',exact:true}).click();
   if(mode!=='code'){
    // A loopback callback, as Codex uses, gets the code at once, like a hosted one.
    await page.waitForURL(mode==='link'?callback:'https://client.example/callback');
   }else{
    const expected='test-only-code';
    const button=page.getByRole('button',{name:'Copy code',exact:true});
    await button.waitFor();
    assert.equal(await page.locator('textarea').inputValue(),expected);
    assert.equal(page.url(),'https://joppa.example/oauth/authorize','local callback must not auto-navigate');
    await page.evaluate(()=>Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async text=>{window.copied=text;}}}));
    await button.click();
    assert.equal(await page.evaluate(()=>window.copied),expected);
    await page.evaluate(()=>Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async()=>{throw Error('Clipboard unavailable');}}}));
    await page.getByRole('button',{name:'Copied',exact:true}).click();
    assert.equal(await page.locator('textarea').evaluate(e=>e.selectionEnd-e.selectionStart),expected.length);
    assert(await page.getByRole('status').textContent().then(t=>t.startsWith('Copy the selected')));
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth),'no mobile overflow');
    await page.clock.fastForward(300001);
    assert.equal(await page.locator('textarea').inputValue(),'');
    assert(await page.getByRole('button',{name:'Code expired'}).isDisabled());
    assert.equal(await page.locator('a[href^="http://localhost"]').count(),0);
   }
   assert.deepEqual(errors,[]);
   await page.close();
  }
  console.log('OAuth browser: loopback and hosted redirect, code copy, clipboard fallback, expiry and mobile layout passed');
 }finally{await browser.close();}
})().catch(e=>{console.error(e);process.exit(1);});
