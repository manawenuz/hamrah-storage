const { chromium } = require('playwright');
const fs = require('fs');
const readline = require('readline');

(async () => {
  const browser = await chromium.launch({
    headless: false,
    proxy: { server: 'http://127.0.0.1:8888' }
  });

  const context = await browser.newContext();
  const page = await context.newPage();

  // We will intercept and save all the exact network APIs to replicate them
  const apiCalls = [];
  page.on('request', async (req) => {
    if (['fetch', 'xhr'].includes(req.resourceType())) {
      apiCalls.push({
        method: req.method(),
        url: req.url(),
        postData: req.postData() ? req.postData().substring(0, 500) : null // Truncate huge payloads
      });
    }
  });

  await page.goto('https://abrehamrahi.ir/');

  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });

  console.log('\n--- 🔴 RECORDING SESSION ---');
  console.log('Please perform the following steps in the opened browser:');
  console.log('1. Login with your username and password.');
  console.log('2. Go to My Files and upload a dummy file.');
  console.log('3. Right-click the file and create a link (set expiry & download count limit).');
  console.log('4. Delete the file.');
  
  rl.question('\nPress ENTER here when you have finished ALL steps...\n', async () => {
    // Save the API network log so I can build you the exact flow!
    fs.writeFileSync('api_flow.json', JSON.stringify(apiCalls, null, 2));
    
    // Dump the current DOM so I can see the login input selectors just in case
    const html = await page.content();
    fs.writeFileSync('last_page.html', html);
    
    console.log('Successfully recorded your flow to api_flow.json!');
    
    await browser.close();
    rl.close();
  });
})();
