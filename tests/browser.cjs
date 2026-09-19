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
      await page.getByText('Сохранено. История дополнена.', {
        exact: true
      }).waitFor();
    }
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.goto(process.env.JOPPA_TEST_URL);
    await page.getByLabel('Токен доступа').fill(process.env.JOPPA_TEST_TOKEN);
    await page.getByRole('button', {
      name: 'Войти',
      exact: true
    }).click();
    await page.locator('#workspaces option').first().waitFor({
      state: 'attached'
    });
    await page.locator('#workspaces').selectOption(process.env.JOPPA_TEST_WORKSPACE);
    await page.getByRole('button', {name: /Ключ до запуска/}).click();
    await page.getByRole('heading', {
      name: 'Ключ до запуска',
      exact: true
    }).waitFor();
    assert.equal(await page.evaluate(() => window.attacked), undefined);
    await page.getByRole('button', {
      name: 'Подтвердить формулировку',
      exact: true
    }).click();
    await page.getByText('В работе', {
      exact: true
    }).waitFor();
    await page.getByText('Оценить готовность', {
      exact: true
    }).click();
    const assess = page.locator('form').filter({
      has: page.getByRole('button', {
        name: 'Записать оценку',
        exact: true
      })
    });
    await assess.getByLabel('Готовность', {
      exact: true
    }).selectOption('yes');
    await assess.getByLabel('Обоснование или конкретный вопрос').fill('Список подтверждён владельцем');
    await assess.getByRole('button', {
      name: 'Записать оценку'
    }).click();
    await page.locator('p').filter({
      hasText: /^Готов к работе$/
    }).waitFor();
    await page.getByText('Проверки · 0', {
      exact: true
    }).click();
    const plan = page.locator('form').filter({
      has: page.getByRole('button', {
        name: 'Запечатать план ручной проверки',
        exact: true
      })
    });
    await plan.getByLabel('ID прогона').fill('manual1');
    await plan.getByLabel('Что проверяем: среда и версия настроек').fill('config-v1');
    await plan.getByLabel('Область проверки').fill('Права ключа перед запуском');
    await plan.getByLabel('Все проверки в этой области собраны').check();
    await plan.getByLabel('key-rights · ожидаемый список').fill('read\nverify');
    await plan.getByRole('button', {
      name: 'Запечатать план ручной проверки'
    }).click();
    await page.getByText('Проверки · 1', {
      exact: true
    }).waitFor();
    await page.getByText('Проверки · 1', {
      exact: true
    }).click();
    await page.getByText('manual1 · Идёт', {
      exact: true
    }).click();
    const report = page.locator('form').filter({
      has: page.getByRole('button', {
        name: 'Записать проверку',
        exact: true
      })
    });
    await report.getByLabel('Фактический список — одна позиция на строку').fill('read\nverify');
    await report.getByLabel('Что и как проверено').fill('Открыл права ключа в панели провайдера');
    await save(report.getByRole('button', {
      name: 'Записать проверку',
      exact: true
    }));
    await page.getByText('Проверки · 1', {
      exact: true
    }).click();
    await page.getByText('manual1 · Идёт', {
      exact: true
    }).click();
    await save(page.getByRole('button', {
      name: 'Завершить прогон',
      exact: true
    }));
    await page.getByRole('button', {
      name: 'Принять результат',
      exact: true
    }).click();
    await page.getByText('Принято', {
      exact: true
    }).waitFor();
    await page.getByText('Обязательные условия запуска приняты', {
      exact: true
    }).waitFor();
    await page.screenshot({
      path: '/tmp/joppa-browser-check.png',
      fullPage: true
    });
    // Editing a typo creates a new revision with no inherited green. Leave it unsaved here.
    await page.getByRole('button', {
      name: 'Редактировать REQ + AC',
      exact: true
    }).click();
    await page.getByRole('heading', {
      name: 'Новая редакция — все проверки начнутся с n/a'
    }).waitFor();
    assert.equal(await page.getByLabel('Постоянный ID условия').inputValue(), 'key-rights');
    await page.goto('file://' + process.env.JOPPA_TEST_SNAPSHOT);
    await page.getByRole('heading', {
      name: 'Ключ до запуска',
      exact: true
    }).waitFor();
    assert.equal(await page.evaluate(() => window.attacked), undefined);
    assert.deepEqual(errors, []);
    console.log('Browser: wording → readiness → exact manual evidence → acceptance → launch gate; XSS-safe snapshot passed');
  } finally {
    await browser.close();
  }
})().catch(e => {
  console.error(e);
  process.exitCode = 1;
});
