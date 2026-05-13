const HamrahClient = require('./HamrahClient');
const { chromium } = require('playwright');

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
  
  console.log('Clicking فایل‌های من...');
  await page.click('text=فایل‌های من');
  await page.waitForTimeout(5000);

  // Dump the text of all files in the list
  const textContent = await page.evaluate(() => document.body.innerText);
  console.log('--- PAGE TEXT ---');
  console.log(textContent);
  console.log('-----------------');

  await page.screenshot({ path: 'my_files_screenshot.png', fullPage: true });
  console.log('Screenshot saved to my_files_screenshot.png');
  
  await browser.close();
})();
