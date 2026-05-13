const { chromium } = require('playwright');
const fs = require('fs');
const readline = require('readline');

(async () => {
  const browser = await chromium.launch({
    headless: false,
    proxy: { server: 'http://127.0.0.1:8888' }
  });

  const context = await browser.newContext({ storageState: 'auth.json' });
  const page = await context.newPage();

  const apiCalls = [];
  page.on('request', async (req) => {
    if (['fetch', 'xhr'].includes(req.resourceType())) {
      apiCalls.push({
        method: req.method(),
        url: req.url(),
        postData: req.postData()
      });
    }
  });

  await page.goto('https://abrehamrahi.ir/drive');

  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });

  console.log('\n--- 🔴 RECORDING DELETION ---');
  console.log('1. Find a file to delete.');
  console.log('2. Delete it.');
  
  rl.question('\nPress ENTER when you have finished deleting the file...\n', async () => {
    fs.writeFileSync('api_flow_deletion.json', JSON.stringify(apiCalls, null, 2));
    console.log('Successfully recorded to api_flow_deletion.json!');
    await browser.close();
    rl.close();
  });
})();
