const { chromium } = require('playwright');
const fs = require('fs');

(async () => {
  console.log('Starting exploration script...');
  const browser = await chromium.launch({
    headless: true,
    proxy: { server: 'http://127.0.0.1:8888' }
  });

  const context = await browser.newContext({
    storageState: 'auth.json'
  });

  const page = await context.newPage();

  // Log XHR/Fetch requests to understand the API
  const requests = [];
  page.on('request', request => {
    if (['fetch', 'xhr'].includes(request.resourceType())) {
      requests.push({ url: request.url(), method: request.method() });
    }
  });

  console.log('Navigating to https://abrehamrahi.ir/ ...');
  await page.goto('https://abrehamrahi.ir/', { waitUntil: 'domcontentloaded' });
  await page.waitForTimeout(5000); // Wait 5 seconds for any subsequent API calls to fire

  console.log('Current URL after load:', page.url());

  const html = await page.content();
  fs.writeFileSync('dashboard.html', html);
  
  await page.screenshot({ path: 'dashboard.png', fullPage: true });
  fs.writeFileSync('requests.json', JSON.stringify(requests, null, 2));

  console.log('Saved dashboard.html, dashboard.png, and requests.json for analysis.');

  await browser.close();
})();
