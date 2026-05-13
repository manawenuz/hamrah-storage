const { chromium } = require('playwright');
const fs = require('fs');

(async () => {
  fs.writeFileSync('hello.txt', 'Hello World from automation!');

  const browser = await chromium.launch({
    headless: true,
    proxy: { server: 'http://127.0.0.1:8888' }
  });

  const context = await browser.newContext({ storageState: 'auth.json' });
  const page = await context.newPage();

  console.log('Navigating to My Files ...');
  await page.goto('https://abrehamrahi.ir/drive', { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(3000);
  
  await page.click('text=فایل‌های من');
  await page.waitForTimeout(3000);

  console.log('Uploading file...');
  // setInputFiles uploads to the first input[type="file"]
  await page.setInputFiles('input[type="file"]', 'hello.txt');
  
  console.log('Waiting for upload to finish (10s)...');
  await page.waitForTimeout(10000); // Wait for upload progress

  console.log('Finding file in UI...');
  const fileLocator = page.locator('text=hello.txt').first();
  await fileLocator.waitFor({ state: 'visible', timeout: 10000 });
  
  console.log('Right clicking file...');
  await fileLocator.click({ button: 'right' });
  await page.waitForTimeout(2000);

  // Dump context menu
  const menuText = await page.evaluate(() => {
    const root = document.querySelector('#context-menu-root');
    return root ? root.innerText : 'No context menu root found';
  });
  console.log('Context menu options:\\n', menuText);

  // Click delete
  console.log('Clicking delete (حذف)...');
  await page.click('text=حذف');
  await page.waitForTimeout(2000);

  // Confirm delete (if there is a confirm dialog, might be text=حذف or تایید)
  console.log('Checking for confirm dialog...');
  const confirmBtn = page.locator('button', { hasText: 'حذف' }).last();
  if (await confirmBtn.isVisible()) {
    await confirmBtn.click();
    console.log('Confirmed delete.');
  } else {
    // try text=بله (Yes) or text=تایید (Confirm)
    const btnYes = page.locator('button', { hasText: 'بله' });
    if (await btnYes.isVisible()) {
      await btnYes.click();
      console.log('Confirmed with بله');
    }
  }

  await page.waitForTimeout(3000);
  console.log('Done!');
  await browser.close();
})();
