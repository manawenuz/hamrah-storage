const { chromium } = require('playwright');
const readline = require('readline');

(async () => {
  console.log('Launching browser with proxy 127.0.0.1:8888...');
  
  const browser = await chromium.launch({
    headless: false, // We keep this false so you can see the page and login
    proxy: {
      server: 'http://127.0.0.1:8888'
    }
  });

  const context = await browser.newContext();
  const page = await context.newPage();

  await page.goto('https://abrehamrahi.ir/');

  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout
  });

  rl.question('\nPlease login manually in the opened browser window. \nOnce you have fully logged in and the dashboard has loaded, press ENTER here in the terminal to save the session...\n', async () => {
    // Save authentication state (cookies and localStorage)
    await context.storageState({ path: 'auth.json' });
    console.log('Authentication state successfully saved to auth.json.');
    
    await browser.close();
    rl.close();
  });
})();
