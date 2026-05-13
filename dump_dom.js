const { chromium } = require('playwright');
const fs = require('fs');

(async () => {
  const browser = await chromium.launch({
    headless: true,
    proxy: { server: 'http://127.0.0.1:8888' }
  });

  const context = await browser.newContext({
    storageState: 'auth.json'
  });

  const page = await context.newPage();

  console.log('Navigating to My Files ...');
  await page.goto('https://abrehamrahi.ir/drive', { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(3000);
  
  // Click on "فایل‌های من"
  await page.click('text=فایل‌های من');
  await page.waitForTimeout(3000);

  // Get all buttons and their texts
  const elements = await page.evaluate(() => {
    const getVisibleText = (el) => el.innerText ? el.innerText.trim().replace(/\n/g, ' ') : '';
    
    const btns = Array.from(document.querySelectorAll('button, a, [role="button"]'))
      .map(b => ({ tag: b.tagName, text: getVisibleText(b), class: b.className }));
      
    const inputs = Array.from(document.querySelectorAll('input'))
      .map(i => ({ type: i.type, name: i.name, id: i.id, placeholder: i.placeholder, class: i.className }));
      
    return { buttons: btns, inputs: inputs };
  });

  fs.writeFileSync('dom_elements.json', JSON.stringify(elements, null, 2));
  console.log('Saved interactive elements to dom_elements.json');

  await page.screenshot({ path: 'dashboard.png', fullPage: true });
  console.log('Saved dashboard.png');

  await browser.close();
})();
