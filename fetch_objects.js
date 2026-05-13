const { chromium } = require('playwright');

(async () => {
  const browser = await chromium.launch({ headless: true, proxy: { server: 'http://127.0.0.1:8888' }});
  const context = await browser.newContext({ storageState: 'auth.json' });
  const page = await context.newPage();
  
  const response = await page.request.get('https://abrehamrahi.ir/api/v2/flat/list-objects/?is_trash=false&limit=1000');
  console.log(await response.json());
  
  await browser.close();
})();
