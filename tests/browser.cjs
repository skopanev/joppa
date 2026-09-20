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
    async function save(locator) {
      await Promise.all([page.waitForResponse(r => r.request().method() === 'POST'), locator.click()]);
      await page.getByText('Saved to history.', {
        exact: true
      }).waitFor();
    }
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
    // Domain and capability navigation explain the model in the actual UI.
    await page.locator('#tree').getByRole('button',{name:'Доступ к данным',exact:true}).click();
    await page.getByText('What is a domain?',{exact:true}).click();
    await page.getByText('A domain is a business area with a clear decision owner.',{exact:true}).waitFor();
    await page.locator('#tree').getByRole('button',{name:'Ограничивать доступ',exact:true}).click();
    await page.getByText('What is a capability?',{exact:true}).click();
    await page.getByText('A capability describes what the product can do. Start with “We can…”',{exact:true}).waitFor();
    await page.locator('#tree').getByRole('button', {name: /Ключ до запуска/}).click();
    await page.getByRole('heading', {
      name: 'Ключ до запуска',
      exact: true
    }).waitFor();
    assert.equal(await page.evaluate(() => window.attacked), undefined);
    await page.getByRole('button', {
      name: 'Confirm wording',
      exact: true
    }).click();
    await page.getByText('In progress', {
      exact: true
    }).waitFor();
    await page.getByText('Assess readiness', {
      exact: true
    }).click();
    const assess = page.locator('form').filter({
      has: page.getByRole('button', {
        name: 'Save assessment',
        exact: true
      })
    });
    await assess.getByLabel('Readiness', {
      exact: true
    }).selectOption('yes');
    await assess.getByLabel('Reason or specific question').fill('Список подтверждён владельцем');
    await assess.getByRole('button', {
      name: 'Save assessment'
    }).click();
    await page.locator('p').filter({
      hasText: /^Ready to work$/
    }).waitFor();
    await page.getByText('Checks · 0', {
      exact: true
    }).click();
    const plan = page.locator('form').filter({
      has: page.getByRole('button', {
        name: 'Seal manual check plan',
        exact: true
      })
    });
    await plan.getByLabel('Run ID').fill('manual1');
    await plan.getByLabel('Environment and configuration version').fill('config-v1');
    await plan.getByLabel('Check scope').fill('Права ключа перед запуском');
    await plan.getByLabel('All checks in this scope are included').check();
    await plan.getByLabel('key-rights · expected list').fill('read\nverify');
    await plan.getByRole('button', {
      name: 'Seal manual check plan'
    }).click();
    await page.getByText('Checks · 1', {
      exact: true
    }).waitFor();
    await page.getByText('Checks · 1', {
      exact: true
    }).click();
    await page.getByText('manual1 · Running', {
      exact: true
    }).click();
    const report = page.locator('form').filter({
      has: page.getByRole('button', {
        name: 'Record check',
        exact: true
      })
    });
    await report.getByLabel('Observed list — one item per line').fill('read\nverify');
    await report.getByLabel('What was checked and how').fill('Открыл права ключа в панели провайдера');
    await save(report.getByRole('button', {
      name: 'Record check',
      exact: true
    }));
    await page.getByText('Checks · 1', {
      exact: true
    }).click();
    await page.getByText('manual1 · Running', {
      exact: true
    }).click();
    await save(page.getByRole('button', {
      name: 'Finish run',
      exact: true
    }));
    await page.getByRole('button', {
      name: 'Accept result',
      exact: true
    }).click();
    await page.getByText('Accepted', {
      exact: true
    }).waitFor();
    await page.screenshot({
      path: '/tmp/joppa-browser-check.png',
      fullPage: true
    });
    // Editing a typo creates a new revision with no inherited green. Leave it unsaved here.
    await page.getByRole('button', {
      name: 'Edit requirement',
      exact: true
    }).click();
    await page.getByRole('heading', {
      name: 'New revision · all checks reset to n/a'
    }).waitFor();
    assert.equal(await page.getByLabel('Stable AC ID').inputValue(), 'key-rights');
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
    await page.locator('#identity').filter({hasText:'owner'}).waitFor();
    assert.equal(await page.evaluate(()=>location.hash),'');
    await page.locator('#workspaces').selectOption(process.env.JOPPA_TEST_WORKSPACE);
    await page.locator('#tree').getByRole('button',{name:'Доступ к данным',exact:true}).waitFor();
    await page.reload();
    await page.locator('#identity').filter({hasText:'owner'}).waitFor();
    await page.setViewportSize({width:390,height:844});
    assert(await page.evaluate(()=>document.documentElement.scrollWidth<=innerWidth));
    await page.getByRole('button',{name:'☰ Product tree'}).click();
    await page.locator('#tree').getByRole('button',{name:'Доступ к данным',exact:true}).click();
    assert.equal(await page.locator('#sidebar').isVisible(),false);
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
    console.log('Browser: wording → readiness → exact manual evidence → acceptance → launch gate; XSS-safe snapshot passed');
  } finally {
    await browser.close();
  }
})().catch(e => {
  console.error(e);
  process.exitCode = 1;
});
