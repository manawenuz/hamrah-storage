const { chromium } = require('playwright');
const fs = require('fs');

(async () => {
  const browser = await chromium.launch({
    headless: true,
    proxy: { server: 'http://127.0.0.1:8888' }
  });

  const context = await browser.newContext({ storageState: 'auth.json' });
  const page = await context.newPage();

  console.log('Navigating to drive...');
  await page.goto('https://abrehamrahi.ir/drive', { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(5000);
  
  await page.click('text=فایل‌های من');
  await page.waitForTimeout(5000);

  const fileLocator = page.locator('text=hello.txt').first();
  await fileLocator.waitFor({ state: 'visible' });

  console.log('Right clicking hello.txt...');
  await fileLocator.click({ button: 'right' });
  await page.waitForTimeout(2000);

  // Take a screenshot of the open context menu
  await page.screenshot({ path: 'context_menu.png' });
  
  // Dump all text on the screen to see what options appeared
  const bodyText = await page.evaluate(() => document.body.innerText);
  fs.writeFileSync('menu_text.txt', bodyText);
  console.log('Saved menu text to menu_text.txt');

  await browser.close();
})();
